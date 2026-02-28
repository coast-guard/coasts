use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{SearchDocsRequest, SearchDocsResponse};

use crate::handlers::docs;
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/docs/search", get(search_docs))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SearchDocsParams {
    pub q: String,
    pub limit: Option<usize>,
    pub language: Option<String>,
}

async fn search_docs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchDocsParams>,
) -> Result<Json<SearchDocsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let req = SearchDocsRequest {
        query: params.q,
        limit: params.limit,
        language: params.language,
    };
    match docs::handle_search_docs(req, &state).await {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}
