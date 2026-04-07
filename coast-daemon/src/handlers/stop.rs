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

/// Check whether the given instance status allows stopping.
///
/// Returns `Ok(())` for `Running`, `CheckedOut`, and `Idle`. Returns an error
/// for `Stopped` (already stopped) and transitional states (`Provisioning`,
/// `Assigning`, `Starting`, `Stopping`, `Unassigning`). `Enqueued` is handled
/// separately by the caller (removed instead of stopped).
fn validate_stoppable(status: &InstanceStatus, name: &str) -> Result<()> {
    match status {
        InstanceStatus::Running | InstanceStatus::CheckedOut | InstanceStatus::Idle => Ok(()),
        InstanceStatus::Stopped => Err(CoastError::state(format!(
            "Instance '{name}' is already stopped. Run `coast start {name}` to start it."
        ))),
        InstanceStatus::Provisioning
        | InstanceStatus::Assigning
        | InstanceStatus::Starting
        | InstanceStatus::Stopping
        | InstanceStatus::Unassigning => Err(CoastError::state(format!(
            "Instance '{name}' is currently {status}. Wait for the operation to complete."
        ))),
        InstanceStatus::Enqueued => Err(CoastError::state(format!(
            "Instance '{name}' is {status} and cannot be stopped directly."
        ))),
    }
}

