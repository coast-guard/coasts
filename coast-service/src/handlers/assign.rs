use std::collections::HashMap;
use std::time::Instant;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{AssignRequest, AssignResponse};
use coast_core::types::AssignAction;

use crate::state::ServiceState;

pub(crate) const COMPOSE_FILE_SH: &str =
    "CF=/coast-artifact/compose.coast-shared.yml; [ -f \"$CF\" ] || CF=/coast-artifact/compose.yml";

fn resolve_project_dir(project: &str) -> String {
    let artifact_dir = super::run::resolve_artifact_dir(project, Some("remote"))
        .or_else(|| super::run::resolve_artifact_dir(project, None));
    artifact_dir
        .as_ref()
        .map(|d| super::run::read_compose_project_dir(d))
        .unwrap_or_else(|| "/workspace".to_string())
}

async fn stop_compose(rt: &coast_docker::dind::DindRuntime, container_id: &str, project_dir: &str) {
    use coast_docker::runtime::Runtime;
    info!(container_id, project_dir, "stopping compose services");
    let cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} down --remove-orphans"
    );
    if let Err(ref e) = rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await {
        warn!(error = %e, "compose down failed, continuing with remount");
    }
}

async fn start_compose(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project_dir: &str,
) {
    use coast_docker::runtime::Runtime;
    info!(container_id, project_dir, "starting compose services");

    let cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} up -d --remove-orphans"
    );
    match rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await {
        Ok(r) if !r.success() => {
            warn!(stderr = %r.stderr, "compose up returned non-zero after assign");
        }
        Err(e) => {
            warn!(error = %e, "compose up failed after assign");
        }
        _ => {}
    }
}

async fn restart_with_new_workspace(
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
) -> Result<()> {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let project_dir = resolve_project_dir(project);
    stop_compose(&rt, container_id, &project_dir).await;
    start_compose(&rt, container_id, &project_dir).await;
    Ok(())
}

fn partition_assign_actions(
    actions: &HashMap<String, AssignAction>,
) -> (bool, Vec<String>, Vec<String>) {
    let all_hot_or_none = actions
        .values()
        .all(|a| matches!(a, AssignAction::Hot | AssignAction::None));

    let restart_names: Vec<String> = actions
        .iter()
        .filter(|(_, a)| matches!(a, AssignAction::Restart))
        .map(|(name, _)| name.clone())
        .collect();

    let rebuild_names: Vec<String> = actions
        .iter()
        .filter(|(_, a)| matches!(a, AssignAction::Rebuild))
        .map(|(name, _)| name.clone())
        .collect();

    (all_hot_or_none, restart_names, rebuild_names)
}

async fn apply_bare_supervisor_actions(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    actions: &HashMap<String, AssignAction>,
) {
    use coast_docker::runtime::Runtime;
    info!("bare services detected, using supervisor restart");
    let stop_cmd = "/coast-supervisor/stop-all.sh 2>/dev/null || true";
    let _ = rt
        .exec_in_coast(container_id, &["sh", "-c", stop_cmd])
        .await;

    for svc in actions.keys() {
        let action = &actions[svc];
        if matches!(action, AssignAction::None | AssignAction::Hot) {
            continue;
        }
        let install_cmd = format!(
            "[ -f /coast-supervisor/{svc}.install.sh ] && /coast-supervisor/{svc}.install.sh || true"
        );
        let _ = rt
            .exec_in_coast(container_id, &["sh", "-c", &install_cmd])
            .await;
    }

    let start_cmd = "/coast-supervisor/start-all.sh 2>/dev/null || true";
    let _ = rt
        .exec_in_coast(container_id, &["sh", "-c", start_cmd])
        .await;
}

async fn compose_build_services(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project_dir: &str,
    services: &[String],
) {
    use coast_docker::runtime::Runtime;
    let svc_list = services.join(" ");
    let cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} build {svc_list}"
    );
    info!(services = %svc_list, "rebuilding services");
    match rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await {
        Ok(r) if !r.success() => {
            warn!(stderr = %r.stderr, "compose build returned non-zero");
        }
        Err(e) => {
            warn!(error = %e, "compose build failed");
        }
        _ => {}
    }
}

