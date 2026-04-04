/// Handler for the `coast exec` command.
///
/// Executes a command inside a coast container via `docker exec` on the
/// host daemon. Captures stdout and stderr for non-interactive use.
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use tracing::info;

use coast_core::compose::{compose_context_for_build, shell_join, shell_quote};
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{ExecRequest, ExecResponse};
use coast_core::types::InstanceStatus;

use crate::server::AppState;

fn host_uid_gid() -> String {
    #[cfg(unix)]
    {
        let uid = unsafe { nix::libc::getuid() };
        let gid = unsafe { nix::libc::getgid() };
        format!("{uid}:{gid}")
    }
    #[cfg(not(unix))]
    {
        "0:0".to_string()
    }
}

fn resolve_command(command: &[String]) -> Vec<String> {
    if command.is_empty() {
        vec!["sh".to_string()]
    } else {
        command.to_vec()
    }
}

fn build_service_exec_script(
    project: &str,
    build_id: Option<&str>,
    service: &str,
    command: &[String],
    user_spec: Option<&str>,
) -> String {
    let ctx = compose_context_for_build(project, build_id);
    let resolve_service = ctx.compose_script(&format!("ps -q {}", shell_quote(service)));
    let user_flag = user_spec
        .map(|user| format!(" -u {}", shell_quote(user)))
        .unwrap_or_default();
    let inner_command = shell_join(command);
    let error_message = shell_quote(&format!("Service '{service}' is not running"));
    format!(
        "cid=\"$({resolve_service} | head -n1)\"; \
         if [ -z \"$cid\" ]; then echo {error_message} >&2; exit 1; fi; \
         exec docker exec{user_flag} \"$cid\" {inner_command}"
    )
}

async fn exec_in_container(
    docker: &bollard::Docker,
    container_id: &str,
    command: &[String],
    user_spec: Option<&str>,
) -> Result<ExecResponse> {
    let exec_options = CreateExecOptions {
        cmd: Some(command.to_vec()),
        user: user_spec.map(std::string::ToString::to_string),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to create exec in container '{container_id}': {e}"
            ))
        })?;

    let output = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to start exec in container '{container_id}': {e}"
            ))
        })?;

    let mut stdout = String::new();
    let mut stderr = String::new();

    if let StartExecResults::Attached { mut output, .. } = output {
        use futures_util::StreamExt;
        while let Some(Ok(msg)) = output.next().await {
            match msg {
                bollard::container::LogOutput::StdOut { message } => {
                    stdout.push_str(&String::from_utf8_lossy(&message));
                }
                bollard::container::LogOutput::StdErr { message } => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }
    }

    let inspect = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|e| CoastError::docker(format!("Failed to inspect exec '{}': {e}", exec.id)))?;

    Ok(ExecResponse {
        exit_code: inspect.exit_code.unwrap_or(1) as i32,
        stdout,
        stderr,
    })
}

/// Handle an exec request.
///
/// Steps:
/// 1. Verify the instance exists and is running.
/// 2. Exec the command inside the coast container.
/// 3. Return stdout, stderr, and exit code.
pub async fn handle(req: ExecRequest, state: &AppState) -> Result<ExecResponse> {
    info!(name = %req.name, project = %req.project, command = ?req.command, "handling exec request");

    // Phase 1: DB read (locked)
    let (container_id, build_id) = {
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

        (
            instance.container_id.ok_or_else(|| {
                CoastError::state(format!(
                    "Instance '{}' has no container ID. This may indicate a corrupt state. \
                     Try `coast rm {}` and `coast run` again.",
                    req.name, req.name
                ))
            })?,
            instance.build_id.clone(),
        )
    };

    // Phase 2: Docker operations (unlocked)
    let command = resolve_command(&req.command);

    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let exec_response = if let Some(service) = req.service.as_deref() {
        let user_spec = if req.root { None } else { Some(host_uid_gid()) };
        let script = build_service_exec_script(
            &req.project,
            build_id.as_deref(),
            service,
            &command,
            user_spec.as_deref(),
        );
        exec_in_container(
            &docker,
            &container_id,
            &["sh".to_string(), "-c".to_string(), script],
            None,
        )
        .await?
    } else {
        let user_spec = if req.root { None } else { Some(host_uid_gid()) };
        exec_in_container(&docker, &container_id, &command, user_spec.as_deref()).await?
    };

    info!(
        name = %req.name,
        service = ?req.service,
        exit_code = exec_response.exit_code,
        "exec completed"
    );

    Ok(exec_response)
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
            service: None,
            root: false,
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
            service: None,
            root: false,
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
            service: None,
            root: false,
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
            service: None,
            root: false,
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
            service: None,
            root: false,
            command: vec!["bash".to_string()],
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no container ID"));
    }
}
