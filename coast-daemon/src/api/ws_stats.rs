use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::container::StatsOptions;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use ts_rs::TS;

use coast_core::protocol::ContainerStats;
use coast_core::types::InstanceStatus;
use rust_i18n::t;

use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct StatsParams {
    pub project: String,
    pub name: String,
}

const HISTORY_CAP: usize = 300;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/stats/stream", get(ws_handler))
        .route("/stats/history", get(get_history))
}

// ---------------------------------------------------------------------------
// REST: history
// ---------------------------------------------------------------------------

async fn get_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatsParams>,
) -> Result<axum::Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let key = format!("{}:{}", params.project, params.name);
    let history = state.stats_history.lock().await;
    let points = history
        .get(&key)
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default();
    Ok(axum::Json(points))
}

// ---------------------------------------------------------------------------
// Background collector: start / stop
// ---------------------------------------------------------------------------

pub fn stats_key(project: &str, name: &str) -> String {
    format!("{project}:{name}")
}

pub async fn start_stats_collector(state: Arc<AppState>, container_id: String, key: String) {
    let mut collectors = state.stats_collectors.lock().await;
    if collectors.contains_key(&key) {
        return;
    }

    let (tx, _) = broadcast::channel::<serde_json::Value>(64);
    state
        .stats_broadcasts
        .lock()
        .await
        .insert(key.clone(), tx.clone());

    let state2 = state.clone();
    let key2 = key.clone();
    let handle = tokio::spawn(async move {
        run_collector(state2, container_id, key2, tx).await;
    });

    collectors.insert(key, handle);
}

pub async fn stop_stats_collector(state: &AppState, key: &str) {
    if let Some(handle) = state.stats_collectors.lock().await.remove(key) {
        handle.abort();
    }
    state.stats_broadcasts.lock().await.remove(key);
}

