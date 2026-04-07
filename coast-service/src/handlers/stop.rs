use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{StopRequest, StopResponse};

use crate::state::ServiceState;

pub async fn handle(req: StopRequest, state: &ServiceState) -> Result<StopResponse> {
    info!(name = %req.name, project = %req.project, "remote stop request");

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
            if let Err(e) = rt.stop_coast_container(container_id).await {
                tracing::warn!(container_id, error = %e, "failed to stop container");
            }
        }
    }

    db.update_instance_status(&req.project, &req.name, "stopped")?;

    Ok(StopResponse { name: req.name })
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

    async fn insert_running_instance(state: &ServiceState, name: &str, project: &str) {
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "running".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
    }

    #[tokio::test]
    async fn test_stop_sets_status() {
        let state = test_state();
        insert_running_instance(&state, "web", "proj").await;

        let resp = handle(
            StopRequest {
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
        assert_eq!(inst.status, "stopped");
    }

    #[tokio::test]
    async fn test_stop_nonexistent_errors() {
        let state = test_state();
        let err = handle(
            StopRequest {
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
