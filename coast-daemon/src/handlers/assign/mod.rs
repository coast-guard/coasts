/// Handler for the `coast assign` command.
///
/// Reassigns a worktree to an existing coast instance (runtime slot) without
/// recreating the DinD container. Uses the `[assign]` Coastfile config to
/// selectively stop/restart/rebuild only the services that need it.
mod classify;
mod explain;
mod gitignored_sync;
mod services;
mod util;
mod worktree;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{AssignRequest, AssignResponse, BuildProgressEvent, CoastEvent};
use coast_core::types::InstanceStatus;

use crate::server::AppState;

use util::{emit, load_coastfile_data, revert_assign_status, TOTAL_STEPS};

pub use explain::handle_explain;
pub use util::{has_compose, read_project_root};
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

    if let Some(ref docker) = state.docker {
        let result = services::run_docker_steps(services::DockerStepsParams {
            req: &req,
            state,
            progress: &progress,
            docker,
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
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
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
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
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
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
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
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
            explain: false,
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
