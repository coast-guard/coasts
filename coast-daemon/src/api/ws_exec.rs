use std::collections::{HashSet, VecDeque};
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::RawFd;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use nix::libc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::protocol::{
    ActivateAgentShellResponse, AgentShellAvailableResponse, CloseAgentShellResponse,
    ExecSessionInfo, SpawnAgentShellResponse, TerminalResize, TerminalSessionInit,
};
use coast_core::types::InstanceStatus;

use rust_i18n::t;

use crate::api::streaming::{resolve_agent_shell_command, spawn_agent_shell};
use crate::api::ws_host_terminal::PtySession;
use crate::server::AppState;

const RESIZE_PREFIX: u8 = 0x01;
const CLEAR_PREFIX: &[u8] = b"\x02clear";
const SCROLLBACK_CAP: usize = 512 * 1024;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ExecParams {
    pub project: String,
    pub name: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ExecSessionsParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct DeleteExecSessionParams {
    pub id: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct AgentShellParams {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SpawnAgentShellRequest {
    pub project: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct AgentShellActionRequest {
    pub project: String,
    pub name: String,
    pub shell_id: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/exec/interactive", get(ws_handler))
        .route("/exec/sessions", get(list_sessions).delete(delete_session))
        .route("/exec/agent-shell", get(agent_shell_available))
        .route("/exec/agent-shell/spawn", post(spawn_agent_shell_session))
        .route("/exec/agent-shell/activate", post(activate_agent_shell))
        .route("/exec/agent-shell/close", post(close_agent_shell))
}

// --- List sessions ---

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExecSessionsParams>,
) -> Json<Vec<ExecSessionInfo>> {
    let scoped_session_ids: Vec<String> = {
        let sessions = state.exec_sessions.lock().await;
        sessions
            .values()
            .filter(|s| s.project == format!("{}:{}", params.project, params.name))
            .map(|s| s.id.clone())
            .collect()
    };

    let live_session_ids: HashSet<&str> = scoped_session_ids.iter().map(String::as_str).collect();
    let db = state.db.lock().await;
    let mut agent_shells = db
        .list_agent_shells(&params.project, &params.name)
        .unwrap_or_default();

    // Self-heal active agent assignment after daemon restarts or stale sessions:
    // if the DB "active" shell no longer has a live PTY, promote the first live shell.
    let has_live_active = agent_shells.iter().any(|a| {
        a.is_active
            && a.session_id
                .as_deref()
                .is_some_and(|sid| live_session_ids.contains(sid))
    });
    if !has_live_active {
        if let Some(promoted) = agent_shells.iter().find(|a| {
            a.session_id
                .as_deref()
                .is_some_and(|sid| live_session_ids.contains(sid))
        }) {
            if let Err(e) = db.set_active_agent_shell(&params.project, &params.name, promoted.id) {
                warn!(
                    project = %params.project,
                    instance = %params.name,
                    shell_id = promoted.id,
                    error = %e,
                    "failed to promote live agent shell as active"
                );
            } else {
                agent_shells = db
                    .list_agent_shells(&params.project, &params.name)
                    .unwrap_or_default();
            }
        }
    }

    let list: Vec<ExecSessionInfo> = scoped_session_ids
        .into_iter()
        .map(|session_id| {
            let title = db
                .get_setting(&format!("session_title:{}", session_id))
                .ok()
                .flatten();
            let agent_match = agent_shells
                .iter()
                .find(|a| a.session_id.as_deref() == Some(session_id.as_str()));
            ExecSessionInfo {
                id: session_id,
                project: params.project.clone(),
                name: params.name.clone(),
                title,
                agent_shell_id: agent_match.map(|a| a.shell_id),
                is_active_agent: agent_match.map(|a| a.is_active),
            }
        })
        .collect();
    Json(list)
}

// --- Delete session ---

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DeleteExecSessionParams>,
) -> StatusCode {
    if close_exec_session_by_id(&state, &params.id).await {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn close_exec_session_by_id(state: &Arc<AppState>, session_id: &str) -> bool {
    let mut sessions = state.exec_sessions.lock().await;
    if let Some(session) = sessions.remove(session_id) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(session.child_pid),
            nix::sys::signal::Signal::SIGHUP,
        );
        unsafe {
            libc::close(session.master_read_fd);
            libc::close(session.master_write_fd);
        }
        true
    } else {
        false
    }
}

async fn agent_shell_available(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AgentShellParams>,
) -> impl IntoResponse {
    let lang = state.language();
    let (build_id, coastfile_type) = {
        let db = state.db.lock().await;
        let instance = match db.get_instance(&params.project, &params.name) {
            Ok(Some(instance)) => instance,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = &params.name, project = &params.project).to_string() })),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        };
        (instance.build_id, instance.coastfile_type)
    };

    let available = resolve_agent_shell_command(
        &params.project,
        build_id.as_deref(),
        coastfile_type.as_deref(),
    )
    .is_some();
    (
        StatusCode::OK,
        Json(AgentShellAvailableResponse { available }),
    )
        .into_response()
}

