use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{
    HostServiceInspectResponse, ProjectSharedSummary, SharedAllResponse, SharedRequest,
};

use crate::handlers;
use crate::server::AppState;

// ---------------------------------------------------------------------------
// Shared services list (GET)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SharedLsParams {
    pub project: String,
}

async fn shared_ls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SharedLsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let req = SharedRequest::Ps {
        project: params.project,
    };
    match handlers::shared::handle(req, &state).await {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

/// Returns all shared services across all projects, grouped by project.
async fn shared_ls_all(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SharedAllResponse>, (StatusCode, Json<serde_json::Value>)> {
    let rows = {
        let db = state.db.lock().await;
        db.list_shared_services(None).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
    };

    let mut map: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for row in &rows {
        let entry = map.entry(row.project.clone()).or_insert((0, 0));
        entry.0 += 1;
        if row.status == "running" {
            entry.1 += 1;
        }
    }

    let projects: Vec<ProjectSharedSummary> = map
        .into_iter()
        .map(|(project, (total, running))| ProjectSharedSummary {
            project,
            total,
            running,
        })
        .collect();

    Ok(Json(SharedAllResponse { projects }))
}

// ---------------------------------------------------------------------------
// Host service inspect (GET) — direct bollard inspect on host container
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct HostServiceInspectParams {
    pub project: String,
    pub service: String,
}

async fn host_service_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostServiceInspectParams>,
) -> Result<Json<HostServiceInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let container_name =
        crate::shared_services::shared_container_name(&params.project, &params.service);
    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Docker not available" })),
        )
    })?;
    let inspect = docker.inspect_container(&container_name, None).await.map_err(|e| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": format!("Container '{}' not found: {}", container_name, e) })))
    })?;
    let val = serde_json::to_value(vec![inspect]).unwrap_or_default();
    Ok(Json(HostServiceInspectResponse { inspect: val }))
}

// ---------------------------------------------------------------------------
// Host image inspect (GET) — inspect image on host daemon
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct HostImageInspectParams {
    pub project: String,
    pub image: String,
}

async fn host_image_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostImageInspectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _project = &params.project;
    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Docker not available" })),
        )
    })?;
    let inspect = docker.inspect_image(&params.image).await.map_err(|e| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": format!("Image '{}' not found: {}", params.image, e) })))
    })?;
    Ok(Json(serde_json::to_value(inspect).unwrap_or_default()))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/shared/ls", get(shared_ls))
        .route("/shared/ls-all", get(shared_ls_all))
        .route("/host-service/inspect", get(host_service_inspect))
        .route("/host-image/inspect", get(host_image_inspect))
}
