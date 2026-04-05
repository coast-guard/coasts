use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bollard::container::{ListContainersOptions, RemoveContainerOptions};
use tracing::warn;

use coast_core::error::Result;
use coast_core::protocol::{
    PrepareForUpdateRequest, PrepareForUpdateResponse, SharedRequest, UpdateSafetyIssue,
    UpdateSafetyIssueKind, UpdateSafetyRequest, UpdateSafetyResponse,
};
use coast_core::types::InstanceStatus;

use crate::server::{ActiveUpdateOperation, AppState};

const DEFAULT_PREPARE_TIMEOUT_MS: u64 = 30_000;
const DRAIN_POLL_INTERVAL_MS: u64 = 100;

pub async fn handle_is_safe_to_update(
    _req: UpdateSafetyRequest,
    state: &AppState,
) -> Result<UpdateSafetyResponse> {
    Ok(collect_update_safety_report(state).await)
}

pub async fn handle_prepare_for_update(
    req: PrepareForUpdateRequest,
    state: &AppState,
) -> Result<PrepareForUpdateResponse> {
    let mut actions = Vec::new();
    let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(DEFAULT_PREPARE_TIMEOUT_MS));

    if state.is_update_quiescing() {
        actions.push("Update quiescing was already enabled.".to_string());
    } else {
        state.set_update_quiescing(true);
        actions.push("Blocked new mutating requests.".to_string());
    }

    let drain_result = wait_for_active_operations_to_drain(state, timeout).await;
    if !drain_result.timed_out {
        actions.push("All active mutating operations drained.".to_string());
    }

    reconcile_stale_resources(state, &mut actions).await;

    if req.close_sessions {
        let closed = close_interactive_sessions(state).await;
        if closed > 0 {
            actions.push(format!("Closed {closed} interactive session(s)."));
        }
    }

    if req.stop_running_instances {
        let stopped = stop_running_instances(state).await;
        if stopped > 0 {
            actions.push(format!("Stopped {stopped} running instance(s)."));
        }
    }

    if req.stop_shared_services {
        let stopped = stop_shared_services(state).await;
        if stopped > 0 {
            actions.push(format!("Stopped {stopped} shared service group(s)."));
        }
    }

    let report = collect_update_safety_report(state).await;
    let ready = !drain_result.timed_out && report.safe;
    if !ready {
        state.set_update_quiescing(false);
    }

    Ok(PrepareForUpdateResponse {
        ready,
        quiescing: state.is_update_quiescing(),
        timed_out: drain_result.timed_out,
        actions,
        report,
    })
}

struct DrainResult {
    timed_out: bool,
}

async fn wait_for_active_operations_to_drain(state: &AppState, timeout: Duration) -> DrainResult {
    let deadline = Instant::now() + timeout;
    loop {
        if state.active_update_operations().is_empty() {
            return DrainResult { timed_out: false };
        }
        if Instant::now() >= deadline {
            return DrainResult { timed_out: true };
        }
        tokio::time::sleep(Duration::from_millis(DRAIN_POLL_INTERVAL_MS)).await;
    }
}

async fn collect_update_safety_report(state: &AppState) -> UpdateSafetyResponse {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    let instances = {
        let db = state.db.lock().await;
        db.list_instances().unwrap_or_default()
    };

    let mut running_count = 0usize;
    let mut checked_out_count = 0usize;
    let mut idle_count = 0usize;
    for instance in &instances {
        match instance.status {
            InstanceStatus::Enqueued
            | InstanceStatus::Provisioning
            | InstanceStatus::Assigning
            | InstanceStatus::Unassigning
            | InstanceStatus::Starting
            | InstanceStatus::Stopping => blockers.push(instance_status_issue(instance)),
            InstanceStatus::Running => running_count += 1,
            InstanceStatus::CheckedOut => checked_out_count += 1,
            InstanceStatus::Idle => idle_count += 1,
            InstanceStatus::Stopped => {}
        }
    }

    if running_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!(
                "{running_count} running instance(s) will reconnect after the daemon restarts."
            ),
            Some("Running instances are generally safe to update, but live sessions will disconnect."),
        ));
    }
    if checked_out_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!(
                "{checked_out_count} checked out instance(s) will have canonical ports restored after restart."
            ),
            Some("Checked-out instances are generally safe to update, but canonical port forwarding will briefly flap."),
        ));
    }
    if idle_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::StaleResource,
            format!(
                "{idle_count} idle instance(s) are running without full restart recovery coverage."
            ),
            Some(
                "Consider stopping idle instances before update if you want a fully quiet restart.",
            ),
        ));
    }

    for op in state.active_update_operations() {
        blockers.push(active_operation_issue(&op));
    }

    if should_block_for_docker(state, &instances).await {
        blockers.push(summary_issue(
            UpdateSafetyIssueKind::DockerUnavailable,
            "Docker is unavailable while Coast instances or shared services still exist."
                .to_string(),
            Some("Restore Docker connectivity before updating the daemon."),
        ));
    }

    warnings.extend(collect_session_warnings(state).await);
    warnings.extend(collect_stale_resource_warnings(state).await);

    UpdateSafetyResponse {
        safe: blockers.is_empty(),
        quiescing: state.is_update_quiescing(),
        blockers,
        warnings,
    }
}