async fn spawn_agent_shell_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpawnAgentShellRequest>,
) -> impl IntoResponse {
    let lang = state.language();
    let (container_id, build_id, coastfile_type) = {
        let db = state.db.lock().await;
        let instance = match db.get_instance(&req.project, &req.name) {
            Ok(Some(instance)) => instance,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = &req.name, project = &req.project).to_string() })),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        };

        if instance.status == InstanceStatus::Stopped {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": t!("error.instance_stopped", locale = &lang, name = &req.name).to_string() })),
            )
                .into_response();
        }

        let Some(container_id) = instance.container_id else {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": t!("error.no_container_id", locale = &lang).to_string() })),
            )
                .into_response();
        };

        (container_id, instance.build_id, instance.coastfile_type)
    };

    let Some(command) =
        resolve_agent_shell_command(&req.project, build_id.as_deref(), coastfile_type.as_deref())
    else {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "No [agent_shell] command configured for this instance build" })),
        )
            .into_response();
    };

    match spawn_agent_shell(
        &state,
        &req.project,
        &req.name,
        &container_id,
        &command,
        false,
    )
    .await
    {
        Ok(spawned) => (
            StatusCode::OK,
            Json(SpawnAgentShellResponse {
                session_id: spawned.session_id,
                agent_shell_id: spawned.shell_id,
                is_active_agent: spawned.is_active_agent,
                title: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

async fn activate_agent_shell(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentShellActionRequest>,
) -> impl IntoResponse {
    let lang = state.language();
    let db = state.db.lock().await;
    let instance = match db.get_instance(&req.project, &req.name) {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = &req.name, project = &req.project).to_string() })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    if instance.status == InstanceStatus::Stopped {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": t!("error.instance_stopped", locale = &lang, name = &req.name).to_string() })),
        )
            .into_response();
    }

    let shell = match db.get_agent_shell_by_shell_id(&req.project, &req.name, req.shell_id) {
        Ok(Some(shell)) => shell,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Agent shell '{}' not found", req.shell_id) })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    if let Err(e) = db.set_active_agent_shell(&req.project, &req.name, shell.id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(ActivateAgentShellResponse {
            shell_id: req.shell_id,
            is_active_agent: true,
        }),
    )
        .into_response()
}

async fn close_agent_shell(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentShellActionRequest>,
) -> impl IntoResponse {
    let lang = state.language();
    let session_id_to_close = {
        let db = state.db.lock().await;
        match db.get_instance(&req.project, &req.name) {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = &req.name, project = &req.project).to_string() })),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        };

        let shell = match db.get_agent_shell_by_shell_id(&req.project, &req.name, req.shell_id) {
            Ok(Some(shell)) => shell,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": format!("Agent shell '{}' not found", req.shell_id) })),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        };

        if shell.is_active {
            return (
                StatusCode::CONFLICT,
                Json(json!({ "error": "Cannot close the active agent shell. Make another shell active first." })),
            )
                .into_response();
        }

        if let Err(e) = db.delete_agent_shell(shell.id) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        shell.session_id
    };

    if let Some(session_id) = session_id_to_close {
        let _ = close_exec_session_by_id(&state, &session_id).await;
    }

    (
        StatusCode::OK,
        Json(CloseAgentShellResponse {
            shell_id: req.shell_id,
            closed: true,
        }),
    )
        .into_response()
}

// --- WebSocket handler ---

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExecParams>,
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

    Ok(ws.on_upgrade(move |socket| {
        handle_ws(socket, state, project, name, container_id, session_id)
    }))
}

