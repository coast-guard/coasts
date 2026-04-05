/// Handler for `coast agent-shell` command family.
use std::collections::VecDeque;
use std::os::fd::FromRawFd;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{broadcast, Mutex};
use tracing::warn;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    self, AgentShellActivateResponse, AgentShellInputResponse, AgentShellLsResponse,
    AgentShellReadResponse, AgentShellRequest, AgentShellResponse, AgentShellSessionStatusResponse,
    AgentShellSpawnResponse, AgentShellSummary, AgentShellTtyAttachedResponse,
    AgentShellTtyClosedResponse, AgentShellTtyOutputResponse, ErrorResponse, Request, Response,
};
use coast_core::types::InstanceStatus;

use crate::api::streaming::{resolve_agent_shell_command, spawn_agent_shell};
use crate::server::AppState;

struct ResolvedShell {
    row_id: i64,
    shell_id: i64,
    is_active: bool,
    session_id: Option<String>,
    is_live: bool,
    output_tx: Option<broadcast::Sender<Vec<u8>>>,
    master_write_fd: Option<i32>,
    scrollback: Option<Arc<Mutex<VecDeque<u8>>>>,
}

fn dead_shell_error(name: &str, shell_id: i64) -> CoastError {
    CoastError::state(format!(
        "Agent shell '{}' for instance '{}' is not available (session is dead). \
         Spawn a new shell with `coast agent-shell {} spawn`.",
        shell_id, name, name
    ))
}

async fn write_response(writer: &mut OwnedWriteHalf, response: &Response) -> Result<()> {
    let bytes = protocol::encode_response(response)
        .map_err(|e| CoastError::protocol(format!("failed to encode response: {e}")))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|e| CoastError::io_simple(format!("failed to write tty response: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| CoastError::io_simple(format!("failed to flush tty response: {e}")))?;
    Ok(())
}

fn as_output_text(scrollback: &Arc<Mutex<VecDeque<u8>>>) -> tokio::task::JoinHandle<String> {
    let scrollback = scrollback.clone();
    tokio::spawn(async move {
        let sb = scrollback.lock().await;
        let bytes: Vec<u8> = sb.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

async fn resolve_shell(
    state: &Arc<AppState>,
    project: &str,
    name: &str,
    shell_id: Option<i64>,
) -> Result<ResolvedShell> {
    let shell = {
        let db = state.db.lock().await;
        let instance =
            db.get_instance(project, name)?
                .ok_or_else(|| CoastError::InstanceNotFound {
                    name: name.to_string(),
                    project: project.to_string(),
                })?;

        if instance.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped. Start it first with `coast start {}`.",
                name, name
            )));
        }

        if let Some(id) = shell_id {
            db.get_agent_shell_by_shell_id(project, name, id)?
                .ok_or_else(|| {
                    CoastError::state(format!(
                        "Agent shell '{}' does not exist for instance '{}'. \
                         Run `coast agent-shell {} ls` to inspect available shells.",
                        id, name, name
                    ))
                })?
        } else {
            db.get_active_agent_shell(project, name)?.ok_or_else(|| {
                CoastError::state(format!(
                    "Instance '{}' has no active agent shell. \
                     Spawn one with `coast agent-shell {} spawn`.",
                    name, name
                ))
            })?
        }
    };

    let (is_live, output_tx, master_write_fd, scrollback) = match shell.session_id.as_deref() {
        Some(sid) => {
            let sessions = state.exec_sessions.lock().await;
            if let Some(session) = sessions.get(sid) {
                (
                    true,
                    Some(session.output_tx.clone()),
                    Some(session.master_write_fd),
                    Some(session.scrollback.clone()),
                )
            } else {
                (false, None, None, None)
            }
        }
        None => (false, None, None, None),
    };

    Ok(ResolvedShell {
        row_id: shell.id,
        shell_id: shell.shell_id,
        is_active: shell.is_active,
        session_id: shell.session_id,
        is_live,
        output_tx,
        master_write_fd,
        scrollback,
    })
}

async fn handle_ls(
    project: String,
    name: String,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let shells = {
        let db = state.db.lock().await;
        db.get_instance(&project, &name)?
            .ok_or_else(|| CoastError::InstanceNotFound {
                name: name.clone(),
                project: project.clone(),
            })?;
        db.list_agent_shells(&project, &name)?
    };

    let live_session_ids = {
        let sessions = state.exec_sessions.lock().await;
        sessions
            .keys()
            .map(std::string::ToString::to_string)
            .collect::<std::collections::HashSet<_>>()
    };

    let summaries = shells
        .into_iter()
        .map(|s| AgentShellSummary {
            shell_id: s.shell_id,
            is_active: s.is_active,
            status: s.status,
            is_live: s
                .session_id
                .as_deref()
                .is_some_and(|sid| live_session_ids.contains(sid)),
        })
        .collect();

    Ok(AgentShellResponse::Ls(AgentShellLsResponse {
        name,
        shells: summaries,
    }))
}

async fn handle_activate(
    project: String,
    name: String,
    shell_id: i64,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let resolved = resolve_shell(state, &project, &name, Some(shell_id)).await?;
    if !resolved.is_live {
        return Err(dead_shell_error(&name, shell_id));
    }
    if resolved.is_active {
        return Ok(AgentShellResponse::Activate(AgentShellActivateResponse {
            shell_id,
            changed: false,
            message: format!("Agent shell '{}' is already active.", shell_id),
        }));
    }

    let db = state.db.lock().await;
    db.set_active_agent_shell(&project, &name, resolved.row_id)?;
    Ok(AgentShellResponse::Activate(AgentShellActivateResponse {
        shell_id,
        changed: true,
        message: format!("Activated agent shell '{}'.", shell_id),
    }))
}

async fn handle_spawn(
    project: String,
    name: String,
    activate: bool,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let (container_id, build_id, coastfile_type) = {
        let db = state.db.lock().await;
        let instance =
            db.get_instance(&project, &name)?
                .ok_or_else(|| CoastError::InstanceNotFound {
                    name: name.clone(),
                    project: project.clone(),
                })?;
        if instance.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped. Start it first with `coast start {}`.",
                name, name
            )));
        }
        let container_id = instance.container_id.ok_or_else(|| {
            CoastError::state(format!(
                "Instance '{}' has no container ID and cannot spawn an agent shell.",
                name
            ))
        })?;
        (container_id, instance.build_id, instance.coastfile_type)
    };

    let command =
        resolve_agent_shell_command(&project, build_id.as_deref(), coastfile_type.as_deref())
            .ok_or_else(|| {
                CoastError::state(
                    "No [agent_shell] command configured for this instance build. \
                 Add an [agent_shell] section in your Coastfile and rebuild.",
                )
            })?;

    let spawned = spawn_agent_shell(state, &project, &name, &container_id, &command, activate)
        .await
        .map_err(CoastError::state)?;

    Ok(AgentShellResponse::Spawn(AgentShellSpawnResponse {
        shell_id: spawned.shell_id,
        session_id: spawned.session_id,
        is_active: spawned.is_active_agent,
    }))
}