#[allow(clippy::cognitive_complexity)]
async fn run_collector(
    state: Arc<AppState>,
    container_id: String,
    key: String,
    tx: broadcast::Sender<serde_json::Value>,
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    info!(key = %key, container = %container_id, "background stats collector started");

    let options = StatsOptions {
        stream: true,
        one_shot: false,
    };
    let mut stream = docker.stats(&container_id, Some(options));
    let mut prev_cpu_total: u64 = 0;
    let mut prev_cpu_system: u64 = 0;

    while let Some(result) = stream.next().await {
        match result {
            Ok(stats) => {
                let cs = extract_stats(&stats, &mut prev_cpu_total, &mut prev_cpu_system);
                if let Ok(json_val) = serde_json::to_value(&cs) {
                    {
                        let mut history = state.stats_history.lock().await;
                        let ring = history.entry(key.clone()).or_insert_with(VecDeque::new);
                        if ring.len() >= HISTORY_CAP {
                            ring.pop_front();
                        }
                        ring.push_back(json_val.clone());
                    }
                    let _ = tx.send(json_val);
                }
            }
            Err(e) => {
                warn!(key = %key, error = %e, "stats stream error");
                break;
            }
        }
    }

    info!(key = %key, "background stats collector stopped");

    // Only clean up if this collector's broadcast sender is still the active one.
    // A replacement collector may have already overwritten these entries.
    {
        let mut broadcasts = state.stats_broadcasts.lock().await;
        if let Some(current_tx) = broadcasts.get(&key) {
            if current_tx.same_channel(&tx) {
                broadcasts.remove(&key);
            }
        }
    }
    // The collectors map entry may already have been replaced; remove only
    // if the handle for our key has finished (is_finished check).
    {
        let mut collectors = state.stats_collectors.lock().await;
        if let Some(handle) = collectors.get(&key) {
            if handle.is_finished() {
                collectors.remove(&key);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// WebSocket: thin subscriber
// ---------------------------------------------------------------------------

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatsParams>,
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

    drop(db);

    let key = stats_key(&params.project, &params.name);

    // Ensure collector is running
    if !state.stats_collectors.lock().await.contains_key(&key) {
        if let Some(cid) = instance.container_id.as_deref() {
            start_stats_collector(state.clone(), cid.to_string(), key.clone()).await;
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(key = %key, "stats WS connected");

    // Send buffered history
    {
        let history = state.stats_history.lock().await;
        if let Some(ring) = history.get(&key) {
            for val in ring.iter() {
                let json_str = val.to_string();
                if socket.send(Message::Text(json_str.into())).await.is_err() {
                    return;
                }
            }
        }
    }

    // Subscribe to live broadcast
    let mut rx = {
        let broadcasts = state.stats_broadcasts.lock().await;
        match broadcasts.get(&key) {
            Some(tx) => tx.subscribe(),
            None => {
                let _ = socket
                    .send(Message::Text("Stats collector not running".into()))
                    .await;
                return;
            }
        }
    };

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(val) => {
                        let json_str = val.to_string();
                        if socket.send(Message::Text(json_str.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(key = %key, skipped = n, "stats WS lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
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

    debug!(key = %key, "stats WS disconnected (collector keeps running)");
}

// ---------------------------------------------------------------------------
// Stats extraction (unchanged)
// ---------------------------------------------------------------------------

fn extract_stats(
    stats: &bollard::container::Stats,
    prev_cpu_total: &mut u64,
    prev_cpu_system: &mut u64,
) -> ContainerStats {
    let cpu_percent = if let Some(ref cpu) = stats.cpu_stats.system_cpu_usage {
        let cpu_delta = stats
            .cpu_stats
            .cpu_usage
            .total_usage
            .saturating_sub(*prev_cpu_total);
        let system_delta = cpu.saturating_sub(*prev_cpu_system);
        let online_cpus = stats.cpu_stats.online_cpus.unwrap_or(1);

        *prev_cpu_total = stats.cpu_stats.cpu_usage.total_usage;
        *prev_cpu_system = *cpu;

        if system_delta > 0 {
            (cpu_delta as f64 / system_delta as f64) * online_cpus as f64 * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let memory_used_bytes = stats.memory_stats.usage.unwrap_or(0)
        - stats
            .memory_stats
            .stats
            .as_ref()
            .map(|s| match s {
                bollard::container::MemoryStatsStats::V1(v1) => v1.cache,
                bollard::container::MemoryStatsStats::V2(v2) => v2.inactive_file,
            })
            .unwrap_or(0);
    let memory_limit_bytes = stats.memory_stats.limit.unwrap_or(0);
    let memory_percent = if memory_limit_bytes > 0 {
        (memory_used_bytes as f64 / memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };

    let (disk_read_bytes, disk_write_bytes) = stats
        .blkio_stats
        .io_service_bytes_recursive
        .as_ref()
        .map(|entries| {
            let mut read = 0u64;
            let mut write = 0u64;
            for entry in entries {
                match entry.op.as_str() {
                    "read" | "Read" => read += entry.value,
                    "write" | "Write" => write += entry.value,
                    _ => {}
                }
            }
            (read, write)
        })
        .unwrap_or((0, 0));

    let (network_rx_bytes, network_tx_bytes) = stats
        .networks
        .as_ref()
        .map(|nets| {
            let mut rx = 0u64;
            let mut tx = 0u64;
            for net in nets.values() {
                rx += net.rx_bytes;
                tx += net.tx_bytes;
            }
            (rx, tx)
        })
        .unwrap_or((0, 0));

    let pids = stats.pids_stats.current.unwrap_or(0);
    let timestamp = stats.read.clone();

    ContainerStats {
        timestamp,
        cpu_percent,
        memory_used_bytes,
        memory_limit_bytes,
        memory_percent,
        disk_read_bytes,
        disk_write_bytes,
        network_rx_bytes,
        network_tx_bytes,
        pids,
    }
}
