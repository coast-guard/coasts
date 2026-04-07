use bollard::Docker;
use tracing::{info, warn};

use crate::state::ServiceState;

/// Reconcile DB state against Docker on startup.
///
/// Called once before the HTTP server begins accepting requests.
/// - `running` instances whose container is still running: no-op
/// - `running` instances whose container exists but is stopped: restart it
/// - `running` instances whose container is gone: delete from DB
/// - `provisioning` instances (interrupted by crash): clean up container if
///   present, then delete from DB
pub async fn reconcile_instances(state: &ServiceState) {
    let Some(ref docker) = state.docker else {
        info!("skipping reconciliation (no docker client)");
        return;
    };

    let Some(candidates) = load_reconciliation_candidates(state).await else {
        return;
    };

    info!(
        count = candidates.len(),
        "reconciling instances against docker"
    );

    for inst in &candidates {
        reconcile_one_instance(state, docker, inst).await;
    }

    info!("reconciliation complete");
}

/// Periodic heal: only checks running instances for dead inner services.
/// Does NOT touch provisioning instances to avoid racing with active runs.
pub async fn heal_running_instances(state: &ServiceState) {
    let Some(ref docker) = state.docker else {
        return;
    };

    let instances = {
        let db = state.db.lock().await;
        db.list_all_instances().unwrap_or_default()
    };

    let running: Vec<_> = instances
        .into_iter()
        .filter(|i| i.status == "running")
        .collect();

    if running.is_empty() {
        return;
    }

    for inst in &running {
        let Some(ref container_id) = inst.container_id else {
            continue;
        };
        if matches!(
            inspect_container(docker, container_id).await,
            ContainerStatus::Running
        ) {
            heal_inner_compose_services(docker, inst, container_id).await;
        }
    }
}

async fn load_reconciliation_candidates(
    state: &ServiceState,
) -> Option<Vec<crate::state::instances::RemoteInstance>> {
    let instances = {
        let db = state.db.lock().await;
        match db.list_all_instances() {
            Ok(list) => list,
            Err(e) => {
                warn!(error = %e, "failed to load instances for reconciliation");
                return None;
            }
        }
    };

    let candidates: Vec<_> = instances
        .into_iter()
        .filter(|i| i.status == "running" || i.status == "provisioning")
        .collect();

    if candidates.is_empty() {
        info!("reconciliation: no active instances to check");
        return None;
    }

    Some(candidates)
}

async fn reconcile_one_instance(
    state: &ServiceState,
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
) {
    let Some(ref container_id) = inst.container_id else {
        handle_no_container(state, inst).await;
        return;
    };

    match inspect_container(docker, container_id).await {
        ContainerStatus::Running => {
            heal_inner_compose_services(docker, inst, container_id).await;
        }
        ContainerStatus::Stopped => {
            handle_stopped_instance(state, docker, inst, container_id).await;
        }
        ContainerStatus::NotFound => {
            handle_missing_instance(state, inst, container_id).await;
        }
    }
}

async fn handle_stopped_instance(
    state: &ServiceState,
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    if inst.status == "provisioning" {
        cleanup_provisioning(state, docker, inst, container_id).await;
    } else {
        restart_stopped_container(state, docker, inst, container_id).await;
    }
}

async fn handle_missing_instance(
    state: &ServiceState,
    inst: &crate::state::instances::RemoteInstance,
    _container_id: &str,
) {
    if inst.status == "provisioning" {
        info!(
            name = %inst.name,
            project = %inst.project,
            "provisioning container gone, removing from DB"
        );
    } else {
        info!(
            name = %inst.name,
            project = %inst.project,
            "container gone, removing from DB"
        );
    }
    let db = state.db.lock().await;
    if let Err(e) = db.delete_instance(&inst.project, &inst.name) {
        warn!(error = %e, "failed to delete stale instance");
    }
}

async fn handle_no_container(state: &ServiceState, inst: &crate::state::instances::RemoteInstance) {
    info!(
        name = %inst.name,
        project = %inst.project,
        status = %inst.status,
        "no container_id, removing from DB"
    );
    let db = state.db.lock().await;
    if let Err(e) = db.delete_instance(&inst.project, &inst.name) {
        warn!(error = %e, "failed to delete instance without container_id");
    }
}

async fn cleanup_provisioning(
    state: &ServiceState,
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    info!(
        name = %inst.name,
        project = %inst.project,
        "provisioning container exists, cleaning up"
    );
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let _ = rt.stop_coast_container(container_id).await;
    let _ = rt.remove_coast_container(container_id).await;

    let db = state.db.lock().await;
    if let Err(e) = db.delete_instance(&inst.project, &inst.name) {
        warn!(error = %e, "failed to delete provisioning instance");
    }
}