async fn handle_read_last_lines(
    project: String,
    name: String,
    lines: usize,
    shell_id: Option<i64>,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let resolved = resolve_shell(state, &project, &name, shell_id).await?;
    if !resolved.is_live {
        return Err(dead_shell_error(&name, resolved.shell_id));
    }
    let scrollback = resolved
        .scrollback
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;
    let output = as_output_text(&scrollback)
        .await
        .map_err(|e| CoastError::state(format!("failed to join output task: {e}")))?;
    let tail = if lines == 0 {
        String::new()
    } else {
        output
            .lines()
            .rev()
            .take(lines)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    };

    Ok(AgentShellResponse::ReadLastLines(AgentShellReadResponse {
        shell_id: resolved.shell_id,
        output: tail,
    }))
}

async fn handle_read_output(
    project: String,
    name: String,
    shell_id: Option<i64>,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let resolved = resolve_shell(state, &project, &name, shell_id).await?;
    if !resolved.is_live {
        return Err(dead_shell_error(&name, resolved.shell_id));
    }
    let scrollback = resolved
        .scrollback
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;
    let output = as_output_text(&scrollback)
        .await
        .map_err(|e| CoastError::state(format!("failed to join output task: {e}")))?;

    Ok(AgentShellResponse::ReadOutput(AgentShellReadResponse {
        shell_id: resolved.shell_id,
        output,
    }))
}

