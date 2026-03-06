use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::OnceCell;

use coast_core::protocol::{DockerInfoResponse, OpenDockerSettingsResponse};

use crate::server::AppState;

static CAN_ADJUST: OnceCell<bool> = OnceCell::const_new();
static PROVIDER: OnceCell<String> = OnceCell::const_new();

fn detect_provider(os: &str) -> &'static str {
    if os.eq_ignore_ascii_case("orbstack") {
        "orbstack"
    } else {
        "docker-desktop"
    }
}

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

async fn docker_info(State(state): State<Arc<AppState>>) -> Json<DockerInfoResponse> {
    let disconnected = Json(DockerInfoResponse {
        connected: false,
        mem_total_bytes: 0,
        cpus: 0,
        os: String::new(),
        server_version: String::new(),
        can_adjust: false,
        provider: String::new(),
    });

    let Some(docker) = state.docker.as_ref() else {
        return disconnected;
    };

    let Ok(info) = docker.info().await else {
        return disconnected;
    };

    let mem_total_bytes = info.mem_total.unwrap_or(0).max(0) as u64;
    let cpus = info.ncpu.unwrap_or(0).max(0) as u64;
    let os = info.operating_system.unwrap_or_default();
    let server_version = info.server_version.unwrap_or_default();
    let can_adjust = can_adjust_memory().await;
    let provider = PROVIDER
        .get_or_init(|| async { detect_provider(&os).to_string() })
        .await
        .clone();

    Json(DockerInfoResponse {
        connected: true,
        mem_total_bytes,
        cpus,
        os,
        server_version,
        can_adjust,
        provider,
    })
}

async fn open_docker_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OpenDockerSettingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let provider = PROVIDER
        .get()
        .map(String::as_str)
        .unwrap_or("docker-desktop");
    let is_orbstack = provider == "orbstack";

    if !is_orbstack {
        if let Some(docker) = state.docker.as_ref() {
            if let Ok(info) = docker.info().await {
                let os = info.operating_system.unwrap_or_default();
                if detect_provider(&os) == "orbstack" {
                    return open_orbstack().await;
                }
            }
        }
    }

    if is_orbstack {
        return open_orbstack().await;
    }

    if cfg!(target_os = "macos") {
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

async fn open_orbstack(
) -> Result<Json<OpenDockerSettingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let status = tokio::process::Command::new("open")
        .args(["-a", "OrbStack"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to open OrbStack: {e}") })),
            )
        })?;

    Ok(Json(OpenDockerSettingsResponse {
        success: status.success(),
    }))
}
