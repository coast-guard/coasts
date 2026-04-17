/// Handler for the `coast unassign` command.
///
/// Returns an instance to the project root directory by remounting
/// `/workspace` to `/host-project`. Does not create any git worktrees
/// or modify git state. The host branch is read for display purposes only.
use std::path::Path;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, UnassignRequest, UnassignResponse};
use coast_core::types::{AssignAction, InstanceStatus};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

/// Validate that an instance can be unassigned.
///
/// Returns `Ok(())` for statuses where `can_assign()` returns true: `Running`,
/// `CheckedOut`, `Idle`, `Assigning`, and `Unassigning`. Returns an error for
/// `Stopped` (needs start first) and other transitional states (`Provisioning`,
/// `Starting`, `Stopping`, `Enqueued`).
fn validate_unassignable(status: &InstanceStatus, name: &str) -> Result<()> {
    match status {
        InstanceStatus::Running
        | InstanceStatus::CheckedOut
        | InstanceStatus::Idle
        | InstanceStatus::Assigning
        | InstanceStatus::Unassigning => Ok(()),
        InstanceStatus::Stopped => Err(CoastError::state(format!(
            "Instance '{name}' is stopped (status: {status}). \
             Run `coast start {name}` to start it first."
        ))),
        InstanceStatus::Provisioning
        | InstanceStatus::Starting
        | InstanceStatus::Stopping
        | InstanceStatus::Enqueued => Err(CoastError::state(format!(
            "Instance '{name}' is currently {status}. Wait for the operation to complete."
        ))),
    }
}

/// Read the current branch of a project root (for display only).
async fn read_host_branch(project_root: &Path) -> Option<String> {
    tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_root)
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Emit a progress event, ignoring send failures.
async fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.send(event).await;
}

const TOTAL_STEPS: u32 = 4;

/// Handle unassign for a remote instance: reset shell workspace, sync, forward to coast-service.
/// Reset the shell container /workspace to project root for a remote instance.
async fn reset_remote_shell_workspace(state: &AppState, project: &str, name: &str) {
    let shell_container = format!("{project}-coasts-{name}-shell");
    let Some(docker) = state.docker.as_ref() else {
        return;
    };
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let mount_cmd = "umount -l /workspace 2>/dev/null; \
        mount --bind /host-project /workspace && \
        mount --make-rshared /workspace";
    match rt
        .exec_in_coast(&shell_container, &["sh", "-c", mount_cmd])
        .await
    {
        Ok(r) if !r.success() => {
            warn!(stderr = %r.stderr, "shell /workspace reset returned non-zero")
        }
        Err(e) => warn!(error = %e, "failed to reset shell /workspace"),
        _ => info!("shell /workspace reset to project root"),
    }
}

/// Sync workspace to remote and forward the unassign to coast-service.
async fn sync_and_forward_remote_unassign(
    state: &AppState,
    req: &UnassignRequest,
    project_root: &Option<std::path::PathBuf>,
    display_branch: &Option<String>,
) -> Result<()> {
    let cf_data = super::assign::load_coastfile_data(&req.project);
    let service_actions: std::collections::HashMap<String, AssignAction> = cf_data
        .assign
        .services
        .keys()
        .map(|svc| (svc.clone(), AssignAction::Hot))
        .collect();

    let remote_config =
        super::remote::resolve_remote_for_instance(&req.project, &req.name, state).await?;
    let client = super::remote::RemoteClient::connect(&remote_config).await?;

    if let Some(ref root) = project_root {
        let service_home = client.query_service_home().await;
        let remote_workspace =
            super::remote::remote_workspace_path(&service_home, &req.project, &req.name);
        if let Err(e) = client.sync_workspace(root, &remote_workspace).await {
            warn!(error = %e, "failed to rsync project root to remote workspace");
        }
    }

    let assign_req = coast_core::protocol::AssignRequest {
        name: req.name.clone(),
        project: req.project.clone(),
        worktree: display_branch.clone().unwrap_or_default(),
        commit_sha: None,
        explain: false,
        force_sync: false,
        service_actions,
    };
    let _ = super::remote::forward::forward_assign(&client, &assign_req).await;
    Ok(())
}

