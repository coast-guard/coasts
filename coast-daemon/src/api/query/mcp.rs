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
    let req = McpLsRequest {
        name: params.name,
        project: params.project,
    };
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
    let req = McpToolsRequest {
        name: params.name,
        project: params.project,
        server: params.server,
        tool: params.tool,
    };
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
