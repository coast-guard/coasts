use tracing::warn;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::{BareServiceConfig, CoastInstance, InstanceStatus, RuntimeType};

use crate::server::AppState;

use super::{detect_coastfile_info, emit, resolve_branch, resolve_latest_build_id};

/// All resolved state from Phase 1 needed by later provisioning phases.
#[derive(Debug)]
pub(super) struct ValidatedRun {
    pub build_id: Option<String>,
    pub has_compose: bool,
    pub compose_rel_dir: Option<String>,
    pub has_services: bool,
    pub bare_services: Vec<BareServiceConfig>,
    pub final_status: InstanceStatus,
    pub total_steps: u32,
}

/// Phase 1: Resolve build_id, validate DB state, insert instance record.
///
/// Handles: build_id resolution, coastfile detection, branch resolution,
/// dangling container check, duplicate instance check, Enqueued replacement,
/// and initial instance insertion.
pub(super) async fn validate_and_insert(
    req: &coast_core::protocol::RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<ValidatedRun> {
    let resolved_build_id = req
        .build_id
        .clone()
        .or_else(|| resolve_latest_build_id(&req.project, req.coastfile_type.as_deref()));

    let (has_compose, compose_rel_dir, has_services, bare_services) =
        detect_coastfile_info(&req.project, resolved_build_id.as_deref());

    let mut plan_steps = vec!["Preparing instance".to_string()];
    if has_compose {
        plan_steps.push("Building images".to_string());
    }
    plan_steps.push("Creating container".to_string());
    plan_steps.push("Loading cached images".to_string());
    if has_compose || has_services {
        plan_steps.push("Starting services".to_string());
    }
    plan_steps.push("Allocating ports".to_string());
    if req.worktree.is_some() {
        plan_steps.push("Assigning worktree".to_string());
    }
    let total_steps = plan_steps.len() as u32;
    emit(progress, BuildProgressEvent::build_plan(plan_steps));
    emit(
        progress,
        BuildProgressEvent::started("Preparing instance", 1, total_steps),
    );

    let final_status = if has_compose || has_services {
        InstanceStatus::Running
    } else {
        InstanceStatus::Idle
    };

    let resolved_branch = resolve_branch(
        req.branch.as_deref(),
        &req.project,
        resolved_build_id.as_deref(),
    )
    .await;

    check_dangling_container(req, state, progress).await?;

    {
        let db = state.db.lock().await;
        let existing = db.get_instance(&req.project, &req.name)?;
        match existing {
            Some(inst) if inst.status == InstanceStatus::Enqueued => {
                db.delete_instance(&req.project, &req.name)?;
            }
            Some(_) => {
                return Err(CoastError::InstanceAlreadyExists {
                    name: req.name.clone(),
                    project: req.project.clone(),
                });
            }
            None => {}
        }

        let instance = CoastInstance {
            name: req.name.clone(),
            project: req.project.clone(),
            status: InstanceStatus::Provisioning,
            branch: resolved_branch,
            commit_sha: req.commit_sha.clone(),
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: resolved_build_id.clone(),
            coastfile_type: req.coastfile_type.clone(),
            remote_host: req.remote.clone(),
        };
        db.insert_instance(&instance)?;
        state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
            name: req.name.clone(),
            project: req.project.clone(),
            status: "provisioning".to_string(),
        });
    }
    emit(
        progress,
        BuildProgressEvent::done("Preparing instance", "ok"),
    );

    Ok(ValidatedRun {
        build_id: resolved_build_id,
        has_compose,
        compose_rel_dir,
        has_services,
        bare_services,
        final_status,
        total_steps,
    })
}

async fn check_dangling_container(
    req: &coast_core::protocol::RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    let Some(docker) = state.docker.as_ref() else {
        return Ok(());
    };

    let containers_to_check = [
        format!("{}-coasts-{}", req.project, req.name),
        format!("{}-coasts-{}-shell", req.project, req.name),
    ];

    for container_name in &containers_to_check {
        if docker.inspect_container(container_name, None).await.is_ok() {
            if req.force_remove_dangling {
                warn!(
                    container = %container_name,
                    "force-removing dangling Docker container before run"
                );
                let opts = bollard::container::RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                };
                if let Err(e) = docker.remove_container(container_name, Some(opts)).await {
                    warn!(
                        container = %container_name,
                        error = %e,
                        "failed to remove dangling container"
                    );
                }
                let cache_vol = coast_docker::dind::dind_cache_volume_name(&req.project, &req.name);
                let _ = docker.remove_volume(&cache_vol, None).await;
                emit(
                    progress,
                    BuildProgressEvent::item(
                        "Preparing instance",
                        format!("Removed dangling container {container_name}"),
                        "warn",
                    ),
                );
            } else {
                return Err(CoastError::DanglingContainerDetected {
                    name: req.name.clone(),
                    project: req.project.clone(),
                    container_name: container_name.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    #[tokio::test]
    async fn test_validate_creates_provisioning_instance() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let progress = discard_progress();

        let req = coast_core::protocol::RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        let result = validate_and_insert(&req, &state, &progress).await;
        assert!(result.is_ok());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Provisioning);
    }

    #[tokio::test]
    async fn test_validate_rejects_duplicate() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let progress = discard_progress();

        let req = coast_core::protocol::RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        let _ = validate_and_insert(&req, &state, &progress).await.unwrap();

        let result = validate_and_insert(&req, &state, &progress).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[tokio::test]
    async fn test_validate_allows_same_name_in_different_projects_before_provisioning() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let progress = discard_progress();

        let req_a = coast_core::protocol::RunRequest {
            name: "main".to_string(),
            project: "project-a".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };
        let req_b = coast_core::protocol::RunRequest {
            name: "main".to_string(),
            project: "project-b".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        validate_and_insert(&req_a, &state, &progress)
            .await
            .unwrap();
        validate_and_insert(&req_b, &state, &progress)
            .await
            .unwrap();

        let db = state.db.lock().await;
        assert!(
            db.get_instance("project-a", "main").unwrap().is_some(),
            "cross-project name reuse should remain valid at the validation layer"
        );
        assert!(
            db.get_instance("project-b", "main").unwrap().is_some(),
            "cross-project name reuse should remain valid at the validation layer"
        );
    }

    #[tokio::test]
    async fn test_validate_replaces_enqueued() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Enqueued,
            branch: None,
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
        let progress = discard_progress();

        let req = coast_core::protocol::RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        let result = validate_and_insert(&req, &state, &progress).await;
        assert!(result.is_ok(), "should replace Enqueued instance");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Provisioning);
    }

    #[tokio::test]
    async fn test_validate_emits_plan() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let req = coast_core::protocol::RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        let _ = validate_and_insert(&req, &state, &tx).await.unwrap();

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert!(events.iter().any(|e| e.status == "plan"));
        assert!(events.iter().any(|e| e.step == "Preparing instance"));
    }

    #[tokio::test]
    async fn test_validate_plan_includes_assigning_worktree_step() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let req = coast_core::protocol::RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: Some("feature-oauth".to_string()),
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };

        let _ = validate_and_insert(&req, &state, &tx).await.unwrap();

        let mut plan = None;
        while let Ok(event) = rx.try_recv() {
            if event.status == "plan" {
                plan = event.plan;
                break;
            }
        }

        let steps = plan.expect("expected a build plan event");
        assert!(steps.iter().any(|step| step == "Assigning worktree"));
        assert_eq!(steps.last().map(String::as_str), Some("Assigning worktree"));
    }
}