async fn handle_input(
    project: String,
    name: String,
    input: String,
    shell_id: Option<i64>,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    let resolved = resolve_shell(state, &project, &name, shell_id).await?;
    if !resolved.is_live {
        return Err(dead_shell_error(&name, resolved.shell_id));
    }
    let write_fd = resolved
        .master_write_fd
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;

    let dup_fd = nix::unistd::dup(write_fd)
        .map_err(|e| CoastError::state(format!("failed to duplicate PTY fd: {e}")))?;
    let mut write_file = tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(dup_fd) });
    write_file
        .write_all(input.as_bytes())
        .await
        .map_err(|e| CoastError::state(format!("failed to write shell input: {e}")))?;

    Ok(AgentShellResponse::Input(AgentShellInputResponse {
        shell_id: resolved.shell_id,
        bytes_written: input.len(),
    }))
}

async fn handle_session_status(
    project: String,
    name: String,
    shell_id: Option<i64>,
    state: &Arc<AppState>,
) -> Result<AgentShellResponse> {
    if shell_id.is_none() {
        let active = {
            let db = state.db.lock().await;
            db.get_instance(&project, &name)?
                .ok_or_else(|| CoastError::InstanceNotFound {
                    name: name.clone(),
                    project: project.clone(),
                })?;
            db.get_active_agent_shell(&project, &name)?
        };
        if active.is_none() {
            return Ok(AgentShellResponse::SessionStatus(
                AgentShellSessionStatusResponse {
                    shell_id: None,
                    status: "none".to_string(),
                    is_active: false,
                    is_live: false,
                    message: format!(
                        "Instance '{}' has no active agent shell. Spawn one with `coast agent-shell {} spawn`.",
                        name, name
                    ),
                },
            ));
        }
    }

    let resolved = resolve_shell(state, &project, &name, shell_id).await?;
    let (status, message) = if resolved.is_live && resolved.is_active {
        (
            "active".to_string(),
            format!("Agent shell '{}' is active.", resolved.shell_id),
        )
    } else if resolved.is_live {
        (
            "inactive".to_string(),
            format!("Agent shell '{}' is live but inactive.", resolved.shell_id),
        )
    } else {
        (
            "dead".to_string(),
            format!(
                "Agent shell '{}' is dead/unavailable. Spawn a new shell with `coast agent-shell {} spawn`.",
                resolved.shell_id, name
            ),
        )
    };

    Ok(AgentShellResponse::SessionStatus(
        AgentShellSessionStatusResponse {
            shell_id: Some(resolved.shell_id),
            status,
            is_active: resolved.is_active,
            is_live: resolved.is_live,
            message,
        },
    ))
}

/// Handle non-streaming agent shell requests.
pub async fn handle(req: AgentShellRequest, state: &Arc<AppState>) -> Result<AgentShellResponse> {
    match req {
        AgentShellRequest::Ls { project, name } => handle_ls(project, name, state).await,
        AgentShellRequest::Activate {
            project,
            name,
            shell_id,
        } => handle_activate(project, name, shell_id, state).await,
        AgentShellRequest::Spawn {
            project,
            name,
            activate,
        } => handle_spawn(project, name, activate, state).await,
        AgentShellRequest::ReadLastLines {
            project,
            name,
            lines,
            shell_id,
        } => handle_read_last_lines(project, name, lines, shell_id, state).await,
        AgentShellRequest::ReadOutput {
            project,
            name,
            shell_id,
        } => handle_read_output(project, name, shell_id, state).await,
        AgentShellRequest::Input {
            project,
            name,
            input,
            shell_id,
        } => handle_input(project, name, input, shell_id, state).await,
        AgentShellRequest::SessionStatus {
            project,
            name,
            shell_id,
        } => handle_session_status(project, name, shell_id, state).await,
        AgentShellRequest::Tty { .. }
        | AgentShellRequest::TtyInput { .. }
        | AgentShellRequest::TtyDetach => Err(CoastError::protocol(
            "TTY agent-shell requests must be handled via streaming connection",
        )),
    }
}

