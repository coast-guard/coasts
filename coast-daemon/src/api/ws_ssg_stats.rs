//! WebSocket endpoint: streams `docker stats` for the outer DinD
//! container of a project's SSG. Backs the SPA's SSG → Stats tab.
//!
//! Wire shape:
//! - `GET /api/v1/ws/ssg-stats?project=<p>` with `Upgrade:
//!   websocket`.
//! - Server emits one JSON frame per second containing
//!   `{cpu_pct, mem_used_bytes, mem_limit_bytes, net_rx_bytes,
//!   net_tx_bytes, block_read_bytes, block_write_bytes}` derived
//!   from bollard's `stats` API.
//! - Connection closes when the client disconnects or the
//!   container exits.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use bollard::container::{Stats, StatsOptions};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use crate::api::query::ssg::resolve_ssg_container_id;
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgStatsParams {
    pub project: String,
}

/// One stats sample. Wire shape matches the per-instance
/// `ContainerStats` format closely so the SPA's existing
/// stats-rendering helpers can be reused if desired.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgStatsSample {
    pub cpu_pct: f64,
    pub mem_used_bytes: u64,
    pub mem_limit_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    /// Unix epoch seconds at the time of the sample.
    pub at_unix: i64,
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

    let mut stream = docker.stats(
        &container_id,
        Some(StatsOptions {
            stream: true,
            one_shot: false,
        }),
    );

    while let Some(chunk) = stream.next().await {
        let stats = match chunk {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "ssg stats stream error");
                break;
            }
        };
        let sample = sample_from_stats(&stats);
        let Ok(json) = serde_json::to_string(&sample) else {
            continue;
        };
        if socket.send(Message::Text(json.into())).await.is_err() {
            break;
        }
    }
}

fn sample_from_stats(s: &Stats) -> SsgStatsSample {
    let cpu_total = s.cpu_stats.cpu_usage.total_usage as i128;
    let cpu_total_prev = s.precpu_stats.cpu_usage.total_usage as i128;
    let cpu_delta = cpu_total - cpu_total_prev;

    let sys_now = s.cpu_stats.system_cpu_usage.unwrap_or(0) as i128;
    let sys_prev = s.precpu_stats.system_cpu_usage.unwrap_or(0) as i128;
    let sys_delta = sys_now - sys_prev;

    let online_cpus = s.cpu_stats.online_cpus.unwrap_or(1) as f64;
    let cpu_pct = if sys_delta > 0 && cpu_delta > 0 {
        (cpu_delta as f64 / sys_delta as f64) * online_cpus * 100.0
    } else {
        0.0
    };

    let mem_used_bytes = s.memory_stats.usage.unwrap_or(0);
    let mem_limit_bytes = s.memory_stats.limit.unwrap_or(0);

    let (net_rx_bytes, net_tx_bytes) = s
        .networks
        .as_ref()
        .map(|nets| {
            nets.values().fold((0_u64, 0_u64), |(rx, tx), n| {
                (rx + n.rx_bytes, tx + n.tx_bytes)
            })
        })
        .unwrap_or((0, 0));

    let (block_read_bytes, block_write_bytes) = s
        .blkio_stats
        .io_service_bytes_recursive
        .as_ref()
        .map(|entries| {
            entries.iter().fold((0_u64, 0_u64), |(read, write), e| {
                match e.op.to_lowercase().as_str() {
                    "read" => (read + e.value, write),
                    "write" => (read, write + e.value),
                    _ => (read, write),
                }
            })
        })
        .unwrap_or((0, 0));

    SsgStatsSample {
        cpu_pct,
        mem_used_bytes,
        mem_limit_bytes,
        net_rx_bytes,
        net_tx_bytes,
        block_read_bytes,
        block_write_bytes,
        at_unix: chrono::Utc::now().timestamp(),
    }
}
