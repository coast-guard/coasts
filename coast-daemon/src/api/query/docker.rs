use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::OnceCell;

use coast_core::protocol::{DockerInfoResponse, OpenDockerSettingsResponse};

use crate::server::AppState;

static CAN_ADJUST: OnceCell<bool> = OnceCell::const_new();

async fn check_docker_desktop_available() -> bool {
    match tokio::process::Command::new("docker")
        .args(["desktop", "version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

async fn can_adjust_memory() -> bool {
    *CAN_ADJUST.get_or_init(check_docker_desktop_available).await
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/docker/info", get(docker_info))
        .route("/docker/open-settings", post(open_docker_settings))
}

async fn docker_info(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DockerInfoResponse>, (StatusCode, Json<serde_json::Value>)> {
    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "Docker not available" })),
        )
    })?;

    let info = docker.info().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to query Docker: {e}") })),
        )
    })?;

    let mem_total_bytes = info.mem_total.unwrap_or(0).max(0) as u64;
    let cpus = info.ncpu.unwrap_or(0).max(0) as u64;
    let os = info.operating_system.unwrap_or_default();
    let server_version = info.server_version.unwrap_or_default();
    let can_adjust = can_adjust_memory().await;

    Ok(Json(DockerInfoResponse {
        mem_total_bytes,
        cpus,
        os,
        server_version,
        can_adjust,
    }))
}

async fn open_docker_settings(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<OpenDockerSettingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    if cfg!(target_os = "macos") {
        // Activate Docker Desktop and send Cmd+, to open Settings
        let status = tokio::process::Command::new("open")
            .args(["-a", "Docker Desktop"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to open Docker Desktop: {e}") })),
                )
            })?;

        Ok(Json(OpenDockerSettingsResponse {
            success: status.success(),
        }))
    } else {
        let status = tokio::process::Command::new("xdg-open")
            .arg("docker-desktop://dashboard/resources")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("Failed to open Docker Desktop: {e}") })),
                )
            })?;

        Ok(Json(OpenDockerSettingsResponse {
            success: status.success(),
        }))
    }
}
