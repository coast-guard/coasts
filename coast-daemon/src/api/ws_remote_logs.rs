use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tracing::{debug, warn};
use ts_rs::TS;

use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct RemoteLogsParams {
    pub name: String,
    #[serde(default = "default_tail")]
    pub tail: u32,
}

fn default_tail() -> u32 {
    500
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/remote/logs/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<RemoteLogsParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let db = state.db.lock().await;
    let entry = db
        .get_remote(&params.name)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Remote '{}' not found", params.name),
            )
        })?;
    drop(db);

    Ok(ws.on_upgrade(move |socket| handle_logs_socket(socket, entry, params.tail)))
}

fn build_logs_script(tail: u32) -> String {
    format!(
        "if command -v journalctl >/dev/null 2>&1; then \
           journalctl -f -n {tail} --no-pager; \
         elif [ -f /var/log/syslog ]; then \
           tail -n {tail} -f /var/log/syslog; \
         elif [ -f /var/log/messages ]; then \
           tail -n {tail} -f /var/log/messages; \
         elif command -v docker >/dev/null 2>&1; then \
           echo '=== Docker container logs ==='; \
           for c in $(docker ps -q 2>/dev/null); do \
             docker logs --tail {tail} -f \"$c\" 2>&1 & \
           done; \
           wait; \
         elif command -v dmesg >/dev/null 2>&1; then \
           dmesg -T --follow 2>/dev/null || dmesg -T | tail -n {tail}; \
         else \
           echo 'No log source found'; \
         fi"
    )
}

fn build_ssh_logs_command(
    entry: &coast_core::types::RemoteEntry,
    script: &str,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-tt",
        "-p",
        &entry.port.to_string(),
    ]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd.arg(format!("{}@{}", entry.user, entry.host));
    cmd.arg(script);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    cmd
}

async fn handle_logs_socket(
    mut socket: WebSocket,
    entry: coast_core::types::RemoteEntry,
    tail: u32,
) {
    debug!(remote = %entry.name, "remote logs WS connected");

    let script = build_logs_script(tail);
    let mut cmd = build_ssh_logs_command(&entry, &script);

    let Ok(mut child) = cmd.spawn() else {
        let _ = socket
            .send(Message::Text("Failed to start SSH".into()))
            .await;
        return;
    };

    let Some(mut stdout) = child.stdout.take() else {
        let _ = socket
            .send(Message::Text("No stdout from SSH".into()))
            .await;
        return;
    };

    if let Some(mut stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf).await {
                if n == 0 {
                    break;
                }
            }
        });
    }

    stream_ssh_output(&mut socket, &mut stdout, &entry.name).await;
    let _ = child.kill().await;
    debug!(remote = %entry.name, "remote logs WS disconnected");
}

async fn stream_ssh_output(
    socket: &mut WebSocket,
    stdout: &mut tokio::process::ChildStdout,
    remote_name: &str,
) {
    let mut buf = [0u8; 8192];
    loop {
        tokio::select! {
            n = stdout.read(&mut buf) => {
                match n {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(remote = %remote_name, error = %e, "SSH stdout read error");
                        break;
                    }
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
}
