/// Handler for the `coast assign` command.
///
/// Reassigns a worktree to an existing coast instance (runtime slot) without
/// recreating the DinD container. Uses the `[assign]` Coastfile config to
/// selectively stop/restart/rebuild only the services that need it.
mod classify;
mod explain;
mod gitignored_sync;
pub(crate) mod services;
mod util;
mod worktree;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{AssignRequest, AssignResponse, BuildProgressEvent, CoastEvent};
use coast_core::types::InstanceStatus;

use crate::server::AppState;

use util::{emit, revert_assign_status, TOTAL_STEPS};

pub use explain::handle_explain;
pub use util::{has_compose, load_coastfile_data, read_project_root};
pub use worktree::detect_worktree_dir_from_git;

/// Handle an assign request with streaming progress.
pub async fn handle(
    req: AssignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<AssignResponse> {
    handle_with_status(req, state, progress, InstanceStatus::Assigning).await
}

/// Handle assign with an explicit transition status.
pub async fn handle_with_status(
    req: AssignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
    transition_status: InstanceStatus,
) -> Result<AssignResponse> {
    let started_at = tokio::time::Instant::now();

    info!(
        name = %req.name,
        project = %req.project,
        worktree = %req.worktree,
        "handling assign request"
    );

    emit(
        &progress,
        BuildProgressEvent::build_plan(vec![
            "Validating instance".into(),
            "Checking inner daemon".into(),
            "Stopping services".into(),
            "Switching worktree".into(),
            "Building images".into(),
            "Starting services".into(),
            "Waiting for healthy".into(),
        ]),
    )
    .await;

    // --- Step 1: Validate instance ---
    emit(
        &progress,
        BuildProgressEvent::started("Validating instance", 1, TOTAL_STEPS),
    )
    .await;

    let db = state.db.lock().await;

    let instance =
        db.get_instance(&req.project, &req.name)?
            .ok_or_else(|| CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            })?;

    if !instance.status.can_assign() {
        return Err(CoastError::state(format!(
            "Instance '{}' is in '{}' state and cannot be assigned a worktree. \
             Only Running or Idle instances can be assigned. \
             Run `coast start {}` to start it first.",
            req.name, instance.status, req.name,
        )));
    }

    if instance.remote_host.is_some() {
        drop(db);
        return handle_remote_assign(req, state, &instance, &progress, started_at).await;
    }

    let previous_branch = instance.branch.clone();
    let container_id = instance.container_id.clone().ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{}' has no container ID. This should not happen for a Running/Idle instance. \
             Try `coast rm {} && coast run {}`.",
            req.name, req.name, req.name,
        ))
    })?;

    let cf_data = load_coastfile_data(&req.project);
    let project_root = read_project_root(&req.project);

    db.update_instance_status(&req.project, &req.name, &transition_status)?;
    drop(db);

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: transition_status.as_db_str().into(),
    });

    emit(
        &progress,
        BuildProgressEvent::done("Validating instance", "ok"),
    )
    .await;

    let prev_status = instance.status.clone();

    // --- Steps 2-7: Docker-dependent steps ---
    emit(
        &progress,
        BuildProgressEvent::started("Checking inner daemon", 2, TOTAL_STEPS),
    )
    .await;

    if let Some(docker) = state.docker.as_ref() {
        let result = services::run_docker_steps(services::DockerStepsParams {
            req: &req,
            state,
            progress: &progress,
            docker: &docker,
            container_id: &container_id,
            instance_status: &instance.status,
            instance_build_id: instance.build_id.as_deref(),
            cf_data: &cf_data,
            assign_config: &cf_data.assign,
            project_root: &project_root,
            previous_branch: &previous_branch,
        })
        .await;

        if let Err(e) = result {
            revert_assign_status(state, &req.project, &req.name, &prev_status).await;
            return Err(e);
        }
    } else {
        services::emit_skip_all(&progress).await;
    }

    // --- Step 8: Update state DB ---
    let final_status = if prev_status == InstanceStatus::Idle {
        InstanceStatus::Running
    } else {
        prev_status.clone()
    };
    let db = state.db.lock().await;
    db.update_instance_branch(
        &req.project,
        &req.name,
        Some(&req.worktree),
        req.commit_sha.as_deref(),
        &final_status,
    )?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().into(),
    });

    info!(
        name = %req.name,
        worktree = %req.worktree,
        previous = ?previous_branch,
        "worktree assigned successfully"
    );

    Ok(AssignResponse {
        name: req.name,
        worktree: req.worktree,
        previous_worktree: previous_branch,
        time_elapsed_ms: started_at.elapsed().as_millis() as u64,
    })
}

