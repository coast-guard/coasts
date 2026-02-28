/// Handler for the `coast logs` command.
///
/// Retrieves logs from inside a coast container by executing
/// `docker compose logs` inside the coast container.
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{LogsRequest, LogsResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

use super::compose_context_for_build;

const DEFAULT_LOG_TAIL_LINES: u32 = 200;

fn resolve_tail_arg(req: &LogsRequest) -> String {
    if req.tail_all {
        "all".to_string()
    } else {
        req.tail.unwrap_or(DEFAULT_LOG_TAIL_LINES).to_string()
    }
}

async fn resolve_logs_target(
    req: &LogsRequest,
    state: &AppState,
) -> Result<(String, Option<String>)> {
    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?;
    let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
        name: req.name.clone(),
        project: req.project.clone(),
    })?;

    if instance.status == InstanceStatus::Stopped {
        return Err(CoastError::state(format!(
            "Instance '{}' is stopped. Run `coast start {}` to view logs.",
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

    let cid = instance.container_id.clone().ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{}' has no container ID. This may indicate a corrupt state. \
             Try `coast rm {}` and `coast run` again.",
            req.name, req.name
        ))
    })?;
    Ok((cid, instance.build_id.clone()))
}

/// Handle a logs request.
pub async fn handle(req: LogsRequest, state: &AppState) -> Result<LogsResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        service = ?req.service,
        tail = ?req.tail,
        tail_all = req.tail_all,
        follow = req.follow,
        "handling logs request"
    );

    // Phase 1: DB read (locked)
    let (container_id, build_id) = resolve_logs_target(&req, state).await?;

    // Phase 2: Docker operations (unlocked)
    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let is_bare = crate::bare_services::has_bare_services(docker, &container_id).await;

    let cmd_parts = if is_bare {
        let tail_cmd = crate::bare_services::generate_logs_command(
            req.service.as_deref(),
            req.tail,
            req.tail_all,
            req.follow,
        );
        vec!["sh".to_string(), "-c".to_string(), tail_cmd]
    } else {
        let tail_arg = resolve_tail_arg(&req);
        let mut subcmd_parts = vec!["logs".to_string(), "--tail".to_string(), tail_arg];
        if req.follow {
            subcmd_parts.push("--follow".to_string());
        }
        if let Some(ref service) = req.service {
            subcmd_parts.push(service.clone());
        }
        let subcmd = subcmd_parts.join(" ");
        let ctx = compose_context_for_build(&req.project, build_id.as_deref());
        ctx.compose_shell(&subcmd)
    };
    let cmd_refs: Vec<&str> = cmd_parts.iter().map(std::string::String::as_str).collect();

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let exec_result = runtime
        .exec_in_coast(&container_id, &cmd_refs)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to retrieve logs for instance '{}': {}",
                req.name, e
            ))
        })?;

    if !exec_result.success() {
        return Err(CoastError::docker(format!(
            "logs command failed in instance '{}' (exit code {}): {}",
            req.name, exec_result.exit_code, exec_result.stderr
        )));
    }

    let output = if exec_result.stdout.is_empty() {
        exec_result.stderr.clone()
    } else {
        exec_result.stdout.clone()
    };

    info!(
        name = %req.name,
        output_len = output.len(),
        "logs retrieved"
    );

    Ok(LogsResponse { output })
}

/// Handle a logs request with incremental streaming chunks.
///
/// Used by the Unix socket server's streaming mode when `follow` is true.
pub async fn handle_with_progress(
    req: LogsRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<LogsResponse>,
) -> Result<LogsResponse> {
    if !req.follow {
        return handle(req, state).await;
    }

    info!(
        name = %req.name,
        project = %req.project,
        service = ?req.service,
        tail = ?req.tail,
        tail_all = req.tail_all,
        "handling streaming logs request"
    );

    let (container_id, build_id) = resolve_logs_target(&req, state).await?;
    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let is_bare = crate::bare_services::has_bare_services(docker, &container_id).await;

    let cmd_parts = if is_bare {
        let tail_cmd = crate::bare_services::generate_logs_command(
            req.service.as_deref(),
            req.tail,
            req.tail_all,
            true,
        );
        vec!["sh".to_string(), "-c".to_string(), tail_cmd]
    } else {
        let tail_arg = resolve_tail_arg(&req);
        let mut subcmd_parts = vec!["logs".to_string(), "--tail".to_string(), tail_arg];
        subcmd_parts.push("--follow".to_string());
        if let Some(ref service) = req.service {
            subcmd_parts.push(service.clone());
        }
        let subcmd = subcmd_parts.join(" ");
        let ctx = compose_context_for_build(&req.project, build_id.as_deref());
        ctx.compose_shell(&subcmd)
    };

    let exec_options = CreateExecOptions {
        cmd: Some(cmd_parts),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };
    let exec = docker
        .create_exec(&container_id, exec_options)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to create logs stream for instance '{}': {}",
                req.name, e
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
                "Failed to start logs stream for instance '{}': {}",
                req.name, e
            ))
        })?;

    if let StartExecResults::Attached { mut output, .. } = output {
        while let Some(chunk) = output.next().await {
            match chunk {
                Ok(
                    bollard::container::LogOutput::StdOut { message }
                    | bollard::container::LogOutput::StdErr { message },
                ) => {
                    let text = String::from_utf8_lossy(&message).to_string();
                    if progress.send(LogsResponse { output: text }).await.is_err() {
                        info!(name = %req.name, "logs stream receiver dropped");
                        break;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    return Err(CoastError::docker(format!(
                        "Failed while streaming logs for instance '{}': {}",
                        req.name, e
                    )));
                }
            }
        }
    } else {
        return Err(CoastError::docker(format!(
            "Failed to attach logs stream for instance '{}'.",
            req.name
        )));
    }

    Ok(LogsResponse {
        output: String::new(),
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
    async fn test_logs_running_instance_no_docker() {
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

        let req = LogsRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Docker is not available"));
    }

    #[tokio::test]
    async fn test_logs_with_service_filter_no_docker() {
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

        let req = LogsRequest {
            name: "feat-b".to_string(),
            project: "my-app".to_string(),
            service: Some("web".to_string()),
            tail: Some(50),
            tail_all: false,
            follow: false,
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Docker is not available"));
    }

    #[tokio::test]
    async fn test_logs_nonexistent_instance() {
        let state = test_state();
        let req = LogsRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_logs_stopped_instance_fails() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "stopped",
                InstanceStatus::Stopped,
                Some("cid"),
            ))
            .unwrap();
        }

        let req = LogsRequest {
            name: "stopped".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
    }

    #[test]
    fn test_resolve_tail_arg_defaults_to_configured_lines() {
        let req = LogsRequest {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        assert_eq!(resolve_tail_arg(&req), DEFAULT_LOG_TAIL_LINES.to_string());
    }

    #[test]
    fn test_resolve_tail_arg_uses_explicit_lines() {
        let req = LogsRequest {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: Some(50),
            tail_all: false,
            follow: false,
        };
        assert_eq!(resolve_tail_arg(&req), "50");
    }

    #[test]
    fn test_resolve_tail_arg_all_overrides_lines() {
        let req = LogsRequest {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            service: None,
            tail: Some(50),
            tail_all: true,
            follow: true,
        };
        assert_eq!(resolve_tail_arg(&req), "all");
    }
}