async fn handle_ws(
    mut socket: WebSocket,
    state: Arc<AppState>,
    project: String,
    name: String,
    container_id: String,
    session_id: Option<String>,
) {
    // Check for reconnection
    if let Some(ref sid) = session_id {
        let sessions = state.exec_sessions.lock().await;
        if sessions.contains_key(sid) {
            drop(sessions);
            reconnect_session(&mut socket, &state, sid).await;
            return;
        }
    }

    let composite_key = format!("{project}:{name}");

    let sid = match create_exec_session(&state, &composite_key, &container_id, None).await {
        Ok(sid) => sid,
        Err(e) => {
            let _ = socket
                .send(Message::Text(
                    format!("Failed to create session: {e}").into(),
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
        let sessions = state.exec_sessions.lock().await;
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
    debug!(session_id = %sid, "exec WS disconnected, session kept alive");
}

/// Create a new exec PTY session and register it in `state.exec_sessions`.
///
/// Returns the session ID. If `command` is `None`, defaults to an interactive
/// bash shell. The session persists in memory until the child process exits.
pub async fn create_exec_session(
    state: &Arc<AppState>,
    composite_key: &str,
    container_id: &str,
    command: Option<&str>,
) -> std::result::Result<String, String> {
    let sid = uuid::Uuid::new_v4().to_string();
    debug!(session_id = %sid, container = %container_id, "creating new exec session");

    let cid = container_id.to_string();
    let cmd = command.map(std::string::ToString::to_string);
    let pty_result =
        tokio::task::spawn_blocking(move || open_docker_exec_pty(&cid, cmd.as_deref())).await;

    let (master_fd, child_pid) = match pty_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => return Err(format!("Failed to open exec PTY: {e}")),
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
            project: composite_key.to_string(),
            child_pid,
            master_read_fd: read_fd,
            master_write_fd: write_fd,
            scrollback: scrollback.clone(),
            output_tx: output_tx.clone(),
        };
        let mut sessions = state.exec_sessions.lock().await;
        sessions.insert(sid.clone(), session);
    }

    tokio::spawn({
        let scrollback = scrollback.clone();
        let output_tx = output_tx.clone();
        let sid_clone = sid.clone();
        let composite_key_clone = composite_key.to_string();
        let state = Arc::clone(state);
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
            let is_agent = {
                let parts: Vec<&str> = composite_key_clone.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let db = state.db.lock().await;
                    db.list_agent_shells(parts[0], parts[1])
                        .unwrap_or_default()
                        .iter()
                        .any(|a| a.session_id.as_deref() == Some(&sid_clone))
                } else {
                    false
                }
            };
            if !is_agent {
                let mut sessions = state.exec_sessions.lock().await;
                sessions.remove(&sid_clone);
            }
            debug!(session_id = %sid_clone, is_agent = is_agent, "exec session ended");
        }
    });

    Ok(sid)
}

async fn reconnect_session(socket: &mut WebSocket, state: &Arc<AppState>, session_id: &str) {
    debug!(session_id = %session_id, "reconnecting exec session");

    let (scrollback_data, scrollback, output_tx, write_fd, read_fd) = {
        let sessions = state.exec_sessions.lock().await;
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
    debug!(session_id = %session_id, "exec reconnect disconnected");
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
                        warn!("exec output lagged, skipped {n} messages");
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

/// Spawn `docker exec -it <container> bash` via a host-side PTY.
fn open_docker_exec_pty(
    container_id: &str,
    command: Option<&str>,
) -> Result<(std::os::fd::OwnedFd, i32), String> {
    use nix::pty::openpty;
    use nix::unistd::{close, dup2, execvp, fork, setsid, ForkResult};
    use std::ffi::CString;

    let shell_cmd = command.unwrap_or("export GIT_PAGER=cat PAGER=cat LESS=-FRX; exec sh");
    // Map to host UID/GID only for the default interactive shell path.
    // Command-driven sessions (e.g. agent shell bootstrap) require root to
    // perform setup operations like adduser/su.
    let user_spec = if command.is_none() {
        let uid = unsafe { nix::libc::getuid() };
        let gid = unsafe { nix::libc::getgid() };
        Some(format!("{uid}:{gid}"))
    } else {
        None
    };

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
            let mut args = vec![
                CString::new("docker").unwrap(),
                CString::new("exec").unwrap(),
                CString::new("-it").unwrap(),
            ];
            if let Some(ref spec) = user_spec {
                if let Ok(user_spec_cstr) = CString::new(spec.clone()) {
                    args.push(CString::new("-u").unwrap());
                    args.push(user_spec_cstr);
                } else {
                    warn!(
                        user_spec = %spec,
                        "invalid UID/GID for docker exec user mapping; falling back to container default user"
                    );
                }
            }
            args.push(CString::new(container_id).unwrap());
            args.push(CString::new("sh").unwrap());
            args.push(CString::new("-c").unwrap());
            args.push(CString::new(shell_cmd).unwrap());
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
