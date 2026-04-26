//! WebSocket endpoint: bidirectional shell into the outer DinD
//! container of a project's SSG. Backs the SPA's SSG → Exec tab.
//!
//! Wire shape (compatible with the SPA's shared `PersistentTerminal`
//! component):
//! - Client → server frames are `Message::Text` strings:
//!   - A leading `0x01` byte means "resize message"; the remainder is a
//!     JSON `{cols, rows}` body that reconfigures the PTY.
//!   - Anything else is forwarded verbatim as stdin to the shell
//!     (xterm sends a single character per keystroke).
//! - Server → client frames are `Message::Text` strings carrying
//!   stdout/stderr from the shell (UTF-8-lossy decoded from the raw
//!   bytes).
//!
//! Unlike `ws_host_terminal.rs` (which manages persistent `forkpty`
//! sessions on the host with full reconnect support), this endpoint
//! is stateless: disconnecting closes the underlying exec.
//! Reconnecting starts a fresh shell. The SPA's `PersistentTerminal`
//! still drives the UX (Shell N tabs, theme picker, fullscreen) — it
//! just doesn't get session persistence on the SSG side.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use bollard::exec::{CreateExecOptions, ResizeExecOptions, StartExecOptions, StartExecResults};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{debug, warn};
use ts_rs::TS;

use crate::api::query::ssg::resolve_ssg_container_id;
use crate::server::AppState;

/// Resize-message marker byte, matched on byte 0 of an inbound Text
/// frame. Mirrors the convention in
/// `coast-guard/src/components/PersistentTerminal.tsx::RESIZE_PREFIX`.
const RESIZE_PREFIX_BYTE: u8 = 0x01;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgTerminalParams {
    pub project: String,
    /// `PersistentTerminal` sends this on reconnect attempts. We
    /// don't currently track sessions for the SSG, so the param is
    /// accepted-and-ignored for forward-compat with persisting
    /// later.
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize)]
struct ResizeMsg {
    rows: u16,
    cols: u16,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ssg/terminal", get(ws_handler))
        .route("/ssg/sessions", get(list_sessions).delete(delete_session))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgTerminalParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, cid, params.project)))
}

/// `GET /api/v1/ssg/sessions?project=<p>` — `PersistentTerminal`
/// fetches this on mount to restore prior sessions. SSG terminals
/// don't persist (each WS connection is a fresh exec), so we
/// always return an empty list and let the SPA spawn a new
/// session.
async fn list_sessions(
    Query(_params): Query<SsgSessionsQuery>,
) -> Json<Vec<coast_core::protocol::SessionInfo>> {
    Json(Vec::new())
}

/// `DELETE /api/v1/ssg/sessions?id=<id>` — no-op accepted, since
/// we don't track sessions. Returning 200 OK keeps the SPA's
/// "close session" flow happy without exposing the missing
/// persistence feature.
async fn delete_session(Query(_params): Query<SsgSessionDeleteQuery>) -> StatusCode {
    StatusCode::OK
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSessionsQuery {
    pub project: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSessionDeleteQuery {
    pub id: String,
}

async fn handle_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    project: String,
) {
    debug!(project = %project, "ssg terminal websocket connected");
    let Some(docker) = state.docker.as_ref() else {
        let mut s = socket;
        let _ = s.send(Message::Text("Docker is unavailable".into())).await;
        return;
    };

    let exec = match docker
        .create_exec(
            &container_id,
            CreateExecOptions {
                cmd: Some(vec!["/bin/sh".to_string()]),
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                env: Some(vec!["TERM=xterm-256color".to_string()]),
                ..Default::default()
            },
        )
        .await
    {
        Ok(e) => e,
        Err(e) => {
            let mut s = socket;
            let _ = s
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
            let mut s = socket;
            let _ = s
                .send(Message::Text(format!("start_exec failed: {e}").into()))
                .await;
            return;
        }
    };

    let StartExecResults::Attached { mut output, input } = started else {
        let mut s = socket;
        let _ = s
            .send(Message::Text(
                "exec started in detached mode (unexpected)".into(),
            ))
            .await;
        return;
    };

    let (mut sender, mut receiver) = socket.split();
    let docker_clone = docker.clone();
    let exec_id = exec.id.clone();

    // Server -> client: stream exec stdout/stderr to the websocket
    // as Text frames so the SPA's `PersistentTerminal` can consume
    // them (it expects MessageEvent<string>).
    let server_to_client = tokio::spawn(async move {
        while let Some(chunk) = output.next().await {
            let text = match chunk {
                Ok(bollard::container::LogOutput::StdOut { message })
                | Ok(bollard::container::LogOutput::StdErr { message })
                | Ok(bollard::container::LogOutput::Console { message }) => {
                    String::from_utf8_lossy(&message).into_owned()
                }
                Ok(bollard::container::LogOutput::StdIn { .. }) => continue,
                Err(e) => {
                    warn!(error = %e, "ssg-terminal output stream error");
                    break;
                }
            };
            if sender.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sender.send(Message::Close(None)).await;
    });

    // Client -> server: forward stdin bytes; if the frame starts
    // with `RESIZE_PREFIX` (0x01) parse the rest as a JSON resize
    // message and call `docker.resize_exec`. Both Text and Binary
    // input are accepted (PersistentTerminal sends Text; older
    // callers might still send Binary).
    let mut stdin = input;
    let client_to_server = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    warn!(error = %e, "ssg-terminal inbound stream error");
                    break;
                }
            };
            match msg {
                Message::Text(t) => {
                    let bytes = t.as_bytes();
                    if bytes.first() == Some(&RESIZE_PREFIX_BYTE) {
                        if let Ok(resize) = serde_json::from_slice::<ResizeMsg>(&bytes[1..]) {
                            let _ = docker_clone
                                .resize_exec(
                                    &exec_id,
                                    ResizeExecOptions {
                                        height: resize.rows,
                                        width: resize.cols,
                                    },
                                )
                                .await;
                            continue;
                        }
                    }
                    if stdin.write_all(bytes).await.is_err() {
                        break;
                    }
                }
                Message::Binary(b) => {
                    if b.first() == Some(&RESIZE_PREFIX_BYTE) {
                        if let Ok(resize) = serde_json::from_slice::<ResizeMsg>(&b[1..]) {
                            let _ = docker_clone
                                .resize_exec(
                                    &exec_id,
                                    ResizeExecOptions {
                                        height: resize.rows,
                                        width: resize.cols,
                                    },
                                )
                                .await;
                            continue;
                        }
                    }
                    if stdin.write_all(&b).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let _ = tokio::join!(server_to_client, client_to_server);
    debug!(project = %project, "ssg terminal websocket closed");
}
