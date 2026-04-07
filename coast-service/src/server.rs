use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use coast_core::protocol::*;
use coast_core::protocol::{McpLsRequest, McpLsResponse, McpToolsRequest, McpToolsResponse};

use crate::handlers;
use crate::state::ServiceState;

pub fn router(state: Arc<ServiceState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/info", get(info))
        .route("/build", post(handle_build))
        .route("/run", post(handle_run))
        .route("/exec", post(handle_exec))
        .route("/stop", post(handle_stop))
        .route("/start", post(handle_start))
        .route("/rm", post(handle_rm))
        .route("/ps", post(handle_ps))
        .route("/logs", post(handle_logs))
        .route("/assign", post(handle_assign))
        .route("/secret", post(handle_secret))
        .route("/restart-services", post(handle_restart_services))
        .route("/service/control", post(handle_service_control))
        .route("/container-stats", post(handle_container_stats))
        .route("/secrets/reveal", post(handle_secrets_reveal))
        .route("/mcp/ls", post(handle_mcp_ls))
        .route("/mcp/tools", post(handle_mcp_tools))
        .route("/prune", post(handle_prune))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn info() -> Json<serde_json::Value> {
    let home = crate::state::service_home();
    Json(serde_json::json!({
        "service_home": home.display().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn handle_build(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<BuildRequest>,
) -> Result<Json<BuildResponse>, (StatusCode, String)> {
    handlers::build::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_run(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<RunRequest>,
) -> Result<Json<RunResponse>, (StatusCode, String)> {
    handlers::run::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_exec(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, (StatusCode, String)> {
    handlers::exec::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_stop(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<StopRequest>,
) -> Result<Json<StopResponse>, (StatusCode, String)> {
    handlers::stop::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_start(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, (StatusCode, String)> {
    handlers::start::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_rm(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<RmRequest>,
) -> Result<Json<RmResponse>, (StatusCode, String)> {
    handlers::rm::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_prune(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<coast_core::protocol::api_types::PruneRequest>,
) -> Result<Json<coast_core::protocol::api_types::PruneResponse>, (StatusCode, String)> {
    handlers::prune::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_ps(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<PsRequest>,
) -> Result<Json<PsResponse>, (StatusCode, String)> {
    handlers::ps::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_logs(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<LogsRequest>,
) -> Result<Json<LogsResponse>, (StatusCode, String)> {
    handlers::logs::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_assign(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<AssignRequest>,
) -> Result<Json<AssignResponse>, (StatusCode, String)> {
    handlers::assign::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_secret(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<SecretRequest>,
) -> Result<Json<SecretResponse>, (StatusCode, String)> {
    handlers::secret::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_service_control(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<RemoteServiceControlRequest>,
) -> Result<Json<RemoteServiceControlResponse>, (StatusCode, String)> {
    handlers::service_control::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_restart_services(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<RestartServicesRequest>,
) -> Result<Json<RestartServicesResponse>, (StatusCode, String)> {
    handlers::restart_services::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_secrets_reveal(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<RevealSecretRequest>,
) -> Result<Json<RevealSecretResponse>, (StatusCode, String)> {
    handlers::secret::handle_reveal(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_mcp_ls(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<McpLsRequest>,
) -> Result<Json<McpLsResponse>, (StatusCode, String)> {
    handlers::mcp::handle_ls(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_mcp_tools(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<McpToolsRequest>,
) -> Result<Json<McpToolsResponse>, (StatusCode, String)> {
    handlers::mcp::handle_tools(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn handle_container_stats(
    State(state): State<Arc<ServiceState>>,
    Json(req): Json<ContainerStatsRequest>,
) -> Result<Json<ContainerStatsResponse>, (StatusCode, String)> {
    handlers::container_stats::handle(req, &state)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServiceDb;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_router() -> Router {
        let state = Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ));
        router(state)
    }

    fn test_router_with_state() -> (Router, Arc<ServiceState>) {
        let state = Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ));
        (router(state.clone()), state)
    }

    fn post_json(uri: &str, body: &impl serde::Serialize) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(body).unwrap()))
            .unwrap()
    }

    fn run_request(name: &str, project: &str) -> RunRequest {
        RunRequest {
            name: name.to_string(),
            project: project.to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        }
    }

    async fn body_string(resp: axum::http::Response<Body>) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn test_health() {
        let app = test_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_string(resp).await, "ok");
    }

    #[tokio::test]
    async fn test_run_success() {
        let app = test_router();
        let resp = app
            .oneshot(post_json("/run", &run_request("inst", "proj")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_string(resp).await;
        let run_resp: RunResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(run_resp.name, "inst");
    }

    #[tokio::test]
    async fn test_run_duplicate_returns_500() {
        let (app, _state) = test_router_with_state();
        let req = run_request("dup", "proj");

        let resp = app.clone().oneshot(post_json("/run", &req)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app.oneshot(post_json("/run", &req)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_stop_success() {
        let (app, _state) = test_router_with_state();
        app.clone()
            .oneshot(post_json("/run", &run_request("web", "proj")))
            .await
            .unwrap();

        let resp = app
            .oneshot(post_json(
                "/stop",
                &StopRequest {
                    name: "web".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_stop_nonexistent_returns_500() {
        let app = test_router();
        let resp = app
            .oneshot(post_json(
                "/stop",
                &StopRequest {
                    name: "nope".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_start_success() {
        let (app, _state) = test_router_with_state();
        app.clone()
            .oneshot(post_json("/run", &run_request("web", "proj")))
            .await
            .unwrap();

        let resp = app
            .oneshot(post_json(
                "/start",
                &StartRequest {
                    name: "web".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_rm_success() {
        let (app, state) = test_router_with_state();
        app.clone()
            .oneshot(post_json("/run", &run_request("web", "proj")))
            .await
            .unwrap();

        let resp = app
            .oneshot(post_json(
                "/rm",
                &RmRequest {
                    name: "web".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let db = state.db.lock().await;
        assert!(db.get_instance("proj", "web").unwrap().is_none());
    }

    #[tokio::test]
    async fn test_ps_nonexistent_returns_500() {
        let app = test_router();
        let resp = app
            .oneshot(post_json(
                "/ps",
                &PsRequest {
                    name: "nope".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_exec_no_container_returns_500() {
        let (app, _state) = test_router_with_state();
        app.clone()
            .oneshot(post_json("/run", &run_request("web", "proj")))
            .await
            .unwrap();

        let resp = app
            .oneshot(post_json(
                "/exec",
                &ExecRequest {
                    name: "web".to_string(),
                    project: "proj".to_string(),
                    service: None,
                    root: false,
                    command: vec!["echo".to_string()],
                },
            ))
            .await
            .unwrap();
        // No docker in test mode => error when container_id is empty
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_assign_success() {
        let (app, _state) = test_router_with_state();
        app.clone()
            .oneshot(post_json("/run", &run_request("web", "proj")))
            .await
            .unwrap();

        let resp = app
            .oneshot(post_json(
                "/assign",
                &AssignRequest {
                    name: "web".to_string(),
                    project: "proj".to_string(),
                    worktree: "feature-x".to_string(),
                    commit_sha: None,
                    explain: false,
                    force_sync: false,
                    service_actions: Default::default(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_string(resp).await;
        let assign_resp: AssignResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(assign_resp.worktree, "feature-x");
    }

    #[tokio::test]
    async fn test_full_lifecycle() {
        let (app, state) = test_router_with_state();

        // 1. Run
        let resp = app
            .clone()
            .oneshot(post_json("/run", &run_request("svc", "proj")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 2. Ps
        let resp = app
            .clone()
            .oneshot(post_json(
                "/ps",
                &PsRequest {
                    name: "svc".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Stop
        let resp = app
            .clone()
            .oneshot(post_json(
                "/stop",
                &StopRequest {
                    name: "svc".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        {
            let db = state.db.lock().await;
            let inst = db.get_instance("proj", "svc").unwrap().unwrap();
            assert_eq!(inst.status, "stopped");
        }

        // 4. Start
        let resp = app
            .clone()
            .oneshot(post_json(
                "/start",
                &StartRequest {
                    name: "svc".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        {
            let db = state.db.lock().await;
            let inst = db.get_instance("proj", "svc").unwrap().unwrap();
            assert_eq!(inst.status, "running");
        }

        // 5. Rm
        let resp = app
            .oneshot(post_json(
                "/rm",
                &RmRequest {
                    name: "svc".to_string(),
                    project: "proj".to_string(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        {
            let db = state.db.lock().await;
            assert!(db.get_instance("proj", "svc").unwrap().is_none());
        }
    }
}
