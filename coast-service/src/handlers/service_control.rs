use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RemoteServiceControlRequest, RemoteServiceControlResponse};

use super::assign::COMPOSE_FILE_SH;
use crate::state::ServiceState;

pub async fn handle(
    req: RemoteServiceControlRequest,
    state: &ServiceState,
) -> Result<RemoteServiceControlResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        service = %req.service,
        action = %req.action,
        "remote service control"
    );

    let valid_actions = ["stop", "start", "restart"];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(CoastError::state(format!(
            "invalid service action '{}', must be one of: {}",
            req.action,
            valid_actions.join(", ")
        )));
    }

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    drop(db);

    let container_id = instance.container_id.ok_or_else(|| {
        CoastError::state(format!("remote instance '{}' has no container", req.name))
    })?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::state("docker is not available on this coast-service host"))?;

    let project_dir = super::run::resolve_artifact_dir(&req.project, Some("remote"))
        .or_else(|| super::run::resolve_artifact_dir(&req.project, None))
        .map(|d| super::run::read_compose_project_dir(&d))
        .unwrap_or_else(|| "/workspace".to_string());

    let cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} {} {}",
        req.action, req.service
    );

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let result = rt.exec_in_coast(&container_id, &["sh", "-c", &cmd]).await?;

    if !result.success() {
        warn!(
            service = %req.service,
            action = %req.action,
            stderr = %result.stderr,
            "service control command failed"
        );
        return Err(CoastError::state(format!(
            "compose {} {} failed: {}",
            req.action, req.service, result.stderr
        )));
    }

    info!(
        service = %req.service,
        action = %req.action,
        "service control succeeded"
    );

    Ok(RemoteServiceControlResponse { success: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};

    fn test_state() -> ServiceState {
        ServiceState::new_for_testing(ServiceDb::open_in_memory().unwrap())
    }

    fn control_req(action: &str) -> RemoteServiceControlRequest {
        RemoteServiceControlRequest {
            project: "proj".to_string(),
            name: "web".to_string(),
            service: "backend".to_string(),
            action: action.to_string(),
        }
    }

    #[tokio::test]
    async fn test_invalid_action() {
        let state = test_state();
        let err = handle(
            RemoteServiceControlRequest {
                action: "destroy".to_string(),
                ..control_req("stop")
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("invalid service action"));
    }

    #[tokio::test]
    async fn test_nonexistent_instance() {
        let state = test_state();
        let err = handle(control_req("stop"), &state).await.unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_no_container_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&crate::state::instances::RemoteInstance {
                name: "web".to_string(),
                project: "proj".to_string(),
                status: "running".to_string(),
                container_id: None,
                build_id: None,
                coastfile_type: None,
                worktree: None,
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .unwrap();
        }
        let err = handle(control_req("stop"), &state).await.unwrap_err();
        assert!(err.to_string().contains("has no container"));
    }

    #[tokio::test]
    async fn test_no_docker() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&crate::state::instances::RemoteInstance {
                name: "web".to_string(),
                project: "proj".to_string(),
                status: "running".to_string(),
                container_id: Some("abc123".to_string()),
                build_id: None,
                coastfile_type: None,
                worktree: None,
                created_at: "2024-01-01T00:00:00Z".to_string(),
            })
            .unwrap();
        }
        let err = handle(control_req("restart"), &state).await.unwrap_err();
        assert!(err.to_string().contains("docker is not available"));
    }
}