/// Detect the local worktree path and determine the workspace source directory.
///
/// Returns the host path (workspace source) together with the resolved worktree
/// location (if found), so callers can also access container mount metadata.
async fn detect_workspace_source(
    req: &AssignRequest,
    cf_data: &util::CoastfileData,
    project_root: &Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, Option<services::WorktreeLocation>)> {
    info!(
        worktree_dirs = ?cf_data.worktree_dirs,
        default_wt_dir = %cf_data.default_worktree_dir,
        project_root = ?project_root,
        worktree = %req.worktree,
        "resolving worktree for remote assign"
    );
    let wt_location = services::detect_worktree_path(
        project_root,
        &cf_data.worktree_dirs,
        &cf_data.default_worktree_dir,
        &req.worktree,
    )
    .await;

    info!(
        resolved = ?wt_location.as_ref().map(|l| &l.host_path),
        "worktree detection result"
    );

    let workspace_source = match &wt_location {
        Some(loc) if loc.host_path.exists() => loc.host_path.clone(),
        _ => {
            let root = project_root.clone().ok_or_else(|| {
                CoastError::state(format!(
                    "cannot resolve project root for '{}'. Is the project built?",
                    req.project
                ))
            })?;
            warn!(
                worktree = %req.worktree,
                "worktree not found on disk, falling back to project root"
            );
            root
        }
    };

    Ok((workspace_source, wt_location))
}

/// Switch the shell container's /workspace mount to the resolved worktree.
///
/// Binds the new worktree source over /workspace and creates a symlink back to
/// the project root. Errors are logged as warnings without failing the assign.
fn build_remount_cmd(
    wt_location: &Option<services::WorktreeLocation>,
    project_root: &Option<std::path::PathBuf>,
) -> (String, String) {
    let mount_src = wt_location
        .as_ref()
        .map(|loc| loc.container_mount_src.clone())
        .unwrap_or_else(|| "/host-project".to_string());

    let root = project_root
        .as_deref()
        .unwrap_or(std::path::Path::new("/workspace"));
    let host_root = root.to_string_lossy();
    let parent = root
        .parent()
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();

    let cmd = format!(
        "umount -l /workspace 2>/dev/null; \
         mount --bind {mount_src} /workspace && \
         mount --make-rshared /workspace && \
         mkdir -p '{parent}' && ln -sfn /host-project '{host_root}'"
    );
    (mount_src, cmd)
}

async fn exec_remount(
    rt: &coast_docker::dind::DindRuntime,
    shell_container: &str,
    mount_cmd: &str,
) {
    use coast_docker::runtime::Runtime;
    match rt
        .exec_in_coast(shell_container, &["sh", "-c", mount_cmd])
        .await
    {
        Ok(r) if !r.success() => {
            warn!(stderr = %r.stderr, "shell /workspace remount returned non-zero");
        }
        Err(e) => {
            warn!(error = %e, "failed to remount shell /workspace");
        }
        _ => {
            info!("shell /workspace remounted to worktree");
        }
    }
}

async fn remount_workspace_in_shell(
    state: &AppState,
    shell_container: &str,
    wt_location: &Option<services::WorktreeLocation>,
    project_root: &Option<std::path::PathBuf>,
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

    let (mount_src, mount_cmd) = build_remount_cmd(wt_location, project_root);

    info!(
        shell_container = %shell_container,
        mount_src = %mount_src,
        "switching shell container /workspace to worktree"
    );

    exec_remount(&rt, shell_container, &mount_cmd).await;
}

/// Resolve the local worktree path and remount /workspace in the shell container.
///
/// Returns the host path to sync from (either the worktree dir or project root fallback).
async fn resolve_workspace_and_remount(
    req: &AssignRequest,
    state: &AppState,
    cf_data: &util::CoastfileData,
    project_root: &Option<std::path::PathBuf>,
    shell_container: &str,
) -> Result<std::path::PathBuf> {
    let (workspace_source, wt_location) =
        detect_workspace_source(req, cf_data, project_root).await?;
    remount_workspace_in_shell(state, shell_container, &wt_location, project_root).await;
    Ok(workspace_source)
}