async fn handle_container_restart_success(
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    info!(
        name = %inst.name,
        project = %inst.project,
        "restarted stopped container, waiting for inner daemon"
    );
    if let Err(e) = crate::handlers::run::wait_for_inner_daemon(docker, container_id).await {
        warn!(
            name = %inst.name,
            project = %inst.project,
            error = %e,
            "inner daemon not healthy after restart"
        );
        return;
    }
    info!(
        name = %inst.name,
        project = %inst.project,
        "inner daemon healthy, restarting compose services"
    );
    restart_compose_services(docker, inst, container_id).await;
}

async fn restart_compose_services(
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let project_dir = crate::handlers::run::resolve_artifact_dir(&inst.project, Some("remote"))
        .or_else(|| crate::handlers::run::resolve_artifact_dir(&inst.project, None))
        .map(|d| crate::handlers::run::read_compose_project_dir(&d))
        .unwrap_or_else(|| "/workspace".to_string());
    let cmd = format!(
        "{}; docker compose -f \"$CF\" --project-directory {} up -d 2>&1",
        crate::handlers::assign::COMPOSE_FILE_SH,
        project_dir,
    );
    match rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await {
        Ok(r) if !r.success() => {
            warn!(name = %inst.name, stderr = %r.stderr, "compose up after restart returned non-zero");
        }
        Err(e) => {
            warn!(name = %inst.name, error = %e, "failed to run compose up after restart");
        }
        _ => {
            info!(name = %inst.name, project = %inst.project, "compose services restarted");
        }
    }
}

async fn heal_inner_compose_services(
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;

    let project_dir = crate::handlers::run::resolve_artifact_dir(&inst.project, Some("remote"))
        .or_else(|| crate::handlers::run::resolve_artifact_dir(&inst.project, None))
        .map(|d| crate::handlers::run::read_compose_project_dir(&d))
        .unwrap_or_else(|| "/workspace".to_string());

    let check_cmd = format!(
        concat!(
            "{}; ",
            "EXPECTED=$(docker compose -f \"$CF\" --project-directory {} config --services 2>/dev/null | wc -l | tr -d ' '); ",
            "CONTAINERS=$(docker compose -f \"$CF\" --project-directory {} ps -a -q 2>/dev/null | wc -l | tr -d ' '); ",
            "echo \"$EXPECTED $CONTAINERS\"; ",
            "docker compose -f \"$CF\" --project-directory {} ps -a --format json 2>/dev/null || true"
        ),
        crate::handlers::assign::COMPOSE_FILE_SH,
        project_dir,
        project_dir,
        project_dir,
    );
    let result = match rt
        .exec_in_coast(container_id, &["sh", "-c", &check_cmd])
        .await
    {
        Ok(r) => r.stdout,
        Err(_) => return,
    };

    let mut lines = result.lines();
    let counts = lines.next().unwrap_or("0 0");
    let parts: Vec<&str> = counts.split_whitespace().collect();
    let expected: usize = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let containers: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let missing_containers = expected > 0 && containers < expected;

    let has_crashed = lines.any(|line| {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        let state = v.get("State").and_then(|s| s.as_str()).unwrap_or("");
        let exit_code = v
            .get("ExitCode")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        (state == "exited" && exit_code != 0) || state == "dead" || state == "removing"
    });

    if !missing_containers && !has_crashed {
        return;
    }

    info!(
        name = %inst.name,
        project = %inst.project,
        expected,
        containers,
        has_crashed,
        "detected unhealthy inner compose services, healing"
    );
    restart_compose_services(docker, inst, container_id).await;
}

async fn restart_stopped_container(
    state: &ServiceState,
    docker: &Docker,
    inst: &crate::state::instances::RemoteInstance,
    container_id: &str,
) {
    info!(
        name = %inst.name,
        project = %inst.project,
        "container stopped, attempting restart"
    );
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    match rt.start_coast_container(container_id).await {
        Ok(()) => {
            handle_container_restart_success(docker, inst, container_id).await;
        }
        Err(e) => {
            warn!(
                name = %inst.name,
                project = %inst.project,
                error = %e,
                "failed to restart container, marking stopped"
            );
            let db = state.db.lock().await;
            if let Err(e) = db.update_instance_status(&inst.project, &inst.name, "stopped") {
                warn!(error = %e, "failed to update instance status to stopped");
            }
        }
    }
}

enum ContainerStatus {
    Running,
    Stopped,
    NotFound,
}

