use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{ServiceInspectResponse, VolumeInspectResponse, VolumeSummaryResponse};

use super::resolve_coast_container;
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/volumes", get(list_volumes))
        .route("/volumes/inspect", get(inspect_volume))
        .route("/service/inspect", get(inspect_service_container))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct VolumesParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct VolumeInspectParams {
    pub project: String,
    pub name: String,
    pub volume: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ServiceInspectParams {
    pub project: String,
    pub name: String,
    pub service: String,
}

pub(super) async fn resolve_inner_container_name(
    state: &crate::server::AppState,
    resolved: &super::ResolvedCoast,
    service: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let ctx =
        crate::handlers::compose_context_for_build(&resolved.project, resolved.build_id.as_deref());
    let cmd_parts = if resolved.remote_host.is_some() {
        let project_dir = match &ctx.compose_rel_dir {
            Some(dir) => format!("/workspace/{dir}"),
            None => "/workspace".to_string(),
        };
        let script = format!(
            "CF=/coast-artifact/compose.coast-shared.yml; \
             [ -f \"$CF\" ] || CF=/coast-artifact/compose.yml; \
             docker compose -f \"$CF\" --project-directory {project_dir} ps --format json {service}"
        );
        vec!["sh".into(), "-c".into(), script]
    } else {
        ctx.compose_shell(&format!("ps --format json {service}"))
    };

    let output = super::exec_in_resolved_coast(state, resolved, cmd_parts)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(name) = val.get("Name").and_then(|v| v.as_str()) {
                return Ok(name.to_string());
            }
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(
            serde_json::json!({ "error": format!("Could not find container for service '{service}'") }),
        ),
    ))
}

async fn list_volumes(
    State(state): State<Arc<AppState>>,
    Query(params): Query<VolumesParams>,
) -> Result<Json<Vec<VolumeSummaryResponse>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;

    let cmd = vec![
        "docker".to_string(),
        "volume".to_string(),
        "ls".to_string(),
        "--format".to_string(),
        "{{json .}}".to_string(),
    ];

    let output = super::exec_in_resolved_coast(&state, &resolved, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let mut volumes = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            volumes.push(VolumeSummaryResponse {
                name: val
                    .get("Name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                driver: val
                    .get("Driver")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                mountpoint: val
                    .get("Mountpoint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                scope: val
                    .get("Scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                labels: val
                    .get("Labels")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }

    Ok(Json(volumes))
}

async fn inspect_volume(
    State(state): State<Arc<AppState>>,
    Query(params): Query<VolumeInspectParams>,
) -> Result<Json<VolumeInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;

    let inspect_cmd = vec![
        "docker".to_string(),
        "volume".to_string(),
        "inspect".to_string(),
        params.volume.clone(),
    ];

    let inspect_output = super::exec_in_resolved_coast(&state, &resolved, inspect_cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let inspect: serde_json::Value = serde_json::from_str(inspect_output.trim()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to parse volume inspect: {e}") })),
        )
    })?;

    let containers_cmd = vec![
        "docker".to_string(),
        "ps".to_string(),
        "-a".to_string(),
        "--filter".to_string(),
        format!("volume={}", params.volume),
        "--format".to_string(),
        "{{json .}}".to_string(),
    ];

    let containers_output = super::exec_in_resolved_coast(&state, &resolved, containers_cmd)
        .await
        .unwrap_or_default();

    let mut containers = Vec::new();
    for line in containers_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            containers.push(val);
        }
    }

    let compose_vol_label = inspect
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("Labels"))
        .and_then(|l| l.get("com.docker.compose.volume"))
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string);

    let mut coastfile_config: Option<serde_json::Value> = None;
    if let Some(ref home) = dirs::home_dir() {
        let cf_path = home
            .join(".coast")
            .join("images")
            .join(&params.project)
            .join("latest")
            .join("coastfile.toml");
        if let Ok(coastfile) = coast_core::coastfile::Coastfile::from_file(&cf_path) {
            for vol in &coastfile.volumes {
                let resolved =
                    coast_core::volume::resolve_volume_name(vol, &params.name, &params.project);
                let matches =
                    resolved == params.volume || compose_vol_label.as_deref() == Some(&vol.name);
                if matches {
                    coastfile_config = Some(serde_json::json!({
                        "name": vol.name,
                        "strategy": vol.strategy,
                        "service": vol.service,
                        "mount": vol.mount,
                        "snapshot_source": vol.snapshot_source,
                    }));
                    break;
                }
            }
        }
    }

    Ok(Json(VolumeInspectResponse {
        inspect,
        containers,
        coastfile: coastfile_config,
    }))
}

async fn inspect_service_container(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ServiceInspectParams>,
) -> Result<Json<ServiceInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;

    let inner_name = resolve_inner_container_name(&state, &resolved, &params.service).await?;

    let cmd = vec!["docker".to_string(), "inspect".to_string(), inner_name];

    let output = super::exec_in_resolved_coast(&state, &resolved, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let parsed: serde_json::Value = serde_json::from_str(output.trim())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": format!("Failed to parse service inspect output: {e}") }))))?;

    Ok(Json(ServiceInspectResponse { inspect: parsed }))
}
