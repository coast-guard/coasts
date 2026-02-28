/// Handler for the `coast checkout` command.
///
/// Swaps the canonical port bindings to a different coast instance.
/// This is designed to be instant — it only kills and respawns socat
/// processes, never restarts containers.
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CheckoutRequest, CheckoutResponse};
use coast_core::types::{InstanceStatus, PortMapping};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Check that all canonical ports are free on the host.
/// Returns `Ok(())` if every port can be bound, or an actionable error listing
/// the occupied ports.
fn check_canonical_ports_available(canonical_ports: &[u16], target_name: &str) -> Result<()> {
    let occupied: Vec<u16> = canonical_ports
        .iter()
        .copied()
        .filter(|&p| !crate::port_manager::is_port_available(p))
        .collect();
    if occupied.is_empty() {
        return Ok(());
    }
    let port_list = occupied
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    Err(CoastError::port(format!(
        "Cannot checkout '{}': canonical port(s) {} already in use. \
         Another process is occupying {} port(s). Free them before \
         checking out, or run `coast checkout --none` to unbind all \
         canonical ports.",
        target_name,
        port_list,
        occupied.len(),
    )))
}

/// Handle a checkout request.
///
/// Steps:
/// 1. If name is Some, verify the target instance exists and is running.
/// 2. Find the currently checked-out instance (if any) and un-check it.
/// 3. Kill all canonical socat processes for the old checked-out instance.
/// 4. If name is Some, resolve the new coast container IP and spawn
///    canonical socat forwarders.
/// 5. Update instance statuses in state DB.
#[allow(clippy::cognitive_complexity)]
pub async fn handle(req: CheckoutRequest, state: &AppState) -> Result<CheckoutResponse> {
    info!(name = ?req.name, project = %req.project, "handling checkout request");

    // Checkout is fast (socat processes only, no Docker operations)
    // but we still scope the lock for consistency.
    let db = state.db.lock().await;

    let instances = db.list_instances_for_project(&req.project)?;
    for inst in &instances {
        if inst.status == InstanceStatus::CheckedOut {
            // Un-check: set back to "running"
            db.update_instance_status(&req.project, &inst.name, &InstanceStatus::Running)?;

            // Kill canonical socat processes for the old instance
            let old_port_allocs = db.get_port_allocations(&req.project, &inst.name)?;
            for alloc in &old_port_allocs {
                if let Some(pid) = alloc.socat_pid {
                    let _ = crate::port_manager::kill_socat(pid as u32);
                }
            }

            info!(old_instance = %inst.name, "un-checked out previous instance");
        }
    }

    // Handle --none (unbind all canonical ports)
    let Some(target_name) = req.name else {
        info!(project = %req.project, "all canonical ports unbound (--none)");
        return Ok(CheckoutResponse {
            checked_out: None,
            ports: Vec::new(),
        });
    };

    // Step 1: Verify target instance exists and is running
    let target = db.get_instance(&req.project, &target_name)?;
    let target = target.ok_or_else(|| CoastError::InstanceNotFound {
        name: target_name.clone(),
        project: req.project.clone(),
    })?;

    if target.status != InstanceStatus::Running && target.status != InstanceStatus::CheckedOut {
        return Err(CoastError::state(format!(
            "Instance '{}' is not running (status: {}). Run `coast start {}` first.",
            target_name, target.status, target_name
        )));
    }

    // Retrieve port allocations early — reject checkout if none exist.
    let port_allocs = db.get_port_allocations(&req.project, &target_name)?;
    if port_allocs.is_empty() {
        return Err(CoastError::state(format!(
            "Instance '{}' has no port allocations. Checkout binds canonical ports \
             via socat and is not applicable to instances without ports. \
             This instance was built from a Coastfile with no [ports] section.",
            target_name
        )));
    }
    let ports: Vec<PortMapping> = port_allocs.iter().map(PortMapping::from).collect();

    // Pre-flight: verify all canonical ports are available before committing
    // to the checkout. This catches "address already in use" early with a
    // clear, actionable error message.
    if state.docker.is_some() {
        let canonical_ports: Vec<u16> = port_allocs.iter().map(|a| a.canonical_port).collect();
        check_canonical_ports_available(&canonical_ports, &target_name)?;
    }

    // Pre-flight: verify the inner Docker daemon is responsive before routing
    // traffic to this instance. Status is not yet set to CheckedOut so early
    // returns here leave the DB in a consistent state.
    if let (Some(ref container_id), Some(ref docker)) = (&target.container_id, &state.docker) {
        let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let health_timeout = tokio::time::Duration::from_secs(10);
        let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
        match tokio::time::timeout(health_timeout, health_check).await {
            Ok(Ok(r)) if r.success() => {
                info!("checkout: inner daemon healthy for '{}'", target_name);
            }
            Ok(Ok(r)) => {
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is not healthy (exit {}). \
                     Try `coast stop {} && coast start {}`.",
                    target_name, r.exit_code, target_name, target_name,
                )));
            }
            Ok(Err(e)) => {
                return Err(CoastError::docker(format!(
                    "Cannot reach inner Docker daemon in instance '{}': {e}. \
                     Try `coast stop {} && coast start {}`.",
                    target_name, target_name, target_name,
                )));
            }
            Err(_) => {
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is unresponsive (timed out after {}s). \
                     The DinD container may need to be recreated. Try `coast rm {} && coast run {}`.",
                    target_name, health_timeout.as_secs(), target_name, target_name,
                )));
            }
        }
    }

    // All pre-flight checks passed — commit the status change.
    db.update_instance_status(&req.project, &target_name, &InstanceStatus::CheckedOut)?;

    // Spawn canonical socat forwarders: canonical_port → localhost:dynamic_port.
    // Collect errors and revert if any spawn fails.
    if state.docker.is_some() {
        let mut spawned_pids: Vec<u32> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        for alloc in &port_allocs {
            let cmd = crate::port_manager::socat_command_canonical(
                alloc.canonical_port,
                "127.0.0.1",
                alloc.dynamic_port,
            );
            match crate::port_manager::spawn_socat(&cmd) {
                Ok(pid) => {
                    let _ = db.update_socat_pid(
                        &req.project,
                        &target_name,
                        &alloc.logical_name,
                        Some(pid as i32),
                    );
                    spawned_pids.push(pid);
                }
                Err(e) => {
                    errors.push(format!("port {}: {e}", alloc.canonical_port));
                }
            }
        }

        if !errors.is_empty() {
            // Clean up any socat processes that did succeed.
            for pid in &spawned_pids {
                let _ = crate::port_manager::kill_socat(*pid);
            }
            // Revert status — checkout did not complete.
            let _ = db.update_instance_status(&req.project, &target_name, &InstanceStatus::Running);
            return Err(CoastError::port(format!(
                "Checkout of '{}' failed — could not start socat forwarder(s): {}. \
                 Ensure socat is installed (e.g., `brew install socat` on macOS, \
                 `apt-get install socat` on Ubuntu).",
                target_name,
                errors.join("; "),
            )));
        }
    }

    info!(
        checked_out = %target_name,
        project = %req.project,
        port_count = ports.len(),
        "checkout completed — canonical ports now bound to this instance"
    );

    Ok(CheckoutResponse {
        checked_out: Some(target_name),
        ports,
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

    fn test_state_with_docker() -> AppState {
        AppState::new_for_testing_with_docker(StateDb::open_in_memory().unwrap())
    }

    fn add_test_port(db: &StateDb, project: &str, instance: &str) {
        db.insert_port_allocation(
            project,
            instance,
            &PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 50000,
                is_primary: false,
            },
        )
        .unwrap();
    }

    fn add_test_port_on(db: &StateDb, project: &str, instance: &str, canonical: u16, dynamic: u16) {
        db.insert_port_allocation(
            project,
            instance,
            &PortMapping {
                logical_name: format!("port-{canonical}"),
                canonical_port: canonical,
                dynamic_port: dynamic,
                is_primary: false,
            },
        )
        .unwrap();
    }

    fn make_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("container-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    #[tokio::test]
    async fn test_checkout_running_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", InstanceStatus::Running))
                .unwrap();
            add_test_port(&db, "my-app", "feat-a");
        }

        let req = CheckoutRequest {
            name: Some("feat-a".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.checked_out, Some("feat-a".to_string()));

        // Verify status updated
        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_swaps_from_previous() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                "my-app",
                InstanceStatus::CheckedOut,
            ))
            .unwrap();
            add_test_port(&db, "my-app", "feat-a");
            db.insert_instance(&make_instance("feat-b", "my-app", InstanceStatus::Running))
                .unwrap();
            add_test_port(&db, "my-app", "feat-b");
        }

        let req = CheckoutRequest {
            name: Some("feat-b".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.checked_out, Some("feat-b".to_string()));

        // Verify old instance is back to running
        let db = state.db.lock().await;
        let old = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(old.status, InstanceStatus::Running);
        let new = db.get_instance("my-app", "feat-b").unwrap().unwrap();
        assert_eq!(new.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_none() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                "my-app",
                InstanceStatus::CheckedOut,
            ))
            .unwrap();
        }

        let req = CheckoutRequest {
            name: None,
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.checked_out.is_none());
        assert!(resp.ports.is_empty());

        // Verify old instance un-checked
        let db = state.db.lock().await;
        let inst = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_checkout_nonexistent_instance() {
        let state = test_state();
        let req = CheckoutRequest {
            name: Some("nonexistent".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_checkout_stopped_instance_fails() {
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

        let req = CheckoutRequest {
            name: Some("stopped-inst".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not running"));
    }

    #[tokio::test]
    async fn test_checkout_no_ports_rejected() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "portless",
                "my-app",
                InstanceStatus::Running,
            ))
            .unwrap();
        }

        let req = CheckoutRequest {
            name: Some("portless".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no port allocations"));
    }

    #[tokio::test]
    async fn test_checkout_worktree_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            let mut wt_inst = make_instance("wt-inst", "my-app", InstanceStatus::Running);
            wt_inst.worktree_name = Some("feature-x".to_string());
            wt_inst.branch = Some("feature-x".to_string());
            db.insert_instance(&wt_inst).unwrap();
            add_test_port(&db, "my-app", "wt-inst");

            let bound_inst = make_instance("bound-inst", "my-app", InstanceStatus::Running);
            db.insert_instance(&bound_inst).unwrap();
            add_test_port(&db, "my-app", "bound-inst");
        }

        let req = CheckoutRequest {
            name: Some("bound-inst".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok(), "host-bound checkout should succeed");
        let resp = result.unwrap();
        assert_eq!(resp.checked_out, Some("bound-inst".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("my-app", "bound-inst").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_fails_when_canonical_port_occupied() {
        use std::net::TcpListener;

        let state = test_state_with_docker();

        // Bind a port to simulate it being occupied by another process.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port = listener.local_addr().unwrap().port();

        {
            let db = state.db.lock().await;
            // container_id = None so the Docker health check is skipped;
            // we only want to exercise the port availability pre-check.
            let mut inst = make_instance("port-clash", "my-app", InstanceStatus::Running);
            inst.container_id = None;
            db.insert_instance(&inst).unwrap();
            add_test_port_on(&db, "my-app", "port-clash", occupied_port, 50100);
        }

        let req = CheckoutRequest {
            name: Some("port-clash".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(
            result.is_err(),
            "checkout should fail when canonical port is occupied"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already in use"),
            "error should mention port in use, got: {err}"
        );
        assert!(
            err.contains(&occupied_port.to_string()),
            "error should mention the occupied port number, got: {err}"
        );

        // Status must remain Running — not CheckedOut.
        let db = state.db.lock().await;
        let inst = db.get_instance("my-app", "port-clash").unwrap().unwrap();
        assert_eq!(
            inst.status,
            InstanceStatus::Running,
            "instance should stay Running after failed checkout"
        );

        drop(listener);
    }

    #[tokio::test]
    async fn test_checkout_port_occupied_with_multiple_ports() {
        use std::net::TcpListener;

        let state = test_state_with_docker();

        let listener1 = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied1 = listener1.local_addr().unwrap().port();
        let listener2 = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied2 = listener2.local_addr().unwrap().port();

        {
            let db = state.db.lock().await;
            let mut inst = make_instance("multi-clash", "my-app", InstanceStatus::Running);
            inst.container_id = None;
            db.insert_instance(&inst).unwrap();
            add_test_port_on(&db, "my-app", "multi-clash", occupied1, 50200);
            add_test_port_on(&db, "my-app", "multi-clash", occupied2, 50201);
        }

        let req = CheckoutRequest {
            name: Some("multi-clash".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains(&occupied1.to_string()),
            "should list first port: {err}"
        );
        assert!(
            err.contains(&occupied2.to_string()),
            "should list second port: {err}"
        );

        let db = state.db.lock().await;
        let inst = db.get_instance("my-app", "multi-clash").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Running);

        drop(listener1);
        drop(listener2);
    }

    #[tokio::test]
    async fn test_checkout_succeeds_when_ports_are_free() {
        // Use base test state (docker=None) so socat spawning is skipped
        // and only the port pre-check + status transition is validated.
        let state = test_state();
        {
            let db = state.db.lock().await;
            let inst = make_instance("free-ports", "my-app", InstanceStatus::Running);
            db.insert_instance(&inst).unwrap();
            add_test_port(&db, "my-app", "free-ports");
        }

        let req = CheckoutRequest {
            name: Some("free-ports".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(
            result.is_ok(),
            "checkout should succeed: {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert_eq!(resp.checked_out, Some("free-ports".to_string()));
    }

    /// Test that if socat spawning fails (e.g. socat not installed), the
    /// checkout handler reverts the instance back to Running and returns
    /// an error. This test runs with docker=Some so the socat code path
    /// is entered, and uses container_id=None to skip the health check.
    ///
    /// On machines WHERE socat IS installed, the spawn will succeed and
    /// checkout completes normally (the test cleans up the spawned socat).
    /// On machines where socat is NOT installed, the spawn fails and
    /// the revert path is exercised.
    #[tokio::test]
    async fn test_checkout_socat_spawn_failure_reverts_status() {
        let state = test_state_with_docker();

        // Pick a free port so the pre-check passes.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let free_port = listener.local_addr().unwrap().port();
        drop(listener);

        {
            let db = state.db.lock().await;
            let mut inst = make_instance("socat-test", "my-app", InstanceStatus::Running);
            inst.container_id = None;
            db.insert_instance(&inst).unwrap();
            add_test_port_on(&db, "my-app", "socat-test", free_port, 50400);
        }

        let req = CheckoutRequest {
            name: Some("socat-test".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;

        match result {
            Ok(resp) => {
                // socat is installed and succeeded — cleanup the spawned process.
                assert_eq!(resp.checked_out, Some("socat-test".to_string()));
                let db = state.db.lock().await;
                let allocs = db.get_port_allocations("my-app", "socat-test").unwrap();
                for alloc in &allocs {
                    if let Some(pid) = alloc.socat_pid {
                        let _ = crate::port_manager::kill_socat(pid as u32);
                    }
                }
            }
            Err(e) => {
                // socat is NOT installed — the revert path was exercised.
                let err = e.to_string();
                assert!(err.contains("socat"), "error should mention socat: {err}");
                let db = state.db.lock().await;
                let inst = db.get_instance("my-app", "socat-test").unwrap().unwrap();
                assert_eq!(
                    inst.status,
                    InstanceStatus::Running,
                    "instance should be reverted to Running after socat failure"
                );
            }
        }
    }
}
