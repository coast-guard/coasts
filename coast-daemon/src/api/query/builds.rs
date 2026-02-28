use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{BuildsRequest, CoastfileTypesResponse};

use crate::handlers;
use crate::server::AppState;

// --- Builds endpoints ---

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct BuildsLsParams {
    pub project: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct BuildsProjectParams {
    pub project: String,
    pub build_id: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct BuildsDockerImageInspectParams {
    pub project: String,
    pub image: String,
}

async fn builds_ls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsLsParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::Ls {
            project: params.project,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::Inspect {
            project: params.project,
            build_id: params.build_id,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_images(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::Images {
            project: params.project,
            build_id: params.build_id,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_docker_images(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::DockerImages {
            project: params.project,
            build_id: params.build_id,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_docker_image_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsDockerImageInspectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::InspectDockerImage {
            project: params.project,
            image: params.image,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_compose(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::Compose {
            project: params.project,
            build_id: params.build_id,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

async fn builds_coastfile(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match handlers::builds::handle(
        BuildsRequest::Coastfile {
            project: params.project,
            build_id: params.build_id,
        },
        &state,
    )
    .await
    {
        Ok(resp) => Ok(Json(serde_json::to_value(resp).unwrap_or_default())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

/// List available Coastfile types for a project by scanning its project root.
///
/// Returns `["default"]` if only `Coastfile` exists, or
/// `["default", "light", "shared"]` if `Coastfile.light` and `Coastfile.shared`
/// also exist.
async fn builds_coastfile_types(
    State(_state): State<Arc<AppState>>,
    Query(params): Query<BuildsProjectParams>,
) -> Result<Json<CoastfileTypesResponse>, (StatusCode, Json<serde_json::Value>)> {
    let project = params.project;

    let project_root = {
        let home = dirs::home_dir().unwrap_or_default();
        let project_dir = home.join(".coast").join("images").join(&project);

        // Try to read project_root from the latest manifest
        let manifest_path = std::fs::read_link(project_dir.join("latest"))
            .ok()
            .map(|t| project_dir.join(t).join("manifest.json"))
            .filter(|p| p.exists())
            .or_else(|| {
                let flat = project_dir.join("manifest.json");
                flat.exists().then_some(flat)
            });

        manifest_path
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| {
                v.get("project_root")?
                    .as_str()
                    .map(std::string::ToString::to_string)
            })
    };

    let Some(root) = project_root else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!(
                    "No build found for project '{}'. Run 'coast build' first.",
                    project
                )
            })),
        ));
    };

    let root_path = std::path::Path::new(&root);
    let mut types: Vec<String> = vec![];

    if let Ok(entries) = std::fs::read_dir(root_path) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname == "Coastfile" {
                types.push("default".to_string());
            } else if let Some(suffix) = fname.strip_prefix("Coastfile.") {
                if !suffix.is_empty() && suffix != "default" {
                    types.push(suffix.to_string());
                }
            }
        }
    }

    types.sort();
    if types.first().map(std::string::String::as_str) != Some("default")
        && !types.contains(&"default".to_string())
    {
        types.insert(0, "default".to_string());
    }

    Ok(Json(CoastfileTypesResponse { project, types }))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/builds", get(builds_ls))
        .route("/builds/inspect", get(builds_inspect))
        .route("/builds/images", get(builds_images))
        .route("/builds/docker-images", get(builds_docker_images))
        .route(
            "/builds/docker-images/inspect",
            get(builds_docker_image_inspect),
        )
        .route("/builds/compose", get(builds_compose))
        .route("/builds/coastfile", get(builds_coastfile))
        .route("/builds/coastfile-types", get(builds_coastfile_types))
}
