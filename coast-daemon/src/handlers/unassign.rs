/// Handler for the `coast unassign` command.
///
/// Returns an instance to the project root directory by remounting
/// `/workspace` to `/host-project`. Does not create any git worktrees
/// or modify git state. The host branch is read for display purposes only.
use std::path::Path;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, UnassignRequest, UnassignResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

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

/// Handle an unassign request with streaming progress.
///
/// Directly remounts `/workspace` back to the project root (`/host-project`)
/// without detecting or caring about git branches. Services are restarted
/// so their bind mounts resolve through the new mount.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
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

        if inst.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped (status: {}). \
                 Run `coast start {}` to start it first.",
                req.name, inst.status, req.name,
            )));
        }
        if !inst.status.can_assign() {
            return Err(CoastError::state(format!(
                "Instance '{}' is currently {}. Wait for the operation to complete.",
                req.name, inst.status
            )));
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

    if let Some(ref docker) = state.docker {
        let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

        let health_timeout = tokio::time::Duration::from_secs(10);
        let health_check = rt.exec_in_coast(&container_id, &["docker", "info"]);
        match tokio::time::timeout(health_timeout, health_check).await {
            Ok(Ok(r)) if r.success() => {
                info!("unassign: inner daemon healthy");
            }
            Ok(Ok(r)) => {
                revert_status(state, &req.project, &req.name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is not healthy (exit {}). \
                     Try `coast stop {} && coast start {}`.",
                    req.name, r.exit_code, req.name, req.name,
                )));
            }
            Ok(Err(e)) => {
                revert_status(state, &req.project, &req.name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Cannot reach inner Docker daemon in instance '{}': {e}. \
                     Try `coast stop {} && coast start {}`.",
                    req.name, req.name, req.name,
                )));
            }
            Err(_) => {
                revert_status(state, &req.project, &req.name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is unresponsive (timed out after {}s). \
                     Try `coast rm {} && coast run {}`.",
                    req.name,
                    health_timeout.as_secs(),
                    req.name,
                    req.name,
                )));
            }
        }

        emit(
            &progress,
            BuildProgressEvent::done("Checking inner daemon", "ok"),
        )
        .await;

        // --- Step 3: Remount /workspace to project root ---
        emit(
            &progress,
            BuildProgressEvent::started("Switching to project root", 3, TOTAL_STEPS),
        )
        .await;

        let mount_cmd =
            "umount -l /workspace 2>/dev/null; mount --bind /host-project /workspace && mount --make-rshared /workspace";
        let mount_result = rt
            .exec_in_coast(&container_id, &["sh", "-c", mount_cmd])
            .await;
        match &mount_result {
            Ok(r) if r.success() => {
                info!(name = %req.name, "remounted /workspace to project root");
            }
            Ok(r) => {
                warn!(name = %req.name, stderr = %r.stderr, "failed to remount /workspace to project root");
            }
            Err(e) => {
                warn!(name = %req.name, error = %e, "failed to remount /workspace to project root");
            }
        }

        {
            let db = state.db.lock().await;
            let _ = db.set_worktree(&req.project, &req.name, None);
        }

        emit(
            &progress,
            BuildProgressEvent::done("Switching to project root", "ok"),
        )
        .await;

        // --- Step 4: Restart services ---
        emit(
            &progress,
            BuildProgressEvent::started("Restarting services", 4, TOTAL_STEPS),
        )
        .await;

        let has_compose = super::assign::has_compose(&req.project);

        if has_compose {
            let ctx = super::compose_context_for_build(&req.project, instance.build_id.as_deref());
            let up_cmd = ctx.compose_shell("up -d --force-recreate --remove-orphans -t 1");
            let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
            let up_result = rt.exec_in_coast(&container_id, &up_refs).await;
            match &up_result {
                Ok(r) if r.success() => {
                    info!(name = %req.name, "compose force-recreate completed after unassign");
                }
                Ok(r) => {
                    warn!(name = %req.name, stderr = %r.stderr, "compose up after unassign had issues");
                }
                Err(e) => {
                    warn!(name = %req.name, error = %e, "compose up after unassign failed");
                }
            }
        }

        if crate::bare_services::has_bare_services(docker, &container_id).await {
            let stop_cmd = crate::bare_services::generate_stop_command();
            match rt
                .exec_in_coast(&container_id, &["sh", "-c", &stop_cmd])
                .await
            {
                Ok(r) if r.success() => {
                    info!(name = %req.name, "bare services stopped for unassign");
                }
                Ok(r) => {
                    warn!(name = %req.name, stderr = %r.stderr, "bare services stop had issues");
                }
                Err(e) => {
                    warn!(name = %req.name, error = %e, "bare services stop failed");
                }
            }

            let home = dirs::home_dir().unwrap_or_default();
            let cf_path = instance
                .build_id
                .as_ref()
                .map(|bid| {
                    home.join(".coast")
                        .join("images")
                        .join(&req.project)
                        .join(bid)
                        .join("coastfile.toml")
                })
                .filter(|p| p.exists())
                .unwrap_or_else(|| {
                    home.join(".coast")
                        .join("images")
                        .join(&req.project)
                        .join("latest")
                        .join("coastfile.toml")
                });
            let svc_list = coast_core::coastfile::Coastfile::from_file(&cf_path)
                .map(|cf| cf.services)
                .unwrap_or_default();

            let stop_cmd = crate::bare_services::generate_stop_command();
            let _ = rt
                .exec_in_coast(&container_id, &["sh", "-c", &stop_cmd])
                .await;
            let start_cmd = crate::bare_services::generate_install_and_start_command(&svc_list);
            match rt
                .exec_in_coast(&container_id, &["sh", "-c", &start_cmd])
                .await
            {
                Ok(r) if r.success() => {
                    info!(name = %req.name, "bare services restarted after unassign");
                }
                Ok(r) => {
                    warn!(name = %req.name, stderr = %r.stderr, stdout = %r.stdout, "bare services start after unassign had issues");
                }
                Err(e) => {
                    warn!(name = %req.name, error = %e, "bare services start after unassign failed");
                }
            }
        }

        emit(
            &progress,
            BuildProgressEvent::done("Restarting services", "ok"),
        )
        .await;
    } else {
        // No Docker client (test mode): just update DB
        emit(
            &progress,
            BuildProgressEvent::done("Checking inner daemon", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Switching to project root", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Restarting services", "skip"),
        )
        .await;

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
}