/// Classify per-service actions and forward the assign request to coast-service.
async fn classify_and_forward_assign(
    req: &mut AssignRequest,
    cf_data: &util::CoastfileData,
    project_root: &Option<std::path::PathBuf>,
    previous_branch: &Option<String>,
    client: &super::remote::RemoteClient,
) -> Result<AssignResponse> {
    let service_names: Vec<String> = cf_data.assign.services.keys().cloned().collect();
    let changed_files = services::diff_changed_files(
        &cf_data.assign,
        project_root,
        previous_branch,
        &req.worktree,
    )
    .await;
    let service_actions = if !service_names.is_empty() {
        classify::classify_services(&service_names, &cf_data.assign, &changed_files)
    } else {
        std::collections::HashMap::new()
    };

    info!(
        actions = ?service_actions,
        "classified service actions for remote assign"
    );

    req.service_actions = service_actions;
    super::remote::forward::forward_assign(client, req).await
}

/// Update the local DB shadow and emit completion events.
async fn emit_assign_completion(
    req: &AssignRequest,
    state: &AppState,
    instance_status: &InstanceStatus,
    previous_branch: &Option<String>,
    remote_host: &str,
    remote_resp: AssignResponse,
    started_at: tokio::time::Instant,
) -> Result<AssignResponse> {
    let final_status = match instance_status {
        InstanceStatus::Assigning | InstanceStatus::Unassigning => InstanceStatus::Running,
        other => other.clone(),
    };
    let db = state.db.lock().await;
    db.update_instance_branch(
        &req.project,
        &req.name,
        Some(&req.worktree),
        req.commit_sha.as_deref(),
        &final_status,
    )?;
    db.set_worktree(&req.project, &req.name, Some(&req.worktree))?;
    drop(db);

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().into(),
    });
    state.emit_event(CoastEvent::InstanceAssigned {
        name: req.name.clone(),
        project: req.project.clone(),
        worktree: req.worktree.clone(),
    });

    info!(
        name = %req.name,
        worktree = %req.worktree,
        previous = ?previous_branch,
        host = %remote_host,
        "remote worktree assigned"
    );

    Ok(AssignResponse {
        name: req.name.clone(),
        worktree: remote_resp.worktree,
        previous_worktree: remote_resp
            .previous_worktree
            .or_else(|| previous_branch.clone()),
        time_elapsed_ms: started_at.elapsed().as_millis() as u64,
    })
}

/// Handle assign for a remote coast instance.
///
/// Instead of remounting /workspace inside a local DinD container, this:
/// 1. Resolves the worktree path locally
/// 2. Rsyncs the new worktree content to the remote /workspace
/// 3. Forwards an AssignRequest to coast-service to restart services
async fn handle_remote_assign(
    mut req: AssignRequest,
    state: &AppState,
    instance: &coast_core::types::CoastInstance,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    started_at: tokio::time::Instant,
) -> Result<AssignResponse> {
    info!(
        name = %req.name,
        worktree = %req.worktree,
        remote_host = ?instance.remote_host,
        "handling remote assign request"
    );

    let previous_branch = instance.branch.clone();
    let remote_host = instance.remote_host.as_deref().unwrap_or("unknown");

    {
        let db = state.db.lock().await;
        let _ = db.update_instance_status(
            &req.project,
            &req.name,
            &coast_core::types::InstanceStatus::Assigning,
        );
    }
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "assigning".into(),
    });

    let prev_status = instance.status.clone();
    let result = handle_remote_assign_inner(
        &mut req,
        instance,
        state,
        progress,
        &previous_branch,
        remote_host,
        started_at,
    )
    .await;

    if result.is_err() {
        revert_assign_status(state, &req.project, &req.name, &prev_status).await;
    }

    result
}

