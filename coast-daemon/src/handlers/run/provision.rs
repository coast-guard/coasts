use std::collections::HashMap;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, RunRequest};
use coast_docker::runtime::Runtime;

use crate::handlers::shared_service_routing::{
    ensure_shared_service_proxies, plan_shared_service_routing,
};
use crate::server::AppState;

use super::service_start::{start_and_wait_for_services, StartServicesRequest};
use super::validate::ValidatedRun;
use super::{
    archive_build, compose_rewrite, emit, host_builds, image_loading, mcp_setup,
    merge_dynamic_port_env_vars, secrets, shared_services_setup,
};

pub(super) struct ProvisionResult {
    pub container_id: String,
    pub pre_allocated_ports: Vec<(String, u16, u16)>,
}

struct CoastfileResources {
    pre_allocated_ports: Vec<(String, u16, u16)>,
    volume_mounts: Vec<coast_docker::runtime::VolumeMount>,
    mcp_servers: Vec<coast_core::types::McpServerConfig>,
    mcp_clients: Vec<coast_core::types::McpClientConnectorConfig>,
    shared_services: Vec<coast_core::types::SharedServiceConfig>,
    shared_service_targets: HashMap<String, String>,
    shared_network: Option<String>,
}

/// Phase 2: Docker provisioning -- create container, load images, start services.
pub(super) async fn provision_instance(
    docker: &bollard::Docker,
    validated: &ValidatedRun,
    req: &RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<ProvisionResult> {
    let code_path = resolve_code_path(&req.project, validated.build_id.as_deref());
    let uses_archive_build =
        detect_archive_build(validated.has_compose, req.branch.as_deref(), &code_path).await;

    let per_instance_image_tags =
        build_host_images(validated, uses_archive_build, &code_path, req, progress).await;

    let artifact_dir = resolve_artifact_dir(&req.project, validated.build_id.as_deref());
    let coastfile_path = artifact_dir.join("coastfile.toml");

    let resources = load_coastfile_resources(&coastfile_path, req, state, progress).await?;

    let secret_plan = secrets::load_secrets_for_instance(&coastfile_path, &req.name);
    let secret_container_paths = secret_plan.container_paths.clone();
    let secret_files_for_exec = secret_plan.files_for_exec.clone();
    let has_volume_mounts = !resources.volume_mounts.is_empty();

    let container_id = create_container(
        docker,
        validated,
        req,
        &code_path,
        &resources,
        secret_plan,
        &resources.pre_allocated_ports,
        progress,
    )
    .await?;

    {
        let db = state.db.lock().await;
        db.update_instance_container_id(&req.project, &req.name, Some(&container_id))?;
    }

    connect_shared_network(state, &resources.shared_network, &container_id).await;

    let shared_service_routing = if resources.shared_services.is_empty() {
        None
    } else {
        Some(
            plan_shared_service_routing(
                docker,
                &container_id,
                &resources.shared_services,
                &resources.shared_service_targets,
            )
            .await?,
        )
    };

    if !uses_archive_build {
        let shared_service_hosts = shared_service_routing.as_ref().map_or_else(
            HashMap::new,
            super::super::shared_service_routing::SharedServiceRoutingPlan::host_map,
        );

        rewrite_compose(
            &artifact_dir,
            &code_path,
            &coastfile_path,
            &shared_service_hosts,
            &per_instance_image_tags,
            has_volume_mounts,
            &secret_container_paths,
            &req.project,
            &req.name,
        );
    }

    if let Some(ref routing) = shared_service_routing {
        ensure_shared_service_proxies(docker, &container_id, routing).await?;
    }

    load_cached_images(docker, &container_id, &req.project, validated, progress).await;

    if uses_archive_build {
        emit(
            progress,
            BuildProgressEvent::started("Building images", 2, validated.total_steps),
        );
        let _archive_tags = archive_build::run_archive_build(
            docker,
            archive_build::ArchiveBuildRequest {
                container_id: &container_id,
                code_path: &code_path,
                branch: req.branch.as_deref().unwrap(),
                project: &req.project,
                instance_name: &req.name,
                artifact_dir: &artifact_dir,
                coastfile_path: &coastfile_path,
                has_volume_mounts,
                secret_container_paths: &secret_container_paths,
                progress,
            },
        )
        .await?;
    } else {
        image_loading::pipe_host_images_to_inner_daemon(&per_instance_image_tags, &container_id)
            .await;
    }

    bind_workspace(docker, &container_id, &req.name).await;

    if !resources.mcp_servers.is_empty() || !resources.mcp_clients.is_empty() {
        mcp_setup::install_mcp_servers(
            &container_id,
            &resources.mcp_servers,
            &resources.mcp_clients,
            docker,
            progress,
        )
        .await?;
    }

    secrets::write_secret_files_via_exec(&secret_files_for_exec, &container_id, docker).await;

    let artifact_dir_opt = if artifact_dir.exists() {
        Some(artifact_dir.as_path())
    } else {
        None
    };
    start_and_wait_for_services(
        docker,
        StartServicesRequest {
            container_id: &container_id,
            instance_name: &req.name,
            project: &req.project,
            has_compose: validated.has_compose,
            has_services: validated.has_services,
            uses_archive_build,
            compose_rel_dir: validated.compose_rel_dir.as_deref(),
            artifact_dir_opt,
            bare_services: &validated.bare_services,
            total_steps: validated.total_steps,
            progress,
        },
    )
    .await?;

    Ok(ProvisionResult {
        container_id,
        pre_allocated_ports: resources.pre_allocated_ports,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_code_path(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = build_id
        .map(|bid| project_dir.join(bid).join("manifest.json"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| project_dir.join("latest").join("manifest.json"));
    if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("project_root")?
                    .as_str()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    } else {
        std::env::current_dir().unwrap_or_default()
    }
}

fn resolve_artifact_dir(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    let project_images_dir = home.join(".coast").join("images").join(project);
    if let Some(bid) = build_id {
        let resolved = project_images_dir.join(bid);
        if resolved.exists() {
            resolved
        } else {
            project_images_dir.join("latest")
        }
    } else {
        project_images_dir.join("latest")
    }
}

async fn detect_archive_build(
    has_compose: bool,
    branch: Option<&str>,
    code_path: &std::path::Path,
) -> bool {
    if !has_compose {
        return false;
    }
    let Some(branch) = branch else {
        return false;
    };
    let host_branch_output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(code_path)
        .output()
        .await;
    let host_branch = host_branch_output
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match host_branch {
        Some(ref hb) if hb != branch => {
            info!(
                host_branch = %hb, requested_branch = %branch,
                "branch differs from host — will use git archive build inside DinD"
            );
            true
        }
        _ => false,
    }
}

async fn build_host_images(
    validated: &ValidatedRun,
    uses_archive_build: bool,
    code_path: &std::path::Path,
    req: &RunRequest,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Vec<(String, String)> {
    if uses_archive_build || !validated.has_compose {
        return Vec::new();
    }
    emit(
        progress,
        BuildProgressEvent::started("Building images", 2, validated.total_steps),
    );
    let tags = host_builds::build_per_instance_images_on_host(
        code_path,
        &req.project,
        &req.name,
        progress,
    )
    .await;
    emit(progress, BuildProgressEvent::done("Building images", "ok"));
    tags
}

async fn load_coastfile_resources(
    coastfile_path: &std::path::Path,
    req: &RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<CoastfileResources> {
    let mut result = CoastfileResources {
        pre_allocated_ports: Vec::new(),
        volume_mounts: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_clients: Vec::new(),
        shared_services: Vec::new(),
        shared_service_targets: HashMap::new(),
        shared_network: None,
    };

    if !coastfile_path.exists() {
        return Ok(result);
    }
    let Ok(coastfile) = coast_core::coastfile::Coastfile::from_file(coastfile_path) else {
        return Ok(result);
    };

    for (port_name, port_num) in &coastfile.ports {
        let dynamic_port = crate::port_manager::allocate_dynamic_port()?;
        result
            .pre_allocated_ports
            .push((port_name.clone(), *port_num, dynamic_port));
    }

    for vol_config in &coastfile.volumes {
        let resolved_name =
            coast_core::volume::resolve_volume_name(vol_config, &req.name, &req.project);
        result
            .volume_mounts
            .push(coast_docker::runtime::VolumeMount {
                volume_name: resolved_name,
                container_path: format!("/coast-volumes/{}", vol_config.name),
                read_only: false,
            });
    }

    copy_snapshot_volumes(&coastfile.volumes, &req.name, &req.project, progress).await?;

    result.mcp_servers = coastfile.mcp_servers.clone();
    result.mcp_clients = coastfile.mcp_clients.clone();
    result.shared_services = coastfile.shared_services.clone();

    if !coastfile.shared_services.is_empty() {
        if let Some(ref docker) = state.docker {
            let shared = shared_services_setup::start_shared_services(
                &req.project,
                &coastfile.shared_services,
                docker,
                state,
            )
            .await?;
            result.shared_service_targets = shared.service_hosts;
            result.shared_network = shared.network_name;
        }
    }

    Ok(result)
}

fn rewrite_compose(
    artifact_dir: &std::path::Path,
    code_path: &std::path::Path,
    coastfile_path: &std::path::Path,
    shared_service_hosts: &HashMap<String, String>,
    per_instance_image_tags: &[(String, String)],
    has_volume_mounts: bool,
    secret_container_paths: &[String],
    project: &str,
    instance_name: &str,
) {
    let compose_path = artifact_dir.join("compose.yml");
    let compose_content = if compose_path.exists() {
        std::fs::read_to_string(&compose_path).ok()
    } else {
        let ws_compose = code_path.join("docker-compose.yml");
        std::fs::read_to_string(&ws_compose).ok()
    };

    let Some(ref content) = compose_content else {
        return;
    };

    let assign_cfg = coast_core::coastfile::Coastfile::from_file(coastfile_path)
        .map(|cf| cf.assign)
        .unwrap_or_default();
    let hot_svcs: Vec<String> = assign_cfg
        .services
        .iter()
        .filter(|(_, a)| **a == coast_core::types::AssignAction::Hot)
        .map(|(s, _)| s.clone())
        .collect();
    let default_hot = assign_cfg.default == coast_core::types::AssignAction::Hot;

    compose_rewrite::rewrite_compose_for_instance(
        content,
        &compose_rewrite::ComposeRewriteConfig {
            shared_service_hosts,
            coastfile_path,
            per_instance_image_tags,
            has_volume_mounts,
            secret_container_paths,
            project,
            instance_name,
            hot_services: &hot_svcs,
            default_hot,
        },
    );
}

async fn create_container(
    docker: &bollard::Docker,
    validated: &ValidatedRun,
    req: &RunRequest,
    code_path: &std::path::Path,
    resources: &CoastfileResources,
    secret_plan: secrets::SecretInjectionPlan,
    pre_allocated_ports: &[(String, u16, u16)],
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let image_cache_dir = home.join(".coast").join("image-cache");
    let image_cache_path = if image_cache_dir.exists() {
        Some(image_cache_dir.as_path())
    } else {
        None
    };

    let artifact_dir_path = resolve_artifact_dir(&req.project, validated.build_id.as_deref());
    let artifact_dir_opt = if artifact_dir_path.exists() {
        Some(artifact_dir_path.as_path())
    } else {
        None
    };

    let coast_image = read_coast_image(&artifact_dir_path);

    let override_dir_path = home
        .join(".coast")
        .join("overrides")
        .join(&req.project)
        .join(&req.name);
    std::fs::create_dir_all(&override_dir_path).map_err(|error| CoastError::Io {
        message: format!("failed to create override directory: {error}"),
        path: override_dir_path.clone(),
        source: Some(error),
    })?;
    let override_dir_opt = Some(override_dir_path.as_path());

    let dind_extra_hosts = vec!["host.docker.internal:host-gateway".to_string()];

    let mut env_vars = secret_plan.env_vars;
    merge_dynamic_port_env_vars(&mut env_vars, pre_allocated_ports);

    let mut config = coast_docker::dind::build_dind_config(
        &req.project,
        &req.name,
        code_path,
        env_vars,
        secret_plan.bind_mounts,
        resources.volume_mounts.clone(),
        Vec::new(),
        image_cache_path,
        artifact_dir_opt,
        coast_image.as_deref(),
        override_dir_opt,
        dind_extra_hosts,
    );

    if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(
        &home
            .join(".coast")
            .join("images")
            .join(&req.project)
            .join("latest")
            .join("coastfile.toml"),
    ) {
        for (idx, resolved) in cf.external_worktree_dirs() {
            config.bind_mounts.push(coast_docker::runtime::BindMount {
                host_path: resolved,
                container_path: coast_core::coastfile::Coastfile::external_mount_path(idx),
                read_only: false,
                propagation: None,
            });
        }
    }

    for (_name, canonical, dynamic) in pre_allocated_ports {
        config
            .published_ports
            .push(coast_docker::runtime::PortPublish {
                host_port: *dynamic,
                container_port: *canonical,
            });
    }

    let creating_step = if validated.has_compose { 3 } else { 2 };
    emit(
        progress,
        BuildProgressEvent::started("Creating container", creating_step, validated.total_steps),
    );

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let manager = coast_docker::container::ContainerManager::new(runtime);
    let container_id = manager.create_and_start(&config).await.map_err(|e| {
        CoastError::docker(format!(
            "Failed to create coast container for instance '{}': {}. \
             Ensure Docker is running and the docker:dind image is available.",
            req.name, e
        ))
    })?;

    emit(
        progress,
        BuildProgressEvent::done("Creating container", "ok"),
    );
    Ok(container_id)
}

fn read_coast_image(artifact_dir: &std::path::Path) -> Option<String> {
    let manifest_path = artifact_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("coast_image")?.as_str().map(String::from))
}

async fn connect_shared_network(
    state: &AppState,
    shared_network: &Option<String>,
    container_id: &str,
) {
    let Some(ref net_name) = shared_network else {
        return;
    };
    let Some(ref docker) = state.docker else {
        return;
    };
    let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
    if let Err(e) = nm.connect_container(net_name, container_id).await {
        tracing::warn!(error = %e, "failed to connect coast container to shared network (may already be connected)");
    } else {
        info!(network = %net_name, container = %container_id, "connected coast container to shared services network");
    }
}

async fn load_cached_images(
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
    validated: &ValidatedRun,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    let loading_step = if validated.has_compose { 4 } else { 3 };
    emit(
        progress,
        BuildProgressEvent::started("Loading cached images", loading_step, validated.total_steps),
    );

    let home = dirs::home_dir().unwrap_or_default();
    let cache_dir = home.join(".coast").join("image-cache");
    if cache_dir.exists() {
        let tarball_names = image_loading::collect_project_tarballs(&cache_dir, project);
        if !tarball_names.is_empty() {
            let existing_images = image_loading::query_existing_images(docker, container_id).await;
            let (tarballs_to_load, skipped) =
                image_loading::filter_tarballs_to_load(tarball_names, &existing_images);

            if skipped > 0 {
                emit(
                    progress,
                    BuildProgressEvent::item(
                        "Loading cached images",
                        format!("{skipped} already present (skipped)"),
                        "skip",
                    ),
                );
                info!(project = %project, skipped = skipped, "skipped loading images already present in inner daemon (persistent volume)");
            }

            info!(project = %project, tarball_count = tarballs_to_load.len(), "loading project-relevant cached images");
            image_loading::load_tarballs_into_inner_daemon(
                &tarballs_to_load,
                docker,
                container_id,
                progress,
            )
            .await;
        }
    }
    emit(
        progress,
        BuildProgressEvent::done("Loading cached images", "ok"),
    );
}

async fn bind_workspace(docker: &bollard::Docker, container_id: &str, instance_name: &str) {
    let mount_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let mount_result = mount_rt
        .exec_in_coast(
            container_id,
            &["sh", "-c", "mkdir -p /workspace && mount --bind /host-project /workspace && mount --make-rshared /workspace"],
        )
        .await;
    match mount_result {
        Ok(r) if r.success() => {
            info!(instance = %instance_name, "bound /host-project -> /workspace")
        }
        Ok(r) => warn!(instance = %instance_name, stderr = %r.stderr, "failed to bind /workspace"),
        Err(e) => warn!(instance = %instance_name, error = %e, "failed to bind /workspace"),
    }
}

// ---------------------------------------------------------------------------
// Snapshot volume copying
// ---------------------------------------------------------------------------

async fn copy_snapshot_volumes(
    volumes: &[coast_core::types::VolumeConfig],
    instance_name: &str,
    project: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    for vol_config in volumes {
        let Some(ref src) = vol_config.snapshot_source else {
            continue;
        };
        let dest = coast_core::volume::resolve_volume_name(vol_config, instance_name, project);
        copy_single_snapshot(src, &dest, &vol_config.name, progress).await?;
    }
    Ok(())
}

async fn copy_single_snapshot(
    src: &str,
    dest: &str,
    volume_name: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    info!(source = %src, dest = %dest, "copying snapshot_source volume");
    emit(
        progress,
        BuildProgressEvent::done(format!("Copying volume {src} \u{2192} {dest}"), "started"),
    );

    let stopped_ids = stop_containers_using_volume(src).await?;

    let cmd = coast_core::volume::snapshot_copy_command(src, dest);
    let output = tokio::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to run snapshot copy for volume '{volume_name}': {e}"
            ))
        })?;

    restart_stopped_containers(&stopped_ids).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::docker(format!(
            "snapshot copy failed for volume '{volume_name}' (source: '{src}'): {stderr}. \
             Verify the source volume exists with: docker volume ls | grep {src}"
        )));
    }
    Ok(())
}

