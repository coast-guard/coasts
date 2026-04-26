//! WebSocket endpoint: streams `docker stats` for the outer DinD
//! container of a project's SSG. Backs the SPA's SSG → Stats tab.
//!
//! Wire shape (Phase 33+):
//! - `GET /api/v1/ssg/stats/stream?project=<p>` with `Upgrade:
//!   websocket`.
//! - Server emits one JSON [`ContainerStats`] frame per second
//!   (the same shape used by the per-instance `/api/v1/stats/stream`
//!   endpoint), so the SPA's `InstanceStatsTab`-shaped chart
//!   renderer can consume both endpoints with identical code.
//! - On connect the server first replays a small in-memory history
//!   buffer (so reconnects after a brief network blip show prior
//!   data) and then streams live samples until the client
//!   disconnects or the container exits.
//!
//! The legacy `SsgStatsSample` shape — `{cpu_pct, mem_used_bytes,
//! ...}` with snake_case + the `at_unix` field — has been replaced
//! by `ContainerStats` for parity with the instance stream. There
//! were no out-of-tree consumers (the only callers were
//! `coast-guard/src/components/ssg/SsgStatsTab.tsx` and the
//! re-export in `coast-guard/src/types/api.ts`, both updated in
//! the same change).

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use bollard::container::StatsOptions;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::protocol::ContainerStats;

use crate::api::query::ssg::resolve_ssg_container_id;
use crate::api::ws_stats::extract_stats;
use crate::server::AppState;

/// Cap on the per-project in-memory history buffer. Same value as
/// the instance stats path (`HISTORY_CAP` in `ws_stats.rs`) so a
/// reconnect renders the same ~5 minutes of 1Hz samples.
const HISTORY_CAP: usize = 300;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgStatsParams {
    pub project: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/ssg/stats/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgStatsParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, cid, params.project)))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    project: String,
) {
    debug!(project = %project, "ssg stats websocket connected");

    let Some(docker) = state.docker.as_ref() else {
        let _ = socket
            .send(Message::Text("Docker is unavailable".into()))
            .await;
        return;
    };

    // Replay any history we already buffered for this project so
    // the SPA's chart isn't blank for the first ~1s. Mirrors the
    // instance path's REST `/stats/history` priming.
    let history: Vec<ContainerStats> = {
        let map = state.ssg_stats_history.lock().await;
        map.get(&project)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    };
    for sample in &history {
        let Ok(json) = serde_json::to_string(sample) else {
            continue;
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    let mut stream = docker.stats(
        &container_id,
        Some(StatsOptions {
            stream: true,
            one_shot: false,
        }),
    );

    let mut prev_cpu_total: u64 = 0;
    let mut prev_cpu_system: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let stats = match chunk {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "ssg stats stream error");
                break;
            }
        };
        let sample = extract_stats(&stats, &mut prev_cpu_total, &mut prev_cpu_system);

        // Append to the per-project history buffer (1Hz cap). We
        // only buffer here (not in a background collector) because
        // there's exactly one SSG per project and no replay-after-
        // close requirement: the history is just a "previous N
        // samples" view for reconnects on the same WS.
        {
            let mut map = state.ssg_stats_history.lock().await;
            let q = map.entry(project.clone()).or_default();
            q.push_back(sample.clone());
            while q.len() > HISTORY_CAP {
                q.pop_front();
            }
        }

        let Ok(json) = serde_json::to_string(&sample) else {
            continue;
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
}