async fn handle_unassign_remote(
    req: &UnassignRequest,
    instance: &coast_core::types::CoastInstance,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    started_at: tokio::time::Instant,
) -> Result<UnassignResponse> {
    let project_root = super::assign::read_project_root(&req.project);
    let display_branch = if let Some(ref root) = project_root {
        read_host_branch(root).await
    } else {
        None
    };

    emit(
        progress,
        BuildProgressEvent::build_plan(vec![
            "Validating instance".into(),
            "Resetting workspace".into(),
            "Syncing and notifying remote".into(),
        ]),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Validating instance", "ok"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Resetting workspace", 2, 3),
    )
    .await;

    reset_remote_shell_workspace(state, &req.project, &req.name).await;

    emit(
        progress,
        BuildProgressEvent::done("Resetting workspace", "ok"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Syncing and notifying remote", 3, 3),
    )
    .await;

    sync_and_forward_remote_unassign(state, req, &project_root, &display_branch).await?;

    emit(
        progress,
        BuildProgressEvent::done("Syncing and notifying remote", "ok"),
    )
    .await;

    let final_status = instance.status.clone();
    {
        let db = state.db.lock().await;
        db.update_instance_branch(
            &req.project,
            &req.name,
            display_branch.as_deref(),
            None,
            &final_status,
        )?;
        db.set_worktree(&req.project, &req.name, None)?;
    }

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().into(),
    });

    Ok(UnassignResponse {
        name: req.name.clone(),
        worktree: display_branch.unwrap_or_else(|| "project root".to_string()),
        previous_worktree: instance.worktree_name.clone(),
        time_elapsed_ms: started_at.elapsed().as_millis() as u64,
    })
}

/// Check inner daemon health, reverting status on failure.
async fn check_inner_daemon_or_revert(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    state: &AppState,
    project: &str,
    name: &str,
    prev_status: &InstanceStatus,
) -> Result<()> {
    let health_timeout = tokio::time::Duration::from_secs(10);
    let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
    match tokio::time::timeout(health_timeout, health_check).await {
        Ok(Ok(r)) if r.success() => {
            info!("unassign: inner daemon healthy");
            Ok(())
        }
        Ok(Ok(r)) => {
            revert_status(state, project, name, prev_status).await;
            Err(CoastError::docker(format!(
                "Inner Docker daemon in instance '{name}' is not healthy (exit {}). \
                 Try `coast stop {name} && coast start {name}`.",
                r.exit_code,
            )))
        }
        Ok(Err(e)) => {
            revert_status(state, project, name, prev_status).await;
            Err(CoastError::docker(format!(
                "Cannot reach inner Docker daemon in instance '{name}': {e}. \
                 Try `coast stop {name} && coast start {name}`.",
            )))
        }
        Err(_) => {
            revert_status(state, project, name, prev_status).await;
            Err(CoastError::docker(format!(
                "Inner Docker daemon in instance '{name}' is unresponsive (timed out after {}s). \
                 Try `coast rm {name} && coast run {name}`.",
                health_timeout.as_secs(),
            )))
        }
    }
}

/// Emit skip events for steps 2–4 when Docker is not available.
async fn emit_skip_steps(progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>) {
    emit(
        progress,
        BuildProgressEvent::done("Checking inner daemon", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Switching to project root", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Restarting services", "skip"),
    )
    .await;
}

