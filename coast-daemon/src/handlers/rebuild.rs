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

/// Handle a rebuild request.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(req: RebuildRequest, state: &AppState) -> Result<RebuildResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        "handling rebuild request"
    );

    {
        let db = state.db.lock().await;
        if let Some(instance) = db.get_instance(&req.project, &req.name)? {
            if instance.remote_host.is_some() {
                drop(db);
                return Err(CoastError::state(
                    "Rebuild for remote instances is not yet supported. \
                     Use `coast build --type remote` to rebuild the remote build artifact, \
                     then `coast assign` to apply changes.",
                ));
            }
        }
    }

    // Phase 1: DB read (locked)
    let container_id = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
            CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            }
        })?;

        if instance.status != InstanceStatus::Running
            && instance.status != InstanceStatus::CheckedOut
        {
            return Err(CoastError::state(format!(
                "Instance '{}' is in '{}' state and cannot be rebuilt. \
                 Only Running or CheckedOut instances can be rebuilt. \
                 Run `coast start {}` first.",
                req.name, instance.status, req.name,
            )));
        }

        instance.container_id.ok_or_else(|| {
            CoastError::state(format!(
                "Instance '{}' has no container ID. This should not happen for a Running instance. \
                 Try `coast rm {} && coast run {}`.",
                req.name, req.name, req.name,
            ))
        })?
    };

    // Phase 2: Docker operations (unlocked)
    let mut services_rebuilt = Vec::new();

    if let Some(docker) = state.docker.as_ref() {
        let compose_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

        // Step 2: docker compose build inside DinD
        // Check for artifact compose and override files
        let has_artifact = compose_rt
            .exec_in_coast(
                &container_id,
                &["test", "-f", "/coast-artifact/compose.yml"],
            )
            .await
            .map(|r| r.success())
            .unwrap_or(false);

        let has_override = compose_rt
            .exec_in_coast(
                &container_id,
                &["test", "-f", "/workspace/docker-compose.override.yml"],
            )
            .await
            .map(|r| r.success())
            .unwrap_or(false);

        let build_cmd: Vec<&str> = if has_artifact && has_override {
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-f",
                "/workspace/docker-compose.override.yml",
                "--project-directory",
                "/workspace",
                "build",
            ]
        } else if has_artifact {
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "--project-directory",
                "/workspace",
                "build",
            ]
        } else {
            vec!["docker", "compose", "build"]
        };

        info!(cmd = ?build_cmd, "running compose build inside DinD");
        let build_result = compose_rt.exec_in_coast(&container_id, &build_cmd).await;

        match build_result {
            Ok(r) if r.success() => {
                info!("compose build completed successfully");
                // Parse output to determine which services were rebuilt
                for line in r.stdout.lines() {
                    let trimmed = line.trim();
                    // docker compose build outputs lines like "Building service_name"
                    // or " => [service_name] ..."
                    if let Some(service) = trimmed.strip_prefix("Building ") {
                        services_rebuilt.push(service.trim().to_string());
                    }
                }
                // If we couldn't parse service names, indicate rebuild happened
                if services_rebuilt.is_empty() {
                    services_rebuilt.push("(all services)".to_string());
                }
            }
            Ok(r) => {
                return Err(CoastError::docker(format!(
                    "docker compose build failed inside instance '{}': {}",
                    req.name,
                    r.stderr.trim()
                )));
            }
            Err(e) => {
                return Err(CoastError::docker(format!(
                    "Failed to exec docker compose build in instance '{}': {e}",
                    req.name
                )));
            }
        }

        // Step 3: docker compose up -d to restart with new images
        let up_cmd: Vec<&str> = if has_artifact && has_override {
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
        } else if has_artifact {
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "--project-directory",
                "/workspace",
                "up",
                "-d",
            ]
        } else {
            vec!["docker", "compose", "up", "-d"]
        };

        info!(cmd = ?up_cmd, "restarting compose services after rebuild");
        let up_result = compose_rt.exec_in_coast(&container_id, &up_cmd).await;

        match up_result {
            Ok(r) if r.success() => {
                info!("compose services restarted after rebuild");
            }
            Ok(r) => {
                return Err(CoastError::docker(format!(
                    "docker compose up failed after rebuild in instance '{}': {}",
                    req.name,
                    r.stderr.trim()
                )));
            }
            Err(e) => {
                return Err(CoastError::docker(format!(
                    "Failed to exec docker compose up in instance '{}': {e}",
                    req.name
                )));
            }
        }
    }

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
}
