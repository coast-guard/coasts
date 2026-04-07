use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RmRequest, RmResponse};

use crate::state::ServiceState;

pub async fn handle(req: RmRequest, state: &ServiceState) -> Result<RmResponse> {
    info!(name = %req.name, project = %req.project, "remote rm request");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;

    if let Some(ref container_id) = instance.container_id {
        remove_container_and_volume(state, container_id, &req.project, &req.name).await;
    }

    remove_workspace(&req.project, &req.name);
    db.delete_instance(&req.project, &req.name)?;

    Ok(RmResponse { name: req.name })
}

async fn remove_container_and_volume(
    state: &ServiceState,
    container_id: &str,
    project: &str,
    name: &str,
) {
    let Some(ref docker) = state.docker else {
        return;
    };
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let _ = rt.stop_coast_container(container_id).await;
    let _ = rt.remove_coast_container(container_id).await;

    let volume_name = coast_docker::dind::dind_cache_volume_name(project, name);
    match docker.remove_volume(&volume_name, None).await {
        Ok(()) => info!(volume = %volume_name, "removed DinD volume"),
        Err(e) => {
            info!(
                volume = %volume_name,
                error = %e,
                "DinD volume not found or already removed"
            )
        }
    }
}

fn remove_workspace(project: &str, name: &str) {
    let ws_path = crate::state::service_home()
        .join("workspaces")
        .join(project)
        .join(name);
    if ws_path.exists() {
        let _ = std::fs::remove_dir_all(&ws_path);
        info!(path = %ws_path.display(), "removed workspace directory");
    }
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

    async fn insert_instance(state: &ServiceState, name: &str, project: &str) {
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
    async fn test_rm_deletes_instance() {
        let state = test_state();
        insert_instance(&state, "web", "proj").await;

        let resp = handle(
            RmRequest {
                name: "web".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.name, "web");

        let db = state.db.lock().await;
        assert!(db.get_instance("proj", "web").unwrap().is_none());
    }

    #[tokio::test]
    async fn test_rm_nonexistent_errors() {
        let state = test_state();
        let err = handle(
            RmRequest {
                name: "nope".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[test]
    fn test_volume_name_derivation() {
        let name = coast_docker::dind::dind_cache_volume_name("cg", "x5");
        assert_eq!(name, "coast-dind--cg--x5");
    }

    #[test]
    fn test_workspace_path_derivation() {
        let path = crate::state::service_home()
            .join("workspaces")
            .join("cg")
            .join("dev-1");
        assert!(path.ends_with("workspaces/cg/dev-1"));
    }
}