/// Remount /workspace back to the project root inside the container.
async fn remount_workspace_to_project_root(
    rt: &coast_docker::dind::DindRuntime,
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
    name: &str,
) {
    let cf_data = super::assign::load_coastfile_data(project);
    let bare_svc_list = coast_core::coastfile::Coastfile::from_file(
        &coast_core::artifact::coast_home()
            .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".coast"))
            .join("images")
            .join(project)
            .join("latest")
            .join("coastfile.toml"),
    )
    .map(|cf| cf.services)
    .unwrap_or_default();
    crate::bare_services::stop_before_remount(docker, container_id, &bare_svc_list).await;
    let unmount_cache =
        coast_core::coastfile::Coastfile::build_cache_unmount_commands(&bare_svc_list);
    let unmount_private = coast_core::coastfile::Coastfile::build_private_paths_unmount_commands(
        &cf_data.private_paths,
    );
    let clear_private = coast_core::coastfile::Coastfile::build_private_paths_clear_commands(
        &cf_data.private_paths,
    );
    let private_cmds = coast_core::coastfile::Coastfile::build_private_paths_mount_commands(
        &cf_data.private_paths,
    );
    let cache_cmds = coast_core::coastfile::Coastfile::build_cache_mount_commands(&bare_svc_list);
    let mount_cmd = format!(
        "{unmount_cache}{unmount_private}{clear_private}umount -l /workspace 2>/dev/null; mount --bind /host-project /workspace && mount --make-rshared /workspace{private_cmds}{cache_cmds}"
    );
    match rt
        .exec_in_coast(container_id, &["sh", "-c", &mount_cmd])
        .await
    {
        Ok(r) if r.success() => info!(name = %name, "remounted /workspace to project root"),
        Ok(r) => warn!(name = %name, stderr = %r.stderr, "failed to remount /workspace"),
        Err(e) => warn!(name = %name, error = %e, "failed to remount /workspace"),
    }
}

/// Run the local Docker operations for unassign: health check, remount, restart services.
async fn run_local_unassign_docker_ops(
    docker: &bollard::Docker,
    container_id: &str,
    state: &AppState,
    req: &UnassignRequest,
    instance: &coast_core::types::CoastInstance,
    prev_status: &InstanceStatus,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

    check_inner_daemon_or_revert(
        &rt,
        container_id,
        state,
        &req.project,
        &req.name,
        prev_status,
    )
    .await?;
    emit(
        progress,
        BuildProgressEvent::done("Checking inner daemon", "ok"),
    )
    .await;

    // --- Remount /workspace to project root ---
    emit(
        progress,
        BuildProgressEvent::started("Switching to project root", 3, TOTAL_STEPS),
    )
    .await;

    remount_workspace_to_project_root(&rt, docker, container_id, &req.project, &req.name).await;

    {
        let db = state.db.lock().await;
        let _ = db.set_worktree(&req.project, &req.name, None);
    }
    emit(
        progress,
        BuildProgressEvent::done("Switching to project root", "ok"),
    )
    .await;

    // --- Restart services ---
    emit(
        progress,
        BuildProgressEvent::started("Restarting services", 4, TOTAL_STEPS),
    )
    .await;
    restart_services_after_unassign(
        &rt,
        docker,
        container_id,
        &req.project,
        &req.name,
        instance.build_id.as_deref(),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Restarting services", "ok"),
    )
    .await;

    Ok(())
}

/// Restart compose services after workspace remount.
async fn restart_compose_after_unassign(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    name: &str,
    build_id: Option<&str>,
) {
    let ctx = super::compose_context_for_build(project, build_id);
    let up_cmd = ctx.compose_shell("up -d --force-recreate --remove-orphans -t 1");
    let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
    match rt.exec_in_coast(container_id, &up_refs).await {
        Ok(r) if r.success() => {
            info!(name = %name, "compose force-recreate completed after unassign")
        }
        Ok(r) => warn!(name = %name, stderr = %r.stderr, "compose up after unassign had issues"),
        Err(e) => warn!(name = %name, error = %e, "compose up after unassign failed"),
    }
}

/// Restart bare services after workspace remount.
async fn restart_bare_after_unassign(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    name: &str,
    build_id: Option<&str>,
) {
    let stop_cmd = crate::bare_services::generate_stop_command();
    let _ = rt
        .exec_in_coast(container_id, &["sh", "-c", &stop_cmd])
        .await;

    let cf_path = super::artifact_coastfile_path(project, build_id);
    let svc_list = coast_core::coastfile::Coastfile::from_file(&cf_path)
        .map(|cf| cf.services)
        .unwrap_or_default();

    let start_cmd = crate::bare_services::generate_install_and_start_command(&svc_list);
    match rt
        .exec_in_coast(container_id, &["sh", "-c", &start_cmd])
        .await
    {
        Ok(r) if r.success() => info!(name = %name, "bare services restarted after unassign"),
        Ok(r) => {
            warn!(name = %name, stderr = %r.stderr, stdout = %r.stdout, "bare services start after unassign had issues")
        }
        Err(e) => warn!(name = %name, error = %e, "bare services start after unassign failed"),
    }
}