/// Handle an interactive tty stream over the unix socket.
pub async fn handle_tty_stream(
    req: AgentShellRequest,
    state: &Arc<AppState>,
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
) -> Result<()> {
    let AgentShellRequest::Tty {
        project,
        name,
        shell_id,
    } = req
    else {
        return Err(CoastError::protocol(
            "handle_tty_stream called with non-tty request",
        ));
    };

    let resolved = resolve_shell(state, &project, &name, shell_id).await?;
    if !resolved.is_live {
        return Err(dead_shell_error(&name, resolved.shell_id));
    }

    let session_id = resolved
        .session_id
        .clone()
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;
    let output_tx = resolved
        .output_tx
        .clone()
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;
    let master_write_fd = resolved
        .master_write_fd
        .ok_or_else(|| dead_shell_error(&name, resolved.shell_id))?;

    write_response(
        writer,
        &Response::AgentShell(AgentShellResponse::TtyAttached(
            AgentShellTtyAttachedResponse {
                shell_id: resolved.shell_id,
                session_id,
            },
        )),
    )
    .await?;

    let mut output_rx = output_tx.subscribe();
    let dup_fd = nix::unistd::dup(master_write_fd)
        .map_err(|e| CoastError::state(format!("failed to duplicate tty write fd: {e}")))?;
    let mut write_file = tokio::fs::File::from_std(unsafe { std::fs::File::from_raw_fd(dup_fd) });

    let mut input_line = String::new();
    loop {
        input_line.clear();
        tokio::select! {
            chunk = output_rx.recv() => {
                match chunk {
                    Ok(data) => {
                        let out = AgentShellTtyOutputResponse {
                            data: String::from_utf8_lossy(&data).into_owned(),
                        };
                        write_response(writer, &Response::AgentShell(AgentShellResponse::TtyOutput(out))).await?;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "agent-shell tty output lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            read = reader.read_line(&mut input_line) => {
                let bytes = read.map_err(|e| CoastError::io_simple(format!("failed reading tty input: {e}")))?;
                if bytes == 0 {
                    break;
                }
                let trimmed = input_line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed = protocol::decode_request(trimmed.as_bytes())
                    .map_err(|e| CoastError::protocol(format!("failed decoding tty input request: {e}")))?;
                match parsed {
                    Request::AgentShell(AgentShellRequest::TtyInput { data }) => {
                        write_file.write_all(data.as_bytes()).await
                            .map_err(|e| CoastError::state(format!("failed writing tty input: {e}")))?;
                    }
                    Request::AgentShell(AgentShellRequest::TtyDetach) => break,
                    _ => {
                        write_response(
                            writer,
                            &Response::Error(ErrorResponse {
                                error: "Unexpected request during agent-shell tty stream. Use TtyInput or TtyDetach.".to_string(),
                            }),
                        ).await?;
                    }
                }
            }
        }
    }

    let _ = write_response(
        writer,
        &Response::AgentShell(AgentShellResponse::TtyClosed(AgentShellTtyClosedResponse {
            reason: Some("detached".to_string()),
        })),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ws_host_terminal::PtySession;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState::new_for_testing(
            StateDb::open_in_memory().unwrap(),
        ))
    }

    fn make_instance(project: &str, name: &str) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    async fn seed_shell(
        state: &Arc<AppState>,
        project: &str,
        name: &str,
        is_active: bool,
        session_id: &str,
        output: &str,
    ) -> i64 {
        let row_id = {
            let db = state.db.lock().await;
            if db.get_instance(project, name).unwrap().is_none() {
                db.insert_instance(&make_instance(project, name)).unwrap();
            }
            let id = db.create_agent_shell(project, name, "claude").unwrap();
            db.update_agent_shell_session_id(id, session_id).unwrap();
            if is_active {
                db.set_active_agent_shell(project, name, id).unwrap();
            }
            id
        };

        let (output_tx, _) = broadcast::channel::<Vec<u8>>(8);
        let mut sb = VecDeque::new();
        for b in output.as_bytes() {
            sb.push_back(*b);
        }
        let session = PtySession {
            id: session_id.to_string(),
            project: format!("{project}:{name}"),
            child_pid: 0,
            master_read_fd: -1,
            master_write_fd: -1,
            scrollback: Arc::new(Mutex::new(sb)),
            output_tx,
        };
        let mut sessions = state.exec_sessions.lock().await;
        sessions.insert(session_id.to_string(), session);
        row_id
    }

    #[tokio::test]
    async fn test_ls_marks_live_and_dead_shells() {
        let state = test_state();
        let project = "proj";
        let name = "dev-1";
        let first_id = seed_shell(&state, project, name, true, "live-1", "hello\n").await;
        let second_id = {
            let db = state.db.lock().await;
            let id = db.create_agent_shell(project, name, "claude").unwrap();
            let shell = db.get_agent_shell_by_id(id).unwrap().unwrap();
            db.update_agent_shell_session_id(id, "dead-2").unwrap();
            assert_eq!(shell.shell_id, 2);
            id
        };
        let _ = second_id;
        let db = state.db.lock().await;
        let first_shell_id = db
            .get_agent_shell_by_id(first_id)
            .unwrap()
            .unwrap()
            .shell_id;
        drop(db);

        let resp = handle(
            AgentShellRequest::Ls {
                project: project.to_string(),
                name: name.to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        match resp {
            AgentShellResponse::Ls(ls) => {
                assert_eq!(ls.shells.len(), 2);
                let first = ls
                    .shells
                    .iter()
                    .find(|s| s.shell_id == first_shell_id)
                    .unwrap();
                assert!(first.is_live);
                assert!(first.is_active);
                let second = ls
                    .shells
                    .iter()
                    .find(|s| s.shell_id != first_shell_id)
                    .unwrap();
                assert!(!second.is_live);
            }
            _ => panic!("expected ls response"),
        }
    }

    #[tokio::test]
    async fn test_activate_noop_when_already_active() {
        let state = test_state();
        let project = "proj";
        let name = "dev-1";
        let row_id = seed_shell(&state, project, name, true, "live-1", "hello\n").await;
        let shell_id = {
            let db = state.db.lock().await;
            db.get_agent_shell_by_id(row_id).unwrap().unwrap().shell_id
        };
        let resp = handle(
            AgentShellRequest::Activate {
                project: project.to_string(),
                name: name.to_string(),
                shell_id,
            },
            &state,
        )
        .await
        .unwrap();
        match resp {
            AgentShellResponse::Activate(a) => {
                assert!(!a.changed);
                assert!(a.message.contains("already active"));
            }
            _ => panic!("expected activate response"),
        }
    }

    #[tokio::test]
    async fn test_read_last_lines_returns_tail() {
        let state = test_state();
        let project = "proj";
        let name = "dev-1";
        seed_shell(&state, project, name, true, "live-1", "one\ntwo\nthree\n").await;
        let resp = handle(
            AgentShellRequest::ReadLastLines {
                project: project.to_string(),
                name: name.to_string(),
                lines: 2,
                shell_id: None,
            },
            &state,
        )
        .await
        .unwrap();
        match resp {
            AgentShellResponse::ReadLastLines(r) => {
                assert_eq!(r.output, "two\nthree");
            }
            _ => panic!("expected read-last-lines response"),
        }
    }

    #[tokio::test]
    async fn test_read_output_returns_full_scrollback() {
        let state = test_state();
        let project = "proj";
        let name = "dev-1";
        seed_shell(&state, project, name, true, "live-1", "one\ntwo\nthree\n").await;
        let resp = handle(
            AgentShellRequest::ReadOutput {
                project: project.to_string(),
                name: name.to_string(),
                shell_id: None,
            },
            &state,
        )
        .await
        .unwrap();
        match resp {
            AgentShellResponse::ReadOutput(r) => {
                assert!(r.output.contains("one"));
                assert!(r.output.contains("two"));
                assert!(r.output.contains("three"));
            }
            _ => panic!("expected read-output response"),
        }
    }

    #[tokio::test]
    async fn test_session_status_without_active_shell() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("proj", "dev-1")).unwrap();
        }
        let resp = handle(
            AgentShellRequest::SessionStatus {
                project: "proj".to_string(),
                name: "dev-1".to_string(),
                shell_id: None,
            },
            &state,
        )
        .await
        .unwrap();
        match resp {
            AgentShellResponse::SessionStatus(status) => {
                assert_eq!(status.status, "none");
                assert!(status.message.contains("no active agent shell"));
            }
            _ => panic!("expected session-status response"),
        }
    }

    #[tokio::test]
    async fn test_input_rejects_dead_session() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("proj", "dev-1")).unwrap();
            let row_id = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
            db.update_agent_shell_session_id(row_id, "missing-session")
                .unwrap();
            db.set_active_agent_shell("proj", "dev-1", row_id).unwrap();
        }

        let err = handle(
            AgentShellRequest::Input {
                project: "proj".to_string(),
                name: "dev-1".to_string(),
                input: "hello".to_string(),
                shell_id: None,
            },
            &state,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not available"));
    }
}
