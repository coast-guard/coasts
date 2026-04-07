use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{LogsRequest, LogsResponse};

use crate::state::ServiceState;

pub async fn handle(req: LogsRequest, state: &ServiceState) -> Result<LogsResponse> {
    info!(name = %req.name, project = %req.project, "remote logs request");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    drop(db);

    let container_id = match instance.container_id {
        Some(ref cid) => cid.clone(),
        None => {
            return Ok(LogsResponse {
                output: String::new(),
            });
        }
    };

    let Some(ref docker) = state.docker else {
        return Ok(LogsResponse {
            output: "(docker not available)".to_string(),
        });
    };

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;

    let artifact_dir = super::run::resolve_artifact_dir(&req.project, Some("remote"))
        .or_else(|| super::run::resolve_artifact_dir(&req.project, None));

    let project_dir = artifact_dir
        .as_ref()
        .map(|d| super::run::read_compose_project_dir(d))
        .unwrap_or_else(|| "/workspace".to_string());

    let compose_file = artifact_dir
        .as_ref()
        .filter(|d| d.join("compose.coast-shared.yml").exists())
        .map(|_| "/coast-artifact/compose.coast-shared.yml")
        .unwrap_or("/coast-artifact/compose.yml");

    let tail = if req.tail_all {
        "all".to_string()
    } else {
        req.tail.unwrap_or(200).to_string()
    };

    let mut cmd_parts = vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        compose_file.to_string(),
        "--project-directory".to_string(),
        project_dir,
        "logs".to_string(),
        "--no-color".to_string(),
        "--tail".to_string(),
        tail,
    ];

    if let Some(ref service) = req.service {
        cmd_parts.push(service.clone());
    }

    let cmd_refs: Vec<&str> = cmd_parts.iter().map(String::as_str).collect();
    let result = rt.exec_in_coast(&container_id, &cmd_refs).await?;

    Ok(LogsResponse {
        output: result.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};

    fn test_state() -> ServiceState {
        ServiceState::new_for_testing(ServiceDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_logs_nonexistent_instance() {
        let state = test_state();
        let req = LogsRequest {
            name: "nope".into(),
            project: "proj".into(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let err = handle(req, &state).await.unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_logs_no_container_returns_empty() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "dev-1".into(),
            project: "proj".into(),
            status: "running".into(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "dev-1".into(),
            project: "proj".into(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.output.is_empty());
    }

    #[tokio::test]
    async fn test_logs_no_docker_returns_message() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "dev-1".into(),
            project: "proj".into(),
            status: "running".into(),
            container_id: Some("abc123".into()),
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "dev-1".into(),
            project: "proj".into(),
            service: None,
            tail: Some(50),
            tail_all: false,
            follow: false,
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.output.contains("docker not available"));
    }

    #[tokio::test]
    async fn test_logs_no_docker_with_tail_all() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "ta".into(),
            project: "proj".into(),
            status: "running".into(),
            container_id: Some("cid".into()),
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "ta".into(),
            project: "proj".into(),
            service: None,
            tail: None,
            tail_all: true,
            follow: false,
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.output.contains("docker not available"));
    }

    #[tokio::test]
    async fn test_logs_no_docker_with_service_filter() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "sf".into(),
            project: "proj".into(),
            status: "running".into(),
            container_id: Some("cid".into()),
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "sf".into(),
            project: "proj".into(),
            service: Some("web".into()),
            tail: Some(100),
            tail_all: false,
            follow: false,
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.output.contains("docker not available"));
    }

    #[tokio::test]
    async fn test_logs_no_docker_default_tail() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "dt".into(),
            project: "proj".into(),
            status: "running".into(),
            container_id: Some("cid".into()),
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "dt".into(),
            project: "proj".into(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.output.contains("docker not available"));
    }

    #[tokio::test]
    async fn test_logs_different_project_errors() {
        let state = test_state();
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: "web".into(),
            project: "proj-a".into(),
            status: "running".into(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            worktree: None,
        })
        .unwrap();
        drop(db);

        let req = LogsRequest {
            name: "web".into(),
            project: "proj-b".into(),
            service: None,
            tail: None,
            tail_all: false,
            follow: false,
        };
        let err = handle(req, &state).await.unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }
}