async fn inspect_container(docker: &Docker, container_id: &str) -> ContainerStatus {
    match docker.inspect_container(container_id, None).await {
        Ok(info) => {
            let running = info.state.and_then(|s| s.running).unwrap_or(false);
            if running {
                ContainerStatus::Running
            } else {
                ContainerStatus::Stopped
            }
        }
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => ContainerStatus::NotFound,
        Err(e) => {
            warn!(container_id, error = %e, "failed to inspect container, treating as not found");
            ContainerStatus::NotFound
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::instances::RemoteInstance;
    use crate::state::{ServiceDb, ServiceState};

    fn test_state() -> ServiceState {
        ServiceState::new_for_testing(ServiceDb::open_in_memory().unwrap())
    }

    fn make_instance(
        name: &str,
        project: &str,
        status: &str,
        container_id: Option<&str>,
    ) -> RemoteInstance {
        RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: status.to_string(),
            container_id: container_id.map(|s| s.to_string()),
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_reconcile_no_docker_is_noop() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("a", "p", "running", Some("cid")))
                .unwrap();
        }

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        let inst = db.get_instance("p", "a").unwrap().unwrap();
        assert_eq!(inst.status, "running");
    }

    #[tokio::test]
    async fn test_reconcile_skips_stopped_instances() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("a", "p", "stopped", Some("cid")))
                .unwrap();
        }

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        let inst = db.get_instance("p", "a").unwrap().unwrap();
        assert_eq!(inst.status, "stopped");
    }

    #[tokio::test]
    async fn test_reconcile_no_docker_leaves_running_without_container_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("a", "p", "running", None))
                .unwrap();
        }

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        let inst = db.get_instance("p", "a").unwrap().unwrap();
        assert_eq!(inst.status, "running");
    }

    #[tokio::test]
    async fn test_reconcile_no_docker_leaves_provisioning_without_container_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("a", "p", "provisioning", None))
                .unwrap();
        }

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        let inst = db.get_instance("p", "a").unwrap().unwrap();
        assert_eq!(inst.status, "provisioning");
    }

    #[tokio::test]
    async fn test_reconcile_empty_db() {
        let state = test_state();
        reconcile_instances(&state).await;
    }

    /// Simulate a coast-service restart: write instances to a file-backed DB,
    /// drop it, reopen, and reconcile. Without Docker the running instance
    /// should survive unchanged (reconciliation is a no-op without Docker).
    /// Stopped instances are left alone regardless.
    #[tokio::test]
    async fn test_db_persists_across_restart_without_docker() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");

        {
            let db = ServiceDb::open(&db_path).unwrap();
            db.insert_instance(&make_instance("web", "proj", "running", Some("cid-abc")))
                .unwrap();
            db.insert_instance(&make_instance("bg", "proj", "stopped", None))
                .unwrap();
        }

        let db = ServiceDb::open(&db_path).unwrap();
        let state = ServiceState::new_for_testing(db);

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        let web = db.get_instance("proj", "web").unwrap().unwrap();
        assert_eq!(web.status, "running");
        assert_eq!(web.container_id.as_deref(), Some("cid-abc"));

        let bg = db.get_instance("proj", "bg").unwrap().unwrap();
        assert_eq!(bg.status, "stopped");
    }

    /// With Docker available, a running instance whose container no longer
    /// exists should be removed from the DB during reconciliation.
    #[tokio::test]
    #[ignore] // Requires running Docker daemon
    async fn test_reconcile_removes_instance_with_missing_container() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");

        {
            let db = ServiceDb::open(&db_path).unwrap();
            db.insert_instance(&make_instance(
                "web",
                "proj",
                "running",
                Some("nonexistent-container-id-xyz"),
            ))
            .unwrap();
        }

        let db = ServiceDb::open(&db_path).unwrap();
        let docker = bollard::Docker::connect_with_local_defaults()
            .expect("docker must be available for this test");
        let state = ServiceState {
            db: tokio::sync::Mutex::new(db),
            docker: Some(docker),
        };

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        assert!(
            db.get_instance("proj", "web").unwrap().is_none(),
            "instance with nonexistent container should be deleted"
        );
    }

    /// Provisioning instances with a nonexistent container should be cleaned up.
    #[tokio::test]
    #[ignore] // Requires running Docker daemon
    async fn test_reconcile_cleans_up_provisioning_with_missing_container() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("state.db");

        {
            let db = ServiceDb::open(&db_path).unwrap();
            db.insert_instance(&make_instance(
                "stale",
                "proj",
                "provisioning",
                Some("nonexistent-provisioning-id"),
            ))
            .unwrap();
        }

        let db = ServiceDb::open(&db_path).unwrap();
        let docker = bollard::Docker::connect_with_local_defaults()
            .expect("docker must be available for this test");
        let state = ServiceState {
            db: tokio::sync::Mutex::new(db),
            docker: Some(docker),
        };

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        assert!(
            db.get_instance("proj", "stale").unwrap().is_none(),
            "provisioning instance with missing container should be deleted"
        );
    }

    /// Running instances without a container_id should be removed during
    /// reconciliation when Docker is available.
    #[tokio::test]
    #[ignore] // Requires running Docker daemon
    async fn test_reconcile_removes_running_without_container_id() {
        let db = ServiceDb::open_in_memory().unwrap();
        db.insert_instance(&make_instance("orphan", "proj", "running", None))
            .unwrap();

        let docker = bollard::Docker::connect_with_local_defaults()
            .expect("docker must be available for this test");
        let state = ServiceState {
            db: tokio::sync::Mutex::new(db),
            docker: Some(docker),
        };

        reconcile_instances(&state).await;

        let db = state.db.lock().await;
        assert!(
            db.get_instance("proj", "orphan").unwrap().is_none(),
            "running instance without container_id should be deleted"
        );
    }
}
