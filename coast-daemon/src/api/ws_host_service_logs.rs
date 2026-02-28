use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::container::LogsOptions;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use rust_i18n::t;

use crate::server::AppState;
use crate::shared_services::shared_container_name;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct HostServiceLogsParams {
    pub project: String,
    pub service: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/host-service/logs", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostServiceLogsParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let container_name = shared_container_name(&params.project, &params.service);

    let lang = state.language();
    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            t!("error.docker_not_available", locale = &lang).to_string(),
        )
    })?;
    docker
        .inspect_container(&container_name, None)
        .await
        .map_err(|_| {
            (
                StatusCode::NOT_FOUND,
                format!("Shared service container '{}' not found", container_name),
            )
        })?;

    Ok(ws.on_upgrade(move |socket| handle_logs_socket(socket, state, container_name, params)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_logs_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    container_name: String,
    params: HostServiceLogsParams,
) {
    debug!(
        project = %params.project,
        service = %params.service,
        "host-service logs stream connected"
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

    let options = LogsOptions::<String> {
        follow: true,
        stdout: true,
        stderr: true,
        tail: "200".to_string(),
        ..Default::default()
    };

    let mut stream = docker.logs(&container_name, Some(options));

    loop {
        tokio::select! {
            chunk = stream.next() => {
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
                        warn!(error = %e, "host-service log stream error");
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

    debug!(
        service = %params.service,
        "host-service logs stream disconnected"
    );
}
