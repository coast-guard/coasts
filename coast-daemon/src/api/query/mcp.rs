use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::handlers;
use crate::server::AppState;

// --- MCP ---

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct McpQueryParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct McpToolsQueryParams {
    pub project: String,
    pub name: String,
    pub server: String,
    pub tool: Option<String>,
}

async fn mcp_ls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<McpQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use coast_core::protocol::McpLsRequest;

    let is_remote = {
        let db = state.db.lock().await;
        db.get_instance(&params.project, &params.name)
            .ok()
            .flatten()
            .is_some_and(|inst| inst.remote_host.is_some())
    };

    let req = McpLsRequest {
        name: params.name.clone(),
        project: params.project.clone(),
    };

    if is_remote {
        let remote_config = crate::handlers::remote::resolve_remote_for_instance(
            &params.project,
            &params.name,
            &state,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
        let client = crate::handlers::remote::RemoteClient::connect(&remote_config)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        let resp = crate::handlers::remote::forward::forward_mcp_ls(&client, &req)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        return Ok(Json(serde_json::to_value(resp).unwrap_or_default()));
    }

    match handlers::mcp::handle_ls(req, &state).await {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn mcp_tools(
    State(state): State<Arc<AppState>>,
    Query(params): Query<McpToolsQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use coast_core::protocol::McpToolsRequest;

    let is_remote = {
        let db = state.db.lock().await;
        db.get_instance(&params.project, &params.name)
            .ok()
            .flatten()
            .is_some_and(|inst| inst.remote_host.is_some())
    };

    let req = McpToolsRequest {
        name: params.name.clone(),
        project: params.project.clone(),
        server: params.server.clone(),
        tool: params.tool.clone(),
    };

    if is_remote {
        let remote_config = crate::handlers::remote::resolve_remote_for_instance(
            &params.project,
            &params.name,
            &state,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;
        let client = crate::handlers::remote::RemoteClient::connect(&remote_config)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        let resp = crate::handlers::remote::forward::forward_mcp_tools(&client, &req)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        return Ok(Json(serde_json::to_value(resp).unwrap_or_default()));
    }

    match handlers::mcp::handle_tools(req, &state).await {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn mcp_locations(
    State(state): State<Arc<AppState>>,
    Query(params): Query<McpQueryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    use coast_core::protocol::McpLocationsRequest;
    let req = McpLocationsRequest {
        name: params.name,
        project: params.project,
    };
    match handlers::mcp::handle_locations(req, &state).await {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mcp/ls", get(mcp_ls))
        .route("/mcp/tools", get(mcp_tools))
        .route("/mcp/locations", get(mcp_locations))
}
