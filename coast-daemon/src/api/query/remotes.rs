use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use coast_core::protocol::RemoteResponse;
use coast_core::types::RemoteStatsResponse;

use crate::server::AppState;

#[derive(Deserialize)]
struct ArchParams {
    name: String,
}

async fn remotes_ls(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RemoteResponse>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let remotes = db.list_remotes().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(RemoteResponse {
        message: format!("{} remote(s)", remotes.len()),
        remotes,
    }))
}

async fn remotes_stats(State(state): State<Arc<AppState>>) -> Json<RemoteStatsResponse> {
    let cache = state.remote_stats_cache.lock().await;
    Json(RemoteStatsResponse {
        stats: cache.clone(),
    })
}

async fn remotes_arch(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ArchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let entry = db.get_remote(&params.name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;
    let entry = entry.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Remote '{}' not found", params.name) })),
        )
    })?;
    let cached_arch = db.get_remote_arch(&params.name).ok().flatten();
    drop(db);

    let connection = coast_core::types::RemoteConnection::from_entry(
        &entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );

    let arch = match crate::handlers::run::query_remote_arch_simple(&connection).await {
        Some(a) => {
            let db = state.db.lock().await;
            let _ = db.set_remote_arch(&params.name, &a);
            a
        }
        None => cached_arch.unwrap_or_else(|| "unknown".to_string()),
    };

    Ok(Json(serde_json::json!({ "arch": arch })))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/remotes", get(remotes_ls))
        .route("/remotes/stats", get(remotes_stats))
        .route("/remotes/arch", get(remotes_arch))
}