fn instance_status_issue(instance: &coast_core::types::CoastInstance) -> UpdateSafetyIssue {
    let suggested_action = match instance.status {
        InstanceStatus::Enqueued => {
            Some("Wait for the queued run to begin or cancel it with `coast stop`/`coast rm`.")
        }
        InstanceStatus::Provisioning => {
            Some("Wait for provisioning to finish before updating. Clean it up manually with `coast rm` only if it is truly stuck.")
        }
        InstanceStatus::Assigning | InstanceStatus::Unassigning => {
            Some("Wait for the worktree operation to finish before updating.")
        }
        InstanceStatus::Starting | InstanceStatus::Stopping => {
            Some("Wait for the lifecycle transition to finish before updating.")
        }
        _ => None,
    };

    UpdateSafetyIssue {
        kind: UpdateSafetyIssueKind::InstanceStatus,
        project: Some(instance.project.clone()),
        instance: Some(instance.name.clone()),
        operation: None,
        summary: format!(
            "Instance '{}' in project '{}' is currently {}.",
            instance.name, instance.project, instance.status
        ),
        suggested_action: suggested_action.map(std::string::ToString::to_string),
    }
}

fn active_operation_issue(operation: &ActiveUpdateOperation) -> UpdateSafetyIssue {
    UpdateSafetyIssue {
        kind: UpdateSafetyIssueKind::ActiveOperation,
        project: operation.project.clone(),
        instance: operation.instance.clone(),
        operation: Some(operation.kind.as_str().to_string()),
        summary: format!(
            "Mutating operation '{}' is still in progress.",
            operation.kind.as_str()
        ),
        suggested_action: Some(
            "Wait for the operation to finish before applying an update.".to_string(),
        ),
    }
}

fn summary_issue(
    kind: UpdateSafetyIssueKind,
    summary: String,
    suggested_action: Option<&str>,
) -> UpdateSafetyIssue {
    UpdateSafetyIssue {
        kind,
        project: None,
        instance: None,
        operation: None,
        summary,
        suggested_action: suggested_action.map(std::string::ToString::to_string),
    }
}

async fn should_block_for_docker(
    state: &AppState,
    instances: &[coast_core::types::CoastInstance],
) -> bool {
    let has_active_instances = instances
        .iter()
        .any(|instance| instance.status != InstanceStatus::Stopped);
    let has_shared_services = {
        let db = state.db.lock().await;
        db.list_shared_services(None)
            .map(|services| services.iter().any(|svc| svc.status == "running"))
            .unwrap_or(false)
    };
    if !has_active_instances && !has_shared_services {
        return false;
    }
    match state.docker.as_ref() {
        Some(docker) => docker.ping().await.is_err(),
        None => true,
    }
}

async fn collect_session_warnings(state: &AppState) -> Vec<UpdateSafetyIssue> {
    let mut warnings = Vec::new();

    let host_terminal_count = state.pty_sessions.lock().await.len();
    if host_terminal_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!("{host_terminal_count} host terminal session(s) are currently attached."),
            Some("These sessions will disconnect when the daemon restarts."),
        ));
    }

    let exec_count = state.exec_sessions.lock().await.len();
    if exec_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!("{exec_count} instance exec session(s) are currently attached."),
            Some("These sessions will disconnect when the daemon restarts."),
        ));
    }

    let service_exec_count = state.service_exec_sessions.lock().await.len();
    if service_exec_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!("{service_exec_count} service exec session(s) are currently attached."),
            Some("These sessions will disconnect when the daemon restarts."),
        ));
    }

    let lsp_count = state.lsp_sessions.lock().await.len();
    if lsp_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::InteractiveSession,
            format!("{lsp_count} LSP session(s) are currently attached."),
            Some("Editor LSP connections will disconnect when the daemon restarts."),
        ));
    }

    warnings
}

