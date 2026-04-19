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

use coast_core::protocol::{ContainerStats, ContainerStatsRequest};
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

pub async fn start_remote_dind_stats_collector(
    state: Arc<AppState>,
    key: String,
    project: String,
    name: String,
) {
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
        run_remote_dind_collector(state2, key2, project, name, tx).await;
    });

    collectors.insert(key, handle);
}

/// Apply /proc-based memory, disk I/O, and network counters onto a
/// `ContainerStats` snapshot, replacing Docker API values that are
/// unreliable when cgroup controllers are missing.
fn apply_proc_overlay(cs: &mut ContainerStats, output: &str) -> bool {
    let parts: Vec<&str> = output.split_whitespace().collect();
    if parts.len() < 6 {
        return false;
    }
    let rss_kb: u64 = parts[0].parse().unwrap_or(0);
    let mem_total_kb: u64 = parts[1].parse().unwrap_or(0);
    cs.memory_used_bytes = rss_kb * 1024;
    cs.memory_limit_bytes = mem_total_kb * 1024;
    cs.memory_percent = if cs.memory_limit_bytes > 0 {
        (cs.memory_used_bytes as f64 / cs.memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };
    cs.disk_read_bytes = parts[2].parse().unwrap_or(0);
    cs.disk_write_bytes = parts[3].parse().unwrap_or(0);
    cs.network_rx_bytes = parts[4].parse().unwrap_or(0);
    cs.network_tx_bytes = parts[5].parse().unwrap_or(0);
    true
}

async fn push_dind_stats(
    history: &tokio::sync::Mutex<std::collections::HashMap<String, VecDeque<serde_json::Value>>>,
    key: &str,
    tx: &broadcast::Sender<serde_json::Value>,
    cs: &ContainerStats,
) {
    if let Ok(json_val) = serde_json::to_value(cs) {
        let mut guard = history.lock().await;
        let ring = guard.entry(key.to_string()).or_insert_with(VecDeque::new);
        if ring.len() >= HISTORY_CAP {
            ring.pop_front();
        }
        ring.push_back(json_val.clone());
        drop(guard);
        let _ = tx.send(json_val);
    }
}

const DIND_PROC_SCRIPT: &str = concat!(
    r#"PSS=0; RB=0; WB=0; "#,
    r#"for p in /proc/[0-9]*/smaps_rollup; do "#,
    r#"r=$(awk '/^Pss:/{print $2}' "$p" 2>/dev/null); PSS=$((PSS+${r:-0})); done; "#,
    r#"for p in /proc/[0-9]*/io; do "#,
    r#"io=$(cat "$p" 2>/dev/null || true); "#,
    r#"rb=$(echo "$io" | awk '/^read_bytes:/{print $2}'); RB=$((RB+${rb:-0})); "#,
    r#"wb=$(echo "$io" | awk '/^write_bytes:/{print $2}'); WB=$((WB+${wb:-0})); done; "#,
    r#"NR=$(awk 'NR>2&&$1!~/lo:/{s+=$2}END{print s+0}' /proc/1/net/dev 2>/dev/null); "#,
    r#"NT=$(awk 'NR>2&&$1!~/lo:/{s+=$10}END{print s+0}' /proc/1/net/dev 2>/dev/null); "#,
    r#"ML=$(awk '/^MemTotal:/{print $2}' /proc/meminfo 2>/dev/null); "#,
    r#"printf '%s %s %s %s %s %s\n' "$PSS" "$ML" "$RB" "$WB" "${NR:-0}" "${NT:-0}""#,
);

async fn fetch_remote_dind_stats(
    state: &AppState,
    project: &str,
    name: &str,
    key: &str,
    stats_req: &ContainerStatsRequest,
) -> Option<ContainerStats> {
    let docker_stats = async {
        let remote_config =
            crate::handlers::remote::resolve_remote_for_instance(project, name, state).await?;
        let client = crate::handlers::remote::RemoteClient::connect(&remote_config).await?;
        crate::handlers::remote::forward::forward_container_stats(&client, stats_req).await
    }
    .await;

    let mut cs = match docker_stats {
        Ok(resp) => resp.stats,
        Err(e) => {
            warn!(key = %key, error = %e, "remote DinD stats poll failed");
            return None;
        }
    };

    let cmd = vec!["sh".into(), "-c".into(), DIND_PROC_SCRIPT.to_string()];
    match crate::api::query::exec_in_remote_coast(state, project, name, cmd).await {
        Err(e) => {
            warn!(key = %key, error = %e, "DinD /proc stats exec failed, using Docker API values");
        }
        Ok(output) => {
            if !apply_proc_overlay(&mut cs, &output) {
                warn!(key = %key, output = %output.trim(), "DinD /proc stats: unexpected output format");
            }
        }
    }

    Some(cs)
}

async fn run_remote_dind_collector(
    state: Arc<AppState>,
    key: String,
    project: String,
    name: String,
    tx: broadcast::Sender<serde_json::Value>,
) {
    info!(key = %key, "remote DinD stats collector started");

    let stats_req = ContainerStatsRequest {
        name: name.clone(),
        project: project.clone(),
    };

    loop {
        if let Some(cs) = fetch_remote_dind_stats(&state, &project, &name, &key, &stats_req).await {
            push_dind_stats(&state.stats_history, &key, &tx, &cs).await;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

/// Clean up collector entries only if this collector is still the active one.
async fn cleanup_collector_if_current(
    state: &AppState,
    key: &str,
    tx: &broadcast::Sender<serde_json::Value>,
) {
    let mut broadcasts = state.stats_broadcasts.lock().await;
    if let Some(current_tx) = broadcasts.get(key) {
        if current_tx.same_channel(tx) {
            broadcasts.remove(key);
        }
    }
    drop(broadcasts);
    let mut collectors = state.stats_collectors.lock().await;
    if let Some(handle) = collectors.get(key) {
        if handle.is_finished() {
            collectors.remove(key);
        }
    }
}

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
                push_dind_stats(&state.stats_history, &key, &tx, &cs).await;
            }
            Err(e) => {
                warn!(key = %key, error = %e, "stats stream error");
                break;
            }
        }
    }

    info!(key = %key, "background stats collector stopped");
    cleanup_collector_if_current(&state, &key, &tx).await;
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

    let key = stats_key(&params.project, &params.name);
    drop(db);

    if instance.remote_host.is_some() {
        if !state.stats_collectors.lock().await.contains_key(&key) {
            start_remote_dind_stats_collector(
                state.clone(),
                key.clone(),
                params.project.clone(),
                params.name.clone(),
            )
            .await;
        }
    } else if !state.stats_collectors.lock().await.contains_key(&key) {
        if let Some(cid) = instance.container_id.as_deref() {
            start_stats_collector(state.clone(), cid.to_string(), key.clone()).await;
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)))
}

