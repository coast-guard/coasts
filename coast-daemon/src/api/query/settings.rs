use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{GetSettingResponse, SettingResponse};

use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/settings", get(get_setting).post(set_setting))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct GetSettingParams {
    pub key: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SetSettingBody {
    pub key: String,
    pub value: String,
}

async fn get_setting(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GetSettingParams>,
) -> Result<Json<GetSettingResponse>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    match db.get_setting(&params.key) {
        Ok(Some(value)) => Ok(Json(GetSettingResponse {
            key: params.key,
            value: Some(value),
        })),
        Ok(None) => Ok(Json(GetSettingResponse {
            key: params.key,
            value: None,
        })),
        Err(e) => Err((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn set_setting(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetSettingBody>,
) -> Result<Json<SettingResponse>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    match db.set_setting(&body.key, &body.value) {
        Ok(()) => Ok(Json(SettingResponse {
            key: body.key,
            value: body.value,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}