async fn collect_stale_resource_warnings(state: &AppState) -> Vec<UpdateSafetyIssue> {
    let mut warnings = Vec::new();

    let stale_checkout_count = {
        let db = state.db.lock().await;
        let instances = db.list_instances().unwrap_or_default();
        let mut count = 0usize;
        for instance in &instances {
            for allocation in db
                .get_port_allocations(&instance.project, &instance.name)
                .unwrap_or_default()
            {
                if let Some(pid) = allocation.socat_pid {
                    if instance.status != InstanceStatus::CheckedOut
                        || crate::port_manager::socat_pid_is_stale(pid as u32)
                    {
                        count += 1;
                    }
                }
            }
        }
        count
    };
    if stale_checkout_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::StaleResource,
            format!("{stale_checkout_count} stale checkout port-forwarding record(s) will be cleaned up."),
            Some("Prepare-for-update can clear stale socat state automatically."),
        ));
    }

    let dangling_container_count = count_dangling_managed_containers(state).await;
    if dangling_container_count > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::StaleResource,
            format!(
                "{dangling_container_count} dangling Coast-managed container(s) have no matching state DB record."
            ),
            Some("Prepare-for-update can remove dangling managed containers automatically."),
        ));
    }

    let shared_service_mismatches = count_shared_service_container_mismatches(state).await;
    if shared_service_mismatches > 0 {
        warnings.push(summary_issue(
            UpdateSafetyIssueKind::StaleResource,
            format!(
                "{shared_service_mismatches} shared service record(s) are marked running but their containers are missing."
            ),
            Some("Prepare-for-update can reconcile shared service records automatically."),
        ));
    }

    warnings
}

