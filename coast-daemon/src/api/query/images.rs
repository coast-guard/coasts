use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{ImageInspectResponse, ImageSummary};

use super::resolve_coast_container;
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

    // Bare-service instances have no compose — skip for remote (local Docker can't see remote container).
    if resolved.remote_host.is_none() {
        if let Some(docker) = state.docker.as_ref() {
            if crate::bare_services::has_bare_services(&docker, &resolved.container_id).await {
                return Ok(Json(vec![]));
            }
        }
    }

    let referenced_images = resolve_referenced_images(&state, &resolved, &params.project).await;

    let cmd = vec![
        "docker".to_string(),
        "images".to_string(),
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
            let is_referenced = referenced_images.is_empty()
                || referenced_images.contains(repository)
                || referenced_images.contains(&full_ref)
                || repository.starts_with(&project_built_prefix);

            if !is_referenced || repository == "<none>" {
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

async fn resolve_referenced_images(
    state: &AppState,
    resolved: &super::ResolvedCoast,
    project: &str,
) -> std::collections::HashSet<String> {
    let cmd = if resolved.remote_host.is_some() {
        vec![
            "sh".into(),
            "-c".into(),
            concat!(
                "CF=/coast-artifact/compose.coast-shared.yml; ",
                "[ -f \"$CF\" ] || CF=/coast-artifact/compose.yml; ",
                "[ -f \"$CF\" ] || exit 0; ",
                "PD=/workspace; ",
                "if [ -f /coast-artifact/coastfile.toml ]; then ",
                "  cd=$(grep -o 'compose *= *\"[^\"]*\"' /coast-artifact/coastfile.toml | head -1 | sed 's/.*\"\\(.*\\)\"/\\1/'); ",
                "  if [ -n \"$cd\" ]; then d=$(dirname \"$cd\"); ",
                "    [ \"$d\" != \".\" ] && [ -n \"$d\" ] && PD=\"/workspace/$(echo $d | sed 's|^\\./||')\"; ",
                "  fi; ",
                "fi; ",
                "docker compose -f \"$CF\" --project-directory \"$PD\" config --images 2>/dev/null",
            )
            .to_string(),
        ]
    } else {
        let compose_ctx =
            crate::handlers::compose_context_for_build(project, resolved.build_id.as_deref());
        compose_ctx.compose_shell("config --images")
    };

    match super::exec_in_resolved_coast(state, resolved, cmd).await {
        Ok(output) => output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    }
}

async fn inspect_image(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ImageInspectParams>,
) -> Result<Json<ImageInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;

    let cmd = vec![
        "docker".to_string(),
        "inspect".to_string(),
        params.image.clone(),
    ];

    let output = super::exec_in_resolved_coast(&state, &resolved, cmd)
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

    Ok(Json(ImageInspectResponse {
        inspect: parsed,
        containers,
    }))
}
