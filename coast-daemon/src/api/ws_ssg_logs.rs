//! WebSocket endpoint: streams logs from a project's SSG.
//!
//! Three modes, picked by the `service` query param:
//!
//! - `service=` omitted (default) **or** `service=__all__`: runs
//!   `docker compose logs --follow` inside the outer DinD with no
//!   service filter. Compose prefixes every line with
//!   `<service>-1  | `, which the SPA's `parseLine` recognises and
//!   renders with a service color tag — visually matches the
//!   regular instance logs view.
//! - `service=__outer__`: streams the outer DinD container's
//!   stdout/stderr via `docker logs --follow`. Useful for
//!   debugging the inner Docker daemon itself (containerd boot
//!   messages, image pulls, etc).
//! - `service=<name>`: runs `docker compose logs --follow <name>`
//!   inside the outer DinD. Compose still prefixes the lines so
//!   the parser keeps working.
//!
//! Backs the SPA's SSG → Logs tab. Stops streaming when the
//! client disconnects or the container exits.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use bollard::container::LogsOptions;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use crate::api::query::ssg::resolve_ssg_container_id;
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgLogsParams {
    pub project: String,
    /// Inner compose service to filter on. When absent, the outer
    /// DinD's stdout/stderr is streamed.
    #[serde(default)]
    pub service: Option<String>,
    /// Tail line count for compose logs. Defaults to 200. Ignored
    /// for outer DinD logs (bollard's `tail: "all"` is used).
    #[serde(default)]
    pub tail: Option<u32>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/ssg/logs/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgLogsParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, cid, params)))
}

async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    params: SsgLogsParams,
) {
    debug!(project = %params.project, service = ?params.service, "ssg logs websocket connected");
    let Some(docker) = state.docker.as_ref() else {
        let mut s = socket;
        let _ = s.send(Message::Text("Docker is unavailable".into())).await;
        return;
    };

    // Resolve the inner compose project name from the daemon's
    // canonical naming convention (`<project>-ssg`), matching what
    // `coast_ssg::runtime::lifecycle::ssg_compose_project` does
    // when it spins up the SSG. Hardcoding `"ssg"` here meant
    // every per-service filter was running against a project
    // compose can't find, so logs were silently empty for
    // anything but the multi-service default.
    let compose_project = format!("{}-ssg", params.project);

    match params.service.as_deref() {
        // Outer DinD: raw container stdout/stderr (containerd
        // boot, image pulls, etc).
        Some("__outer__") => stream_outer_dind_logs(socket, &docker, &container_id).await,
        // Single inner service.
        Some(svc) if !svc.is_empty() && svc != "__all__" => {
            stream_compose_logs(
                socket,
                &docker,
                &container_id,
                &compose_project,
                Some(svc),
                params.tail,
            )
            .await
        }
        // Default: all inner services. Compose prefixes lines with
        // `<service>-1  | ` so the SPA can render service tags via
        // the same `parseLine` it uses for instance logs.
        _ => {
            stream_compose_logs(
                socket,
                &docker,
                &container_id,
                &compose_project,
                None,
                params.tail,
            )
            .await
        }
    }
}

/// Stream the outer DinD container's logs via bollard's native
/// `logs` API (no exec needed — these are the DinD container's
/// own stdout/stderr).
async fn stream_outer_dind_logs(
    mut socket: WebSocket,
    docker: &bollard::Docker,
    container_id: &str,
) {
    let mut stream = docker.logs(
        container_id,
        Some(LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow: true,
            tail: "200".to_string(),
            timestamps: false,
            ..Default::default()
        }),
    );
    while let Some(chunk) = stream.next().await {
        let text = match chunk {
            Ok(bollard::container::LogOutput::StdOut { message })
            | Ok(bollard::container::LogOutput::StdErr { message })
            | Ok(bollard::container::LogOutput::Console { message }) => {
                String::from_utf8_lossy(&message).to_string()
            }
            Ok(_) => continue,
            Err(e) => {
                warn!(error = %e, "outer DinD log stream error");
                break;
            }
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

/// Stream `docker compose -p <compose_project> logs --follow
/// [service]` from inside the outer DinD via a bollard exec.
/// When `service` is `None`, no service filter is applied so all
/// inner services stream together with compose's standard
/// `<service>-1  | ` line prefix.
async fn stream_compose_logs(
    mut socket: WebSocket,
    docker: &bollard::Docker,
    outer_container_id: &str,
    compose_project: &str,
    service: Option<&str>,
    tail: Option<u32>,
) {
    let tail_n = tail.unwrap_or(200);
    let mut cmd: Vec<String> = vec![
        "docker".to_string(),
        "compose".to_string(),
        "-p".to_string(),
        compose_project.to_string(),
        "logs".to_string(),
        "--no-color".to_string(),
        "--follow".to_string(),
        format!("--tail={tail_n}"),
    ];
    if let Some(svc) = service {
        cmd.push(svc.to_string());
    }

    let exec = match docker
        .create_exec(
            outer_container_id,
            CreateExecOptions {
                cmd: Some(cmd),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
    {
        Ok(e) => e,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("create_exec failed: {e}").into()))
                .await;
            return;
        }
    };

    let started = match docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(o) => o,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("start_exec failed: {e}").into()))
                .await;
            return;
        }
    };

    let StartExecResults::Attached { mut output, .. } = started else {
        let _ = socket
            .send(Message::Text("exec started in detached mode".into()))
            .await;
        return;
    };

    while let Some(chunk) = output.next().await {
        let text = match chunk {
            Ok(bollard::container::LogOutput::StdOut { message })
            | Ok(bollard::container::LogOutput::StdErr { message })
            | Ok(bollard::container::LogOutput::Console { message }) => {
                String::from_utf8_lossy(&message).to_string()
            }
            Ok(_) => continue,
            Err(e) => {
                warn!(error = %e, "compose log stream error");
                break;
            }
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}
