use std::collections::VecDeque;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::protocol::{SessionInfo, TerminalResize, TerminalSessionInit};

use crate::api::ws_host_terminal::PtySession;
use crate::server::AppState;

const RESIZE_PREFIX: u8 = 0x01;
const CLEAR_PREFIX: &[u8] = b"\x02clear";
const SCROLLBACK_CAP: usize = 512 * 1024;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct RemoteExecParams {
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct RemoteExecSessionsParams {
    pub name: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct DeleteRemoteExecSessionParams {
    pub id: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/remote/exec/interactive", get(ws_handler))
        .route(
            "/remote/exec/sessions",
            get(list_sessions).delete(delete_session),
        )
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RemoteExecSessionsParams>,
) -> Json<Vec<SessionInfo>> {
    let match_key = params.scope.clone().unwrap_or_else(|| params.name.clone());
    let session_ids: Vec<String> = {
        let sessions = state.remote_exec_sessions.lock().await;
        sessions
            .values()
            .filter(|s| s.project == match_key)
            .map(|s| s.id.clone())
            .collect()
    };

    let db = state.db.lock().await;
    let list: Vec<SessionInfo> = session_ids
        .into_iter()
        .map(|id| {
            let title = db
                .get_setting(&format!("session_title:{id}"))
                .ok()
                .flatten();
            SessionInfo {
                id,
                project: params.name.clone(),
                title,
            }
        })
        .collect();
    Json(list)
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DeleteRemoteExecSessionParams>,
) -> StatusCode {
    let mut sessions = state.remote_exec_sessions.lock().await;
    if let Some(session) = sessions.remove(&params.id) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(session.child_pid),
            nix::sys::signal::Signal::SIGHUP,
        );
        unsafe {
            nix::libc::close(session.master_read_fd);
            nix::libc::close(session.master_write_fd);
        }
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<RemoteExecParams>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    let entry = db.get_remote(&params.name).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let entry = entry.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Remote '{}' not found", params.name) })),
        )
    })?;
    drop(db);

    let session_id = params.session_id.clone();
    let remote_name = params.name.clone();
    let command = params.command.clone();
    let scope = params.scope.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(
            socket,
            state,
            remote_name,
            entry,
            session_id,
            command,
            scope,
        )
    }))
}

async fn handle_ws(
    mut socket: WebSocket,
    state: Arc<AppState>,
    remote_name: String,
    entry: coast_core::types::RemoteEntry,
    session_id: Option<String>,
    command: Option<String>,
    scope: Option<String>,
) {
    if let Some(ref sid) = session_id {
        let sessions = state.remote_exec_sessions.lock().await;
        if sessions.contains_key(sid) {
            drop(sessions);
            reconnect_session(&mut socket, &state, sid).await;
            return;
        }
    }

    let session_scope = scope.unwrap_or_else(|| remote_name.clone());
    let sid = match create_ssh_session(
        &state,
        &remote_name,
        &session_scope,
        &entry,
        command.as_deref(),
    )
    .await
    {
        Ok(sid) => sid,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    format!("Failed to create SSH session: {e}").into(),
                ))
                .await;
            return;
        }
    };

    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: sid.clone(),
    })
    .unwrap();
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    let (output_tx, write_fd, read_fd, scrollback) = {
        let sessions = state.remote_exec_sessions.lock().await;
        let Some(session) = sessions.get(&sid) else {
            return;
        };
        (
            session.output_tx.clone(),
            session.master_write_fd,
            session.master_read_fd,
            session.scrollback.clone(),
        )
    };

    bridge_ws(&mut socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(session_id = %sid, "remote exec WS disconnected, session kept alive");
}

