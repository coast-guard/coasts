use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bytes::BytesMut;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};
use ts_rs::TS;

use coast_core::types::InstanceStatus;
use rust_i18n::t;

use crate::server::AppState;

/// Language ID to LSP server command mapping.
/// Supports all languages from `vscode-langservers-extracted` plus TS, Python, Rust, Go, YAML.
fn lsp_command(language: &str) -> Option<Vec<String>> {
    match language {
        "typescript" | "javascript" | "typescriptreact" | "javascriptreact" => {
            Some(vec!["typescript-language-server".into(), "--stdio".into()])
        }
        "rust" => Some(vec!["rust-analyzer".into()]),
        "python" => Some(vec!["pyright-langserver".into(), "--stdio".into()]),
        "go" => Some(vec!["gopls".into(), "serve".into()]),
        "json" | "jsonc" => Some(vec!["vscode-json-language-server".into(), "--stdio".into()]),
        "yaml" => Some(vec!["yaml-language-server".into(), "--stdio".into()]),
        "css" | "scss" | "less" => {
            Some(vec!["vscode-css-language-server".into(), "--stdio".into()])
        }
        "html" => Some(vec!["vscode-html-language-server".into(), "--stdio".into()]),
        _ => None,
    }
}

/// Normalize language IDs that share the same LSP server into canonical keys for session reuse.
/// e.g. typescript/typescriptreact/javascript/javascriptreact all share one TS server per root.
fn normalize_language(lang: &str) -> &str {
    match lang {
        "typescriptreact" | "javascriptreact" | "javascript" => "typescript",
        "jsonc" => "json",
        "scss" | "less" => "css",
        _ => lang,
    }
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct LspParams {
    pub project: String,
    pub name: String,
    pub language: String,
    #[serde(default)]
    pub root_path: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/lsp", get(ws_handler))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(params): Query<LspParams>,
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

    let language = params.language.clone();
    let cmd = lsp_command(&language).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Unsupported language for LSP: '{language}'"),
        )
    })?;

    let normalized = normalize_language(&language).to_string();
    let root_path = params.root_path.clone();

    Ok(ws.on_upgrade(move |socket| {
        handle_lsp_socket(
            socket,
            state,
            container_id,
            params.project,
            params.name,
            normalized,
            cmd,
            root_path,
        )
    }))
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
async fn handle_lsp_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    container_id: String,
    project: String,
    name: String,
    language: String,
    cmd: Vec<String>,
    root_path: Option<String>,
) {
    info!(
        project = %project,
        name = %name,
        language = %language,
        "LSP WebSocket connected"
    );

    let Some(docker) = state.docker.as_ref() else {
        let lang = state.language();
        let docker_err = t!("error.docker_not_available", locale = &lang).to_string();
        let msg = format!(
            r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"{docker_err}"}},"id":null}}"#
        );
        let _ = socket.send(Message::Text(msg.into())).await;
        return;
    };

    // First check that the LSP server binary exists in the container
    let check_cmd = format!("command -v {} >/dev/null 2>&1", cmd[0]);
    let check_exec = docker
        .create_exec(
            &container_id,
            CreateExecOptions {
                cmd: Some(vec!["sh".to_string(), "-c".to_string(), check_cmd]),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await;

    if let Ok(exec) = check_exec {
        if let Ok(output) = docker
            .start_exec(&exec.id, Some(StartExecOptions::default()))
            .await
        {
            if let StartExecResults::Attached { mut output, .. } = output {
                while output.next().await.is_some() {}
            }
            if let Ok(inspect) = docker.inspect_exec(&exec.id).await {
                if inspect.exit_code != Some(0) {
                    let msg = format!(
                        r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"{} not found in container. Add it to your Coastfile [coast.setup] packages."}},"id":null}}"#,
                        cmd[0]
                    );
                    let _ = socket.send(Message::Text(msg.into())).await;
                    return;
                }
            }
        }
    }

    // Start the LSP server via docker exec with stdin attached.
    // Use root_path if provided so the server finds the correct tsconfig/project config.
    let working_dir = root_path.unwrap_or_else(|| "/workspace".to_string());
    let exec_options = CreateExecOptions {
        cmd: Some(cmd.clone()),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        working_dir: Some(working_dir.clone()),
        ..Default::default()
    };

    let exec = match docker.create_exec(&container_id, exec_options).await {
        Ok(e) => e,
        Err(e) => {
            let msg = format!(
                r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to start LSP: {e}"}},"id":null}}"#
            );
            let _ = socket.send(Message::Text(msg.into())).await;
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
            let msg = format!(
                r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to start LSP exec: {e}"}},"id":null}}"#
            );
            let _ = socket.send(Message::Text(msg.into())).await;
            return;
        }
    };

    let session_key = format!("{project}:{name}:{language}:{working_dir}");

    // Track this session
    {
        let mut sessions = state.lsp_sessions.lock().await;
        sessions.insert(
            session_key.clone(),
            LspSession {
                exec_id: exec.id.clone(),
                language: language.clone(),
            },
        );
    }

    if let StartExecResults::Attached {
        mut output,
        mut input,
    } = output
    {
        debug!(session = %session_key, "LSP server attached, entering bridge loop");

        // Buffer for accumulating stdout chunks and parsing Content-Length frames
        let mut stdout_buf = BytesMut::new();

        loop {
            tokio::select! {
                // LSP server stdout -> WebSocket
                chunk = output.next() => {
                    match chunk {
                        Some(Ok(msg)) => {
                            let bytes = match msg {
                                bollard::container::LogOutput::StdOut { message } => message,
                                bollard::container::LogOutput::StdErr { message } => {
                                    debug!(
                                        session = %session_key,
                                        stderr = %String::from_utf8_lossy(&message),
                                        "LSP stderr"
                                    );
                                    continue;
                                }
                                _ => continue,
                            };

                            stdout_buf.extend_from_slice(&bytes);

                            // Parse Content-Length framed messages from the buffer
                            while let Some(json_msg) = extract_lsp_message(&mut stdout_buf) {
                                if socket.send(Message::Text(json_msg.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!(session = %session_key, error = %e, "LSP output stream error");
                            break;
                        }
                        None => {
                            debug!(session = %session_key, "LSP server exited");
                            break;
                        }
                    }
                }

                // WebSocket -> LSP server stdin
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let json_bytes = text.as_bytes();
                            let header = format!("Content-Length: {}\r\n\r\n", json_bytes.len());
                            if input.write_all(header.as_bytes()).await.is_err() {
                                break;
                            }
                            if input.write_all(json_bytes).await.is_err() {
                                break;
                            }
                            if input.flush().await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            debug!(session = %session_key, "WebSocket closed");
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Clean up session
    {
        let mut sessions = state.lsp_sessions.lock().await;
        sessions.remove(&session_key);
    }

    info!(
        project = %project,
        name = %name,
        language = %language,
        "LSP WebSocket disconnected"
    );
}

/// Extract a complete LSP message from a Content-Length framed buffer.
/// Returns the JSON body if a complete message is available, otherwise None.
fn extract_lsp_message(buf: &mut BytesMut) -> Option<String> {
    let data = &buf[..];

    // Find the end of the header block (\r\n\r\n)
    let header_end = find_header_end(data)?;

    // Parse Content-Length from headers
    let header_str = std::str::from_utf8(&data[..header_end]).ok()?;
    let content_length = parse_content_length(header_str)?;

    let total_len = header_end + 4 + content_length; // headers + \r\n\r\n + body
    if data.len() < total_len {
        return None; // Not enough data yet
    }

    let body_start = header_end + 4;
    let body = std::str::from_utf8(&data[body_start..body_start + content_length])
        .ok()?
        .to_string();

    // Consume the message from the buffer
    let _ = buf.split_to(total_len);

    Some(body)
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    (0..data.len().saturating_sub(3)).find(|&i| {
        data[i] == b'\r' && data[i + 1] == b'\n' && data[i + 2] == b'\r' && data[i + 3] == b'\n'
    })
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.split("\r\n") {
        let lower = line.to_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

/// Metadata about a running LSP server session.
#[allow(dead_code)]
pub struct LspSession {
    pub exec_id: String,
    pub language: String,
}