async fn handle_remote_assign_inner(
    req: &mut AssignRequest,
    instance: &coast_core::types::CoastInstance,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    previous_branch: &Option<String>,
    remote_host: &str,
    started_at: tokio::time::Instant,
) -> Result<AssignResponse> {
    const REMOTE_STEPS: u32 = 4;
    emit(
        progress,
        BuildProgressEvent::build_plan(vec![
            "Validating instance".into(),
            "Loading remote config".into(),
            "Syncing worktree".into(),
            "Remote assign".into(),
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
        BuildProgressEvent::started("Loading remote config", 2, REMOTE_STEPS),
    )
    .await;

    let cf_data = load_coastfile_data(&req.project);
    let remote_config =
        super::remote::resolve_remote_for_instance(&req.project, &req.name, state).await?;

    emit(
        progress,
        BuildProgressEvent::done("Loading remote config", "ok"),
    )
    .await;

    let session_name = super::remote::sync::mutagen_session_name(&req.project, &req.name);
    let shell_container = format!("{}-coasts-{}-shell", req.project, req.name);
    if let Some(docker) = state.docker.as_ref() {
        let _ =
            super::remote::sync::stop_mutagen_in_shell(&docker, &shell_container, &session_name)
                .await;
    }

    emit(
        progress,
        BuildProgressEvent::started("Syncing worktree", 3, REMOTE_STEPS),
    )
    .await;

    let project_root = read_project_root(&req.project);
    let workspace_source =
        resolve_workspace_and_remount(req, state, &cf_data, &project_root, &shell_container)
            .await?;

    let client = super::remote::RemoteClient::connect(&remote_config).await?;
    let service_home = client.query_service_home().await;
    let remote_workspace =
        super::remote::remote_workspace_path(&service_home, &req.project, &req.name);
    client
        .sync_workspace(&workspace_source, &remote_workspace)
        .await?;

    emit(progress, BuildProgressEvent::done("Syncing worktree", "ok")).await;

    emit(
        progress,
        BuildProgressEvent::started("Remote assign", 4, REMOTE_STEPS),
    )
    .await;

    let remote_resp =
        classify_and_forward_assign(req, &cf_data, &project_root, previous_branch, &client).await?;

    emit(progress, BuildProgressEvent::done("Remote assign", "ok")).await;

    if let Some(docker) = state.docker.as_ref() {
        super::run::start_mutagen_in_shell(
            &docker,
            &shell_container,
            &req.project,
            &req.name,
            &remote_workspace,
            &remote_config,
        )
        .await;
    }

    emit_assign_completion(
        req,
        state,
        &instance.status,
        previous_branch,
        remote_host,
        remote_resp,
        started_at,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppState;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn sample_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("old-branch".to_string()),
            commit_sha: None,
            container_id: Some(format!("{project}-coasts-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    /// Create a progress sender that discards events.
    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    #[tokio::test]
    async fn test_assign_instance_not_found() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "nonexistent".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_assign_stopped_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Stopped))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
        assert!(err.contains("coast start"));
    }

    #[tokio::test]
    async fn test_assign_checked_out_instance_preserves_status() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::CheckedOut,
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.worktree, "feature/x");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_assign_idle_instance_no_compose_down() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Idle,
            branch: None,
            commit_sha: None,
            container_id: Some("proj-coasts-dev-1".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert_eq!(resp.worktree, "feature/x");
        assert!(resp.previous_worktree.is_none());
    }

    #[tokio::test]
    async fn test_assign_running_instance_without_docker() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/new".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert_eq!(resp.worktree, "feature/new");
        assert_eq!(resp.previous_worktree, Some("old-branch".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.branch, Some("feature/new".to_string()));
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_assign_no_container_id_errors() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no container ID"));
    }

    #[tokio::test]
    async fn test_assign_stopped_instance_status_not_changed() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Stopped))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let _ = handle(req, &state, discard_progress()).await;

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(
            inst.status,
            InstanceStatus::Stopped,
            "status should remain Stopped after rejected assign"
        );
    }

    #[tokio::test]
    async fn test_assign_no_container_id_reverts_status() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Running,
            "no container ID error happens before status transition, so status should remain Running");
    }

    #[tokio::test]
    async fn test_assign_running_without_docker_status_becomes_running() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(
            inst.status,
            InstanceStatus::Running,
            "Running instance should stay Running after successful assign without Docker"
        );
        assert_eq!(inst.branch, Some("feature/x".to_string()));
    }

    #[tokio::test]
    async fn test_assign_idle_becomes_running_after_assign() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Idle,
            branch: None,
            commit_sha: None,
            container_id: Some("proj-coasts-dev-1".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(
            inst.status,
            InstanceStatus::Running,
            "Idle instance should become Running after successful assign"
        );
    }

    #[tokio::test]
    async fn test_assign_progress_events_emitted() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        };

        let result = handle(req, &state, tx).await;
        assert!(result.is_ok());

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        assert!(!events.is_empty(), "should emit progress events");
        assert!(
            events.iter().any(|e| e.status == "plan"),
            "should emit a build plan"
        );
        assert!(
            events.iter().any(|e| e.step == "Validating instance"),
            "should have validation step"
        );
    }
}
