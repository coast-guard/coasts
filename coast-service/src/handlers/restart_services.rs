use crate::state::ServiceState;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RestartServicesRequest, RestartServicesResponse};
use tracing::info;

pub async fn handle(
    req: RestartServicesRequest,
    state: &ServiceState,
) -> Result<RestartServicesResponse> {
    info!(name = %req.name, project = %req.project, "remote restart-services");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    drop(db);

    if let (Some(ref cid), Some(ref docker)) = (&instance.container_id, &state.docker) {
        let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
        use coast_docker::runtime::Runtime;
        let _ = rt.exec_in_coast(cid, &["sh", "-c",
            "docker compose -f /coast-artifact/compose.yml down -t 2 && docker compose -f /coast-artifact/compose.yml up -d --remove-orphans"
        ]).await;
    }

    Ok(RestartServicesResponse {
        name: req.name,
        services_restarted: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::instances::RemoteInstance;
    use crate::state::{ServiceDb, ServiceState};

    fn test_state() -> ServiceState {
        ServiceState::new_for_testing(ServiceDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str) -> RemoteInstance {
        RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "running".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_restart_nonexistent_instance() {
        let state = test_state();
        let req = RestartServicesRequest {
            name: "ghost".to_string(),
            project: "noproject".to_string(),
        };
        let err = handle(req, &state).await.unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_restart_no_docker() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("web", "myapp")).unwrap();
        }
        let req = RestartServicesRequest {
            name: "web".to_string(),
            project: "myapp".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.name, "web");
        assert!(resp.services_restarted.is_empty());
    }

    #[tokio::test]
    async fn test_restart_no_container_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            let mut inst = make_instance("api", "proj");
            inst.container_id = None;
            db.insert_instance(&inst).unwrap();
        }
        let req = RestartServicesRequest {
            name: "api".to_string(),
            project: "proj".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.name, "api");
        assert!(resp.services_restarted.is_empty());
    }

    #[tokio::test]
    async fn test_restart_success() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            let mut inst = make_instance("svc", "proj");
            inst.container_id = Some("abc123".to_string());
            db.insert_instance(&inst).unwrap();
        }
        let req = RestartServicesRequest {
            name: "svc".to_string(),
            project: "proj".to_string(),
        };
        // docker is None so the exec branch is skipped, but we still get Ok
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.name, "svc");
        assert!(resp.services_restarted.is_empty());
    }
}
