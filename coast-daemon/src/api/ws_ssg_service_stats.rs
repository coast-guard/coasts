//! WebSocket endpoint: streams `docker stats` for a single inner
//! service container running inside the SSG outer DinD. Backs the
//! SPA's `SsgServiceDetailPage → Stats` tab.
//!
//! Wire shape:
//! - `GET /api/v1/ssg/services/stats/stream?project=<p>&service=<n>`
//!   with `Upgrade: websocket`.
//! - Server emits one JSON [`ContainerStats`] frame every ~2s,
//!   produced by `docker stats <inner_name> --no-stream --format
//!   '{{json .}}'` exec'd inside the outer DinD. Same wire shape as
//!   the per-instance `/api/v1/service/stats/stream` endpoint so
//!   the SPA's existing chart helpers can be reused.
//! - On connect the server replays a small per-(project, service)
//!   in-memory history buffer so the chart isn't blank for the
//!   first ~2s.
//! - Stream stops when the client disconnects, the inner container
//!   exits, or the outer DinD container goes away.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use crate::api::query::ssg::{resolve_ssg_container_id, resolve_ssg_inner_container_name};
use crate::api::ws_service_stats::parse_docker_stats_json;
use crate::server::AppState;

/// Cap on the per-(project, service) history buffer. ~5 minutes
/// at the 2s polling cadence the inner-DinD `docker stats` exec
/// uses.
const HISTORY_CAP: usize = 150;

/// Polling cadence for `docker stats --no-stream` inside the
/// outer DinD. Matches `ws_service_stats.rs` so the chart axes
/// look identical between SSG and instance per-service views.
const POLL_INTERVAL_SECS: u64 = 2;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceStatsParams {
    pub project: String,
    pub service: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/ssg/services/stats/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgServiceStatsParams>,
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
        handle_socket(
            socket,
            state,
            params.project,
            params.service,
            outer_cid,
            inner_name,
        )
    }))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    project: String,
    service: String,
    outer_cid: String,
    inner_name: String,
) {
    let key = format!("{project}:{service}");
    debug!(
        project = %project,
        service = %service,
        inner = %inner_name,
        "ssg service stats WS connected"
    );

    if !replay_history(&mut socket, &state, &key).await {
        return;
    }

    let Some(docker) = state.docker.as_ref() else {
        let _ = socket
            .send(Message::Text("Docker is unavailable".into()))
            .await;
        return;
    };

    let stats_cmd = format!(
        "docker stats {} --no-stream --format '{{{{json .}}}}'",
        inner_name
    );

    run_poll_loop(
        &mut socket,
        &state,
        &docker,
        &outer_cid,
        &stats_cmd,
        &key,
        &project,
        &service,
    )
    .await;

    debug!(
        project = %project,
        service = %service,
        "ssg service stats WS disconnected"
    );
}

/// Drain the per-(project, service) history ring buffer to the
/// fresh socket so the SPA's chart isn't blank for the first ~2s.
/// Returns `false` when the socket closed mid-replay (the caller
/// should bail out before opening the docker stats stream).
async fn replay_history(socket: &mut WebSocket, state: &AppState, key: &str) -> bool {
    let history: Vec<serde_json::Value> = {
        let map = state.ssg_service_stats_history.lock().await;
        map.get(key)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    };
    for sample in &history {
        if socket
            .send(Message::Text(sample.to_string().into()))
            .await
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Long-running 2s poll loop. Race the docker exec against the
/// socket recv so a closed browser tab tears down the loop without
/// waiting for the next poll interval. Returns when either the
/// docker exec fails or the socket closes.
async fn run_poll_loop(
    socket: &mut WebSocket,
    state: &AppState,
    docker: &bollard::Docker,
    outer_cid: &str,
    stats_cmd: &str,
    key: &str,
    project: &str,
    service: &str,
) {
    loop {
        let outcome = tokio::select! {
            poll = poll_inner_stats_once(docker, outer_cid, stats_cmd) => {
                handle_poll_outcome(socket, state, key, project, service, poll).await
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => LoopOutcome::Stop,
                    _ => LoopOutcome::Continue,
                }
            }
        };

        if matches!(outcome, LoopOutcome::Stop) {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

/// Outcome of one iteration of the poll loop. Pulled out so
/// `run_poll_loop`'s body is a flat `tokio::select!` rather than
/// the deeply-nested match clippy flagged as too complex.
enum LoopOutcome {
    Continue,
    Stop,
}

/// Append `sample` to the per-(project, service) history ring and
/// forward it on the socket. Bounded by `HISTORY_CAP`; oldest
/// samples evict.
async fn append_and_forward(
    socket: &mut WebSocket,
    state: &AppState,
    key: &str,
    sample: serde_json::Value,
) -> LoopOutcome {
    {
        let mut map = state.ssg_service_stats_history.lock().await;
        let q = map.entry(key.to_string()).or_default();
        q.push_back(sample.clone());
        while q.len() > HISTORY_CAP {
            q.pop_front();
        }
    }
    if socket
        .send(Message::Text(sample.to_string().into()))
        .await
        .is_err()
    {
        LoopOutcome::Stop
    } else {
        LoopOutcome::Continue
    }
}

/// Decide what to do with a single `poll_inner_stats_once` result.
async fn handle_poll_outcome(
    socket: &mut WebSocket,
    state: &AppState,
    key: &str,
    project: &str,
    service: &str,
    poll: Result<Option<serde_json::Value>, ()>,
) -> LoopOutcome {
    match poll {
        Ok(Some(sample)) => append_and_forward(socket, state, key, sample).await,
        // exec returned no parseable JSON; the next poll round will retry.
        Ok(None) => LoopOutcome::Continue,
        Err(()) => {
            warn!(
                project = %project,
                service = %service,
                "ssg service stats exec failed; closing WS"
            );
            LoopOutcome::Stop
        }
    }
}

/// Run `docker stats <name> --no-stream` inside the outer DinD via
/// bollard exec, returning the parsed `ContainerStats`-shaped JSON
/// value (or `None` when the exec produced no parseable output).
/// `Err(())` on transport errors so the loop tears down cleanly.
async fn poll_inner_stats_once(
    docker: &bollard::Docker,
    outer_cid: &str,
    stats_cmd: &str,
) -> Result<Option<serde_json::Value>, ()> {
    let exec_opts = CreateExecOptions {
        cmd: Some(vec![
            "sh".to_string(),
            "-c".to_string(),
            stats_cmd.to_string(),
        ]),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(outer_cid, exec_opts)
        .await
        .map_err(|_| ())?;
    let started = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await
        .map_err(|_| ())?;

    let mut buf = String::new();
    if let StartExecResults::Attached { mut output, .. } = started {
        while let Some(chunk) = output.next().await {
            if let Ok(bollard::container::LogOutput::StdOut { message }) = chunk {
                buf.push_str(&String::from_utf8_lossy(&message));
            }
        }
    }
    Ok(parse_docker_stats_json(&buf))
}