async fn count_dangling_managed_containers(state: &AppState) -> usize {
    let Some(docker) = state.docker.as_ref() else {
        return 0;
    };

    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec!["coast.managed=true".to_string()]);
    let containers = docker
        .list_containers(Some(ListContainersOptions::<String> {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
        .unwrap_or_default();

    let known_instances: HashSet<(String, String)> = {
        let db = state.db.lock().await;
        db.list_instances()
            .unwrap_or_default()
            .into_iter()
            .map(|instance| (instance.project, instance.name))
            .collect()
    };

    containers
        .iter()
        .filter(|container| {
            let Some(labels) = container.labels.as_ref() else {
                return false;
            };
            let Some(project) = labels.get("coast.project") else {
                return false;
            };
            let Some(instance) = labels.get("coast.instance") else {
                return false;
            };
            !known_instances.contains(&(project.clone(), instance.clone()))
        })
        .count()
}

async fn count_shared_service_container_mismatches(state: &AppState) -> usize {
    let Some(docker) = state.docker.as_ref() else {
        return 0;
    };

    let services = {
        let db = state.db.lock().await;
        db.list_shared_services(None).unwrap_or_default()
    };

    let mut mismatches = 0usize;
    for service in &services {
        if service.status != "running" {
            continue;
        }
        let container_name =
            crate::shared_services::shared_container_name(&service.project, &service.service_name);
        if docker
            .inspect_container(&container_name, None)
            .await
            .is_err()
        {
            mismatches += 1;
        }
    }
    mismatches
}

async fn reconcile_stale_resources(state: &AppState, actions: &mut Vec<String>) {
    crate::port_manager::cleanup_orphaned_socat();
    reconcile_stale_checkout_state(state, actions).await;
    remove_dangling_managed_containers(state, actions).await;
    reconcile_shared_service_records(state, actions).await;
}

async fn reconcile_stale_checkout_state(state: &AppState, actions: &mut Vec<String>) {
    let stale_rows: Vec<(String, String, String, i32, InstanceStatus)> = {
        let db = state.db.lock().await;
        let instances = db.list_instances().unwrap_or_default();
        let mut rows = Vec::new();
        for instance in &instances {
            for allocation in db
                .get_port_allocations(&instance.project, &instance.name)
                .unwrap_or_default()
            {
                let Some(pid) = allocation.socat_pid else {
                    continue;
                };
                if instance.status != InstanceStatus::CheckedOut
                    || crate::port_manager::socat_pid_is_stale(pid as u32)
                {
                    rows.push((
                        instance.project.clone(),
                        instance.name.clone(),
                        allocation.logical_name,
                        pid,
                        instance.status.clone(),
                    ));
                }
            }
        }
        rows
    };

    if stale_rows.is_empty() {
        return;
    }

    let db = state.db.lock().await;
    for (project, instance, logical_name, pid, status) in stale_rows {
        let _ = crate::port_manager::kill_socat(pid as u32);
        let _ = db.update_socat_pid(&project, &instance, &logical_name, None);
        if status == InstanceStatus::CheckedOut {
            let _ = db.update_instance_status(&project, &instance, &InstanceStatus::Running);
        }
        actions.push(format!(
            "Cleared stale checkout forwarder '{logical_name}' for {project}/{instance}."
        ));
    }
}

async fn remove_dangling_managed_containers(state: &AppState, actions: &mut Vec<String>) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec!["coast.managed=true".to_string()]);
    let containers = docker
        .list_containers(Some(ListContainersOptions::<String> {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
        .unwrap_or_default();

    let known_instances: HashSet<(String, String)> = {
        let db = state.db.lock().await;
        db.list_instances()
            .unwrap_or_default()
            .into_iter()
            .map(|instance| (instance.project, instance.name))
            .collect()
    };

    for container in &containers {
        let Some(labels) = container.labels.as_ref() else {
            continue;
        };
        let Some(project) = labels.get("coast.project") else {
            continue;
        };
        let Some(instance) = labels.get("coast.instance") else {
            continue;
        };
        if known_instances.contains(&(project.clone(), instance.clone())) {
            continue;
        }
        let Some(container_id) = container.id.as_deref() else {
            continue;
        };

        let remove_options = RemoveContainerOptions {
            force: true,
            v: true,
            ..Default::default()
        };
        match docker
            .remove_container(container_id, Some(remove_options))
            .await
        {
            Ok(()) => {
                let cache_volume = coast_docker::dind::dind_cache_volume_name(project, instance);
                let _ = docker.remove_volume(&cache_volume, None).await;
                actions.push(format!(
                    "Removed dangling managed container for {project}/{instance}."
                ));
            }
            Err(error) => {
                warn!(
                    project = %project,
                    instance = %instance,
                    error = %error,
                    "failed to remove dangling managed container during update preparation"
                );
            }
        }
    }
}

async fn reconcile_shared_service_records(state: &AppState, actions: &mut Vec<String>) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    let services = {
        let db = state.db.lock().await;
        db.list_shared_services(None).unwrap_or_default()
    };

    for service in &services {
        if service.status != "running" {
            continue;
        }
        let container_name =
            crate::shared_services::shared_container_name(&service.project, &service.service_name);
        if docker
            .inspect_container(&container_name, None)
            .await
            .is_ok()
        {
            continue;
        }
        let db = state.db.lock().await;
        let _ = db.update_shared_service_status(&service.project, &service.service_name, "stopped");
        let _ =
            db.update_shared_service_container_id(&service.project, &service.service_name, None);
        drop(db);
        actions.push(format!(
            "Marked shared service {}/{} as stopped because its container was missing.",
            service.project, service.service_name
        ));
    }
}

async fn close_interactive_sessions(state: &AppState) -> usize {
    let mut closed = 0usize;
    {
        let mut sessions = state.pty_sessions.lock().await;
        closed += close_session_map(&mut sessions);
    }
    {
        let mut sessions = state.exec_sessions.lock().await;
        closed += close_session_map(&mut sessions);
    }
    {
        let mut sessions = state.service_exec_sessions.lock().await;
        closed += close_session_map(&mut sessions);
    }

    let mut lsp_sessions = state.lsp_sessions.lock().await;
    closed += lsp_sessions.len();
    lsp_sessions.clear();

    closed
}

fn close_session_map(
    sessions: &mut tokio::sync::MutexGuard<
        '_,
        HashMap<String, crate::api::ws_host_terminal::PtySession>,
    >,
) -> usize {
    let mut closed = 0usize;
    for (_id, session) in sessions.drain() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(session.child_pid),
            nix::sys::signal::Signal::SIGHUP,
        );
        unsafe {
            nix::libc::close(session.master_read_fd);
            nix::libc::close(session.master_write_fd);
        }
        closed += 1;
    }
    closed
}