async fn stop_containers_using_volume(volume: &str) -> Result<Vec<String>> {
    let using_output = tokio::process::Command::new("docker")
        .args(["ps", "-q", "--filter", &format!("volume={volume}")])
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to check containers using volume '{volume}': {e}"
            ))
        })?;

    let ids: Vec<String> = String::from_utf8_lossy(&using_output.stdout)
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    if ids.is_empty() {
        return Ok(ids);
    }

    info!(volume = %volume, containers = ?ids, "stopping containers for consistent snapshot copy");
    let mut stop_args = vec!["stop".to_string()];
    stop_args.extend(ids.clone());
    let stop_out = tokio::process::Command::new("docker")
        .args(&stop_args)
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to stop containers using volume '{volume}': {e}"
            ))
        })?;
    if !stop_out.status.success() {
        warn!(
            "failed to stop containers on volume '{}': {}",
            volume,
            String::from_utf8_lossy(&stop_out.stderr)
        );
    }
    Ok(ids)
}

async fn restart_stopped_containers(stopped_ids: &[String]) {
    if stopped_ids.is_empty() {
        return;
    }
    info!(containers = ?stopped_ids, "restarting containers after snapshot copy");
    let mut start_args = vec!["start".to_string()];
    start_args.extend(stopped_ids.iter().cloned());
    let _ = tokio::process::Command::new("docker")
        .args(&start_args)
        .output()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    fn sample_run_request() -> RunRequest {
        RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
        }
    }

    #[test]
    fn test_resolve_artifact_dir_with_build_id_missing_falls_back_to_latest() {
        let path = resolve_artifact_dir("myproj", Some("nonexistent-build-id"));
        assert!(
            path.to_string_lossy().contains("latest"),
            "should fall back to latest when build_id dir doesn't exist"
        );
    }

    #[test]
    fn test_resolve_artifact_dir_without_build_id_uses_latest() {
        let path = resolve_artifact_dir("myproj", None);
        assert!(
            path.to_string_lossy().contains("latest"),
            "should use latest when no build_id"
        );
        assert!(path.to_string_lossy().contains("myproj"));
    }

    #[test]
    fn test_resolve_code_path_no_manifest_uses_cwd() {
        let path = resolve_code_path("nonexistent-project", None);
        let cwd = std::env::current_dir().unwrap_or_default();
        assert_eq!(path, cwd, "should fall back to CWD when no manifest exists");
    }

    #[test]
    fn test_read_coast_image_missing_dir() {
        let result = read_coast_image(std::path::Path::new("/nonexistent/dir"));
        assert!(result.is_none(), "should return None for missing dir");
    }

    #[test]
    fn test_read_coast_image_valid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"coast_image": "my-custom:latest"}"#).unwrap();
        let result = read_coast_image(dir.path());
        assert_eq!(result, Some("my-custom:latest".to_string()));
    }

    #[test]
    fn test_read_coast_image_no_coast_image_field() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"project_root": "/some/path"}"#).unwrap();
        let result = read_coast_image(dir.path());
        assert!(
            result.is_none(),
            "should return None when coast_image field is missing"
        );
    }

    #[tokio::test]
    async fn test_detect_archive_build_no_compose() {
        let result = detect_archive_build(false, Some("main"), std::path::Path::new(".")).await;
        assert!(!result, "should be false when has_compose is false");
    }

    #[tokio::test]
    async fn test_detect_archive_build_no_branch() {
        let result = detect_archive_build(true, None, std::path::Path::new(".")).await;
        assert!(!result, "should be false when no branch specified");
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_reads_ports_and_volume_mounts() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"
compose = "./docker-compose.yml"

[ports]
web = 3000

[volumes.cache]
strategy = "shared"
service = "redis"
mount = "/data"
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert_eq!(resources.pre_allocated_ports.len(), 1);
        assert_eq!(resources.pre_allocated_ports[0].0, "web");
        assert_eq!(resources.pre_allocated_ports[0].1, 3000);
        assert!(resources.pre_allocated_ports[0].2 > 0);

        assert_eq!(resources.volume_mounts.len(), 1);
        assert_eq!(
            resources.volume_mounts[0].volume_name,
            "coast-shared--proj--cache"
        );
        assert_eq!(
            resources.volume_mounts[0].container_path,
            "/coast-volumes/cache"
        );
        assert!(!resources.volume_mounts[0].read_only);

        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_missing_file_returns_empty_resources() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("missing-coastfile.toml");

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert!(resources.pre_allocated_ports.is_empty());
        assert!(resources.volume_mounts.is_empty());
        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_invalid_coastfile_returns_empty_resources() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"

[volumes.bad]
strategy = "shared"
service = "db"
mount = "/data"
snapshot_source = "seed-volume"
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert!(resources.pre_allocated_ports.is_empty());
        assert!(resources.volume_mounts.is_empty());
        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }
}
