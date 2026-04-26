//! WebSocket endpoint: PTY exec into a single inner compose
//! service running inside the SSG outer DinD. Backs the SPA's
//! `SsgServiceDetailPage → Exec` tab.
//!
//! Wire shape (mirrors `/api/v1/service/exec`):
//! - `GET /api/v1/ssg/services/exec?project=<p>&service=<n>[&session_id=<id>]`
//!   with `Upgrade: websocket`.
//! - The daemon spawns a host-side `docker exec -it <ssg_outer>
//!   docker exec -it <inner_container> sh` PTY (same shape as
//!   `ws_service_exec.rs`'s nested-docker-exec invocation).
//! - On first connect the server emits a `TerminalSessionInit`
//!   JSON frame with the new session id; subsequent text frames
//!   are stdout bytes. The `0x01` resize prefix and `\x02clear`
//!   sentinel match the per-instance protocol exactly.
//! - Sessions live in `state.service_exec_sessions` (the same
//!   pool used for instance services) but with a `composite_key`
//!   prefix of `"ssg:<project>:<service>"` so the project namespaces
//!   never collide with `<project>:<instance>:<service>` keys.
//!
//! Reconnect with `?session_id=<id>` to attach to an existing
//! shell. Sessions auto-evict on PTY EOF.

use std::collections::VecDeque;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex};
use tracing::debug;
use ts_rs::TS;

use coast_core::protocol::{ServiceExecSessionInfo, TerminalSessionInit};

use crate::api::query::ssg::{resolve_ssg_container_id, resolve_ssg_inner_container_name};
use crate::api::ws_host_terminal::PtySession;
use crate::api::ws_service_exec::{
    bridge_ws, open_pty_for_service, reconnect_session, send_ws_error, spawn_pty_reader,
    SCROLLBACK_CAP,
};
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceExecParams {
    pub project: String,
    pub service: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceSessionsParams {
    pub project: String,
    pub service: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ssg/services/exec", get(ws_handler))
        .route("/ssg/services/sessions", get(list_sessions))
}

/// Composite key used to scope sessions in
/// `state.service_exec_sessions`. Prefixed with `"ssg:"` so a
/// project named the same as an instance can never accidentally
/// share a session pool. Format: `"ssg:<project>:<service>"`.
fn ssg_composite_key(project: &str, service: &str) -> String {
    format!("ssg:{project}:{service}")
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgServiceSessionsParams>,
) -> Json<Vec<ServiceExecSessionInfo>> {
    let composite_key = ssg_composite_key(&params.project, &params.service);
    let sessions = state.service_exec_sessions.lock().await;
    let db = state.db.lock().await;
    let list: Vec<ServiceExecSessionInfo> = sessions
        .values()
        .filter(|s| s.project == composite_key)
        .map(|s| {
            let title = db
                .get_setting(&format!("session_title:{}", s.id))
                .ok()
                .flatten();
            ServiceExecSessionInfo {
                id: s.id.clone(),
                // The SPA's `ServiceExecSessionInfo` carries an
                // `instance` field (`name`) that doesn't apply to
                // SSGs. We surface the SSG sentinel `__ssg__` so
                // any UI listing the sessions can disambiguate
                // SSG sessions from per-instance ones; the SPA
                // path (`SsgServiceExecTab`) ignores this field.
                project: params.project.clone(),
                name: "__ssg__".to_string(),
                service: params.service.clone(),
                title,
            }
        })
        .collect();
    Json(list)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgServiceExecParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if params.project.is_empty() || params.service.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project' or 'service'",
            })),
        ));
    }
    let outer_cid = resolve_ssg_container_id(&state, &params.project).await?;
    let inner_name =
        resolve_ssg_inner_container_name(&state, &params.project, &outer_cid, &params.service)
            .await?;

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(
            socket,
            state,
            params.project,
            params.service,
            outer_cid,
            inner_name,
            params.session_id,
        )
    }))
}

async fn handle_ws(
    mut socket: WebSocket,
    state: Arc<AppState>,
    project: String,
    service: String,
    outer_container_id: String,
    inner_container_name: String,
    session_id: Option<String>,
) {
    // Reconnect path: an existing SSG session matches this id and
    // we just reattach the `bridge_ws` loop. The shared
    // `service_exec_sessions` map and `reconnect_session` logic
    // already handle the scrollback replay + RESIZE/CLEAR
    // multiplexing identically for both SSG and instance shells.
    if let Some(ref sid) = session_id {
        let sessions = state.service_exec_sessions.lock().await;
        if sessions.contains_key(sid) {
            drop(sessions);
            reconnect_session(&mut socket, &state, sid).await;
            return;
        }
    }

    if state.docker.is_none() {
        send_ws_error(&mut socket, "Docker is unavailable on the host daemon").await;
        return;
    }

    let composite_key = ssg_composite_key(&project, &service);
    let sid = uuid::Uuid::new_v4().to_string();
    debug!(
        session_id = %sid,
        outer = %outer_container_id,
        inner = %inner_container_name,
        "creating new SSG service exec session"
    );

    let Some((master_fd, child_pid)) =
        open_pty_for_service(&mut socket, &outer_container_id, &inner_container_name).await
    else {
        // `open_pty_for_service` already sent an error frame on
        // failure; nothing else to do here.
        return;
    };

    let read_fd = master_fd.as_raw_fd();
    let write_fd = nix::unistd::dup(read_fd).expect("dup master PTY fd");
    std::mem::forget(master_fd);

    let scrollback = Arc::new(Mutex::new(VecDeque::<u8>::with_capacity(SCROLLBACK_CAP)));
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(256);

    {
        let session = PtySession {
            id: sid.clone(),
            project: composite_key,
            child_pid,
            master_read_fd: read_fd,
            master_write_fd: write_fd,
            scrollback: scrollback.clone(),
            output_tx: output_tx.clone(),
        };
        let mut sessions = state.service_exec_sessions.lock().await;
        sessions.insert(sid.clone(), session);
    }

    spawn_pty_reader(
        state.clone(),
        sid.clone(),
        read_fd,
        scrollback.clone(),
        output_tx.clone(),
    );

    // Emit the session id so the SPA can persist + reconnect.
    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: sid.clone(),
    })
    .unwrap();
    if let Ok(text) = serde_json::to_string(&init_msg) {
        // No-op decode-then-encode; just drop the round-trip.
        let _ = text;
    }
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    bridge_ws(&mut socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(session_id = %sid, "SSG service exec WS disconnected, session kept alive");

    // Reuse the `service_exec_sessions` reaper from
    // `spawn_pty_reader`: when the underlying PTY closes, the
    // reader removes the session. Disconnects without EOF leave
    // the session warm for reconnect via `?session_id=<sid>`.
    let _ = std::mem::take(&mut Some(()));
}
