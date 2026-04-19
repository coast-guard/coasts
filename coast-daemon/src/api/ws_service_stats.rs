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

use crate::handlers::{compose_context, compose_context_for_build};
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

/// Execute a single `docker stats --no-stream` poll and parse the result.
async fn poll_service_stats_once(
    docker: &bollard::Docker,
    coast_container_id: &str,
    stats_cmd: &str,
) -> Result<Option<serde_json::Value>, ()> {
    let poll_cmd = vec!["sh".to_string(), "-c".to_string(), stats_cmd.to_string()];
    let exec_options = CreateExecOptions {
        cmd: Some(poll_cmd),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    match docker.create_exec(coast_container_id, exec_options).await {
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
                    Ok(parse_docker_stats_json(&buf))
                }
                _ => Ok(None),
            }
        }
        Err(_) => Err(()),
    }
}

/// Remove the collector and broadcast entries for a key.
async fn cleanup_service_collector(state: &AppState, key: &str) {
    state.service_stats_broadcasts.lock().await.remove(key);
    state.service_stats_collectors.lock().await.remove(key);
}

/// Run the polling loop: poll stats every 2s, push to history + broadcast.
async fn run_service_poll_loop(
    docker: &bollard::Docker,
    coast_container_id: &str,
    inner_name: &str,
    state: &AppState,
    key: &str,
    tx: &broadcast::Sender<serde_json::Value>,
) {
    let stats_cmd = format!(
        "docker stats {} --no-stream --format '{{{{json .}}}}'",
        inner_name
    );

    loop {
        match poll_service_stats_once(docker, coast_container_id, &stats_cmd).await {
            Ok(Some(json_val)) => {
                push_service_stats(&state.service_stats_history, key, tx, json_val).await;
            }
            Ok(None) => {}
            Err(()) => {
                warn!(key = %key, "service stats exec failed, stopping collector");
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

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
        resolve_inner_container(&docker, &coast_container_id, &project, &service).await
    else {
        warn!(key = %key, "could not resolve inner container for service stats");
        cleanup_service_collector(&state, &key).await;
        return;
    };

    info!(key = %key, inner_container = %inner_name, "background service stats collector started");

    run_service_poll_loop(&docker, &coast_container_id, &inner_name, &state, &key, &tx).await;

    info!(key = %key, "background service stats collector stopped");
    cleanup_service_collector(&state, &key).await;
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

    if instance.remote_host.is_some() {
        let build_id = instance.build_id.clone();
        drop(db);

        let key = service_stats_key(&params.project, &params.name, &params.service);

        if !state
            .service_stats_collectors
            .lock()
            .await
            .contains_key(&key)
        {
            start_remote_service_stats_collector(
                state.clone(),
                key.clone(),
                params.project.clone(),
                params.name.clone(),
                params.service.clone(),
                build_id,
            )
            .await;
        }

        return Ok(ws.on_upgrade(move |socket| handle_stats_socket(socket, state, key)));
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

/// Send buffered history to a newly connected WebSocket client.
async fn replay_service_stats_history(socket: &mut WebSocket, state: &AppState, key: &str) -> bool {
    let history = state.service_stats_history.lock().await;
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
async fn forward_service_broadcast(
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
            warn!(key = %key, skipped = n, "service stats WS lagged");
            std::ops::ControlFlow::Continue(())
        }
        Err(broadcast::error::RecvError::Closed) => std::ops::ControlFlow::Break(()),
    }
}

async fn handle_stats_socket(mut socket: WebSocket, state: Arc<AppState>, key: String) {
    debug!(key = %key, "service stats WS connected");

    if !replay_service_stats_history(&mut socket, &state, &key).await {
        return;
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
                if forward_service_broadcast(&mut socket, result, &key).await.is_break() {
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

/// Build a shell script that collects container stats using /proc fallbacks
/// for metrics unavailable via cgroup (memory, disk I/O, network).
/// CPU and PIDs come from `docker stats` (cgroup cpu controller works).
/// Memory uses PSS from /proc/{pid}/smaps_rollup (proportional set size,
/// avoids double-counting shared pages unlike VmRSS).
/// Disk I/O from /proc/{pid}/io.
/// Network from /proc/{pid}/net/dev.
fn remote_proc_stats_script(container_name: &str) -> String {
    let mut s = String::with_capacity(1024);
    s.push_str("C='");
    s.push_str(container_name);
    s.push_str("'; ");
    s.push_str(concat!(
        r#"DS=$(docker stats "$C" --no-stream --format '{{.CPUPerc}}|{{.PIDs}}' 2>/dev/null); "#,
        r#"CPU=$(echo "$DS" | cut -d'|' -f1); CPU=${CPU:-0%}; "#,
        r#"PC=$(echo "$DS" | cut -d'|' -f2); PC=${PC:-0}; "#,
        r#"PID=$(docker inspect "$C" --format '{{.State.Pid}}' 2>/dev/null); "#,
        r#"PSS=0; RB=0; WB=0; NR=0; NT=0; "#,
        r#"for p in $(docker top "$C" -o pid 2>/dev/null | tail -n +2); do "#,
        r#"r=$(awk '/^Pss:/{print $2}' /proc/$p/smaps_rollup 2>/dev/null); PSS=$((PSS+${r:-0})); "#,
        r#"io=$(cat /proc/$p/io 2>/dev/null || true); "#,
        r#"rb=$(echo "$io" | awk '/^read_bytes:/{print $2}'); RB=$((RB+${rb:-0})); "#,
        r#"wb=$(echo "$io" | awk '/^write_bytes:/{print $2}'); WB=$((WB+${wb:-0})); "#,
        r#"done; "#,
        r#"if [ -n "$PID" ] && [ "$PID" != "0" ] && [ -d "/proc/$PID" ]; then "#,
        r#"NR=$(awk 'NR>2&&$1!~/lo:/{s+=$2}END{print s+0}' /proc/$PID/net/dev 2>/dev/null); NR=${NR:-0}; "#,
        r#"NT=$(awk 'NR>2&&$1!~/lo:/{s+=$10}END{print s+0}' /proc/$PID/net/dev 2>/dev/null); NT=${NT:-0}; "#,
        r#"fi; "#,
        r#"PSS_B=$((PSS*1024)); "#,
        r#"ML=$(awk '/^MemTotal:/{print $2}' /proc/meminfo 2>/dev/null); ML=${ML:-0}; ML_B=$((ML*1024)); "#,
        r#"MP=0; [ "$ML_B" -gt 0 ] 2>/dev/null && MP=$(awk -v r="$PSS_B" -v m="$ML_B" 'BEGIN{printf "%.2f",r/m*100}'); "#,
        r#"printf '{"BlockIO":"%sB / %sB","CPUPerc":"%s","Container":"%s","MemPerc":"%s%%","MemUsage":"%sB / %sB","Name":"%s","NetIO":"%sB / %sB","PIDs":"%s"}\n' "#,
        r#""$RB" "$WB" "$CPU" "$C" "$MP" "$PSS_B" "$ML_B" "$C" "$NR" "$NT" "$PC""#,
    ));
    s
}

async fn resolve_inner_container_remote(
    state: &AppState,
    project: &str,
    name: &str,
    build_id: Option<&str>,
    service: &str,
) -> Option<String> {
    let ctx = compose_context_for_build(project, build_id);
    let project_dir = match &ctx.compose_rel_dir {
        Some(dir) => format!("/workspace/{dir}"),
        None => "/workspace".to_string(),
    };
    let script = format!(
        "CF=/coast-artifact/compose.coast-shared.yml; \
         [ -f \"$CF\" ] || CF=/coast-artifact/compose.yml; \
         docker compose -f \"$CF\" --project-directory {project_dir} ps --format json {service}"
    );
    let cmd = vec!["sh".into(), "-c".into(), script];
    let output = crate::api::query::exec_in_remote_coast(state, project, name, cmd)
        .await
        .ok()?;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if let Some(container_name) = val.get("Name").and_then(|v| v.as_str()) {
                return Some(container_name.to_string());
            }
        }
    }
    None
}

pub async fn start_remote_service_stats_collector(
    state: Arc<AppState>,
    key: String,
    project: String,
    name: String,
    service: String,
    build_id: Option<String>,
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
        run_remote_service_collector(state2, key2, project, name, service, build_id, tx).await;
    });

    let mut collectors = state.service_stats_collectors.lock().await;
    collectors.insert(key, handle);
}

async fn remove_service_stats_collector(state: &AppState, key: &str) {
    state.service_stats_broadcasts.lock().await.remove(key);
    state.service_stats_collectors.lock().await.remove(key);
}

async fn push_service_stats(
    history: &tokio::sync::Mutex<std::collections::HashMap<String, VecDeque<serde_json::Value>>>,
    key: &str,
    tx: &broadcast::Sender<serde_json::Value>,
    value: serde_json::Value,
) {
    let mut guard = history.lock().await;
    let ring = guard.entry(key.to_string()).or_insert_with(VecDeque::new);
    if ring.len() >= HISTORY_CAP {
        ring.pop_front();
    }
    ring.push_back(value.clone());
    drop(guard);
    let _ = tx.send(value);
}

/// Execute one stats collection cycle: run the script on the remote,
/// parse the output, and push to the history ring + broadcast channel.
/// Returns `false` if the exec failed (caller should stop the loop).
async fn collect_remote_service_sample(
    state: &AppState,
    project: &str,
    name: &str,
    key: &str,
    stats_script: &str,
    tx: &broadcast::Sender<serde_json::Value>,
) -> bool {
    let cmd = vec!["sh".into(), "-c".into(), stats_script.to_string()];
    let output = match crate::api::query::exec_in_remote_coast(state, project, name, cmd).await {
        Ok(o) => o,
        Err(e) => {
            warn!(key = %key, error = %e, "remote service stats exec failed, stopping collector");
            return false;
        }
    };
    if let Some(json_val) = parse_docker_stats_json(&output) {
        push_service_stats(&state.service_stats_history, key, tx, json_val).await;
    }
    true
}

async fn run_remote_service_collector(
    state: Arc<AppState>,
    key: String,
    project: String,
    name: String,
    service: String,
    build_id: Option<String>,
    tx: broadcast::Sender<serde_json::Value>,
) {
    let Some(inner_name) =
        resolve_inner_container_remote(&state, &project, &name, build_id.as_deref(), &service)
            .await
    else {
        warn!(key = %key, "could not resolve remote inner container for service stats");
        remove_service_stats_collector(&state, &key).await;
        return;
    };

    info!(
        key = %key,
        inner_container = %inner_name,
        "remote background service stats collector started"
    );

    let stats_script = remote_proc_stats_script(&inner_name);

    loop {
        if !collect_remote_service_sample(&state, &project, &name, &key, &stats_script, &tx).await {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    info!(key = %key, "remote background service stats collector stopped");
    remove_service_stats_collector(&state, &key).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_push_service_stats_inserts_value() {
        let history =
            tokio::sync::Mutex::new(HashMap::<String, VecDeque<serde_json::Value>>::new());
        let (tx, _rx) = broadcast::channel(16);
        let val = serde_json::json!({"cpu": "5%"});

        push_service_stats(&history, "key1", &tx, val.clone()).await;

        let h = history.lock().await;
        assert_eq!(h.get("key1").unwrap().len(), 1);
        assert_eq!(h.get("key1").unwrap()[0], val);
    }

    #[tokio::test]
    async fn test_push_service_stats_evicts_at_cap() {
        let history =
            tokio::sync::Mutex::new(HashMap::<String, VecDeque<serde_json::Value>>::new());
        let (tx, _rx) = broadcast::channel(16);

        for i in 0..HISTORY_CAP {
            push_service_stats(&history, "k", &tx, serde_json::json!(i)).await;
        }
        push_service_stats(&history, "k", &tx, serde_json::json!("new")).await;

        let h = history.lock().await;
        let ring = h.get("k").unwrap();
        assert_eq!(ring.len(), HISTORY_CAP);
        assert_eq!(ring[0], serde_json::json!(1));
        assert_eq!(ring[HISTORY_CAP - 1], serde_json::json!("new"));
    }

    #[tokio::test]
    async fn test_push_service_stats_broadcasts() {
        let history =
            tokio::sync::Mutex::new(HashMap::<String, VecDeque<serde_json::Value>>::new());
        let (tx, mut rx) = broadcast::channel(16);
        let val = serde_json::json!({"mem": "128MB"});

        push_service_stats(&history, "k", &tx, val.clone()).await;
        assert_eq!(rx.recv().await.unwrap(), val);
    }

    #[test]
    fn test_parse_docker_stats_json_valid() {
        let output = r#"'{"Name":"web","CPUPerc":"5.00%","MemUsage":"128MiB / 1GiB","MemPerc":"12.50%","NetIO":"1kB / 2kB","BlockIO":"0B / 0B","PIDs":"5"}'"#;
        let result = parse_docker_stats_json(output);
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_docker_stats_json_empty() {
        assert!(parse_docker_stats_json("").is_none());
    }

    #[test]
    fn test_parse_docker_stats_json_invalid() {
        assert!(parse_docker_stats_json("not json").is_none());
    }
}
