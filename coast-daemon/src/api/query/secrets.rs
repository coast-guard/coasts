use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{RevealSecretResponse, SecretInfo, SecretRequest};

use super::images::SecretsParams;
use crate::handlers;
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/secrets", get(list_secrets))
        .route("/secrets/reveal", get(reveal_secret))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct RevealSecretParams {
    pub project: String,
    pub name: String,
    pub secret: String,
}

async fn list_secrets(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SecretsParams>,
) -> Result<Json<Vec<SecretInfo>>, (StatusCode, Json<serde_json::Value>)> {
    let req = SecretRequest::List {
        instance: params.name,
        project: params.project,
    };

    let resp = handlers::secret::handle(req, &state).await.map_err(|e| {
        let message = e.to_string();
        let status = if message.contains("not found") {
            StatusCode::NOT_FOUND
        } else if message.contains("stopped") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, Json(serde_json::json!({ "error": message })))
    })?;

    Ok(Json(resp.secrets))
}

async fn reveal_secret(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RevealSecretParams>,
) -> Result<Json<RevealSecretResponse>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let _instance = db
        .get_instance(&params.project, &params.name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "error": format!("Instance '{}' not found", params.name) }),
                ),
            )
        })?;
    drop(db);

    if state.docker.is_none() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Docker not available" })),
        ));
    }

    let home = dirs::home_dir().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Cannot resolve home directory" })),
        )
    })?;
    let keystore_db_path = home.join(".coast").join("keystore.db");
    let keystore_key_path = home.join(".coast").join("keystore.key");

    let keystore = coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path)
        .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Keystore error: {e}") })),
        )
    })?;

    // Check per-instance override first, then project-level
    let instance_key = format!("{}/{}", params.project, params.name);
    let stored = keystore
        .get_secret(&instance_key, &params.secret)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
        .or_else(|| {
            keystore
                .get_secret(&params.project, &params.secret)
                .ok()
                .flatten()
        });

    match stored {
        Some(s) => {
            let value_str = String::from_utf8_lossy(&s.value).to_string();
            Ok(Json(RevealSecretResponse {
                name: params.secret,
                value: value_str,
            }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Secret '{}' not found", params.secret) })),
        )),
    }
}
