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

/// Send a JSON-RPC error message over the WebSocket.
async fn send_jsonrpc_error(socket: &mut WebSocket, message: &str) {
    let msg =
        format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"{message}"}},"id":null}}"#);
    let _ = socket.send(Message::Text(msg.into())).await;
}

/// Check that the LSP binary exists in the container.
async fn verify_lsp_binary_exists(
    docker: &bollard::Docker,
    container_id: &str,
    binary_name: &str,
    socket: &mut WebSocket,
) -> bool {
    let check_cmd = format!("command -v {binary_name} >/dev/null 2>&1");
    let check_exec = docker
        .create_exec(
            container_id,
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
                    send_jsonrpc_error(
                        socket,
                        &format!(
                            "{binary_name} not found in container. Add it to your Coastfile [coast.setup] packages."
                        ),
                    )
                    .await;
                    return false;
                }
            }
        }
    }
    true
}

/// Create and start the LSP server exec with stdin/stdout/stderr attached.
async fn create_lsp_exec(
    docker: &bollard::Docker,
    container_id: &str,
    cmd: Vec<String>,
    working_dir: String,
    socket: &mut WebSocket,
) -> Option<(String, StartExecResults)> {
    let exec_options = CreateExecOptions {
        cmd: Some(cmd),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        working_dir: Some(working_dir),
        ..Default::default()
    };

    let exec = match docker.create_exec(container_id, exec_options).await {
        Ok(e) => e,
        Err(e) => {
            send_jsonrpc_error(socket, &format!("Failed to start LSP: {e}")).await;
            return None;
        }
    };

    match docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await
    {
        Ok(o) => Some((exec.id.clone(), o)),
        Err(e) => {
            send_jsonrpc_error(socket, &format!("Failed to start LSP exec: {e}")).await;
            None
        }
    }
}

/// Process one LSP stdout chunk: extract StdOut bytes, accumulate in buffer, parse frames, send to socket.
async fn process_lsp_stdout_chunk(
    socket: &mut WebSocket,
    stdout_buf: &mut BytesMut,
    chunk: Option<Result<bollard::container::LogOutput, bollard::errors::Error>>,
    session_key: &str,
) -> std::ops::ControlFlow<()> {
    match chunk {
        Some(Ok(bollard::container::LogOutput::StdOut { message })) => {
            stdout_buf.extend_from_slice(&message);
            while let Some(json_msg) = extract_lsp_message(stdout_buf) {
                if socket.send(Message::Text(json_msg.into())).await.is_err() {
                    return std::ops::ControlFlow::Break(());
                }
            }
            std::ops::ControlFlow::Continue(())
        }
        Some(Ok(bollard::container::LogOutput::StdErr { message })) => {
            debug!(session = %session_key, stderr = %String::from_utf8_lossy(&message), "LSP stderr");
            std::ops::ControlFlow::Continue(())
        }
        Some(Ok(_)) => std::ops::ControlFlow::Continue(()),
        Some(Err(e)) => {
            warn!(session = %session_key, error = %e, "LSP output stream error");
            std::ops::ControlFlow::Break(())
        }
        None => {
            debug!(session = %session_key, "LSP server exited");
            std::ops::ControlFlow::Break(())
        }
    }
}

/// Write a WebSocket text message to LSP stdin with Content-Length framing.
async fn write_to_lsp_stdin(
    input: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    text: &str,
) -> bool {
    let json_bytes = text.as_bytes();
    let header = format!("Content-Length: {}\r\n\r\n", json_bytes.len());
    input.write_all(header.as_bytes()).await.is_ok()
        && input.write_all(json_bytes).await.is_ok()
        && input.flush().await.is_ok()
}

/// Run the LSP bridge loop: forward stdout→WS and WS→stdin.
async fn run_lsp_bridge_loop(
    socket: &mut WebSocket,
    output: &mut (impl futures_util::Stream<
        Item = Result<bollard::container::LogOutput, bollard::errors::Error>,
    > + Unpin),
    input: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    session_key: &str,
) {
    let mut stdout_buf = BytesMut::new();

    loop {
        tokio::select! {
            chunk = output.next() => {
                if process_lsp_stdout_chunk(socket, &mut stdout_buf, chunk, session_key).await.is_break() {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if !write_to_lsp_stdin(input, &text).await {
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
        send_jsonrpc_error(&mut socket, &docker_err).await;
        return;
    };

    if !verify_lsp_binary_exists(&docker, &container_id, &cmd[0], &mut socket).await {
        return;
    }

    let working_dir = root_path.unwrap_or_else(|| "/workspace".to_string());
    let Some((exec_id, output)) = create_lsp_exec(
        &docker,
        &container_id,
        cmd.clone(),
        working_dir.clone(),
        &mut socket,
    )
    .await
    else {
        return;
    };

    let session_key = format!("{project}:{name}:{language}:{working_dir}");

    // Track this session
    {
        let mut sessions = state.lsp_sessions.lock().await;
        sessions.insert(
            session_key.clone(),
            LspSession {
                exec_id,
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
        run_lsp_bridge_loop(&mut socket, &mut output, &mut input, &session_key).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_extract_lsp_message_complete_frame() {
        let body = r#"{"id":1,"ok":1}"#;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let mut buf = BytesMut::from(format!("{header}{body}").as_str());
        let msg = extract_lsp_message(&mut buf);
        assert_eq!(msg.unwrap(), body);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_extract_lsp_message_partial_body() {
        let mut buf = BytesMut::from("Content-Length: 100\r\n\r\n{\"partial\"");
        let msg = extract_lsp_message(&mut buf);
        assert!(msg.is_none());
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_extract_lsp_message_no_header() {
        let mut buf = BytesMut::from("not a header");
        let msg = extract_lsp_message(&mut buf);
        assert!(msg.is_none());
    }

    #[test]
    fn test_find_header_end_found() {
        let data = b"Content-Length: 5\r\n\r\nhello";
        assert_eq!(find_header_end(data), Some(17));
    }

    #[test]
    fn test_find_header_end_not_found() {
        let data = b"no header here";
        assert!(find_header_end(data).is_none());
    }

    #[test]
    fn test_parse_content_length_valid() {
        assert_eq!(parse_content_length("Content-Length: 42"), Some(42));
    }

    #[test]
    fn test_parse_content_length_missing() {
        assert!(parse_content_length("X-Custom: value").is_none());
    }

    #[test]
    fn test_parse_content_length_case_insensitive() {
        assert_eq!(parse_content_length("content-length: 10"), Some(10));
    }
}
