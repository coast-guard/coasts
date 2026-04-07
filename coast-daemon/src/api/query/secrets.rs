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
    let is_remote = {
        let db = state.db.lock().await;
        db.get_instance(&params.project, &params.name)
            .ok()
            .flatten()
            .is_some_and(|inst| inst.remote_host.is_some())
    };

    if is_remote {
        let req = SecretRequest::List {
            instance: params.name.clone(),
            project: params.project.clone(),
        };
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
        let resp = crate::handlers::remote::forward::forward_secret_list(&client, &req)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        return Ok(Json(resp.secrets));
    }

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
    let is_remote = {
        let db = state.db.lock().await;
        db.get_instance(&params.project, &params.name)
            .ok()
            .flatten()
            .is_some_and(|inst| inst.remote_host.is_some())
    };

    if is_remote {
        let req = coast_core::protocol::RevealSecretRequest {
            project: params.project.clone(),
            name: params.name.clone(),
            secret: params.secret.clone(),
        };
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
        let resp = crate::handlers::remote::forward::forward_secrets_reveal(&client, &req)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
            })?;
        return Ok(Json(resp));
    }

    let (build_id, is_override) = {
        let db = state.db.lock().await;
        let instance = db
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
        let bid = instance.build_id.clone();
        drop(db);

        let override_key = format!("{}/{}", params.project, params.name);
        let is_override_secret = if state.docker.is_some() {
            dirs::home_dir()
                .and_then(|home| {
                    let ks_db = home.join(".coast").join("keystore.db");
                    let ks_key = home.join(".coast").join("keystore.key");
                    coast_secrets::keystore::Keystore::open(&ks_db, &ks_key)
                        .ok()
                        .and_then(|ks| ks.get_secret(&override_key, &params.secret).ok().flatten())
                })
                .is_some()
        } else {
            false
        };
        (bid, is_override_secret)
    };

    if !is_override {
        let declared = handlers::declared_secret_names(&params.project, build_id.as_deref());
        if let Some(ref allowed) = declared {
            if !allowed.contains(&params.secret) {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(
                        serde_json::json!({ "error": format!("Secret '{}' not found", params.secret) }),
                    ),
                ));
            }
        }
    }

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
