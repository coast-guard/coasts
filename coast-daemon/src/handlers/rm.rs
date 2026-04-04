/// Handler for the `coast rm` command.
///
/// Removes a coast instance: stops if running, removes the container,
/// deletes isolated volumes, kills socat processes, deallocates ports,
/// and removes the instance from the state DB.
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, RmRequest, RmResponse};
use coast_core::types::{CoastInstance, InstanceStatus};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Check for a dangling Docker container (no DB record) and clean it up.
///
/// Returns `true` if a dangling container was found and removed.
async fn cleanup_dangling_container(
    docker: &bollard::Docker,
    project: &str,
    name: &str,
) -> Result<bool> {
    let expected = format!("{project}-coasts-{name}");
    if docker.inspect_container(&expected, None).await.is_err() {
        return Ok(false);
    }

    warn!(
        name = %name,
        project = %project,
        container = %expected,
        "removing dangling container during rm"
    );
    remove_container(docker, &expected).await;
    remove_isolated_volumes(docker, project, name).await;
    Ok(true)
}

/// Set transitional "stopping" status so the UI shows the correct pill during teardown.
async fn set_stopping_transition(
    instance: &CoastInstance,
    state: &AppState,
    project: &str,
    name: &str,
) -> Result<()> {
    let db = state.db.lock().await;
    if instance.status == InstanceStatus::CheckedOut {
        super::clear_checked_out_state(&db, project, name, &InstanceStatus::Stopping)?;
    } else {
        let _ = db.update_instance_status(project, name, &InstanceStatus::Stopping);
    }
    drop(db);
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: name.to_string(),
        project: project.to_string(),
        status: "stopping".to_string(),
    });
    Ok(())
}

/// Kill agent shell processes, close FDs, and delete shell records from the DB.
///
/// Takes the DB lock internally so the `StateDb` reference (which is not `Send`)
/// is not held across the exec_sessions `.await`.
async fn cleanup_agent_shells(state: &AppState, project: &str, name: &str) {
    let shells = {
        let db = state.db.lock().await;
        match db.list_agent_shells(project, name) {
            Ok(s) => s,
            Err(_) => return,
        }
    };
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
    drop(exec_sessions);
    let db = state.db.lock().await;
    let _ = db.delete_agent_shells_for_instance(project, name);
}

/// Stop compose services and the coast container for a running/checked-out instance.
async fn stop_running_services(docker: &bollard::Docker, container_id: &str) {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let _ = runtime
        .exec_in_coast(container_id, &["docker", "compose", "down"])
        .await;
    let _ = runtime.stop_coast_container(container_id).await;
}

/// Remove the coast container from the host daemon.
async fn remove_container(docker: &bollard::Docker, container_id: &str) {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    if let Err(e) = runtime.remove_coast_container(container_id).await {
        warn!(container_id = %container_id, error = %e, "failed to remove container");
    }
}

/// Delete isolated volumes matching `coast--{instance}--*` and the cache volume.
async fn remove_isolated_volumes(docker: &bollard::Docker, project: &str, name: &str) {
    let prefix = format!("coast--{name}--");
    if let Ok(volumes) = docker.list_volumes::<String>(None).await {
        if let Some(vols) = volumes.volumes {
            for vol in vols {
                if vol.name.starts_with(&prefix) {
                    let _ = docker.remove_volume(&vol.name, None).await;
                    info!(volume = %vol.name, "removed isolated volume");
                }
            }
        }
    }
    let cache_vol = coast_docker::dind::dind_cache_volume_name(project, name);
    let _ = docker.remove_volume(&cache_vol, None).await;
}

/// Kill socat processes and deallocate ports from the DB.
fn cleanup_socat_and_ports(db: &crate::state::StateDb, project: &str, name: &str) -> Result<()> {
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
    db.delete_port_allocations(project, name)?;
    Ok(())
}

/// Run all Docker teardown steps: stop services, remove container, delete volumes.
async fn docker_teardown(instance: &CoastInstance, state: &AppState, project: &str, name: &str) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };
    let is_active =
        instance.status == InstanceStatus::Running || instance.status == InstanceStatus::CheckedOut;
    if is_active {
        if let Some(ref cid) = instance.container_id {
            stop_running_services(&docker, cid).await;
        }
        info!(name = %name, "stopped running instance before removal");
    }
    if let Some(ref cid) = instance.container_id {
        remove_container(&docker, cid).await;
    }
    remove_isolated_volumes(&docker, project, name).await;
}