/// Restart compose and bare services after workspace remount.
async fn restart_services_after_unassign(
    rt: &coast_docker::dind::DindRuntime,
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
    name: &str,
    build_id: Option<&str>,
) {
    if super::assign::has_compose(project) {
        restart_compose_after_unassign(rt, container_id, project, name, build_id).await;
    }
    if crate::bare_services::has_bare_services(docker, container_id).await {
        restart_bare_after_unassign(rt, container_id, project, name, build_id).await;
    }
}

/// Handle an unassign request with streaming progress.
///
/// Directly remounts `/workspace` back to the project root (`/host-project`)
/// without detecting or caring about git branches. Services are restarted
/// so their bind mounts resolve through the new mount.
pub async fn handle(
    req: UnassignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<UnassignResponse> {
    let started_at = tokio::time::Instant::now();

    info!(
        name = %req.name,
        project = %req.project,
        "handling unassign request"
    );

    emit(
        &progress,
        BuildProgressEvent::build_plan(vec![
            "Validating instance".into(),
            "Checking inner daemon".into(),
            "Switching to project root".into(),
            "Restarting services".into(),
        ]),
    )
    .await;

    // --- Step 1: Validate instance ---
    emit(
        &progress,
        BuildProgressEvent::started("Validating instance", 1, TOTAL_STEPS),
    )
    .await;

    let instance = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
            CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            }
        })?;

        validate_unassignable(&inst.status, &req.name)?;

        if inst.remote_host.is_some() {
            db.update_instance_status(&req.project, &req.name, &InstanceStatus::Unassigning)?;
            drop(db);
            state.emit_event(CoastEvent::InstanceStatusChanged {
                name: req.name.clone(),
                project: req.project.clone(),
                status: "unassigning".into(),
            });
            return handle_unassign_remote(&req, &inst, state, &progress, started_at).await;
        }

        db.update_instance_status(&req.project, &req.name, &InstanceStatus::Unassigning)?;
        inst
    };

    let previous_worktree = instance.worktree_name.clone();
    let prev_status = instance.status.clone();
    let container_id = instance.container_id.clone().ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{}' has no container ID. \
             Try `coast rm {} && coast run {}`.",
            req.name, req.name, req.name,
        ))
    })?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "unassigning".to_string(),
    });

    emit(
        &progress,
        BuildProgressEvent::done("Validating instance", "ok"),
    )
    .await;

    let project_root = super::assign::read_project_root(&req.project);

    // --- Step 2: Check inner daemon ---
    emit(
        &progress,
        BuildProgressEvent::started("Checking inner daemon", 2, TOTAL_STEPS),
    )
    .await;

    if let Some(docker) = state.docker.as_ref() {
        run_local_unassign_docker_ops(
            &docker,
            &container_id,
            state,
            &req,
            &instance,
            &prev_status,
            &progress,
        )
        .await?;
    } else {
        emit_skip_steps(&progress).await;
        let db = state.db.lock().await;
        let _ = db.set_worktree(&req.project, &req.name, None);
    }

    // Read host branch for display purposes only
    let display_branch = if let Some(ref root) = project_root {
        read_host_branch(root).await
    } else {
        None
    };

    // Final DB update
    let final_status = if prev_status == InstanceStatus::Idle {
        InstanceStatus::Running
    } else {
        prev_status
    };

    {
        let db = state.db.lock().await;
        db.update_instance_branch(
            &req.project,
            &req.name,
            display_branch.as_deref(),
            None,
            &final_status,
        )?;
    }

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().into(),
    });

    let elapsed_ms = started_at.elapsed().as_millis() as u64;

    info!(
        name = %req.name,
        project = %req.project,
        elapsed_ms,
        "unassign completed — instance back on project root"
    );

    Ok(UnassignResponse {
        name: req.name,
        worktree: display_branch.unwrap_or_else(|| "project root".to_string()),
        previous_worktree,
        time_elapsed_ms: elapsed_ms,
    })
}

