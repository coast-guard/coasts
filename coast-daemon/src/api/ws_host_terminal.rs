use std::collections::VecDeque;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::RawFd;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use nix::libc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::protocol::{SessionInfo, TerminalResize, TerminalSessionInit};

use crate::server::AppState;

const RESIZE_PREFIX: u8 = 0x01;
const CLEAR_PREFIX: &[u8] = b"\x02clear";
const SCROLLBACK_CAP: usize = 512 * 1024;

/// A persistent PTY session that survives WebSocket disconnects.
pub struct PtySession {
    pub id: String,
    pub project: String,
    pub child_pid: i32,
    pub master_read_fd: RawFd,
    pub master_write_fd: RawFd,
    pub scrollback: Arc<Mutex<VecDeque<u8>>>,
    /// Broadcast channel for live output. Each WS subscriber gets a receiver.
    pub output_tx: broadcast::Sender<Vec<u8>>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct HostTerminalParams {
    pub project: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SessionsQueryParams {
    pub project: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/host/terminal", get(ws_handler))
        .route("/host/sessions", get(list_sessions).delete(delete_session))
}

fn resolve_project_root(project: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("project_root")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
}

// --- List sessions ---

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SessionsQueryParams>,
) -> Json<Vec<SessionInfo>> {
    let sessions = state.pty_sessions.lock().await;
    let db = state.db.lock().await;
    let list: Vec<SessionInfo> = sessions
        .values()
        .filter(|s| s.project == params.project)
        .map(|s| {
            let title = db
                .get_setting(&format!("session_title:{}", s.id))
                .ok()
                .flatten();
            SessionInfo {
                id: s.id.clone(),
                project: s.project.clone(),
                title,
            }
        })
        .collect();
    Json(list)
}

// --- Delete session ---

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct DeleteSessionParams {
    pub id: String,
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DeleteSessionParams>,
) -> StatusCode {
    let mut sessions = state.pty_sessions.lock().await;
    if let Some(session) = sessions.remove(&params.id) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(session.child_pid),
            nix::sys::signal::Signal::SIGHUP,
        );
        unsafe {
            libc::close(session.master_read_fd);
            libc::close(session.master_write_fd);
        }
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

// --- WebSocket handler ---

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<HostTerminalParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let project_root = resolve_project_root(&params.project).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Project '{}' not found or has no manifest", params.project),
        )
    })?;

    let project = params.project.clone();
    let session_id = params.session_id.clone();

    Ok(ws.on_upgrade(move |socket| handle_ws(socket, state, project, project_root, session_id)))
}

async fn handle_ws(
    mut socket: WebSocket,
    state: Arc<AppState>,
    project: String,
    project_root: String,
    session_id: Option<String>,
) {
    // Determine if reconnecting or creating new
    if let Some(ref sid) = session_id {
        let sessions = state.pty_sessions.lock().await;
        if sessions.contains_key(sid) {
            drop(sessions);
            reconnect_session(&mut socket, &state, sid).await;
            return;
        }
        // Session not found, fall through to create new
    }

    // Create new session
    let sid = uuid::Uuid::new_v4().to_string();
    debug!(session_id = %sid, cwd = %project_root, "creating new PTY session");

    let pty_result = tokio::task::spawn_blocking({
        let root = project_root.clone();
        move || open_pty_shell(&root)
    })
    .await;

    let (master_fd, child_pid) = match pty_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            let _ = socket
                .send(Message::Text(format!("Failed to open PTY: {e}").into()))
                .await;
            return;
        }
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("PTY task panicked: {e}").into()))
                .await;
            return;
        }
    };

    let read_fd = master_fd.as_raw_fd();
    let write_fd = nix::unistd::dup(read_fd).expect("dup master PTY fd");
    std::mem::forget(master_fd);

    let scrollback = Arc::new(Mutex::new(VecDeque::<u8>::with_capacity(SCROLLBACK_CAP)));
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(256);

    // Store session in registry
    {
        let session = PtySession {
            id: sid.clone(),
            project: project.clone(),
            child_pid,
            master_read_fd: read_fd,
            master_write_fd: write_fd,
            scrollback: scrollback.clone(),
            output_tx: output_tx.clone(),
        };
        let mut sessions = state.pty_sessions.lock().await;
        sessions.insert(sid.clone(), session);
    }

    // Spawn background reader that buffers scrollback and broadcasts
    tokio::spawn({
        let scrollback = scrollback.clone();
        let output_tx = output_tx.clone();
        let sid = sid.clone();
        let state = state.clone();
        async move {
            let mut read_file =
                tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(read_fd) });
            let mut buf = [0u8; 4096];
            loop {
                match read_file.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = buf[..n].to_vec();
                        // Append to scrollback
                        {
                            let mut sb = scrollback.lock().await;
                            for &b in &chunk {
                                if sb.len() >= SCROLLBACK_CAP {
                                    sb.pop_front();
                                }
                                sb.push_back(b);
                            }
                        }
                        // Broadcast to any connected WS clients
                        let _ = output_tx.send(chunk);
                    }
                    Err(_) => break,
                }
            }
            // PTY closed -- remove session from registry
            let mut sessions = state.pty_sessions.lock().await;
            sessions.remove(&sid);
            debug!(session_id = %sid, "PTY session ended, removed from registry");
        }
    });

    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: sid.clone(),
    })
    .unwrap();
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    // Bridge this WS connection to the session
    bridge_ws_to_session(&mut socket, &output_tx, write_fd, read_fd, &scrollback).await;

    debug!(session_id = %sid, "WS disconnected, PTY session kept alive");
}