/// Handle an rm request.
///
/// Steps:
/// 1. Verify the instance exists.
/// 2. If running or checked_out, stop it first.
/// 3. Remove the coast container from the host daemon.
/// 4. Delete isolated volumes for this instance.
/// 5. Kill any remaining socat processes.
/// 6. Deallocate ports from state DB.
/// 7. Delete instance from state DB.
///
/// IMPORTANT: `coast rm` does NOT delete shared service data.
/// Use `coast shared-services rm` for that.
pub async fn handle(req: RmRequest, state: &AppState) -> Result<RmResponse> {
    info!(name = %req.name, project = %req.project, "handling rm request");

    // Phase 1: Validate (locked)
    let instance = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&req.project, &req.name)?;
        let Some(inst) = inst else {
            if let Some(docker) = state.docker.as_ref() {
                if cleanup_dangling_container(&docker, &req.project, &req.name).await? {
                    return Ok(RmResponse { name: req.name });
                }
            }
            return Err(CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            });
        };
        inst
    };

    if instance.status == InstanceStatus::Enqueued {
        let db = state.db.lock().await;
        db.delete_port_allocations(&req.project, &req.name)?;
        db.delete_instance(&req.project, &req.name)?;
        return Ok(RmResponse { name: req.name });
    }

    if instance.status == InstanceStatus::Running || instance.status == InstanceStatus::CheckedOut {
        set_stopping_transition(&instance, state, &req.project, &req.name).await?;
    }

    // Phase 2: Docker operations (unlocked)
    docker_teardown(&instance, state, &req.project, &req.name).await;

    // Phase 3: DB cleanup (locked)
    {
        let db = state.db.lock().await;
        cleanup_socat_and_ports(&db, &req.project, &req.name)?;
    }
    cleanup_agent_shells(state, &req.project, &req.name).await;
    let db = state.db.lock().await;

    // Step 7: Delete instance from state DB
    db.delete_instance(&req.project, &req.name)?;

    info!(
        name = %req.name,
        project = %req.project,
        "instance removed. Note: Shared service data (volumes) has been preserved. \
         Use `coast shared-services rm <service>` to remove shared services."
    );

    Ok(RmResponse { name: req.name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{PortMapping, RuntimeType};

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
    async fn test_rm_stopped_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", InstanceStatus::Stopped))
                .unwrap();
        }

        let req = RmRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "feat-a");

        // Verify removed from DB
        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feat-a").unwrap();
        assert!(instance.is_none());
    }

    #[tokio::test]
    async fn test_rm_running_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "running-one",
                "my-app",
                InstanceStatus::Running,
            ))
            .unwrap();
        }

        let req = RmRequest {
            name: "running-one".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "running-one").unwrap();
        assert!(instance.is_none());
    }

    #[tokio::test]
    async fn test_rm_nonexistent_instance() {
        let state = test_state();
        let req = RmRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_rm_deallocates_ports() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "with-ports",
                "my-app",
                InstanceStatus::Stopped,
            ))
            .unwrap();
            db.insert_port_allocation(
                "my-app",
                "with-ports",
                &PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 52340,
                    is_primary: false,
                },
            )
            .unwrap();
        }

        let req = RmRequest {
            name: "with-ports".to_string(),
            project: "my-app".to_string(),
        };
        assert!(handle(req, &state).await.is_ok());

        let db = state.db.lock().await;
        let ports = db.get_port_allocations("my-app", "with-ports").unwrap();
        assert!(ports.is_empty());
    }

    #[tokio::test]
    async fn test_rm_nonexistent_no_docker_returns_not_found() {
        // Without a Docker client the dangling check is skipped,
        // so we still get InstanceNotFound.
        let state = test_state();
        assert!(state.docker.is_none());

        let req = RmRequest {
            name: "ghost".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_rm_enqueued_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "queued-one",
                "my-app",
                InstanceStatus::Enqueued,
            ))
            .unwrap();
        }

        let req = RmRequest {
            name: "queued-one".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "queued-one");

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "queued-one").unwrap();
        assert!(instance.is_none());
    }

    // --- set_stopping_transition tests ---

    #[tokio::test]
    async fn test_stopping_transition_running() {
        let state = test_state();
        let instance = make_instance("feat-a", "my-app", InstanceStatus::Running);
        {
            let db = state.db.lock().await;
            db.insert_instance(&instance).unwrap();
        }

        set_stopping_transition(&instance, &state, "my-app", "feat-a")
            .await
            .unwrap();

        let db = state.db.lock().await;
        let updated = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(updated.status, InstanceStatus::Stopping);
    }

    #[tokio::test]
    async fn test_stopping_transition_emits_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        let instance = make_instance("feat-c", "my-app", InstanceStatus::Running);
        {
            let db = state.db.lock().await;
            db.insert_instance(&instance).unwrap();
        }

        set_stopping_transition(&instance, &state, "my-app", "feat-c")
            .await
            .unwrap();

        let mut found = false;
        while let Ok(event) = rx.try_recv() {
            if let CoastEvent::InstanceStatusChanged { name, status, .. } = event {
                assert_eq!(name, "feat-c");
                assert_eq!(status, "stopping");
                found = true;
            }
        }
        assert!(found, "expected InstanceStatusChanged event");
    }
}