/// Send buffered history to a newly connected WebSocket client.
async fn replay_stats_history(socket: &mut WebSocket, state: &AppState, key: &str) -> bool {
    let history = state.stats_history.lock().await;
    if let Some(ring) = history.get(key) {
        for val in ring.iter() {
            if socket
                .send(Message::Text(val.to_string().into()))
                .await
                .is_err()
            {
                return false;
            }
        }
    }
    true
}

/// Forward a broadcast stats value to the WebSocket client.
async fn forward_stats_broadcast(
    socket: &mut WebSocket,
    result: Result<serde_json::Value, broadcast::error::RecvError>,
    key: &str,
) -> std::ops::ControlFlow<()> {
    match result {
        Ok(val) => {
            if socket
                .send(Message::Text(val.to_string().into()))
                .await
                .is_err()
            {
                std::ops::ControlFlow::Break(())
            } else {
                std::ops::ControlFlow::Continue(())
            }
        }
        Err(broadcast::error::RecvError::Lagged(n)) => {
            warn!(key = %key, skipped = n, "stats WS lagged");
            std::ops::ControlFlow::Continue(())
        }
        Err(broadcast::error::RecvError::Closed) => std::ops::ControlFlow::Break(()),
    }
}

async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(key = %key, "stats WS connected");

    if !replay_stats_history(&mut socket, &state, &key).await {
        return;
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
                if forward_stats_broadcast(&mut socket, result, &key).await.is_break() {
                    break;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_stats() -> ContainerStats {
        ContainerStats {
            timestamp: String::new(),
            cpu_percent: 0.0,
            memory_used_bytes: 0,
            memory_limit_bytes: 0,
            memory_percent: 0.0,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            pids: 0,
        }
    }

    #[test]
    fn test_apply_proc_overlay_valid_output() {
        let mut cs = blank_stats();
        let output = "1024 2048000 500 600 700 800\n";
        assert!(apply_proc_overlay(&mut cs, output));
        assert_eq!(cs.memory_used_bytes, 1024 * 1024);
        assert_eq!(cs.memory_limit_bytes, 2048000 * 1024);
        assert!(cs.memory_percent > 0.0);
        assert_eq!(cs.disk_read_bytes, 500);
        assert_eq!(cs.disk_write_bytes, 600);
        assert_eq!(cs.network_rx_bytes, 700);
        assert_eq!(cs.network_tx_bytes, 800);
    }

    #[test]
    fn test_apply_proc_overlay_too_few_fields() {
        let mut cs = blank_stats();
        assert!(!apply_proc_overlay(&mut cs, "1024 2048"));
        assert_eq!(cs.memory_used_bytes, 0);
    }

    #[test]
    fn test_apply_proc_overlay_empty_input() {
        let mut cs = blank_stats();
        assert!(!apply_proc_overlay(&mut cs, ""));
    }

    #[test]
    fn test_apply_proc_overlay_zero_mem_total() {
        let mut cs = blank_stats();
        let output = "1024 0 100 200 300 400";
        assert!(apply_proc_overlay(&mut cs, output));
        assert_eq!(cs.memory_percent, 0.0);
    }

    #[test]
    fn test_apply_proc_overlay_non_numeric_falls_back_to_zero() {
        let mut cs = blank_stats();
        let output = "abc def 100 200 300 400";
        assert!(apply_proc_overlay(&mut cs, output));
        assert_eq!(cs.memory_used_bytes, 0);
        assert_eq!(cs.memory_limit_bytes, 0);
        assert_eq!(cs.disk_read_bytes, 100);
    }

    #[test]
    fn test_apply_proc_overlay_preserves_cpu_and_pids() {
        let mut cs = blank_stats();
        cs.cpu_percent = 42.5;
        cs.pids = 7;
        let output = "1024 2048 100 200 300 400";
        apply_proc_overlay(&mut cs, output);
        assert_eq!(cs.cpu_percent, 42.5);
        assert_eq!(cs.pids, 7);
    }

    // --- push_dind_stats tests ---

    #[tokio::test]
    async fn test_push_dind_stats_inserts_and_broadcasts() {
        let history = tokio::sync::Mutex::new(std::collections::HashMap::<
            String,
            VecDeque<serde_json::Value>,
        >::new());
        let (tx, mut rx) = broadcast::channel(16);
        let cs = blank_stats();

        push_dind_stats(&history, "key1", &tx, &cs).await;

        let h = history.lock().await;
        assert_eq!(h.get("key1").unwrap().len(), 1);
        drop(h);
        assert!(rx.recv().await.is_ok());
    }

    #[tokio::test]
    async fn test_push_dind_stats_evicts_at_cap() {
        let history = tokio::sync::Mutex::new(std::collections::HashMap::<
            String,
            VecDeque<serde_json::Value>,
        >::new());
        let (tx, _rx) = broadcast::channel(16);
        let cs = blank_stats();

        for _ in 0..HISTORY_CAP {
            push_dind_stats(&history, "k", &tx, &cs).await;
        }
        push_dind_stats(&history, "k", &tx, &cs).await;

        let h = history.lock().await;
        assert_eq!(h.get("k").unwrap().len(), HISTORY_CAP);
    }
}