async fn create_ssh_session(
    state: &Arc<AppState>,
    remote_name: &str,
    scope: &str,
    entry: &coast_core::types::RemoteEntry,
    command: Option<&str>,
) -> Result<String, String> {
    let sid = uuid::Uuid::new_v4().to_string();
    debug!(session_id = %sid, remote = %remote_name, scope = %scope, command = ?command, "creating SSH session");

    let host = entry.host.clone();
    let user = entry.user.clone();
    let port = entry.port.to_string();
    let ssh_key = entry.ssh_key.clone();
    let scoped_name = scope.to_string();
    let cmd = command.map(std::string::ToString::to_string);

    let pty_result = tokio::task::spawn_blocking(move || {
        if let Some(ref c) = cmd {
            open_ssh_pty_with_command(&user, &host, &port, ssh_key.as_deref(), c)
        } else {
            open_ssh_pty(&user, &host, &port, ssh_key.as_deref())
        }
    })
    .await;

    let (master_fd, child_pid) = match pty_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => return Err(format!("Failed to open SSH PTY: {e}")),
        Err(e) => return Err(format!("PTY task panicked: {e}")),
    };

    let read_fd = master_fd.as_raw_fd();
    let write_fd = nix::unistd::dup(read_fd).expect("dup master PTY fd");
    std::mem::forget(master_fd);

    let scrollback = Arc::new(Mutex::new(VecDeque::<u8>::with_capacity(SCROLLBACK_CAP)));
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(256);

    {
        let session = PtySession {
            id: sid.clone(),
            project: scoped_name,
            child_pid,
            master_read_fd: read_fd,
            master_write_fd: write_fd,
            scrollback: scrollback.clone(),
            output_tx: output_tx.clone(),
        };
        let mut sessions = state.remote_exec_sessions.lock().await;
        sessions.insert(sid.clone(), session);
    }

    tokio::spawn({
        let scrollback = scrollback.clone();
        let output_tx = output_tx.clone();
        let sid = sid.clone();
        async move {
            let mut reader =
                tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(read_fd) });
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = buf[..n].to_vec();
                        {
                            let mut sb = scrollback.lock().await;
                            sb.extend(&data);
                            while sb.len() > SCROLLBACK_CAP {
                                sb.pop_front();
                            }
                        }
                        let _ = output_tx.send(data);
                    }
                    Err(_) => break,
                }
            }
            debug!(session_id = %sid, "SSH session reader ended");
        }
    });

    Ok(sid)
}

