/// Handler for the `coast exec` command.
///
/// Executes a command inside a coast container via `docker exec` on the
/// host daemon. Captures stdout and stderr for non-interactive use.
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{ExecRequest, ExecResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Handle an exec request.
///
/// Steps:
/// 1. Verify the instance exists and is running.
/// 2. Exec the command inside the coast container.
/// 3. Return stdout, stderr, and exit code.
pub async fn handle(req: ExecRequest, state: &AppState) -> Result<ExecResponse> {
    info!(name = %req.name, project = %req.project, command = ?req.command, "handling exec request");

    // Phase 1: DB read (locked)
    let container_id = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?;
        let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
            name: req.name.clone(),
            project: req.project.clone(),
        })?;

        if instance.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped. Run `coast start {}` before executing commands.",
                req.name, req.name
            )));
        }
        if instance.status == InstanceStatus::Provisioning
            || instance.status == InstanceStatus::Assigning
        {
            let action = if instance.status == InstanceStatus::Provisioning {
                "provisioned"
            } else {
                "assigned"
            };
            return Err(CoastError::state(format!(
                "Instance '{}' is still being {action}. Wait for the operation to complete.",
                req.name
            )));
        }

        instance.container_id.ok_or_else(|| {
            CoastError::state(format!(
                "Instance '{}' has no container ID. This may indicate a corrupt state. \
                 Try `coast rm {}` and `coast run` again.",
                req.name, req.name
            ))
        })?
    };

    // Phase 2: Docker operations (unlocked)
    let command = if req.command.is_empty() {
        vec!["bash".to_string()]
    } else {
        req.command.clone()
    };

    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let cmd_refs: Vec<&str> = command.iter().map(std::string::String::as_str).collect();
    let exec_result = runtime
        .exec_in_coast(&container_id, &cmd_refs)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to execute command in instance '{}': {}. \
             Verify the instance is running with `coast ps {}`.",
                req.name, e, req.name
            ))
        })?;

    info!(
        name = %req.name,
        exit_code = exec_result.exit_code,
        "exec completed"
    );

    Ok(ExecResponse {
        exit_code: exec_result.exit_code as i32,
        stdout: exec_result.stdout,
        stderr: exec_result.stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(
        name: &str,
        status: InstanceStatus,
        container_id: Option<&str>,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: "my-app".to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: container_id.map(|s| s.to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    #[tokio::test]
    async fn test_exec_running_instance_no_docker() {
        // With docker: None in the test state, exec should return an error
        // indicating Docker is not available.
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                InstanceStatus::Running,
                Some("container-123"),
            ))
            .unwrap();
        }

        let req = ExecRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
            command: vec!["echo".to_string(), "hello".to_string()],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Docker is not available"));
    }

    #[tokio::test]
    async fn test_exec_default_command_no_docker() {
        // With docker: None, exec should fail even with default command.
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-b",
                InstanceStatus::Running,
                Some("container-456"),
            ))
            .unwrap();
        }

        let req = ExecRequest {
            name: "feat-b".to_string(),
            project: "my-app".to_string(),
            command: vec![],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Docker is not available"));
    }

    #[tokio::test]
    async fn test_exec_nonexistent_instance() {
        let state = test_state();
        let req = ExecRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
            command: vec!["bash".to_string()],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_exec_stopped_instance_fails() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "stopped-inst",
                InstanceStatus::Stopped,
                Some("cid"),
            ))
            .unwrap();
        }

        let req = ExecRequest {
            name: "stopped-inst".to_string(),
            project: "my-app".to_string(),
            command: vec!["bash".to_string()],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
    }

    #[tokio::test]
    async fn test_exec_no_container_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("no-cid", InstanceStatus::Running, None))
                .unwrap();
        }

        let req = ExecRequest {
            name: "no-cid".to_string(),
            project: "my-app".to_string(),
            command: vec!["bash".to_string()],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no container ID"));
    }
}
