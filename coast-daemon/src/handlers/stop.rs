/// Handler for the `coast stop` command.
///
/// Stops a running coast instance: runs `docker compose down` inside the coast
/// container, stops the coast container itself, kills socat processes,
/// and updates the state DB.
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, StopRequest, StopResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Emit a progress event if a sender is provided.
fn emit(tx: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>, event: BuildProgressEvent) {
    if let Some(tx) = tx {
        let _ = tx.try_send(event);
    }
}

const TOTAL_STOP_STEPS: u32 = 3;

/// Handle a stop request with optional progress streaming.
///
/// Steps:
/// 1. Verify the instance exists and is running (or checked_out).
/// 2. Run `docker compose down` inside the coast container.
/// 3. Stop the coast container on the host daemon.
/// 4. Kill all socat processes for this instance.
/// 5. Update instance status to "stopped" in state DB.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: StopRequest,
    state: &AppState,
    progress: Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<StopResponse> {
    info!(name = %req.name, project = %req.project, "handling stop request");

    // Phase 1: Validate and set transitional state (locked)
    let instance = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&req.project, &req.name)?;
        let Some(inst) = inst else {
            // Instance not in DB — check if a dangling Docker container exists.
            // If so, treat stop as a silent no-op (use `coast rm` to clean up).
            let expected = format!("{}-coasts-{}", req.project, req.name);
            if let Some(ref docker) = state.docker {
                if docker.inspect_container(&expected, None).await.is_ok() {
                    warn!(
                        name = %req.name,
                        project = %req.project,
                        container = %expected,
                        "dangling container found during stop, treating as no-op"
                    );
                    return Ok(StopResponse { name: req.name });
                }
            }
            return Err(CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            });
        };
        if inst.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is already stopped. Run `coast start {}` to start it.",
                req.name, req.name
            )));
        }
        if inst.status == InstanceStatus::Provisioning
            || inst.status == InstanceStatus::Assigning
            || inst.status == InstanceStatus::Starting
            || inst.status == InstanceStatus::Stopping
        {
            return Err(CoastError::state(format!(
                "Instance '{}' is currently {}. Wait for the operation to complete.",
                req.name, inst.status
            )));
        }
        db.update_instance_status(&req.project, &req.name, &InstanceStatus::Stopping)?;
        inst
    };

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "stopping".to_string(),
    });

    emit(
        &progress,
        BuildProgressEvent::build_plan(vec![
            "Running compose down".into(),
            "Stopping container".into(),
            "Killing socat processes".into(),
        ]),
    );

    // Phase 2: Docker operations (unlocked)
    // Step 1: Compose down
    emit(
        &progress,
        BuildProgressEvent::started("Running compose down", 1, TOTAL_STOP_STEPS),
    );

    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

            let health_timeout = tokio::time::Duration::from_secs(10);
            let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
            match tokio::time::timeout(health_timeout, health_check).await {
                Ok(Ok(r)) if r.success() => {
                    info!("stop: inner daemon healthy");
                }
                Ok(Ok(r)) => {
                    warn!(
                        name = %req.name,
                        exit_code = r.exit_code,
                        "inner daemon unhealthy, skipping compose down"
                    );
                }
                Ok(Err(e)) => {
                    warn!(
                        name = %req.name,
                        error = %e,
                        "cannot reach inner daemon, skipping compose down"
                    );
                }
                Err(_) => {
                    warn!(
                        name = %req.name,
                        timeout_secs = health_timeout.as_secs(),
                        "inner daemon unresponsive, skipping compose down"
                    );
                }
            }

            // Stop bare services if the supervisor directory exists
            if crate::bare_services::has_bare_services(docker, container_id).await {
                let stop_cmd = crate::bare_services::generate_stop_command();
                let _ = rt
                    .exec_in_coast(container_id, &["sh", "-c", &stop_cmd])
                    .await;
            }

            let ctx = super::compose_context_for_build(&req.project, instance.build_id.as_deref());
            let down_cmd = ctx.compose_shell("down -t 2");
            let down_refs: Vec<&str> = down_cmd.iter().map(std::string::String::as_str).collect();
            let _ = rt.exec_in_coast(container_id, &down_refs).await;
        }
    }
    emit(
        &progress,
        BuildProgressEvent::item("Running compose down", "compose down", "ok"),
    );

    // Step 2: Stop the coast container
    emit(
        &progress,
        BuildProgressEvent::started("Stopping container", 2, TOTAL_STOP_STEPS),
    );
    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
            if let Err(e) = runtime.stop_coast_container(container_id).await {
                warn!(container_id = %container_id, error = %e, "failed to stop container, it may already be stopped");
            }
        }
    }
    emit(
        &progress,
        BuildProgressEvent::item("Stopping container", "container", "ok"),
    );

    // Phase 3: Final DB operations (locked)
    // Step 3: Kill socat processes
    emit(
        &progress,
        BuildProgressEvent::started("Killing socat processes", 3, TOTAL_STOP_STEPS),
    );
    let db = state.db.lock().await;
    let port_allocs = db.get_port_allocations(&req.project, &req.name)?;
    for alloc in &port_allocs {
        if let Some(pid) = alloc.socat_pid {
            if let Err(e) = crate::port_manager::kill_socat(pid as u32) {
                warn!(pid = pid, error = %e, "failed to kill socat process");
            }
        }
    }
    emit(
        &progress,
        BuildProgressEvent::item("Killing socat processes", "socat", "ok"),
    );

    // Clean up agent shells: kill PTY processes and remove DB records
    if let Ok(shells) = db.list_agent_shells(&req.project, &req.name) {
        let mut exec_sessions = state.exec_sessions.lock().await;
        for shell in &shells {
            if let Some(ref sid) = shell.session_id {
                if let Some(session) = exec_sessions.remove(sid) {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(session.child_pid),
                        nix::sys::signal::Signal::SIGHUP,
                    );
                    unsafe {
                        nix::libc::close(session.master_read_fd);
                        nix::libc::close(session.master_write_fd);
                    }
                }
            }
        }
        let _ = db.delete_agent_shells_for_instance(&req.project, &req.name);
    }

    db.update_instance_status(&req.project, &req.name, &InstanceStatus::Stopped)?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "stopped".to_string(),
    });

    info!(name = %req.name, project = %req.project, "instance stopped");

    Ok(StopResponse { name: req.name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    #[tokio::test]
    async fn test_stop_running_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", InstanceStatus::Running))
                .unwrap();
        }

        let req = StopRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "feat-a");

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Stopped);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_instance() {
        let state = test_state();
        let req = StopRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_stop_already_stopped_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "stopped-inst",
                "my-app",
                InstanceStatus::Stopped,
            ))
            .unwrap();
        }

        let req = StopRequest {
            name: "stopped-inst".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already stopped"));
    }

    #[tokio::test]
    async fn test_stop_checked_out_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "checked-out",
                "my-app",
                InstanceStatus::CheckedOut,
            ))
            .unwrap();
        }

        let req = StopRequest {
            name: "checked-out".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "checked-out").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Stopped);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_no_docker_returns_not_found() {
        // Without a Docker client the dangling check is skipped,
        // so we still get InstanceNotFound.
        let state = test_state();
        assert!(state.docker.is_none());

        let req = StopRequest {
            name: "ghost".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }
}
