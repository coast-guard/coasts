use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::types::InstanceStatus;
use rust_i18n::t;

use crate::handlers::compose_context;
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct LogsStreamParams {
    pub project: String,
    pub name: String,
    #[serde(default)]
    pub service: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/logs/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<LogsStreamParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let lang = state.language();
    let db = state.db.lock().await;
    let instance = db
        .get_instance(&params.project, &params.name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                t!(
                    "error.instance_not_found",
                    locale = &lang,
                    name = &params.name,
                    project = &params.project
                )
                .to_string(),
            )
        })?;

    if instance.status == InstanceStatus::Stopped {
        return Err((
            StatusCode::CONFLICT,
            t!(
                "error.instance_stopped",
                locale = &lang,
                name = &params.name
            )
            .to_string(),
        ));
    }

    let container_id = instance.container_id.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            t!("error.no_container_id", locale = &lang).to_string(),
        )
    })?;

    drop(db);

    Ok(ws.on_upgrade(move |socket| handle_logs_socket(socket, state, container_id, params)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_logs_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    params: LogsStreamParams,
) {
    debug!(
        name = %params.name,
        project = %params.project,
        "logs stream websocket connected"
    );

    let Some(docker) = state.docker.as_ref() else {
        let lang = state.language();
        let _ = socket
            .send(Message::Text(
                t!("error.docker_not_available", locale = &lang)
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };

    let is_bare = crate::bare_services::has_bare_services(docker, &container_id).await;
    let cmd_parts = if is_bare {
        let tail_cmd = crate::bare_services::generate_logs_command(
            params.service.as_deref(),
            None,
            false,
            true,
        );
        vec!["sh".to_string(), "-c".to_string(), tail_cmd]
    } else {
        let ctx = compose_context(&params.project);
        let mut subcmd = "logs --tail 200 --follow".to_string();
        if let Some(ref svc) = params.service {
            subcmd.push(' ');
            subcmd.push_str(svc);
        }
        ctx.compose_shell(&subcmd)
    };
    let cmd_refs: Vec<&str> = cmd_parts.iter().map(std::string::String::as_str).collect();

    let exec_options = CreateExecOptions {
        cmd: Some(
            cmd_refs
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = match docker.create_exec(&container_id, exec_options).await {
        Ok(e) => e,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to create exec: {e}").into()))
                .await;
            return;
        }
    };

    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    let output = match docker.start_exec(&exec.id, Some(start_options)).await {
        Ok(o) => o,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to start exec: {e}").into()))
                .await;
            return;
        }
    };

    if let StartExecResults::Attached { mut output, .. } = output {
        loop {
            tokio::select! {
                chunk = output.next() => {
                    match chunk {
                        Some(Ok(msg)) => {
                            let text = match msg {
                                bollard::container::LogOutput::StdOut { message } |
                                bollard::container::LogOutput::StdErr { message } => {
                                    String::from_utf8_lossy(&message).to_string()
                                }
                                _ => continue,
                            };
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "log stream error");
                            break;
                        }
                        None => break,
                    }
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }
    }

    debug!(
        name = %params.name,
        "logs stream websocket disconnected"
    );
}
