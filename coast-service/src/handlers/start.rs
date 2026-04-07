use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{StartRequest, StartResponse};

use crate::state::ServiceState;

pub async fn handle(req: StartRequest, state: &ServiceState) -> Result<StartResponse> {
    info!(name = %req.name, project = %req.project, "remote start request");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;

    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            use coast_docker::runtime::Runtime;
            rt.start_coast_container(container_id).await?;
        }
    }

    db.update_instance_status(&req.project, &req.name, "running")?;

    Ok(StartResponse {
        name: req.name,
        ports: vec![],
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

    async fn insert_stopped_instance(state: &ServiceState, name: &str, project: &str) {
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "stopped".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
    }

    #[tokio::test]
    async fn test_start_sets_status() {
        let state = test_state();
        insert_stopped_instance(&state, "web", "proj").await;

        let resp = handle(
            StartRequest {
                name: "web".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.name, "web");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "web").unwrap().unwrap();
        assert_eq!(inst.status, "running");
    }

    #[tokio::test]
    async fn test_start_nonexistent_errors() {
        let state = test_state();
        let err = handle(
            StartRequest {
                name: "nope".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }
}