async fn stop_running_instances(state: &AppState) -> usize {
    let instances = {
        let db = state.db.lock().await;
        db.list_instances()
            .unwrap_or_default()
            .into_iter()
            .filter(|instance| {
                matches!(
                    instance.status,
                    InstanceStatus::Running | InstanceStatus::CheckedOut | InstanceStatus::Idle
                )
            })
            .map(|instance| (instance.project, instance.name))
            .collect::<Vec<_>>()
    };

    let mut stopped = 0usize;
    for (project, name) in instances {
        let request = coast_core::protocol::StopRequest {
            project: project.clone(),
            name: name.clone(),
        };
        match crate::handlers::stop::handle(request, state, None).await {
            Ok(_) => stopped += 1,
            Err(error) => warn!(
                project = %project,
                name = %name,
                error = %error,
                "failed to stop instance while preparing for update"
            ),
        }
    }
    stopped
}

async fn stop_shared_services(state: &AppState) -> usize {
    let projects: HashSet<String> = {
        let db = state.db.lock().await;
        db.list_shared_services(None)
            .unwrap_or_default()
            .into_iter()
            .filter(|service| service.status == "running")
            .map(|service| service.project)
            .collect()
    };

    let mut stopped = 0usize;
    for project in projects {
        match crate::handlers::shared::handle(
            SharedRequest::Stop {
                project: project.clone(),
                service: None,
            },
            state,
        )
        .await
        {
            Ok(response) => {
                if !response.services.is_empty() {
                    stopped += 1;
                }
            }
            Err(error) => warn!(
                project = %project,
                error = %error,
                "failed to stop shared services while preparing for update"
            ),
        }
    }
    stopped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{AppState, UpdateOperationKind};
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn make_instance(
        name: &str,
        project: &str,
        status: InstanceStatus,
    ) -> coast_core::types::CoastInstance {
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
            remote_name: None,
        }
    }

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_is_safe_to_update_blocks_provisioning_instances() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "dev-1",
                "proj",
                InstanceStatus::Provisioning,
            ))
            .unwrap();
        }

        let response = handle_is_safe_to_update(UpdateSafetyRequest::default(), &state)
            .await
            .unwrap();
        assert!(!response.safe);
        assert!(response
            .blockers
            .iter()
            .any(|issue| issue.kind == UpdateSafetyIssueKind::InstanceStatus));
    }

    #[tokio::test]
    async fn test_is_safe_to_update_blocks_active_operations() {
        let state = test_state();
        let _guard = state
            .begin_update_operation(UpdateOperationKind::Assign, Some("proj"), Some("dev-1"))
            .unwrap();

        let response = handle_is_safe_to_update(UpdateSafetyRequest::default(), &state)
            .await
            .unwrap();
        assert!(!response.safe);
        assert!(response
            .blockers
            .iter()
            .any(|issue| issue.kind == UpdateSafetyIssueKind::ActiveOperation));
    }

    #[tokio::test]
    async fn test_prepare_for_update_times_out_when_active_operation_never_drains() {
        let state = test_state();
        let _guard = state
            .begin_update_operation(UpdateOperationKind::Build, Some("proj"), None)
            .unwrap();

        let response = handle_prepare_for_update(
            PrepareForUpdateRequest {
                timeout_ms: Some(1),
                ..PrepareForUpdateRequest::default()
            },
            &state,
        )
        .await
        .unwrap();

        assert!(!response.ready);
        assert!(response.timed_out);
        assert!(!state.is_update_quiescing());
    }

    #[tokio::test]
    async fn test_prepare_for_update_succeeds_when_no_blockers_exist() {
        let state = test_state();

        let response = handle_prepare_for_update(PrepareForUpdateRequest::default(), &state)
            .await
            .unwrap();

        assert!(response.ready);
        assert!(!response.timed_out);
        assert!(state.is_update_quiescing());
    }
}
