use std::collections::VecDeque;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use ts_rs::TS;

use coast_core::types::InstanceStatus;
use rust_i18n::t;

use crate::handlers::compose_context;
use crate::server::AppState;

const HISTORY_CAP: usize = 300;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ServiceStatsParams {
    pub project: String,
    pub name: String,
    pub service: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/service/stats/stream", get(ws_handler))
        .route("/service/stats/history", get(get_history))
}

fn service_stats_key(project: &str, name: &str, service: &str) -> String {
    format!("{project}:{name}:{service}")
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ServiceStatsParams>,
) -> Result<axum::Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let key = service_stats_key(&params.project, &params.name, &params.service);
    let history = state.service_stats_history.lock().await;
    let points = history
        .get(&key)
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default();
    Ok(axum::Json(points))
}

async fn resolve_inner_container(
    docker: &bollard::Docker,
    coast_container_id: &str,
    project: &str,
    service: &str,
) -> Option<String> {
    let ctx = compose_context(project);
    let cmd_parts = ctx.compose_shell(&format!("ps --format json {service}"));
    let cmd_refs: Vec<String> = cmd_parts.clone();

    let exec_options = CreateExecOptions {
        cmd: Some(cmd_refs),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(coast_container_id, exec_options)
        .await
        .ok()?;
    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    if let Ok(StartExecResults::Attached { mut output, .. }) =
        docker.start_exec(&exec.id, Some(start_options)).await
    {
        let mut buf = String::new();
        while let Some(chunk) = output.next().await {
            if let Ok(
                bollard::container::LogOutput::StdOut { message }
                | bollard::container::LogOutput::StdErr { message },
            ) = chunk
            {
                buf.push_str(&String::from_utf8_lossy(&message));
            }
        }

        for line in buf.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('{') {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(name) = val.get("Name").and_then(|v| v.as_str()) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Discover all running inner services for a coast instance and start
/// background stats collectors for each one.
pub async fn discover_and_start_service_collectors(
    state: Arc<AppState>,
    coast_container_id: String,
    project: String,
    name: String,
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    let ctx = compose_context(&project);
    let cmd_parts = ctx.compose_shell("ps --format json");
    let cmd_refs: Vec<String> = cmd_parts.clone();

    let exec_options = CreateExecOptions {
        cmd: Some(cmd_refs),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let services: Vec<String> = match docker.create_exec(&coast_container_id, exec_options).await {
        Ok(exec) => {
            let start_options = StartExecOptions {
                detach: false,
                ..Default::default()
            };
            match docker.start_exec(&exec.id, Some(start_options)).await {
                Ok(StartExecResults::Attached { mut output, .. }) => {
                    let mut buf = String::new();
                    while let Some(chunk) = output.next().await {
                        if let Ok(
                            bollard::container::LogOutput::StdOut { message }
                            | bollard::container::LogOutput::StdErr { message },
                        ) = chunk
                        {
                            buf.push_str(&String::from_utf8_lossy(&message));
                        }
                    }
                    parse_service_names(&buf)
                }
                _ => vec![],
            }
        }
        Err(_) => vec![],
    };

    for svc in services {
        let key = service_stats_key(&project, &name, &svc);
        if state
            .service_stats_collectors
            .lock()
            .await
            .contains_key(&key)
        {
            continue;
        }
        start_service_stats_collector(
            state.clone(),
            coast_container_id.clone(),
            key,
            project.clone(),
            svc,
        )
        .await;
    }
}

fn parse_service_names(output: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(svc) = val.get("Service").and_then(|v| v.as_str()) {
                if !names.contains(&svc.to_string()) {
                    names.push(svc.to_string());
                }
            }
        }
    }
    names
}

/// Stop all service stats collectors for a given coast instance.
pub async fn stop_all_service_collectors_for_instance(state: &AppState, project: &str, name: &str) {
    let prefix = format!("{project}:{name}:");
    let keys: Vec<String> = {
        let collectors = state.service_stats_collectors.lock().await;
        collectors
            .keys()
            .filter(|k| k.starts_with(&prefix))
            .cloned()
            .collect()
    };
    for key in keys {
        stop_service_stats_collector(state, &key).await;
    }
    let mut history = state.service_stats_history.lock().await;
    history.retain(|k, _| !k.starts_with(&prefix));
}

pub async fn start_service_stats_collector(
    state: Arc<AppState>,
    coast_container_id: String,
    key: String,
    project: String,
    service: String,
) {
    {
        let collectors = state.service_stats_collectors.lock().await;
        if collectors.contains_key(&key) {
            return;
        }
    }

    let (tx, _) = broadcast::channel::<serde_json::Value>(64);
    {
        let mut broadcasts = state.service_stats_broadcasts.lock().await;
        broadcasts.insert(key.clone(), tx.clone());
    }

    let state2 = state.clone();
    let key2 = key.clone();
    let handle = tokio::spawn(async move {
        run_service_collector(state2, coast_container_id, key2, project, service, tx).await;
    });

    let mut collectors = state.service_stats_collectors.lock().await;
    collectors.insert(key, handle);
}

pub async fn stop_service_stats_collector(state: &AppState, key: &str) {
    if let Some(handle) = state.service_stats_collectors.lock().await.remove(key) {
        handle.abort();
    }
    state.service_stats_broadcasts.lock().await.remove(key);
}

#[allow(clippy::cognitive_complexity)]
async fn run_service_collector(
    state: Arc<AppState>,
    coast_container_id: String,
    key: String,
    project: String,
    service: String,
    tx: broadcast::Sender<serde_json::Value>,
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    let Some(inner_name) =
        resolve_inner_container(docker, &coast_container_id, &project, &service).await
    else {
        warn!(
            key = %key,
            "could not resolve inner container for service stats"
        );
        state.service_stats_broadcasts.lock().await.remove(&key);
        state.service_stats_collectors.lock().await.remove(&key);
        return;
    };

    info!(
        key = %key,
        inner_container = %inner_name,
        "background service stats collector started"
    );

    let stats_cmd = format!(
        "docker stats {} --no-stream --format '{{{{json .}}}}'",
        inner_name
    );

    loop {
        let poll_cmd = vec!["sh".to_string(), "-c".to_string(), stats_cmd.clone()];

        let exec_options = CreateExecOptions {
            cmd: Some(poll_cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let stats_json = match docker.create_exec(&coast_container_id, exec_options).await {
            Ok(exec) => {
                let start_options = StartExecOptions {
                    detach: false,
                    ..Default::default()
                };
                match docker.start_exec(&exec.id, Some(start_options)).await {
                    Ok(StartExecResults::Attached { mut output, .. }) => {
                        let mut buf = String::new();
                        while let Some(chunk) = output.next().await {
                            if let Ok(bollard::container::LogOutput::StdOut { message }) = chunk {
                                buf.push_str(&String::from_utf8_lossy(&message));
                            }
                        }
                        parse_docker_stats_json(&buf)
                    }
                    _ => None,
                }
            }
            Err(e) => {
                warn!(key = %key, error = %e, "service stats exec failed, stopping collector");
                break;
            }
        };

        if let Some(json_val) = stats_json {
            {
                let mut history = state.service_stats_history.lock().await;
                let ring = history.entry(key.clone()).or_insert_with(VecDeque::new);
                if ring.len() >= HISTORY_CAP {
                    ring.pop_front();
                }
                ring.push_back(json_val.clone());
            }
            let _ = tx.send(json_val);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    info!(key = %key, "background service stats collector stopped");
    state.service_stats_broadcasts.lock().await.remove(&key);
    state.service_stats_collectors.lock().await.remove(&key);
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<ServiceStatsParams>,
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

    let container_id = instance.container_id.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            t!("error.no_container_id", locale = &lang).to_string(),
        )
    })?;

    drop(db);

    let key = service_stats_key(&params.project, &params.name, &params.service);

    if !state
        .service_stats_collectors
        .lock()
        .await
        .contains_key(&key)
    {
        start_service_stats_collector(
            state.clone(),
            container_id,
            key.clone(),
            params.project.clone(),
            params.service.clone(),
        )
        .await;
    }

    Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(key = %key, "service stats WS connected");

    {
        let history = state.service_stats_history.lock().await;
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
        let broadcasts = state.service_stats_broadcasts.lock().await;
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
                        warn!(key = %key, skipped = n, "service stats WS lagged");
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

    debug!(key = %key, "service stats WS disconnected (collector keeps running)");
}

fn parse_docker_stats_json(output: &str) -> Option<serde_json::Value> {
    for line in output.lines() {
        let trimmed = line.trim().trim_matches('\'');
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let timestamp = chrono::Utc::now().to_rfc3339();

            let cpu_str = val.get("CPUPerc").and_then(|v| v.as_str()).unwrap_or("0%");
            let cpu_percent: f64 = cpu_str.trim_end_matches('%').parse().unwrap_or(0.0);

            let (mem_used, mem_limit) = parse_mem_usage(
                val.get("MemUsage")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0B / 0B"),
            );

            let mem_percent: f64 = val
                .get("MemPerc")
                .and_then(|v| v.as_str())
                .unwrap_or("0%")
                .trim_end_matches('%')
                .parse()
                .unwrap_or(0.0);

            let (net_rx, net_tx) = parse_io_pair(
                val.get("NetIO")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0B / 0B"),
            );

            let (disk_read, disk_write) = parse_io_pair(
                val.get("BlockIO")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0B / 0B"),
            );

            let pids: u64 = val
                .get("PIDs")
                .and_then(|v| v.as_str())
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);

            return Some(serde_json::json!({
                "timestamp": timestamp,
                "cpu_percent": cpu_percent,
                "memory_used_bytes": mem_used,
                "memory_limit_bytes": mem_limit,
                "memory_percent": mem_percent,
                "network_rx_bytes": net_rx,
                "network_tx_bytes": net_tx,
                "disk_read_bytes": disk_read,
                "disk_write_bytes": disk_write,
                "pids": pids,
            }));
        }
    }
    None
}

fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() || s == "0" {
        return 0;
    }
    let (num_str, mult) = if let Some(n) = s.strip_suffix("GiB") {
        (n, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(n) = s.strip_suffix("MiB") {
        (n, 1024.0 * 1024.0)
    } else if let Some(n) = s.strip_suffix("KiB") {
        (n, 1024.0)
    } else if let Some(n) = s.strip_suffix("GB") {
        (n, 1_000_000_000.0)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1_000_000.0)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1_000.0)
    } else if let Some(n) = s.strip_suffix("kB") {
        (n, 1_000.0)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1.0)
    } else {
        (s, 1.0)
    };
    num_str.trim().parse::<f64>().unwrap_or(0.0) as u64 * mult as u64
}

fn parse_mem_usage(s: &str) -> (u64, u64) {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        (parse_size(parts[0]), parse_size(parts[1]))
    } else {
        (0, 0)
    }
}

fn parse_io_pair(s: &str) -> (u64, u64) {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        (parse_size(parts[0]), parse_size(parts[1]))
    } else {
        (0, 0)
    }
}
