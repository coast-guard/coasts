use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use coast_core::protocol::{
    CoastEvent, GetAnalyticsResponse, GetLanguageResponse, SetAnalyticsResponse,
    SetLanguageResponse,
};

use crate::analytics::AnalyticsMetadata;
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/config/language", get(get_language).post(set_language))
        .route("/config/analytics", get(get_analytics).post(set_analytics))
        .route("/analytics/track", post(track_event))
}

async fn get_language(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GetLanguageResponse>, (StatusCode, Json<serde_json::Value>)> {
    Ok(Json(GetLanguageResponse {
        language: state.language(),
    }))
}

#[derive(Deserialize)]
struct SetLanguageBody {
    language: String,
}

async fn set_language(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetLanguageBody>,
) -> Result<Json<SetLanguageResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !coast_i18n::is_valid_language(&body.language) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!(
                    "Unsupported language '{}'. Supported languages: {}",
                    body.language,
                    coast_i18n::SUPPORTED_LANGUAGES.join(", "),
                )
            })),
        ));
    }

    let db = state.db.lock().await;
    db.set_language(&body.language).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;
    drop(db);

    let _ = state.language_tx.send(body.language.clone());

    state.emit_event(CoastEvent::ConfigLanguageChanged {
        language: body.language.clone(),
    });

    Ok(Json(SetLanguageResponse {
        language: body.language,
    }))
}

async fn get_analytics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GetAnalyticsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let enabled = db.get_analytics_enabled().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(GetAnalyticsResponse { enabled }))
}

#[derive(Deserialize)]
struct SetAnalyticsBody {
    enabled: bool,
}

async fn set_analytics(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetAnalyticsBody>,
) -> Result<Json<SetAnalyticsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    db.set_analytics_enabled(body.enabled).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;
    drop(db);

    state.emit_event(CoastEvent::ConfigAnalyticsChanged {
        enabled: body.enabled,
    });

    Ok(Json(SetAnalyticsResponse {
        enabled: body.enabled,
    }))
}

#[derive(Deserialize)]
struct TrackEventBody {
    event: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    metadata: AnalyticsMetadata,
}

async fn track_event(
    State(state): State<Arc<AppState>>,
    Json(body): Json<TrackEventBody>,
) -> StatusCode {
    let metadata = if body.metadata.is_empty() {
        None
    } else {
        Some(body.metadata)
    };
    state
        .analytics
        .track_web_event(&body.event, body.url.as_deref(), metadata);
    StatusCode::NO_CONTENT
}