/// Kill all socat processes for an instance and clear their PIDs in the DB.
fn kill_instance_socat_processes(
    db: &crate::state::StateDb,
    project: &str,
    name: &str,
) -> Result<()> {
    let port_allocs = db.get_port_allocations(project, name)?;
    for alloc in &port_allocs {
        if let Some(pid) = alloc.socat_pid {
            if let Err(e) = crate::port_manager::kill_socat(pid as u32) {
                warn!(pid = pid, error = %e, "failed to kill socat process");
            } else if let Err(e) = db.update_socat_pid(project, name, &alloc.logical_name, None) {
                warn!(
                    logical_name = %alloc.logical_name,
                    error = %e,
                    "failed to clear socat pid after killing process"
                );
            }
        }
    }
    Ok(())
}

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
            if let Some(docker) = state.docker.as_ref() {
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
        if inst.status == InstanceStatus::Enqueued {
            db.delete_instance(&req.project, &req.name)?;
            drop(db);
            state.emit_event(CoastEvent::InstanceRemoved {
                name: req.name.clone(),
                project: req.project.clone(),
            });
            return Ok(StopResponse { name: req.name });
        }
        validate_stoppable(&inst.status, &req.name)?;
        if inst.status == InstanceStatus::CheckedOut {
            super::clear_checked_out_state(
                &db,
                &req.project,
                &req.name,
                &InstanceStatus::Stopping,
            )?;
        } else {
            db.update_instance_status(&req.project, &req.name, &InstanceStatus::Stopping)?;
        }
        inst
    };

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "stopping".to_string(),
    });

    if instance.remote_host.is_some() {
        return handle_remote_stop(req, &instance, state, &progress).await;
    }

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
        if let Some(docker) = state.docker.as_ref() {
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
            if crate::bare_services::has_bare_services(&docker, container_id).await {
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
        if let Some(docker) = state.docker.as_ref() {
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
    kill_instance_socat_processes(&db, &req.project, &req.name)?;
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

/// Handle stop for a remote coast instance.
///
/// Forwards the stop to coast-service, then kills local SSH tunnel processes
/// and updates the shadow instance status.
async fn handle_remote_stop(
    req: StopRequest,
    instance: &coast_core::types::CoastInstance,
    state: &AppState,
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<StopResponse> {
    info!(
        name = %req.name,
        remote_host = ?instance.remote_host,
        "stopping remote instance"
    );

    emit(
        progress,
        BuildProgressEvent::build_plan(vec![
            "Stopping file sync".into(),
            "Forwarding stop to remote".into(),
            "Killing tunnels".into(),
        ]),
    );

    emit(
        progress,
        BuildProgressEvent::started("Stopping file sync", 1, 3),
    );

    let session_name = super::remote::sync::mutagen_session_name(&req.project, &req.name);
    let shell_container = format!("{}-coasts-{}-shell", req.project, req.name);
    if let Some(docker) = state.docker.as_ref() {
        let _ =
            super::remote::sync::stop_mutagen_in_shell(&docker, &shell_container, &session_name)
                .await;
    }

    emit(
        progress,
        BuildProgressEvent::done("Stopping file sync", "ok"),
    );

    emit(
        progress,
        BuildProgressEvent::started("Forwarding stop to remote", 2, 3),
    );

    if let Ok(remote_config) =
        super::remote::resolve_remote_for_instance(&req.project, &req.name, state).await
    {
        if let Ok(client) = super::remote::RemoteClient::connect(&remote_config).await {
            let _ = super::remote::forward::forward_stop(&client, &req).await;
        }
    }

    emit(
        progress,
        BuildProgressEvent::done("Forwarding stop to remote", "ok"),
    );

    // Kill local SSH tunnel forwarding processes
    emit(
        progress,
        BuildProgressEvent::started("Killing tunnels", 3, 3),
    );

    let db = state.db.lock().await;
    kill_instance_socat_processes(&db, &req.project, &req.name)?;
    db.update_instance_status(&req.project, &req.name, &InstanceStatus::Stopped)?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "stopped".to_string(),
    });

    emit(progress, BuildProgressEvent::done("Killing tunnels", "ok"));

    info!(name = %req.name, "remote instance stopped");

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
            remote_host: None,
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
        unsafe {
            std::env::remove_var("WSL_DISTRO_NAME");
            std::env::remove_var("WSL_INTEROP");
        }
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "checked-out",
                "my-app",
                InstanceStatus::CheckedOut,
            ))
            .unwrap();
            db.insert_port_allocation(
                "my-app",
                "checked-out",
                &coast_core::types::PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 50000,
                    is_primary: false,
                },
            )
            .unwrap();
            db.update_socat_pid("my-app", "checked-out", "web", Some(4_194_304))
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
        let allocs = db.get_port_allocations("my-app", "checked-out").unwrap();
        assert!(allocs[0].socat_pid.is_none());
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

    #[tokio::test]
    async fn test_stop_enqueued_instance_removes_it() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "queued-inst",
                "my-app",
                InstanceStatus::Enqueued,
            ))
            .unwrap();
        }

        let req = StopRequest {
            name: "queued-inst".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "queued-inst");

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "queued-inst").unwrap();
        assert!(instance.is_none());
    }

    // --- validate_stoppable tests ---

    #[test]
    fn test_validate_stoppable_running_ok() {
        assert!(validate_stoppable(&InstanceStatus::Running, "inst").is_ok());
    }

    #[test]
    fn test_validate_stoppable_checked_out_ok() {
        assert!(validate_stoppable(&InstanceStatus::CheckedOut, "inst").is_ok());
    }

    #[test]
    fn test_validate_stoppable_idle_ok() {
        assert!(validate_stoppable(&InstanceStatus::Idle, "inst").is_ok());
    }

    #[test]
    fn test_validate_stoppable_stopped_errors() {
        let err = validate_stoppable(&InstanceStatus::Stopped, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already stopped"));
    }

    #[test]
    fn test_validate_stoppable_provisioning_errors() {
        let err = validate_stoppable(&InstanceStatus::Provisioning, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_stoppable_assigning_errors() {
        let err = validate_stoppable(&InstanceStatus::Assigning, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_stoppable_starting_errors() {
        let err = validate_stoppable(&InstanceStatus::Starting, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_stoppable_stopping_errors() {
        let err = validate_stoppable(&InstanceStatus::Stopping, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    // --- kill_instance_socat_processes tests ---

    #[tokio::test]
    async fn test_kill_socat_clears_pids() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&make_instance("inst", "proj", InstanceStatus::Running))
            .unwrap();
        db.insert_port_allocation(
            "proj",
            "inst",
            &coast_core::types::PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 50000,
                is_primary: false,
            },
        )
        .unwrap();
        db.insert_port_allocation(
            "proj",
            "inst",
            &coast_core::types::PortMapping {
                logical_name: "api".to_string(),
                canonical_port: 8080,
                dynamic_port: 50001,
                is_primary: false,
            },
        )
        .unwrap();
        // Use PID values that won't exist (ESRCH → treated as success by kill_socat)
        db.update_socat_pid("proj", "inst", "web", Some(4_194_304))
            .unwrap();
        db.update_socat_pid("proj", "inst", "api", Some(4_194_305))
            .unwrap();

        kill_instance_socat_processes(&db, "proj", "inst").unwrap();

        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        for alloc in &allocs {
            assert!(
                alloc.socat_pid.is_none(),
                "pid should be cleared for {}",
                alloc.logical_name
            );
        }
    }

    #[tokio::test]
    async fn test_kill_socat_no_pids_is_noop() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&make_instance("inst", "proj", InstanceStatus::Running))
            .unwrap();
        db.insert_port_allocation(
            "proj",
            "inst",
            &coast_core::types::PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 50000,
                is_primary: false,
            },
        )
        .unwrap();

        // No socat PIDs set — should be a no-op
        kill_instance_socat_processes(&db, "proj", "inst").unwrap();

        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        assert!(allocs[0].socat_pid.is_none());
    }

    #[tokio::test]
    async fn test_kill_socat_no_allocations_is_noop() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&make_instance("inst", "proj", InstanceStatus::Running))
            .unwrap();

        // No port allocations at all — should be a no-op
        kill_instance_socat_processes(&db, "proj", "inst").unwrap();
    }
}