async fn compose_up_services(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project_dir: &str,
    services: &[String],
) {
    use coast_docker::runtime::Runtime;
    let all: Vec<&str> = services.iter().map(String::as_str).collect();
    let svc_list = all.join(" ");
    let cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} up -d --no-deps {svc_list}"
    );
    info!(services = %svc_list, "restarting services");
    match rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await {
        Ok(r) if !r.success() => {
            warn!(stderr = %r.stderr, "compose up returned non-zero");
        }
        Err(e) => {
            warn!(error = %e, "compose up failed");
        }
        _ => {}
    }
}

async fn apply_compose_actions(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project_dir: &str,
    restart_services: &[String],
    rebuild_services: &[String],
) {
    if !rebuild_services.is_empty() {
        compose_build_services(rt, container_id, project_dir, rebuild_services).await;
    }

    if !restart_services.is_empty() || !rebuild_services.is_empty() {
        let union: Vec<String> = restart_services
            .iter()
            .chain(rebuild_services.iter())
            .cloned()
            .collect();
        compose_up_services(rt, container_id, project_dir, &union).await;
    }
}

async fn apply_service_strategies(
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
    actions: &HashMap<String, AssignAction>,
) -> Result<()> {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let project_dir = resolve_project_dir(project);
    use coast_docker::runtime::Runtime;

    let (all_hot_or_none, restart_names, rebuild_names) = partition_assign_actions(actions);
    if all_hot_or_none {
        info!("all services hot/none, relying on hot-reload and periodic reconciler");
        return Ok(());
    }

    let has_bare = {
        let check = "[ -f /coast-supervisor/stop-all.sh ] && echo bare || echo compose".to_string();
        match rt.exec_in_coast(container_id, &["sh", "-c", &check]).await {
            Ok(r) => r.stdout.trim() == "bare",
            Err(_) => false,
        }
    };

    if has_bare {
        apply_bare_supervisor_actions(&rt, container_id, actions).await;
    } else {
        apply_compose_actions(
            &rt,
            container_id,
            &project_dir,
            &restart_names,
            &rebuild_names,
        )
        .await;
    }

    Ok(())
}