/// Revert instance status on error.
async fn revert_status(state: &AppState, project: &str, name: &str, prev: &InstanceStatus) {
    if let Ok(db) = state.db.try_lock() {
        let _ = db.update_instance_status(project, name, prev);
    }
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: name.to_string(),
        project: project.to_string(),
        status: prev.as_db_str().into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppState;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn sample_instance(
        name: &str,
        project: &str,
        status: InstanceStatus,
        worktree: Option<&str>,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("feature-x".to_string()),
            commit_sha: None,
            container_id: Some(format!("{project}-coasts-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: worktree.map(String::from),
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    #[tokio::test]
    async fn test_unassign_instance_not_found() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);

        let req = UnassignRequest {
            name: "nonexistent".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_unassign_stopped_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::Stopped,
            Some("feature-x"),
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = UnassignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
    }

    #[tokio::test]
    async fn test_unassign_running_instance_clears_worktree() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::Running,
            Some("feature-x"),
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = UnassignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());

        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert_eq!(resp.previous_worktree, Some("feature-x".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert!(inst.worktree_name.is_none(), "worktree should be cleared");
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_unassign_idle_instance_transitions_to_running() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::Idle,
            None,
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = UnassignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_unassign_preserves_checked_out_status() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::CheckedOut,
            Some("feature-x"),
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = UnassignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);
    }

    // --- validate_unassignable tests ---

    #[test]
    fn test_validate_unassignable_running_ok() {
        assert!(validate_unassignable(&InstanceStatus::Running, "inst").is_ok());
    }

    #[test]
    fn test_validate_unassignable_checked_out_ok() {
        assert!(validate_unassignable(&InstanceStatus::CheckedOut, "inst").is_ok());
    }

    #[test]
    fn test_validate_unassignable_idle_ok() {
        assert!(validate_unassignable(&InstanceStatus::Idle, "inst").is_ok());
    }

    #[test]
    fn test_validate_unassignable_assigning_ok() {
        assert!(validate_unassignable(&InstanceStatus::Assigning, "inst").is_ok());
    }

    #[test]
    fn test_validate_unassignable_unassigning_ok() {
        assert!(validate_unassignable(&InstanceStatus::Unassigning, "inst").is_ok());
    }

    #[test]
    fn test_validate_unassignable_stopped_errors() {
        let err = validate_unassignable(&InstanceStatus::Stopped, "inst")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("stopped"),
            "error should mention 'stopped': {err}"
        );
    }

    #[test]
    fn test_validate_unassignable_provisioning_errors() {
        let err = validate_unassignable(&InstanceStatus::Provisioning, "inst")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("currently"),
            "error should mention 'currently': {err}"
        );
    }

    #[test]
    fn test_validate_unassignable_starting_errors() {
        let err = validate_unassignable(&InstanceStatus::Starting, "inst")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("currently"),
            "error should mention 'currently': {err}"
        );
    }

    #[test]
    fn test_validate_unassignable_stopping_errors() {
        let err = validate_unassignable(&InstanceStatus::Stopping, "inst")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("currently"),
            "error should mention 'currently': {err}"
        );
    }

    #[test]
    fn test_validate_unassignable_enqueued_errors() {
        let err = validate_unassignable(&InstanceStatus::Enqueued, "inst")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("currently"),
            "error should mention 'currently': {err}"
        );
    }

    // --- reset_remote_shell_workspace tests ---

    #[tokio::test]
    async fn test_reset_remote_shell_workspace_no_docker_is_noop() {
        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        assert!(state.docker.is_none());
        // Should not panic with no Docker client
        reset_remote_shell_workspace(&state, "proj", "inst").await;
    }

    // --- emit_skip_steps tests ---

    #[tokio::test]
    async fn test_emit_skip_steps_does_not_panic() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        emit_skip_steps(&tx).await;
        let mut count = 0;
        while rx.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 3, "should emit 3 skip events");
    }
}
