use std::path::Path;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::BuildProgressEvent;
use coast_docker::runtime::Runtime;

use super::emit;

/// Pipe a branch's code into the DinD container via git archive, build per-instance images
/// inside DinD, and write a compose override with image/volume/extra_hosts overrides.
///
/// Returns the list of (service_name, image_tag) pairs built inside DinD.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_archive_build(
    docker: &bollard::Docker,
    container_id: &str,
    code_path: &Path,
    branch: &str,
    project: &str,
    instance_name: &str,
    artifact_dir: &Path,
    coastfile_path: &Path,
    has_volume_mounts: bool,
    secret_container_paths: &[String],
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<Vec<(String, String)>> {
    let archive_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let mut per_instance_image_tags: Vec<(String, String)> = Vec::new();

    // Create temp build directory inside DinD
    let _ = archive_rt
        .exec_in_coast(container_id, &["mkdir", "-p", "/tmp/coast-build"])
        .await;

    // Pipe git archive into the container
    let root_owned = code_path.to_path_buf();
    let branch_owned = branch.to_string();
    let cid_owned = container_id.to_string();
    let archive_result = tokio::task::spawn_blocking(move || {
        let mut archive = std::process::Command::new("git")
            .args(["archive", &branch_owned])
            .current_dir(&root_owned)
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let archive_stdout = archive.stdout.take().expect("archive stdout was piped");
        let extract_output = std::process::Command::new("docker")
            .args([
                "exec",
                "-i",
                &cid_owned,
                "tar",
                "-x",
                "-C",
                "/tmp/coast-build",
            ])
            .stdin(archive_stdout)
            .output()?;
        archive.wait()?;
        Ok::<_, std::io::Error>(extract_output)
    })
    .await;

    match archive_result {
        Ok(Ok(output)) if output.status.success() => {
            info!(branch = %branch, "piped git archive into DinD at /tmp/coast-build");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::git(format!(
                "Failed to extract git archive into DinD: {}",
                stderr.trim()
            )));
        }
        Ok(Err(e)) => {
            return Err(CoastError::git(format!(
                "Failed to run git archive for branch '{branch}': {e}"
            )));
        }
        Err(e) => {
            return Err(CoastError::git(format!(
                "spawn_blocking failed for git archive: {e}"
            )));
        }
    }

    // Build per-instance images INSIDE DinD from /tmp/coast-build
    build_images_inside_dind(
        &archive_rt,
        container_id,
        code_path,
        project,
        instance_name,
        &mut per_instance_image_tags,
        progress,
    )
    .await;

    // Write compose override inside DinD at /tmp/coast-build/
    write_archive_compose_override(
        &archive_rt,
        container_id,
        artifact_dir,
        code_path,
        coastfile_path,
        has_volume_mounts,
        &per_instance_image_tags,
        secret_container_paths,
    )
    .await;

    emit(progress, BuildProgressEvent::done("Building images", "ok"));

    Ok(per_instance_image_tags)
}

#[allow(clippy::cognitive_complexity)]
async fn build_images_inside_dind(
    runtime: &coast_docker::dind::DindRuntime,
    container_id: &str,
    code_path: &Path,
    project: &str,
    instance_name: &str,
    image_tags: &mut Vec<(String, String)>,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    let compose_candidates = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ];
    let original_compose_path = compose_candidates
        .iter()
        .map(|name| code_path.join(name))
        .find(|p| p.exists());

    let Some(compose_path) = original_compose_path else {
        return;
    };
    let Ok(compose_content) = std::fs::read_to_string(&compose_path) else {
        return;
    };
    let Ok(parse_result) =
        coast_docker::compose_build::parse_compose_file(&compose_content, project)
    else {
        return;
    };

    for directive in &parse_result.build_directives {
        let instance_tag = coast_docker::compose_build::coast_built_instance_image_tag(
            project,
            &directive.service_name,
            instance_name,
        );
        let build_context = if directive.context == "." {
            "/tmp/coast-build".to_string()
        } else {
            format!("/tmp/coast-build/{}", directive.context)
        };

        info!(
            service = %directive.service_name,
            tag = %instance_tag,
            context = %build_context,
            "building per-instance image inside DinD"
        );

        let _ = runtime
            .exec_in_coast(container_id, &["docker", "builder", "prune", "-af"])
            .await;

        let mut build_cmd = vec![
            "docker".to_string(),
            "build".to_string(),
            "-t".to_string(),
            instance_tag.clone(),
        ];
        if let Some(ref dockerfile) = directive.dockerfile {
            if dockerfile != "Dockerfile" {
                build_cmd.push("-f".to_string());
                build_cmd.push(format!("{}/{}", build_context, dockerfile));
            }
        }
        build_cmd.push(build_context);

        let cmd_refs: Vec<&str> = build_cmd.iter().map(std::string::String::as_str).collect();
        let build_result = runtime.exec_in_coast(container_id, &cmd_refs).await;

        match build_result {
            Ok(r) if r.success() => {
                image_tags.push((directive.service_name.clone(), instance_tag));
                emit(
                    progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "ok"),
                );
                info!(
                    service = %directive.service_name,
                    "per-instance image built inside DinD"
                );
            }
            Ok(r) => {
                emit(
                    progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "warn")
                        .with_verbose(r.stderr.clone()),
                );
                tracing::warn!(
                    service = %directive.service_name,
                    stderr = %r.stderr,
                    "failed to build per-instance image inside DinD"
                );
            }
            Err(e) => {
                emit(
                    progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "warn")
                        .with_verbose(e.to_string()),
                );
                tracing::warn!(
                    service = %directive.service_name,
                    error = %e,
                    "failed to exec docker build inside DinD"
                );
            }
        }
    }
}

