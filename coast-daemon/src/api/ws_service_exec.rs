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

use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use coast_core::protocol::{ServiceExecSessionInfo, TerminalResize, TerminalSessionInit};
use coast_core::types::InstanceStatus;
use futures_util::StreamExt;

use rust_i18n::t;

use crate::api::ws_host_terminal::PtySession;
use crate::handlers::compose_context;
use crate::server::AppState;

const RESIZE_PREFIX: u8 = 0x01;
const CLEAR_PREFIX: &[u8] = b"\x02clear";
const SCROLLBACK_CAP: usize = 512 * 1024;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ServiceExecParams {
    pub project: String,
    pub name: String,
    pub service: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ServiceSessionsParams {
    pub project: String,
    pub name: String,
    pub service: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct DeleteServiceSessionParams {
    pub id: String,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/service/exec", get(ws_handler)).route(
        "/service/sessions",
        get(list_sessions).delete(delete_session),
    )
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ServiceSessionsParams>,
) -> Json<Vec<ServiceExecSessionInfo>> {
    let composite_key = format!("{}:{}:{}", params.project, params.name, params.service);
    let sessions = state.service_exec_sessions.lock().await;
    let db = state.db.lock().await;
    let list: Vec<ServiceExecSessionInfo> = sessions
        .values()
        .filter(|s| s.project == composite_key)
        .map(|s| {
            let title = db
                .get_setting(&format!("session_title:{}", s.id))
                .ok()
                .flatten();
            ServiceExecSessionInfo {
                id: s.id.clone(),
                project: params.project.clone(),
                name: params.name.clone(),
                service: params.service.clone(),
                title,
            }
        })
        .collect();
    Json(list)
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DeleteServiceSessionParams>,
) -> StatusCode {
    let mut sessions = state.service_exec_sessions.lock().await;
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

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<ServiceExecParams>,
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

    let session_id = params.session_id.clone();
    let project = params.project.clone();
    let name = params.name.clone();
    let service = params.service.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(
            socket,
            state,
            project,
            name,
            service,
            container_id,
            session_id,
        )
    }))
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

#[allow(clippy::cognitive_complexity)]
async fn handle_ws(
    mut socket: WebSocket,
    state: Arc<AppState>,
    project: String,
    name: String,
    service: String,
    container_id: String,
    session_id: Option<String>,
) {
    if let Some(ref sid) = session_id {
        let sessions = state.service_exec_sessions.lock().await;
        if sessions.contains_key(sid) {
            drop(sessions);
            reconnect_session(&mut socket, &state, sid).await;
            return;
        }
    }

    let composite_key = format!("{project}:{name}:{service}");

    let sid = uuid::Uuid::new_v4().to_string();
    debug!(session_id = %sid, container = %container_id, service = %service, "creating new service exec session");

    let Some(docker) = state.docker.as_ref() else {
        let lang = state.language();
        let _ = socket
            .send(Message::Text(
                t!("error.docker_not_available", locale = &lang)
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };

    let Some(inner_container) =
        resolve_inner_container(docker, &container_id, &project, &service).await
    else {
        let _ = socket
            .send(Message::Text(
                format!("Could not find running container for service '{service}'").into(),
            ))
            .await;
        return;
    };

    debug!(inner_container = %inner_container, "resolved inner container for service exec");

    let pty_result = tokio::task::spawn_blocking({
        let cid = container_id.clone();
        let inner = inner_container.clone();
        move || open_service_exec_pty(&cid, &inner)
    })
    .await;

    let (master_fd, child_pid) = match pty_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            let _ = socket
                .send(Message::Text(
                    format!("Failed to open service exec PTY: {e}").into(),
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
    std::mem::forget(master_fd);

    let scrollback = Arc::new(Mutex::new(VecDeque::<u8>::with_capacity(SCROLLBACK_CAP)));
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(256);

    {
        let session = PtySession {
            id: sid.clone(),
            project: composite_key,
            child_pid,
            master_read_fd: read_fd,
            master_write_fd: write_fd,
            scrollback: scrollback.clone(),
            output_tx: output_tx.clone(),
        };
        let mut sessions = state.service_exec_sessions.lock().await;
        sessions.insert(sid.clone(), session);
    }

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
                        {
                            let mut sb = scrollback.lock().await;
                            for &b in &chunk {
                                if sb.len() >= SCROLLBACK_CAP {
                                    sb.pop_front();
                                }
                                sb.push_back(b);
                            }
                        }
                        let _ = output_tx.send(chunk);
                    }
                    Err(_) => break,
                }
            }
            let mut sessions = state.service_exec_sessions.lock().await;
            sessions.remove(&sid);
            debug!(session_id = %sid, "service exec session ended");
        }
    });

    let init_msg = serde_json::to_string(&TerminalSessionInit {
        session_id: sid.clone(),
    })
    .unwrap();
    if socket.send(Message::Text(init_msg.into())).await.is_err() {
        return;
    }

    bridge_ws(&mut socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(session_id = %sid, "service exec WS disconnected, session kept alive");
}

async fn reconnect_session(socket: &mut WebSocket, state: &Arc<AppState>, session_id: &str) {
    debug!(session_id = %session_id, "reconnecting service exec session");

    let (scrollback_data, scrollback, output_tx, write_fd, read_fd) = {
        let sessions = state.service_exec_sessions.lock().await;
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

    bridge_ws(socket, &output_tx, write_fd, read_fd, &scrollback).await;
    debug!(session_id = %session_id, "service exec reconnect disconnected");
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
                        warn!("service exec output lagged, skipped {n} messages");
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

/// Spawn `docker exec -it <coast_container> docker exec -it <inner_container> sh` via a host-side PTY.
fn open_service_exec_pty(
    coast_container_id: &str,
    inner_container_name: &str,
) -> Result<(std::os::fd::OwnedFd, i32), String> {
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

            std::env::set_var("TERM", "xterm-256color");

            let docker = CString::new("docker").unwrap();
            let args = [
                CString::new("docker").unwrap(),
                CString::new("exec").unwrap(),
                CString::new("-it").unwrap(),
                CString::new(coast_container_id).unwrap(),
                CString::new("docker").unwrap(),
                CString::new("exec").unwrap(),
                CString::new("-it").unwrap(),
                CString::new(inner_container_name).unwrap(),
                CString::new("sh").unwrap(),
                CString::new("-c").unwrap(),
                CString::new("export GIT_PAGER=cat PAGER=cat LESS=-FRX; exec sh").unwrap(),
            ];
            let _ = execvp(&docker, &args);
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