async fn reconnect_session(socket: &mut WebSocket, state: &Arc<AppState>, session_id: &str) {
    debug!(session_id = %session_id, "reconnecting to existing PTY session");

    let (scrollback_data, scrollback, output_tx, write_fd, read_fd) = {
        let sessions = state.pty_sessions.lock().await;
        let Some(session) = sessions.get(session_id) else {
            let _ = socket.send(Message::Text("Session not found".into())).await;
            return;
        };
        let sb = session.scrollback.lock().await;
        let data: Vec<u8> = sb.iter().copied().collect();
        (
            data,
            session.scrollback.clone(),
            session.output_tx.clone(),
            session.master_write_fd,
            session.master_read_fd,
        )
    };

    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: session_id.to_string(),
    })
    .unwrap();
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    // Send buffered scrollback so terminal shows recent history
    if !scrollback_data.is_empty() {
        let text = String::from_utf8_lossy(&scrollback_data);
        if socket
            .send(Message::Text(text.into_owned().into()))
            .await
            .is_err()
        {
            return;
        }
    }

    // Bridge this WS connection to the session
    bridge_ws_to_session(socket, &output_tx, write_fd, read_fd, &scrollback).await;

    debug!(session_id = %session_id, "WS reconnect disconnected, PTY kept alive");
}

async fn bridge_ws_to_session(
    socket: &mut WebSocket,
    output_tx: &broadcast::Sender<Vec<u8>>,
    write_fd: RawFd,
    read_fd: RawFd,
    scrollback: &Arc<Mutex<VecDeque<u8>>>,
) {
    let mut output_rx = output_tx.subscribe();

    let mut write_file = tokio::fs::File::from_std(unsafe {
        std::fs::File::from_raw_fd(nix::unistd::dup(write_fd).expect("dup write fd"))
    });

    loop {
        tokio::select! {
            chunk = output_rx.recv() => {
                match chunk {
                    Ok(data) => {
                        let text = String::from_utf8_lossy(&data);
                        if socket.send(Message::Text(text.into_owned().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("terminal output lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let text_str: &str = &text;
                        if text_str.as_bytes() == CLEAR_PREFIX {
                            let mut sb = scrollback.lock().await;
                            sb.clear();
                        } else if text_str.as_bytes().first() == Some(&RESIZE_PREFIX) {
                            if let Ok(resize) = serde_json::from_str::<TerminalResize>(&text_str[1..]) {
                                resize_pty(read_fd, resize.cols, resize.rows);
                            } else if write_file.write_all(text_str.as_bytes()).await.is_err() {
                                break;
                            }
                        } else if write_file.write_all(text_str.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        if write_file.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Don't close the dup'd write fd here -- tokio::fs::File will close it on drop.
    // The original write_fd in the session stays open.
}

fn open_pty_shell(cwd: &str) -> Result<(std::os::fd::OwnedFd, i32), String> {
    use nix::pty::openpty;
    use nix::unistd::{close, dup2, execvp, fork, setsid, ForkResult};
    use std::ffi::CString;

    let pty = openpty(None, None).map_err(|e| format!("openpty failed: {e}"))?;

    let master_raw = pty.master.as_raw_fd();
    let slave_raw = pty.slave.as_raw_fd();

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            drop(pty.master);
            let _ = setsid();
            unsafe {
                libc::ioctl(slave_raw, libc::TIOCSCTTY as _, 0);
            }
            let _ = dup2(slave_raw, 0);
            let _ = dup2(slave_raw, 1);
            let _ = dup2(slave_raw, 2);
            if slave_raw > 2 {
                let _ = close(slave_raw);
            }
            let _ = std::env::set_current_dir(cwd);
            std::env::set_var("TERM", "xterm-256color");
            std::env::set_var("LESS", "-FRX");
            std::env::set_var("GIT_PAGER", "cat");
            std::env::set_var("PAGER", "cat");
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            let shell_c = CString::new(shell.as_str()).unwrap();
            let args = [shell_c.clone()];
            let _ = execvp(&shell_c, &args);
            std::process::exit(1);
        }
        Ok(ForkResult::Parent { child }) => {
            drop(pty.slave);
            let master_fd: std::os::fd::OwnedFd =
                unsafe { std::os::fd::OwnedFd::from_raw_fd(master_raw) };
            std::mem::forget(pty.master);
            Ok((master_fd, child.as_raw()))
        }
        Err(e) => Err(format!("fork failed: {e}")),
    }
}

fn resize_pty(master_fd: i32, cols: u16, rows: u16) {
    let ws = libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        libc::ioctl(master_fd, libc::TIOCSWINSZ, &ws);
    }
}
