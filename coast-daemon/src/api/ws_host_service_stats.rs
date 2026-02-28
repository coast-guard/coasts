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

use rust_i18n::t;

use crate::server::AppState;
use crate::shared_services::shared_container_name;
use coast_core::protocol::ContainerStats;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct HostServiceStatsParams {
    pub project: String,
    pub service: String,
}

const HISTORY_CAP: usize = 300;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/host-service/stats/stream", get(ws_handler))
        .route("/host-service/stats/history", get(get_history))
}

pub fn stats_key(project: &str, service: &str) -> String {
    format!("host:{project}:{service}")
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostServiceStatsParams>,
) -> Result<axum::Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let key = stats_key(&params.project, &params.service);
    let history = state.stats_history.lock().await;
    let points = history
        .get(&key)
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default();
    Ok(axum::Json(points))
}

pub async fn start_host_service_collector(
    state: Arc<AppState>,
    container_name: String,
    key: String,
) {
    {
        let collectors = state.stats_collectors.lock().await;
        if collectors.contains_key(&key) {
            return;
        }
    }

    let (tx, _) = broadcast::channel::<serde_json::Value>(64);
    {
        let mut broadcasts = state.stats_broadcasts.lock().await;
        broadcasts.insert(key.clone(), tx.clone());
    }

    let state2 = state.clone();
    let key2 = key.clone();
    let handle = tokio::spawn(async move {
        run_collector(state2, container_name, key2, tx).await;
    });

    let mut collectors = state.stats_collectors.lock().await;
    collectors.insert(key, handle);
}

#[allow(clippy::cognitive_complexity)]
async fn run_collector(
    state: Arc<AppState>,
    container_name: String,
    key: String,
    tx: broadcast::Sender<serde_json::Value>,
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    info!(key = %key, container = %container_name, "host-service stats collector started");

    let options = StatsOptions {
        stream: true,
        one_shot: false,
    };
    let mut stream = docker.stats(&container_name, Some(options));
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
                warn!(key = %key, error = %e, "host-service stats stream error");
                break;
            }
        }
    }

    info!(key = %key, "host-service stats collector stopped");

    state.stats_broadcasts.lock().await.remove(&key);
    state.stats_collectors.lock().await.remove(&key);
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostServiceStatsParams>,
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

    let key = stats_key(&params.project, &params.service);

    if !state.stats_collectors.lock().await.contains_key(&key) {
        start_host_service_collector(state.clone(), container_name, key.clone()).await;
    }

    Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(key = %key, "host-service stats WS connected");

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
                        warn!(key = %key, skipped = n, "host-service stats WS lagged");
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

    debug!(key = %key, "host-service stats WS disconnected");
}

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
