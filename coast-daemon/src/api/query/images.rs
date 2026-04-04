use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{ImageInspectResponse, ImageSummary};

use super::{exec_in_coast, resolve_coast_container};
use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/images", get(list_images))
        .route("/images/inspect", get(inspect_image))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ImagesParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SecretsParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ImageInspectParams {
    pub project: String,
    pub name: String,
    pub image: String,
}

async fn list_images(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ImagesParams>,
) -> Result<Json<Vec<ImageSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    // Bare-service instances have no compose — no relevant Docker images to show.
    if let Some(docker) = state.docker.as_ref() {
        if crate::bare_services::has_bare_services(&docker, container_id).await {
            return Ok(Json(vec![]));
        }
    }

    // Get the list of images referenced by compose services.
    let compose_ctx =
        crate::handlers::compose_context_for_build(&params.project, resolved.build_id.as_deref());
    let config_images_cmd = compose_ctx.compose_shell("config --images");
    let referenced_images: std::collections::HashSet<String> =
        match exec_in_coast(&state, container_id, config_images_cmd).await {
            Ok(output) => output
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            Err(_) => std::collections::HashSet::new(),
        };

    // If no compose images found (idle instance, or compose config failed), return empty.
    if referenced_images.is_empty() {
        return Ok(Json(vec![]));
    }

    let cmd = vec![
        "docker".to_string(),
        "images".to_string(),
        "--format".to_string(),
        "{{json .}}".to_string(),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let project_built_prefix = format!("coast-built/{}/", params.project.replace(['/', ':'], "_"));

    let mut images = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let repository = val
                .get("Repository")
                .and_then(|v| v.as_str())
                .unwrap_or("<none>");
            let tag = val.get("Tag").and_then(|v| v.as_str()).unwrap_or("<none>");

            let full_ref = format!("{repository}:{tag}");
            let is_referenced = referenced_images.contains(repository)
                || referenced_images.contains(&full_ref)
                || repository.starts_with(&project_built_prefix);

            if !is_referenced {
                continue;
            }

            images.push(ImageSummary {
                id: val
                    .get("ID")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                repository: repository.to_string(),
                tag: tag.to_string(),
                created: val
                    .get("CreatedSince")
                    .or_else(|| val.get("CreatedAt"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                size: val
                    .get("Size")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            });
        }
    }

    Ok(Json(images))
}

async fn inspect_image(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ImageInspectParams>,
) -> Result<Json<ImageInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let cmd = vec![
        "docker".to_string(),
        "inspect".to_string(),
        params.image.clone(),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let parsed: serde_json::Value = serde_json::from_str(output.trim()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to parse inspect output: {e}") })),
        )
    })?;

    let containers_cmd = vec![
        "docker".to_string(),
        "ps".to_string(),
        "-a".to_string(),
        "--filter".to_string(),
        format!("ancestor={}", params.image),
        "--format".to_string(),
        "{{json .}}".to_string(),
    ];

    let containers_output = exec_in_coast(&state, container_id, containers_cmd)
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

    Ok(Json(ImageInspectResponse {
        inspect: parsed,
        containers,
    }))
}
