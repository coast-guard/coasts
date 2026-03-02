/// Handler for the `coast rm` command.
///
/// Removes a coast instance: stops if running, removes the container,
/// deletes isolated volumes, kills socat processes, deallocates ports,
/// and removes the instance from the state DB.
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, RmRequest, RmResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

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
#[allow(clippy::cognitive_complexity)]
pub async fn handle(req: RmRequest, state: &AppState) -> Result<RmResponse> {
    info!(name = %req.name, project = %req.project, "handling rm request");

    // Phase 1: Validate (locked)
    let instance = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&req.project, &req.name)?;
        let Some(inst) = inst else {
            // Instance not in DB — check for a dangling Docker container and
            // clean it up so `rm` always leaves a clean state.
            let expected = format!("{}-coasts-{}", req.project, req.name);
            if let Some(ref docker) = state.docker {
                if docker.inspect_container(&expected, None).await.is_ok() {
                    warn!(
                        name = %req.name,
                        project = %req.project,
                        container = %expected,
                        "removing dangling container during rm"
                    );
                    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
                    if let Err(e) = runtime.remove_coast_container(&expected).await {
                        warn!(container = %expected, error = %e, "failed to remove dangling container");
                    }
                    let vol_prefix = format!("coast--{}--", req.name);
                    if let Ok(volumes) = docker.list_volumes::<String>(None).await {
                        if let Some(vols) = volumes.volumes {
                            for vol in vols {
                                if vol.name.starts_with(&vol_prefix) {
                                    let _ = docker.remove_volume(&vol.name, None).await;
                                    info!(volume = %vol.name, "removed dangling isolated volume");
                                }
                            }
                        }
                    }
                    let cache_vol =
                        coast_docker::dind::dind_cache_volume_name(&req.project, &req.name);
                    let _ = docker.remove_volume(&cache_vol, None).await;
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

    // Set transitional status so the UI shows "stopping" pill during teardown
    if instance.status == InstanceStatus::Running || instance.status == InstanceStatus::CheckedOut {
        let db = state.db.lock().await;
        let _ = db.update_instance_status(&req.project, &req.name, &InstanceStatus::Stopping);
        drop(db);
        state.emit_event(CoastEvent::InstanceStatusChanged {
            name: req.name.clone(),
            project: req.project.clone(),
            status: "stopping".to_string(),
        });
    }

    // Phase 2: Docker operations (unlocked)
    if instance.status == InstanceStatus::Running || instance.status == InstanceStatus::CheckedOut {
        if let Some(ref container_id) = instance.container_id {
            if let Some(ref docker) = state.docker {
                let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
                let _ = runtime
                    .exec_in_coast(container_id, &["docker", "compose", "down"])
                    .await;
                let _ = runtime.stop_coast_container(container_id).await;
            }
        }
        info!(name = %req.name, "stopped running instance before removal");
    }

    // Step 3: Remove the coast container
    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
            if let Err(e) = runtime.remove_coast_container(container_id).await {
                warn!(container_id = %container_id, error = %e, "failed to remove container");
            }
        }
    }

    // Step 4: Delete isolated volumes (best-effort, requires Docker)
    if let Some(ref docker) = state.docker {
        // Volume names follow the pattern: coast--{instance}--{volume_name}
        let prefix = format!("coast--{}--", req.name);
        if let Ok(volumes) = docker.list_volumes::<String>(None).await {
            if let Some(volumes_list) = volumes.volumes {
                for vol in volumes_list {
                    if vol.name.starts_with(&prefix) {
                        let _ = docker.remove_volume(&vol.name, None).await;
                        info!(volume = %vol.name, "removed isolated volume");
                    }
                }
            }
        }
    }

    // Phase 3: DB cleanup (locked)
    let db = state.db.lock().await;
    let port_allocs = db.get_port_allocations(&req.project, &req.name)?;
    for alloc in &port_allocs {
        if let Some(pid) = alloc.socat_pid {
            if let Err(e) = crate::port_manager::kill_socat(pid as u32) {
                warn!(pid = pid, error = %e, "failed to kill socat process");
            }
        }
    }

    // Step 6: Deallocate ports
    db.delete_port_allocations(&req.project, &req.name)?;

    // Step 6b: Clean up agent shells
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
    use coast_core::types::{CoastInstance, PortMapping, RuntimeType};

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
}
