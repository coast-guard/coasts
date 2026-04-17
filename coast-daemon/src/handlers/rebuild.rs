/// Handler for the `coast rebuild` command.
///
/// Rebuilds images inside the DinD container from the bind-mounted `/workspace`
/// and restarts compose services. This is used after editing code in the
/// checked-out coast to pick up changes without a full reassign.
///
/// Internal flow:
/// 1. Verify instance exists and is Running or CheckedOut
/// 2. `docker compose build` inside DinD (reads from /workspace bind-mount)
/// 3. `docker compose up -d` to restart with new images
/// 4. Return list of rebuilt services
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RebuildRequest, RebuildResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Detect whether artifact compose and override files exist in the container.
async fn detect_compose_files(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
) -> (bool, bool) {
    let has_artifact = rt
        .exec_in_coast(container_id, &["test", "-f", "/coast-artifact/compose.yml"])
        .await
        .map(|r| r.success())
        .unwrap_or(false);

    let has_override = rt
        .exec_in_coast(
            container_id,
            &["test", "-f", "/workspace/docker-compose.override.yml"],
        )
        .await
        .map(|r| r.success())
        .unwrap_or(false);

    (has_artifact, has_override)
}

/// Build a compose command with the appropriate `-f` flags based on detected files.
fn build_compose_command<'a>(
    subcmd: &[&'a str],
    has_artifact: bool,
    has_override: bool,
) -> Vec<&'a str> {
    let mut cmd = vec!["docker", "compose"];
    if has_artifact {
        cmd.extend(["-f", "/coast-artifact/compose.yml"]);
        if has_override {
            cmd.extend(["-f", "/workspace/docker-compose.override.yml"]);
        }
        cmd.extend(["--project-directory", "/workspace"]);
    }
    cmd.extend_from_slice(subcmd);
    cmd
}

/// Execute a compose command inside a container, returning a consistent error on failure.
async fn execute_compose_step(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    cmd: &[&str],
    step_name: &str,
    instance_name: &str,
) -> Result<coast_docker::runtime::ExecResult> {
    let result = rt.exec_in_coast(container_id, cmd).await.map_err(|e| {
        CoastError::docker(format!(
            "Failed to exec {step_name} in instance '{instance_name}': {e}"
        ))
    })?;
    if !result.success() {
        return Err(CoastError::docker(format!(
            "{step_name} failed inside instance '{instance_name}': {}",
            result.stderr.trim()
        )));
    }
    Ok(result)
}

/// Validate the instance is rebuildable and return its container_id.
async fn validate_rebuild_target(state: &AppState, project: &str, name: &str) -> Result<String> {
    let db = state.db.lock().await;
    let instance = db
        .get_instance(project, name)?
        .ok_or_else(|| CoastError::InstanceNotFound {
            name: name.to_string(),
            project: project.to_string(),
        })?;

    if instance.remote_host.is_some() {
        return Err(CoastError::state(
            "Rebuild for remote instances is not yet supported. \
             Use `coast build --type remote` to rebuild the remote build artifact, \
             then `coast assign` to apply changes.",
        ));
    }

    if instance.status != InstanceStatus::Running && instance.status != InstanceStatus::CheckedOut {
        return Err(CoastError::state(format!(
            "Instance '{name}' is in '{}' state and cannot be rebuilt. \
             Only Running or CheckedOut instances can be rebuilt. \
             Run `coast start {name}` first.",
            instance.status,
        )));
    }

    instance.container_id.ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{name}' has no container ID. This should not happen for a Running instance. \
             Try `coast rm {name} && coast run {name}`.",
        ))
    })
}

/// Parse service names from `docker compose build` output.
fn parse_rebuilt_services(stdout: &str) -> Vec<String> {
    let mut services: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("Building ")
                .map(|s| s.trim().to_string())
        })
        .collect();
    if services.is_empty() {
        services.push("(all services)".to_string());
    }
    services
}

