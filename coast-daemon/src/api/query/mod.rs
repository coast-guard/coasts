pub mod builds;
pub mod config;
pub mod docker;
pub mod docs;
pub mod files;
pub mod images;
pub mod mcp;
pub mod project_git;
pub mod remotes;
pub mod secrets;
pub mod services;
pub mod settings;
pub mod ssh_keys;
pub mod update;
pub mod volumes;

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{LsRequest, LsResponse};
use coast_core::types::InstanceStatus;

use crate::handlers;
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct LsParams {
    pub project: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ls", get(ls))
        .merge(project_git::routes())
        .merge(settings::routes())
        .merge(images::routes())
        .merge(secrets::routes())
        .merge(volumes::routes())
        .merge(services::routes())
        .merge(files::routes())
        .merge(builds::routes())
        .merge(remotes::routes())
        .merge(ssh_keys::routes())
        .merge(mcp::routes())
        .merge(config::routes())
        .merge(docker::routes())
        .merge(update::routes())
        .merge(docs::routes())
}

async fn ls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LsParams>,
) -> Result<Json<LsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let req = LsRequest {
        project: params.project,
    };
    match handlers::ls::handle(req, &state).await {
        Ok(resp) => Ok(Json(resp)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

/// Resolved coast container info.
#[derive(Debug)]
pub(crate) struct ResolvedCoast {
    pub container_id: String,
    pub build_id: Option<String>,
    pub remote_host: Option<String>,
    pub project: String,
    pub name: String,
}

pub(crate) async fn resolve_coast_container(
    state: &AppState,
    project: &str,
    name: &str,
) -> Result<ResolvedCoast, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let instance = db
        .get_instance(project, name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("Instance '{name}' not found") })),
            )
        })?;

    if instance.status == InstanceStatus::Stopped {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": format!("Instance '{name}' is stopped") })),
        ));
    }

    if instance.status == InstanceStatus::Provisioning
        || instance.status == InstanceStatus::Assigning
        || instance.status == InstanceStatus::Unassigning
    {
        let action = match instance.status {
            InstanceStatus::Provisioning => "provisioning",
            InstanceStatus::Unassigning => "unassigning",
            _ => "assigning",
        };
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": format!("Instance '{name}' is still {action}") })),
        ));
    }

    let container_id = instance.container_id.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "No container ID" })),
        )
    })?;

    Ok(ResolvedCoast {
        container_id,
        build_id: instance.build_id,
        remote_host: instance.remote_host,
        project: project.to_string(),
        name: name.to_string(),
    })
}

pub(crate) async fn exec_in_coast(
    state: &AppState,
    container_id: &str,
    cmd: Vec<String>,
) -> Result<String, String> {
    let docker = state.docker.as_ref().ok_or("Docker not available")?;

    let exec_options = CreateExecOptions {
        cmd: Some(cmd),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| format!("Failed to create exec: {e}"))?;

    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    let output = docker
        .start_exec(&exec.id, Some(start_options))
        .await
        .map_err(|e| format!("Failed to start exec: {e}"))?;

    let mut stdout = String::new();
    if let StartExecResults::Attached { mut output, .. } = output {
        while let Some(chunk) = output.next().await {
            if let Ok(bollard::container::LogOutput::StdOut { message }) = chunk {
                stdout.push_str(&String::from_utf8_lossy(&message));
            }
        }
    }

    Ok(stdout)
}

/// Execute a command inside a remote coast's DinD container via coast-service /exec.
pub(crate) async fn exec_in_remote_coast(
    state: &AppState,
    project: &str,
    name: &str,
    cmd: Vec<String>,
) -> Result<String, String> {
    let remote_config = crate::handlers::remote::resolve_remote_for_instance(project, name, state)
        .await
        .map_err(|e| format!("Failed to resolve remote: {e}"))?;
    let client = crate::handlers::remote::RemoteClient::connect(&remote_config)
        .await
        .map_err(|e| format!("Failed to connect to remote: {e}"))?;

    let exec_req = coast_core::protocol::ExecRequest {
        name: name.to_string(),
        project: project.to_string(),
        service: None,
        root: false,
        command: cmd,
    };

    let resp = crate::handlers::remote::forward::forward_exec(&client, &exec_req)
        .await
        .map_err(|e| format!("Remote exec failed: {e}"))?;

    Ok(resp.stdout)
}

/// Execute in the appropriate context: local Docker for local instances,
/// coast-service /exec for remote instances.
pub(crate) async fn exec_in_resolved_coast(
    state: &AppState,
    resolved: &ResolvedCoast,
    cmd: Vec<String>,
) -> Result<String, String> {
    if resolved.remote_host.is_some() {
        exec_in_remote_coast(state, &resolved.project, &resolved.name, cmd).await
    } else {
        exec_in_coast(state, &resolved.container_id, cmd).await
    }
}
