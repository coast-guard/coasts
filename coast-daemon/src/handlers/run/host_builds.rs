use std::path::Path;

use tracing::info;

use coast_core::protocol::BuildProgressEvent;

use super::emit;

/// Build per-instance Docker images on the HOST daemon for services with `build:` directives.
///
/// Parses the compose file to find build directives, runs `docker build` on the host for each,
/// and returns the list of (service_name, image_tag) pairs that were successfully built.
/// Uses the host's Docker layer cache from `coast build`, making rebuilds fast.
#[allow(clippy::cognitive_complexity)]
pub(super) async fn build_per_instance_images_on_host(
    code_path: &Path,
    project: &str,
    instance_name: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Vec<(String, String)> {
    let mut image_tags: Vec<(String, String)> = Vec::new();

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
        return image_tags;
    };
    let Ok(compose_content) = std::fs::read_to_string(&compose_path) else {
        return image_tags;
    };
    let Ok(parse_result) =
        coast_docker::compose_build::parse_compose_file(&compose_content, project)
    else {
        return image_tags;
    };

    for directive in &parse_result.build_directives {
        let instance_tag = coast_docker::compose_build::coast_built_instance_image_tag(
            project,
            &directive.service_name,
            instance_name,
        );
        info!(
            service = %directive.service_name,
            tag = %instance_tag,
            "building per-instance image on HOST"
        );
        let mut build_directive = directive.clone();
        build_directive.coast_image_tag = instance_tag.clone();
        let cmd_args = coast_docker::compose_build::docker_build_cmd(&build_directive, code_path);
        match tokio::process::Command::new(&cmd_args[0])
            .args(&cmd_args[1..])
            .output()
            .await
        {
            Ok(output) if output.status.success() => {
                image_tags.push((directive.service_name.clone(), instance_tag));
                emit(
                    progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "ok"),
                );
                info!(
                    service = %directive.service_name,
                    "per-instance image built on HOST"
                );
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                emit(
                    progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "warn")
                        .with_verbose(stderr.trim().to_string()),
                );
                tracing::warn!(
                    service = %directive.service_name,
                    stderr = %stderr,
                    "failed to build per-instance image on HOST, inner compose will build"
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
                    "failed to run docker build on HOST, inner compose will build"
                );
            }
        }
    }

    image_tags
}