fn open_ssh_pty(
    user: &str,
    host: &str,
    port: &str,
    ssh_key: Option<&str>,
) -> Result<(std::os::fd::OwnedFd, i32), String> {
    use nix::pty::openpty;
    use nix::unistd::{close, dup2, execvp, fork, setsid, ForkResult};
    use std::ffi::CString;

    let initial_size = nix::pty::Winsize {
        ws_row: 50,
        ws_col: 200,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = openpty(Some(&initial_size), None).map_err(|e| format!("openpty failed: {e}"))?;
    let master_raw = pty.master.as_raw_fd();
    let slave_raw = pty.slave.as_raw_fd();

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            drop(pty.master);
            let _ = setsid();
            unsafe {
                nix::libc::ioctl(slave_raw, nix::libc::TIOCSCTTY as _, 0);
            }
            let _ = dup2(slave_raw, 0);
            let _ = dup2(slave_raw, 1);
            let _ = dup2(slave_raw, 2);
            if slave_raw > 2 {
                let _ = close(slave_raw);
            }

            std::env::set_var("TERM", "xterm-256color");

            let ssh = CString::new("ssh").unwrap();
            let mut args = vec![
                CString::new("ssh").unwrap(),
                CString::new("-o").unwrap(),
                CString::new("StrictHostKeyChecking=accept-new").unwrap(),
                CString::new("-p").unwrap(),
                CString::new(port).unwrap(),
                CString::new("-t").unwrap(),
            ];
            if let Some(key) = ssh_key {
                args.push(CString::new("-i").unwrap());
                args.push(CString::new(key).unwrap());
            }
            args.push(CString::new(format!("{user}@{host}")).unwrap());
            let _ = execvp(&ssh, &args);
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

async fn reconnect_session(socket: &mut WebSocket, state: &Arc<AppState>, session_id: &str) {
    let (output_tx, write_fd, read_fd, scrollback) = {
        let sessions = state.remote_exec_sessions.lock().await;
        let Some(session) = sessions.get(session_id) else {
            return;
        };
        (
            session.output_tx.clone(),
            session.master_write_fd,
            session.master_read_fd,
            session.scrollback.clone(),
        )
    };

    {
        let sb = scrollback.lock().await;
        if !sb.is_empty() {
            let data: Vec<u8> = sb.iter().copied().collect();
            let text = String::from_utf8_lossy(&data);
            if socket
                .send(Message::Text(text.into_owned().into()))
                .await
                .is_err()
            {
                return;
            }
        }
    }

    bridge_ws(socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(session_id = %session_id, "remote exec reconnect disconnected");
}

async fn bridge_ws(
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
                        warn!("remote exec output lagged, skipped {n} messages");
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
}

fn resize_pty(master_fd: i32, cols: u16, rows: u16) {
    let ws = nix::libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    unsafe {
        nix::libc::ioctl(master_fd, nix::libc::TIOCSWINSZ, &ws);
    }
}

fn open_ssh_pty_with_command(
    user: &str,
    host: &str,
    port: &str,
    ssh_key: Option<&str>,
    remote_command: &str,
) -> Result<(std::os::fd::OwnedFd, i32), String> {
    use nix::pty::openpty;
    use nix::unistd::{close, dup2, execvp, fork, setsid, ForkResult};
    use std::ffi::CString;

    let initial_size = nix::pty::Winsize {
        ws_row: 50,
        ws_col: 200,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = openpty(Some(&initial_size), None).map_err(|e| format!("openpty failed: {e}"))?;
    let master_raw = pty.master.as_raw_fd();
    let slave_raw = pty.slave.as_raw_fd();

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            drop(pty.master);
            let _ = setsid();
            unsafe {
                nix::libc::ioctl(slave_raw, nix::libc::TIOCSCTTY as _, 0);
            }
            let _ = dup2(slave_raw, 0);
            let _ = dup2(slave_raw, 1);
            let _ = dup2(slave_raw, 2);
            if slave_raw > 2 {
                let _ = close(slave_raw);
            }

            std::env::set_var("TERM", "xterm-256color");

            let ssh = CString::new("ssh").unwrap();
            let mut args = vec![
                CString::new("ssh").unwrap(),
                CString::new("-o").unwrap(),
                CString::new("StrictHostKeyChecking=accept-new").unwrap(),
                CString::new("-p").unwrap(),
                CString::new(port).unwrap(),
                CString::new("-t").unwrap(),
            ];
            if let Some(key) = ssh_key {
                args.push(CString::new("-i").unwrap());
                args.push(CString::new(key).unwrap());
            }
            args.push(CString::new(format!("{user}@{host}")).unwrap());
            args.push(CString::new(remote_command).unwrap());
            let _ = execvp(&ssh, &args);
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

pub fn open_ssh_pty_with_command_pub(
    user: &str,
    host: &str,
    port: &str,
    ssh_key: Option<&str>,
    remote_command: &str,
) -> Result<(std::os::fd::OwnedFd, i32), String> {
    open_ssh_pty_with_command(user, host, port, ssh_key, remote_command)
}

pub async fn handle_remote_exec_socket_for_instance(
    mut socket: WebSocket,
    state: Arc<AppState>,
    remote_name: String,
    project: String,
    instance_name: String,
) {
    debug!(remote = %remote_name, project = %project, instance = %instance_name, "remote instance exec WS connected");

    let db = state.db.lock().await;
    let remotes = db.list_remotes().unwrap_or_default();
    let entry = remotes
        .into_iter()
        .find(|r| r.name == remote_name || r.host == remote_name);
    drop(db);

    let Some(entry) = entry else {
        let _ = socket
            .send(Message::Text(
                format!("Remote '{}' not found", remote_name).into(),
            ))
            .await;
        return;
    };

    let container_name = format!("{}-coasts-{}", project, instance_name);
    let remote_command = format!("docker exec -it {} sh", container_name);

    let host = entry.host.clone();
    let user = entry.user.clone();
    let port = entry.port.to_string();
    let ssh_key = entry.ssh_key.clone();

    let pty_result = tokio::task::spawn_blocking(move || {
        open_ssh_pty_with_command(&user, &host, &port, ssh_key.as_deref(), &remote_command)
    })
    .await;

    let (master_fd, _child_pid) = match pty_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            let _ = socket
                .send(Message::Text(
                    format!("Failed to open remote exec: {e}").into(),
                ))
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

    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: format!("remote-instance-{}-{}", project, instance_name),
    })
    .unwrap();
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    let (output_tx, _) = broadcast::channel::<Vec<u8>>(64);
    let scrollback = Arc::new(Mutex::new(VecDeque::<u8>::new()));

    bridge_ws(&mut socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(remote = %remote_name, instance = %instance_name, "remote instance exec WS disconnected");
}
