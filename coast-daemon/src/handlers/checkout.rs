/// Handler for the `coast checkout` command.
///
/// Swaps the canonical port bindings to a different coast instance.
/// This is designed to be instant — it only kills and respawns socat
/// processes, never restarts containers.
use std::collections::{BTreeSet, HashSet};

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CheckoutRequest, CheckoutResponse};
use coast_core::types::{InstanceStatus, PortMapping};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

#[derive(Debug, Default, PartialEq, Eq)]
struct CanonicalPortCheck {
    occupied: Vec<u16>,
    permission_denied: Vec<u16>,
    unexpected: Vec<(u16, String)>,
}

fn format_port_list(ports: &[u16]) -> String {
    ports
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn inspect_canonical_ports_with<F>(canonical_ports: &[u16], inspect: F) -> CanonicalPortCheck
where
    F: Fn(u16) -> crate::port_manager::PortBindStatus,
{
    let mut result = CanonicalPortCheck::default();
    for port in canonical_ports {
        match inspect(*port) {
            crate::port_manager::PortBindStatus::Available => {}
            crate::port_manager::PortBindStatus::InUse => result.occupied.push(*port),
            crate::port_manager::PortBindStatus::PermissionDenied => {
                result.permission_denied.push(*port)
            }
            crate::port_manager::PortBindStatus::UnexpectedError(error) => {
                result.unexpected.push((*port, error))
            }
        }
    }
    result
}

fn check_canonical_ports_available_with<F>(
    canonical_ports: &[u16],
    target_name: &str,
    inspect: F,
) -> Result<Vec<u16>>
where
    F: Fn(u16) -> crate::port_manager::PortBindStatus,
{
    let result = inspect_canonical_ports_with(canonical_ports, inspect);
    if result.occupied.is_empty() && result.unexpected.is_empty() {
        return Ok(result.permission_denied);
    }

    let mut parts = Vec::new();
    if !result.occupied.is_empty() {
        parts.push(format!(
            "canonical port(s) {} already in use. Another process is occupying {} port(s). \
             Free them before checking out, or run `coast checkout --none` to unbind all canonical ports.",
            format_port_list(&result.occupied),
            result.occupied.len(),
        ));
    }
    if !result.permission_denied.is_empty() {
        parts.push(format!(
            "canonical port(s) {} require elevated bind privileges on this host. \
             On Linux, ports below 1024 are restricted unless you raise \
             `net.ipv4.ip_unprivileged_port_start` or grant bind capability to the forwarding process or binary.",
            format_port_list(&result.permission_denied),
        ));
    }
    if !result.unexpected.is_empty() {
        let details = result
            .unexpected
            .iter()
            .map(|(port, error)| format!("{port}: {error}"))
            .collect::<Vec<_>>()
            .join("; ");
        parts.push(format!(
            "failed to inspect canonical port availability: {details}"
        ));
    }

    Err(CoastError::port(format!(
        "Cannot checkout '{}': {}",
        target_name,
        parts.join(" Also, ")
    )))
}

/// Check canonical ports before checkout.
///
/// Truly occupied ports and unexpected probe failures are fatal. Low ports that
/// return `PermissionDenied` are recorded and retried through verified socat
/// binding, which allows Linux hosts configured via `setcap` on socat.
fn check_canonical_ports_available(canonical_ports: &[u16], target_name: &str) -> Result<Vec<u16>> {
    check_canonical_ports_available_with(
        canonical_ports,
        target_name,
        crate::port_manager::inspect_port_binding,
    )
}

fn format_linux_permission_error(target_name: &str, ports: &[u16]) -> String {
    format!(
        "Checkout of '{}' failed — canonical port(s) {} require Linux host setup before Coast can bind them. \
         Ports below 1024 are restricted unless you raise `net.ipv4.ip_unprivileged_port_start` \
         or grant bind capability to the forwarding process or binary.",
        target_name,
        format_port_list(ports),
    )
}

fn format_checkout_bind_failure(target_name: &str, errors: &[String]) -> String {
    let needs_install_hint = errors
        .iter()
        .any(|error| error.contains("Failed to spawn socat process"));
    let install_hint = if needs_install_hint {
        " Ensure socat is installed (e.g., `brew install socat` on macOS, `sudo apt-get install socat` on Ubuntu)."
    } else {
        ""
    };

    format!(
        "Checkout of '{}' failed — could not bind canonical port forwarder(s): {}.{}",
        target_name,
        errors.join("; "),
        install_hint
    )
}

/// Find checked-out instances in other projects that own canonical ports needed
/// by the target checkout.
fn conflicting_checked_out_instances(
    db: &crate::state::StateDb,
    target_project: &str,
    target_canonical_ports: &[u16],
) -> Result<Vec<(String, String)>> {
    let target_canonical_ports: HashSet<u16> = target_canonical_ports.iter().copied().collect();
    if target_canonical_ports.is_empty() {
        return Ok(Vec::new());
    }

    let mut conflicts = BTreeSet::new();
    for inst in db.list_instances()? {
        if inst.project == target_project || inst.status != InstanceStatus::CheckedOut {
            continue;
        }

        let allocs = db.get_port_allocations(&inst.project, &inst.name)?;
        if allocs
            .iter()
            .any(|alloc| target_canonical_ports.contains(&alloc.canonical_port))
        {
            conflicts.insert((inst.project, inst.name));
        }
    }

    Ok(conflicts.into_iter().collect())
}

/// Handle a checkout request.
///
/// Steps:
/// 1. If name is Some, verify the target instance exists and is running.
/// 2. Find the currently checked-out instance in the same project (if any)
///    and un-check it.
/// 3. Release conflicting checked-out instances from other projects whose
///    canonical ports overlap the target checkout.
/// 4. If name is Some, resolve the new coast container IP and spawn
///    canonical socat forwarders.
/// 5. Update instance statuses in state DB.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(req: CheckoutRequest, state: &AppState) -> Result<CheckoutResponse> {
    info!(name = ?req.name, project = %req.project, "handling checkout request");

    // Checkout is fast (socat processes only, no Docker operations)
    // but we still scope the lock for consistency.
    let db = state.db.lock().await;

    let instances = db.list_instances_for_project(&req.project)?;
    for inst in &instances {
        if inst.status == InstanceStatus::CheckedOut {
            super::clear_checked_out_state(
                &db,
                &req.project,
                &inst.name,
                &InstanceStatus::Running,
            )?;

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

    let canonical_ports: Vec<u16> = port_allocs.iter().map(|a| a.canonical_port).collect();
    let conflicting_instances =
        conflicting_checked_out_instances(&db, &req.project, &canonical_ports)?;
    for (project, name) in &conflicting_instances {
        super::clear_checked_out_state(&db, project, name, &InstanceStatus::Running)?;
        info!(
            conflicting_project = %project,
            conflicting_instance = %name,
            target_project = %req.project,
            target_instance = %target_name,
            "auto-unchecked out conflicting instance from another project"
        );
    }

    // Pre-flight: verify all canonical ports are available before committing
    // to the checkout. This catches "address already in use" early with a
    // clear, actionable error message.
    // Pre-flight: verify the inner Docker daemon is responsive before routing
    // traffic to this instance. Status is not yet set to CheckedOut so early
    // returns here leave the DB in a consistent state.
    if let (Some(container_id), Some(docker)) =
        (target.container_id.as_ref(), state.docker.as_ref())
    {
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

    // Spawn canonical socat forwarders: canonical_port → localhost:dynamic_port.
    // Collect errors and revert if any spawn fails.
    if state.docker.is_some() {
        let use_wsl_bridge = crate::port_manager::running_in_wsl();
        let permission_denied_ports: HashSet<u16> =
            check_canonical_ports_available(&canonical_ports, &target_name)?
                .into_iter()
                .collect();
        let mut spawned_pids: Vec<u32> = Vec::new();
        let mut spawned_logical_names: Vec<String> = Vec::new();
        let mut bind_errors: Vec<String> = Vec::new();
        let mut permission_errors: Vec<u16> = Vec::new();

        if use_wsl_bridge {
            let bridge_ports = port_allocs
                .iter()
                .map(|alloc| crate::port_manager::CheckoutBridgePort {
                    _logical_name: &alloc.logical_name,
                    canonical_port: alloc.canonical_port,
                    dynamic_port: alloc.dynamic_port,
                })
                .collect::<Vec<_>>();

            match crate::port_manager::start_checkout_bridge(
                &req.project,
                &target_name,
                &bridge_ports,
            ) {
                Ok(()) => {
                    spawned_logical_names
                        .extend(port_allocs.iter().map(|alloc| alloc.logical_name.clone()));
                }
                Err(e) => {
                    bind_errors.push(e.to_string());
                }
            }
        } else {
            for alloc in &port_allocs {
                let cmd = crate::port_manager::socat_command_canonical(
                    alloc.canonical_port,
                    "127.0.0.1",
                    alloc.dynamic_port,
                );
                match crate::port_manager::spawn_socat_verified(&cmd, alloc.canonical_port) {
                    Ok(pid) => {
                        let _ = db.update_socat_pid(
                            &req.project,
                            &target_name,
                            &alloc.logical_name,
                            Some(pid as i32),
                        );
                        spawned_pids.push(pid);
                        spawned_logical_names.push(alloc.logical_name.clone());
                    }
                    Err(e) => {
                        let error = e.to_string();
                        if permission_denied_ports.contains(&alloc.canonical_port)
                            && !error.contains("Failed to spawn socat process")
                        {
                            permission_errors.push(alloc.canonical_port);
                        } else {
                            bind_errors.push(format!("port {}: {error}", alloc.canonical_port));
                        }
                    }
                }
            }
        }

        if !permission_errors.is_empty() || !bind_errors.is_empty() {
            // Clean up any socat processes that did succeed.
            for pid in &spawned_pids {
                let _ = crate::port_manager::kill_socat(*pid);
            }
            if use_wsl_bridge {
                let _ = crate::port_manager::remove_checkout_bridge(&req.project, &target_name);
            } else {
                for logical_name in &spawned_logical_names {
                    let _ = db.update_socat_pid(&req.project, &target_name, logical_name, None);
                }
            }
            let mut messages = Vec::new();
            if !permission_errors.is_empty() {
                permission_errors.sort_unstable();
                permission_errors.dedup();
                messages.push(format_linux_permission_error(
                    &target_name,
                    &permission_errors,
                ));
            }
            if !bind_errors.is_empty() {
                messages.push(format_checkout_bind_failure(&target_name, &bind_errors));
            }
            return Err(CoastError::port(messages.join(" ")));
        }
    }

    db.update_instance_status(&req.project, &target_name, &InstanceStatus::CheckedOut)?;

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
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn test_state_with_docker() -> AppState {
        AppState::new_for_testing_with_docker(StateDb::open_in_memory().unwrap())
    }

    fn add_test_port(db: &StateDb, project: &str, instance: &str) {
        add_test_port_named(db, project, instance, "web", 3000, 50000);
    }

    fn add_test_port_named(
        db: &StateDb,
        project: &str,
        instance: &str,
        logical_name: &str,
        canonical: u16,
        dynamic: u16,
    ) {
        db.insert_port_allocation(
            project,
            instance,
            &PortMapping {
                logical_name: logical_name.to_string(),
                canonical_port: canonical,
                dynamic_port: dynamic,
                is_primary: false,
            },
        )
        .unwrap();
    }

    fn add_test_port_on(db: &StateDb, project: &str, instance: &str, canonical: u16, dynamic: u16) {
        add_test_port_named(
            db,
            project,
            instance,
            &format!("port-{canonical}"),
            canonical,
            dynamic,
        );
    }

    fn mark_checked_out(db: &StateDb, project: &str, instance: &str, logical_name: &str, pid: i32) {
        db.update_instance_status(project, instance, &InstanceStatus::CheckedOut)
            .unwrap();
        db.update_socat_pid(project, instance, logical_name, Some(pid))
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

    #[test]
    fn test_check_canonical_ports_permission_denied_is_deferred_for_verified_bind() {
        let denied = check_canonical_ports_available_with(&[80, 443], "linux-low-ports", |_| {
            crate::port_manager::PortBindStatus::PermissionDenied
        })
        .unwrap();

        assert_eq!(denied, vec![80, 443]);
    }

    #[test]
    fn test_check_canonical_ports_mixed_occupied_and_permission_denied_reports_both() {
        let err =
            check_canonical_ports_available_with(&[80, 443, 3000], "mixed", |port| match port {
                80 | 443 => crate::port_manager::PortBindStatus::PermissionDenied,
                3000 => crate::port_manager::PortBindStatus::InUse,
                _ => crate::port_manager::PortBindStatus::Available,
            })
            .unwrap_err()
            .to_string();

        assert!(err.contains("3000"));
        assert!(err.contains("already in use"));
        assert!(err.contains("80, 443"));
        assert!(err.contains("ip_unprivileged_port_start"));
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
            db.update_socat_pid("my-app", "feat-a", "web", Some(4_194_304))
                .unwrap();
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
        let old_allocs = db.get_port_allocations("my-app", "feat-a").unwrap();
        assert!(old_allocs[0].socat_pid.is_none());
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
            add_test_port(&db, "my-app", "feat-a");
            db.update_socat_pid("my-app", "feat-a", "web", Some(4_194_304))
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
        let allocs = db.get_port_allocations("my-app", "feat-a").unwrap();
        assert!(allocs[0].socat_pid.is_none());
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

    #[tokio::test]
    async fn test_checkout_auto_unchecks_out_conflicting_instance_from_other_project() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "project-a-main",
                "project-a",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port(&db, "project-a", "project-a-main");
            mark_checked_out(&db, "project-a", "project-a-main", "web", 4_194_304);

            db.insert_instance(&make_instance(
                "project-b-main",
                "project-b",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port(&db, "project-b", "project-b-main");
        }

        let req = CheckoutRequest {
            name: Some("project-b-main".to_string()),
            project: "project-b".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.checked_out, Some("project-b-main".to_string()));

        let db = state.db.lock().await;
        let old = db
            .get_instance("project-a", "project-a-main")
            .unwrap()
            .unwrap();
        assert_eq!(old.status, InstanceStatus::Running);
        let old_allocs = db
            .get_port_allocations("project-a", "project-a-main")
            .unwrap();
        assert!(old_allocs[0].socat_pid.is_none());

        let new = db
            .get_instance("project-b", "project-b-main")
            .unwrap()
            .unwrap();
        assert_eq!(new.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_auto_unchecks_multiple_conflicting_projects() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "a-main",
                "project-a",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-a", "a-main", "web", 3000, 50000);
            mark_checked_out(&db, "project-a", "a-main", "web", 4_194_304);

            db.insert_instance(&make_instance(
                "c-main",
                "project-c",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-c", "c-main", "api", 8080, 50001);
            mark_checked_out(&db, "project-c", "c-main", "api", 4_194_305);

            db.insert_instance(&make_instance(
                "b-main",
                "project-b",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-b", "b-main", "web", 3000, 51000);
            add_test_port_named(&db, "project-b", "b-main", "api", 8080, 51001);
        }

        let req = CheckoutRequest {
            name: Some("b-main".to_string()),
            project: "project-b".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.checked_out, Some("b-main".to_string()));

        let db = state.db.lock().await;
        for (project, instance) in [("project-a", "a-main"), ("project-c", "c-main")] {
            let old = db.get_instance(project, instance).unwrap().unwrap();
            assert_eq!(old.status, InstanceStatus::Running);
            let allocs = db.get_port_allocations(project, instance).unwrap();
            assert!(allocs.iter().all(|alloc| alloc.socat_pid.is_none()));
        }
        let new = db.get_instance("project-b", "b-main").unwrap().unwrap();
        assert_eq!(new.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_leaves_non_conflicting_other_project_checked_out() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "a-main",
                "project-a",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-a", "a-main", "web", 9000, 50000);
            mark_checked_out(&db, "project-a", "a-main", "web", 4_194_304);

            db.insert_instance(&make_instance(
                "b-main",
                "project-b",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-b", "b-main", "web", 3000, 51000);
        }

        let req = CheckoutRequest {
            name: Some("b-main".to_string()),
            project: "project-b".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.checked_out, Some("b-main".to_string()));

        let db = state.db.lock().await;
        let other = db.get_instance("project-a", "a-main").unwrap().unwrap();
        assert_eq!(other.status, InstanceStatus::CheckedOut);
        let other_allocs = db.get_port_allocations("project-a", "a-main").unwrap();
        assert_eq!(other_allocs[0].socat_pid, Some(4_194_304));
    }

    #[tokio::test]
    async fn test_checkout_multi_port_partial_overlap_evicts_conflicting_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "a-main",
                "project-a",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-a", "a-main", "web", 3000, 50000);
            add_test_port_named(&db, "project-a", "a-main", "db", 5432, 50001);
            mark_checked_out(&db, "project-a", "a-main", "web", 4_194_304);
            db.update_socat_pid("project-a", "a-main", "db", Some(4_194_305))
                .unwrap();

            db.insert_instance(&make_instance(
                "b-main",
                "project-b",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-b", "b-main", "web", 3000, 51000);
            add_test_port_named(&db, "project-b", "b-main", "api", 8080, 51001);
        }

        let req = CheckoutRequest {
            name: Some("b-main".to_string()),
            project: "project-b".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.checked_out, Some("b-main".to_string()));

        let db = state.db.lock().await;
        let old = db.get_instance("project-a", "a-main").unwrap().unwrap();
        assert_eq!(old.status, InstanceStatus::Running);
        let old_allocs = db.get_port_allocations("project-a", "a-main").unwrap();
        assert!(old_allocs.iter().all(|alloc| alloc.socat_pid.is_none()));
        let new = db.get_instance("project-b", "b-main").unwrap().unwrap();
        assert_eq!(new.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_checkout_ignores_stopped_or_running_instances_in_other_projects() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "a-stopped",
                "project-a",
                InstanceStatus::Stopped,
            ))
            .unwrap();
            add_test_port_named(&db, "project-a", "a-stopped", "web", 3000, 50000);
            db.update_socat_pid("project-a", "a-stopped", "web", Some(4_194_304))
                .unwrap();

            db.insert_instance(&make_instance(
                "c-running",
                "project-c",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-c", "c-running", "web", 3000, 50001);
            db.update_socat_pid("project-c", "c-running", "web", Some(4_194_305))
                .unwrap();

            db.insert_instance(&make_instance(
                "b-main",
                "project-b",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-b", "b-main", "web", 3000, 51000);
        }

        let req = CheckoutRequest {
            name: Some("b-main".to_string()),
            project: "project-b".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.checked_out, Some("b-main".to_string()));

        let db = state.db.lock().await;
        let stopped = db.get_instance("project-a", "a-stopped").unwrap().unwrap();
        assert_eq!(stopped.status, InstanceStatus::Stopped);
        let stopped_allocs = db.get_port_allocations("project-a", "a-stopped").unwrap();
        assert_eq!(stopped_allocs[0].socat_pid, Some(4_194_304));

        let running = db.get_instance("project-c", "c-running").unwrap().unwrap();
        assert_eq!(running.status, InstanceStatus::Running);
        let running_allocs = db.get_port_allocations("project-c", "c-running").unwrap();
        assert_eq!(running_allocs[0].socat_pid, Some(4_194_305));
    }

    #[tokio::test]
    async fn test_checkout_clears_coast_conflict_but_still_fails_for_external_process() {
        use std::net::TcpListener;

        let state = test_state_with_docker();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port = listener.local_addr().unwrap().port();

        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "a-main",
                "project-a",
                InstanceStatus::Running,
            ))
            .unwrap();
            add_test_port_named(&db, "project-a", "a-main", "web", 3000, 50000);
            mark_checked_out(&db, "project-a", "a-main", "web", 4_194_304);

            let mut target = make_instance("b-main", "project-b", InstanceStatus::Running);
            target.container_id = None;
            db.insert_instance(&target).unwrap();
            add_test_port_named(&db, "project-b", "b-main", "web", 3000, 51000);
            add_test_port_named(&db, "project-b", "b-main", "api", occupied_port, 51001);
        }

        let req = CheckoutRequest {
            name: Some("b-main".to_string()),
            project: "project-b".to_string(),
        };
        let err = handle(req, &state).await.unwrap_err().to_string();
        assert!(err.contains(&occupied_port.to_string()));

        let db = state.db.lock().await;
        let old = db.get_instance("project-a", "a-main").unwrap().unwrap();
        assert_eq!(old.status, InstanceStatus::Running);
        let old_allocs = db.get_port_allocations("project-a", "a-main").unwrap();
        assert!(old_allocs[0].socat_pid.is_none());

        let target = db.get_instance("project-b", "b-main").unwrap().unwrap();
        assert_eq!(target.status, InstanceStatus::Running);

        drop(listener);
    }

    #[tokio::test]
    async fn test_checkout_cross_project_takeover_is_symmetric() {
        for (source_project, target_project) in
            [("project-a", "project-b"), ("project-b", "project-a")]
        {
            let state = test_state();
            {
                let db = state.db.lock().await;
                db.insert_instance(&make_instance(
                    "source-main",
                    source_project,
                    InstanceStatus::Running,
                ))
                .unwrap();
                add_test_port_named(&db, source_project, "source-main", "web", 3000, 50000);
                mark_checked_out(&db, source_project, "source-main", "web", 4_194_304);

                db.insert_instance(&make_instance(
                    "target-main",
                    target_project,
                    InstanceStatus::Running,
                ))
                .unwrap();
                add_test_port_named(&db, target_project, "target-main", "web", 3000, 51000);
            }

            let req = CheckoutRequest {
                name: Some("target-main".to_string()),
                project: target_project.to_string(),
            };
            let result = handle(req, &state).await.unwrap();
            assert_eq!(result.checked_out, Some("target-main".to_string()));

            let db = state.db.lock().await;
            let old = db
                .get_instance(source_project, "source-main")
                .unwrap()
                .unwrap();
            assert_eq!(old.status, InstanceStatus::Running);
            let new = db
                .get_instance(target_project, "target-main")
                .unwrap()
                .unwrap();
            assert_eq!(new.status, InstanceStatus::CheckedOut);
        }
    }

    #[tokio::test]
    async fn test_checkout_verified_bind_failure_reverts_status() {
        let _guard = env_lock().lock().unwrap();
        if crate::port_manager::running_in_wsl() {
            return;
        }
        let state = test_state_with_docker();

        // Pick a free port so the pre-check passes.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let free_port = listener.local_addr().unwrap().port();
        drop(listener);

        let dir = tempfile::tempdir().unwrap();
        let fake_socat = dir.path().join("socat");
        std::fs::write(&fake_socat, "#!/bin/sh\nsleep 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake_socat).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_socat, perms).unwrap();
        }

        let old_path = std::env::var("PATH").unwrap_or_default();
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", dir.path().display(), old_path));
        }

        {
            let db = state.db.lock().await;
            let mut inst = make_instance("bind-failure", "my-app", InstanceStatus::Running);
            inst.container_id = None;
            db.insert_instance(&inst).unwrap();
            add_test_port_on(&db, "my-app", "bind-failure", free_port, 50400);
        }

        let req = CheckoutRequest {
            name: Some("bind-failure".to_string()),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;

        unsafe {
            std::env::set_var("PATH", old_path);
        }

        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("could not bind canonical port forwarder"),
            "error should mention bind failure: {err}"
        );
        assert!(
            err.contains("did not bind the port in time")
                || err.contains("exited before binding the port"),
            "error should mention verified bind failure: {err}"
        );

        let db = state.db.lock().await;
        let inst = db.get_instance("my-app", "bind-failure").unwrap().unwrap();
        assert_eq!(
            inst.status,
            InstanceStatus::Running,
            "instance should be reverted to Running after bind failure"
        );
        let allocs = db.get_port_allocations("my-app", "bind-failure").unwrap();
        assert!(allocs.iter().all(|alloc| alloc.socat_pid.is_none()));
    }
}
