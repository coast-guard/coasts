use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{ExecRequest, ExecResponse};

use crate::state::ServiceState;

pub async fn handle(req: ExecRequest, state: &ServiceState) -> Result<ExecResponse> {
    info!(name = %req.name, project = %req.project, "remote exec request");

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

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let cmd_refs: Vec<&str> = req.command.iter().map(String::as_str).collect();
    use coast_docker::runtime::Runtime;
    let result = rt.exec_in_coast(&container_id, &cmd_refs).await?;

    Ok(ExecResponse {
        exit_code: result.exit_code as i32,
        stdout: result.stdout,
        stderr: result.stderr,
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

    fn exec_req(name: &str, project: &str) -> ExecRequest {
        ExecRequest {
            name: name.to_string(),
            project: project.to_string(),
            service: None,
            root: false,
            command: vec!["echo".to_string(), "hello".to_string()],
        }
    }

    #[tokio::test]
    async fn test_exec_nonexistent_instance() {
        let state = test_state();
        let err = handle(exec_req("nope", "proj"), &state).await.unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_exec_no_container_id() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "inst".to_string(),
            project: "proj".to_string(),
            status: "running".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
        drop(db);

        let err = handle(exec_req("inst", "proj"), &state).await.unwrap_err();
        assert!(err.to_string().contains("has no container"));
    }

    #[tokio::test]
    async fn test_exec_no_docker_returns_error() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "inst".to_string(),
            project: "proj".to_string(),
            status: "running".to_string(),
            container_id: Some("abc123".to_string()),
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
        drop(db);

        let err = handle(exec_req("inst", "proj"), &state).await.unwrap_err();
        assert!(err.to_string().contains("docker is not available"));
    }
}
