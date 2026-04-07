use std::sync::Arc;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::server::AppState;

const SKIP_FILES: &[&str] = &[
    "known_hosts",
    "known_hosts.old",
    "config",
    "authorized_keys",
    "environment",
];

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SshKeyEntry {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SshKeysResponse {
    pub keys: Vec<SshKeyEntry>,
}

async fn ssh_keys_ls() -> Result<Json<SshKeysResponse>, (StatusCode, Json<serde_json::Value>)> {
    let ssh_dir = dirs::home_dir().map(|h| h.join(".ssh")).ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "cannot determine home directory" })),
        )
    })?;

    let mut keys = Vec::new();

    let entries = std::fs::read_dir(&ssh_dir)
        .unwrap_or_else(|_| std::fs::read_dir("/dev/null").expect("fallback read_dir"));

    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();

        if fname.starts_with('.') || fname.ends_with(".pub") || fname.ends_with(".sock") {
            continue;
        }
        if SKIP_FILES.contains(&fname.as_str()) || fname.starts_with("config") {
            continue;
        }

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Ok(content) = std::fs::read_to_string(&path) {
            let first_line = content.lines().next().unwrap_or("");
            if !first_line.starts_with("-----BEGIN") || !first_line.contains("PRIVATE KEY") {
                continue;
            }
        } else {
            continue;
        }

        keys.push(SshKeyEntry {
            path: path.to_string_lossy().to_string(),
            name: fname,
        });
    }

    keys.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(SshKeysResponse { keys }))
}

#[derive(Debug, Deserialize)]
struct ValidateParams {
    path: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SshKeyValidateResponse {
    pub valid: bool,
    pub error: Option<String>,
}

async fn ssh_key_validate(Query(params): Query<ValidateParams>) -> Json<SshKeyValidateResponse> {
    let path = shellexpand::tilde(&params.path).to_string();
    let path = std::path::Path::new(&path);

    if !path.exists() {
        return Json(SshKeyValidateResponse {
            valid: false,
            error: Some("File not found".to_string()),
        });
    }

    if !path.is_file() {
        return Json(SshKeyValidateResponse {
            valid: false,
            error: Some("Not a file".to_string()),
        });
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return Json(SshKeyValidateResponse {
                valid: false,
                error: Some(format!("Cannot read file: {e}")),
            });
        }
    };

    let first_line = content.lines().next().unwrap_or("");

    if first_line.starts_with("-----BEGIN") && first_line.contains("PRIVATE KEY") {
        return Json(SshKeyValidateResponse {
            valid: true,
            error: None,
        });
    }

    if first_line.starts_with("-----BEGIN") && first_line.contains("PUBLIC KEY") {
        return Json(SshKeyValidateResponse {
            valid: false,
            error: Some("This is a public key. Use the private key instead.".to_string()),
        });
    }

    if first_line.starts_with("ssh-") || first_line.starts_with("ecdsa-") {
        return Json(SshKeyValidateResponse {
            valid: false,
            error: Some("This is a public key (.pub). Use the private key instead.".to_string()),
        });
    }

    Json(SshKeyValidateResponse {
        valid: false,
        error: Some("Not a recognized SSH private key format".to_string()),
    })
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ssh-keys", get(ssh_keys_ls))
        .route("/ssh-keys/validate", get(ssh_key_validate))
}