#[allow(clippy::cognitive_complexity)]
async fn write_archive_compose_override(
    runtime: &coast_docker::dind::DindRuntime,
    container_id: &str,
    artifact_dir: &Path,
    code_path: &Path,
    coastfile_path: &Path,
    has_volume_mounts: bool,
    per_instance_image_tags: &[(String, String)],
    secret_container_paths: &[String],
) {
    let mut needs_override = false;
    let mut override_yaml = String::from("# Auto-generated by Coast - do not edit\n");

    if has_volume_mounts {
        needs_override = true;
        override_yaml.push_str("volumes:\n");
        if coastfile_path.exists() {
            if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(coastfile_path) {
                for vol_config in &cf.volumes {
                    let container_mount = format!("/coast-volumes/{}", vol_config.name);
                    override_yaml.push_str(&format!(
                        "  {}:\n    driver: local\n    driver_opts:\n      type: none\n      device: {}\n      o: bind\n",
                        vol_config.name, container_mount
                    ));
                }
            }
        }
    }

    let mut svc_image: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut svc_volumes: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut svc_extra_hosts: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for (service_name, tag) in per_instance_image_tags {
        svc_image.insert(service_name.clone(), tag.clone());
    }

    let compose_path = artifact_dir.join("compose.yml");
    let compose_content_archive = if compose_path.exists() {
        std::fs::read_to_string(&compose_path).ok()
    } else {
        let ws_compose = code_path.join("docker-compose.yml");
        std::fs::read_to_string(&ws_compose).ok()
    };

    if !secret_container_paths.is_empty() {
        if let Some(ref content) = compose_content_archive {
            if let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(content) {
                if let Some(services) = yaml.get("services").and_then(|s| s.as_mapping()) {
                    for (svc_name, _) in services {
                        if let Some(name) = svc_name.as_str() {
                            let vols: Vec<String> = secret_container_paths
                                .iter()
                                .map(|cp| format!("{}:{}:ro", cp, cp))
                                .collect();
                            svc_volumes.insert(name.to_string(), vols);
                        }
                    }
                }
            }
        }
    }

    if let Some(ref content) = compose_content_archive {
        let svc_names = coast_docker::compose::extract_compose_services(content);
        for svc_name in svc_names {
            svc_extra_hosts
                .entry(svc_name)
                .or_default()
                .push("host.docker.internal:host-gateway".to_string());
        }
    }

    let all_services: std::collections::BTreeSet<&str> = svc_image
        .keys()
        .map(std::string::String::as_str)
        .chain(svc_volumes.keys().map(std::string::String::as_str))
        .chain(svc_extra_hosts.keys().map(std::string::String::as_str))
        .collect();

    if !all_services.is_empty() {
        needs_override = true;
        override_yaml.push_str("services:\n");
        for svc in &all_services {
            override_yaml.push_str(&format!("  {}:\n", svc));
            if let Some(tag) = svc_image.get(*svc) {
                override_yaml.push_str(&format!("    image: {}\n", tag));
            }
            if let Some(vols) = svc_volumes.get(*svc) {
                override_yaml.push_str("    volumes:\n");
                for vol in vols {
                    override_yaml.push_str(&format!("      - {}\n", vol));
                }
            }
            if let Some(hosts) = svc_extra_hosts.get(*svc) {
                override_yaml.push_str("    extra_hosts:\n");
                for h in hosts {
                    override_yaml.push_str(&format!("      - \"{}\"\n", h));
                }
            }
            override_yaml.push_str("    environment:\n");
            override_yaml.push_str("      WATCHPACK_POLLING: \"true\"\n");
        }
    }

    if needs_override {
        let write_cmd = format!(
            "cat > /tmp/coast-build/docker-compose.override.yml << 'COAST_EOF'\n{}\nCOAST_EOF",
            override_yaml
        );
        let _ = runtime
            .exec_in_coast(container_id, &["sh", "-c", &write_cmd])
            .await;
        info!("wrote compose override inside DinD at /tmp/coast-build/");
    }
}
