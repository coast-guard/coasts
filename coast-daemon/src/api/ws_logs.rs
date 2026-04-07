use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use ts_rs::TS;

use coast_core::types::InstanceStatus;
use rust_i18n::t;

use coast_docker::runtime::Runtime;

use crate::handlers::{compose_context, compose_context_for_build};
use crate::server::AppState;

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct LogsStreamParams {
    pub project: String,
    pub name: String,
    #[serde(default)]
    pub service: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/logs/stream", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<LogsStreamParams>,
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
        return Ok(
            ws.on_upgrade(move |socket| handle_remote_logs_socket(socket, state, params, build_id))
        );
    }

    let container_id = instance.container_id.clone().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            t!("error.no_container_id", locale = &lang).to_string(),
        )
    })?;

    drop(db);

    Ok(ws.on_upgrade(move |socket| handle_logs_socket(socket, state, container_id, params)))
}

#[allow(clippy::cognitive_complexity)]
async fn handle_logs_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    params: LogsStreamParams,
) {
    debug!(
        name = %params.name,
        project = %params.project,
        "logs stream websocket connected"
    );

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

    let has_bare = crate::bare_services::has_bare_services(&docker, &container_id).await;
    let has_compose = crate::handlers::assign::has_compose(&params.project);

    let cmd_parts = if has_bare && has_compose && params.service.is_none() {
        // Mixed mode, no service filter: merge both log streams in parallel
        let bare_cmd = crate::bare_services::generate_logs_command(None, None, false, true);
        let compose_script =
            compose_context(&params.project).compose_script("logs --tail 200 --follow");
        vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("({bare_cmd}) & ({compose_script}) & wait"),
        ]
    } else if has_bare && has_compose {
        // Mixed mode with a service filter: route to the right log source
        let service = params.service.as_deref().unwrap();
        let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let log_path = format!("{}/{}.log", crate::bare_services::LOG_DIR, service);
        let is_bare_svc = runtime
            .exec_in_coast(&container_id, &["test", "-f", &log_path])
            .await
            .map(|r| r.success())
            .unwrap_or(false);
        if is_bare_svc {
            let tail_cmd =
                crate::bare_services::generate_logs_command(Some(service), None, false, true);
            vec!["sh".to_string(), "-c".to_string(), tail_cmd]
        } else {
            let ctx = compose_context(&params.project);
            ctx.compose_shell(&format!("logs --tail 200 --follow {service}"))
        }
    } else if has_bare {
        let tail_cmd = crate::bare_services::generate_logs_command(
            params.service.as_deref(),
            None,
            false,
            true,
        );
        vec!["sh".to_string(), "-c".to_string(), tail_cmd]
    } else {
        let ctx = compose_context(&params.project);
        let mut subcmd = "logs --tail 200 --follow".to_string();
        if let Some(ref svc) = params.service {
            subcmd.push(' ');
            subcmd.push_str(svc);
        }
        ctx.compose_shell(&subcmd)
    };
    let cmd_refs: Vec<&str> = cmd_parts.iter().map(std::string::String::as_str).collect();

    let exec_options = CreateExecOptions {
        cmd: Some(
            cmd_refs
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = match docker.create_exec(&container_id, exec_options).await {
        Ok(e) => e,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to create exec: {e}").into()))
                .await;
            return;
        }
    };

    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    let output = match docker.start_exec(&exec.id, Some(start_options)).await {
        Ok(o) => o,
        Err(e) => {
            let _ = socket
                .send(Message::Text(format!("Failed to start exec: {e}").into()))
                .await;
            return;
        }
    };

    if let StartExecResults::Attached { mut output, .. } = output {
        loop {
            tokio::select! {
                chunk = output.next() => {
                    match chunk {
                        Some(Ok(msg)) => {
                            let text = match msg {
                                bollard::container::LogOutput::StdOut { message } |
                                bollard::container::LogOutput::StdErr { message } => {
                                    String::from_utf8_lossy(&message).to_string()
                                }
                                _ => continue,
                            };
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            warn!(error = %e, "log stream error");
                            break;
                        }
                        None => break,
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

    debug!(
        name = %params.name,
        "logs stream websocket disconnected"
    );
}

fn remote_compose_log_cmd(
    project: &str,
    build_id: Option<&str>,
    time_flag: &str,
    service: Option<&str>,
) -> Vec<String> {
    let ctx = compose_context_for_build(project, build_id);
    let project_dir = match &ctx.compose_rel_dir {
        Some(dir) => format!("/workspace/{dir}"),
        None => "/workspace".to_string(),
    };
    let svc = service.unwrap_or("");
    let script = format!(
        "CF=/coast-artifact/compose.coast-shared.yml; \
         [ -f \"$CF\" ] || CF=/coast-artifact/compose.yml; \
         docker compose -f \"$CF\" --project-directory {project_dir} logs {time_flag} {svc}"
    );
    vec!["sh".into(), "-c".into(), script]
}

/// Execute a remote compose log command and send the output over a WebSocket.
///
/// Returns `Ok(true)` if output was sent, `Ok(false)` if output was empty,
/// and `Err(msg)` with a human-readable error if the exec failed or the
/// socket send failed.
async fn exec_and_send_logs(
    socket: &mut WebSocket,
    state: &AppState,
    project: &str,
    name: &str,
    cmd: Vec<String>,
) -> Result<bool, String> {
    match crate::api::query::exec_in_remote_coast(state, project, name, cmd).await {
        Ok(output) if !output.is_empty() => socket
            .send(Message::Text(output.into()))
            .await
            .map(|()| true)
            .map_err(|e| format!("websocket send failed: {e}")),
        Ok(_) => Ok(false),
        Err(e) => {
            warn!(error = %e, "remote log exec error");
            Err(format!("Failed to fetch logs: {e}"))
        }
    }
}

async fn handle_remote_logs_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    params: LogsStreamParams,
    build_id: Option<String>,
) {
    debug!(
        name = %params.name,
        project = %params.project,
        "remote logs stream websocket connected"
    );

    let initial_cmd = remote_compose_log_cmd(
        &params.project,
        build_id.as_deref(),
        "--tail 200",
        params.service.as_deref(),
    );

    if let Err(msg) = exec_and_send_logs(
        &mut socket,
        &state,
        &params.project,
        &params.name,
        initial_cmd,
    )
    .await
    {
        let _ = socket.send(Message::Text(msg.into())).await;
        return;
    }

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                let poll_cmd = remote_compose_log_cmd(
                    &params.project,
                    build_id.as_deref(),
                    "--since 3s",
                    params.service.as_deref(),
                );
                if exec_and_send_logs(&mut socket, &state, &params.project, &params.name, poll_cmd)
                    .await
                    .is_err()
                {
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

    debug!(
        name = %params.name,
        "remote logs stream websocket disconnected"
    );
}