/// Run a single compose step (build or up) inside a container.
async fn run_compose_step(
    compose_rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    subcmd: &[&str],
    has_artifact: bool,
    has_override: bool,
    step_name: &str,
    instance_name: &str,
) -> Result<coast_docker::runtime::ExecResult> {
    let cmd = build_compose_command(subcmd, has_artifact, has_override);
    execute_compose_step(compose_rt, container_id, &cmd, step_name, instance_name).await
}

/// Run compose build + up inside the container, returning rebuilt service names.
async fn rebuild_and_restart(
    docker: &bollard::Docker,
    container_id: &str,
    instance_name: &str,
) -> Result<Vec<String>> {
    let compose_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let (has_artifact, has_override) = detect_compose_files(&compose_rt, container_id).await;

    let build_result = run_compose_step(
        &compose_rt,
        container_id,
        &["build"],
        has_artifact,
        has_override,
        "docker compose build",
        instance_name,
    )
    .await?;
    info!("compose build completed successfully");

    run_compose_step(
        &compose_rt,
        container_id,
        &["up", "-d"],
        has_artifact,
        has_override,
        "docker compose up",
        instance_name,
    )
    .await?;
    info!("compose services restarted after rebuild");

    Ok(parse_rebuilt_services(&build_result.stdout))
}

/// Handle a rebuild request.
pub async fn handle(req: RebuildRequest, state: &AppState) -> Result<RebuildResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        "handling rebuild request"
    );

    let container_id = validate_rebuild_target(state, &req.project, &req.name).await?;

    let services_rebuilt = match state.docker.as_ref() {
        Some(docker) => rebuild_and_restart(&docker, &container_id, &req.name).await?,
        None => Vec::new(),
    };

    info!(
        name = %req.name,
        services = ?services_rebuilt,
        "rebuild completed"
    );

    Ok(RebuildResponse {
        name: req.name,
        services_rebuilt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppState;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn sample_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("{project}-coasts-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    #[tokio::test]
    async fn test_rebuild_instance_not_found() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);

        let req = RebuildRequest {
            name: "nonexistent".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_rebuild_stopped_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Stopped))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = RebuildRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot be rebuilt"));
    }

    #[tokio::test]
    async fn test_rebuild_idle_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Idle))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = RebuildRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot be rebuilt"));
    }

    #[tokio::test]
    async fn test_rebuild_running_without_docker() {
        // Without Docker client, the handler should succeed (no-op on compose)
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = RebuildRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert!(resp.services_rebuilt.is_empty());
    }

    #[tokio::test]
    async fn test_rebuild_no_container_id_errors() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = RebuildRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no container ID"));
    }

    // --- build_compose_command tests ---

    #[test]
    fn test_build_compose_command_no_artifact() {
        let cmd = build_compose_command(&["build"], false, false);
        assert_eq!(cmd, vec!["docker", "compose", "build"]);
    }

    #[test]
    fn test_build_compose_command_artifact_only() {
        let cmd = build_compose_command(&["build"], true, false);
        assert_eq!(
            cmd,
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "--project-directory",
                "/workspace",
                "build",
            ]
        );
    }

    #[test]
    fn test_build_compose_command_artifact_and_override() {
        let cmd = build_compose_command(&["up", "-d"], true, true);
        assert_eq!(
            cmd,
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-f",
                "/workspace/docker-compose.override.yml",
                "--project-directory",
                "/workspace",
                "up",
                "-d",
            ]
        );
    }

    // --- parse_rebuilt_services tests ---

    #[test]
    fn test_parse_rebuilt_services_with_names() {
        let output = "Building web\nBuilding worker\n";
        let services = parse_rebuilt_services(output);
        assert_eq!(services, vec!["web", "worker"]);
    }

    #[test]
    fn test_parse_rebuilt_services_empty_output() {
        let services = parse_rebuilt_services("");
        assert_eq!(services, vec!["(all services)"]);
    }

    #[test]
    fn test_parse_rebuilt_services_no_building_prefix() {
        let output = "Step 1/5: FROM node:18\nStep 2/5: COPY . .\n";
        let services = parse_rebuilt_services(output);
        assert_eq!(services, vec!["(all services)"]);
    }
}