pub async fn handle(req: AssignRequest, state: &ServiceState) -> Result<AssignResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        worktree = %req.worktree,
        service_actions = ?req.service_actions,
        "remote assign request"
    );

    let started = Instant::now();

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    let previous_worktree = instance.worktree.clone();
    drop(db);

    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            if req.service_actions.is_empty() {
                restart_with_new_workspace(docker, container_id, &req.project).await?;
            } else {
                apply_service_strategies(docker, container_id, &req.project, &req.service_actions)
                    .await?;
            }
        }
    }

    let db = state.db.lock().await;
    db.update_instance_worktree(&req.project, &req.name, Some(&req.worktree))?;
    drop(db);

    let elapsed = started.elapsed();
    info!(
        name = %req.name,
        worktree = %req.worktree,
        previous = ?previous_worktree,
        elapsed_ms = elapsed.as_millis(),
        "assign complete"
    );

    Ok(AssignResponse {
        name: req.name,
        worktree: req.worktree,
        previous_worktree,
        time_elapsed_ms: elapsed.as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};
    use std::sync::Arc;

    fn test_state() -> Arc<ServiceState> {
        Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ))
    }

    fn assign_req(name: &str, project: &str, worktree: &str) -> AssignRequest {
        AssignRequest {
            name: name.to_string(),
            project: project.to_string(),
            worktree: worktree.to_string(),
            commit_sha: None,
            explain: false,
            force_sync: false,
            service_actions: Default::default(),
        }
    }

    async fn insert_instance(
        state: &ServiceState,
        name: &str,
        project: &str,
        worktree: Option<&str>,
    ) {
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "running".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: worktree.map(String::from),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
    }

    #[tokio::test]
    async fn test_assign_updates_worktree() {
        let state = test_state();
        insert_instance(&state, "web", "proj", None).await;

        let resp = handle(assign_req("web", "proj", "feature-branch"), &state)
            .await
            .unwrap();
        assert_eq!(resp.name, "web");
        assert_eq!(resp.worktree, "feature-branch");
        assert!(resp.previous_worktree.is_none());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "web").unwrap().unwrap();
        assert_eq!(inst.worktree.as_deref(), Some("feature-branch"));
    }

    #[tokio::test]
    async fn test_assign_returns_previous_worktree() {
        let state = test_state();
        insert_instance(&state, "web", "proj", Some("main")).await;

        let resp = handle(assign_req("web", "proj", "feature-x"), &state)
            .await
            .unwrap();
        assert_eq!(resp.worktree, "feature-x");
        assert_eq!(resp.previous_worktree, Some("main".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "web").unwrap().unwrap();
        assert_eq!(inst.worktree.as_deref(), Some("feature-x"));
    }

    #[tokio::test]
    async fn test_assign_nonexistent_errors() {
        let state = test_state();
        let err = handle(assign_req("nope", "proj", "main"), &state)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_assign_no_docker_still_updates_state() {
        let state = test_state();
        insert_instance(&state, "web", "proj", Some("old-branch")).await;

        let resp = handle(assign_req("web", "proj", "new-branch"), &state)
            .await
            .unwrap();
        assert_eq!(resp.worktree, "new-branch");
        assert_eq!(resp.previous_worktree, Some("old-branch".to_string()));
        assert!(resp.time_elapsed_ms < 1000);
    }

    #[tokio::test]
    async fn test_assign_with_container_id_no_docker_still_updates() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "with-cid".to_string(),
            project: "proj".to_string(),
            status: "running".to_string(),
            container_id: Some("abc123".to_string()),
            build_id: None,
            coastfile_type: None,
            worktree: Some("main".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
        drop(db);

        let resp = handle(assign_req("with-cid", "proj", "dev-branch"), &state)
            .await
            .unwrap();
        assert_eq!(resp.worktree, "dev-branch");
        assert_eq!(resp.previous_worktree, Some("main".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "with-cid").unwrap().unwrap();
        assert_eq!(inst.worktree.as_deref(), Some("dev-branch"));
    }

    #[tokio::test]
    async fn test_assign_multiple_sequential() {
        let state = test_state();
        insert_instance(&state, "multi", "proj", None).await;

        let r1 = handle(assign_req("multi", "proj", "branch-1"), &state)
            .await
            .unwrap();
        assert!(r1.previous_worktree.is_none());
        assert_eq!(r1.worktree, "branch-1");

        let r2 = handle(assign_req("multi", "proj", "branch-2"), &state)
            .await
            .unwrap();
        assert_eq!(r2.previous_worktree, Some("branch-1".to_string()));
        assert_eq!(r2.worktree, "branch-2");

        let r3 = handle(assign_req("multi", "proj", "branch-3"), &state)
            .await
            .unwrap();
        assert_eq!(r3.previous_worktree, Some("branch-2".to_string()));
        assert_eq!(r3.worktree, "branch-3");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "multi").unwrap().unwrap();
        assert_eq!(inst.worktree.as_deref(), Some("branch-3"));
    }

    #[tokio::test]
    async fn test_assign_same_worktree_is_noop() {
        let state = test_state();
        insert_instance(&state, "same", "proj", Some("main")).await;

        let resp = handle(assign_req("same", "proj", "main"), &state)
            .await
            .unwrap();
        assert_eq!(resp.worktree, "main");
        assert_eq!(resp.previous_worktree, Some("main".to_string()));
    }

    #[tokio::test]
    async fn test_assign_elapsed_time_is_reasonable() {
        let state = test_state();
        insert_instance(&state, "timing", "proj", None).await;

        let resp = handle(assign_req("timing", "proj", "feat"), &state)
            .await
            .unwrap();
        assert!(resp.time_elapsed_ms < 5000);
    }
}
