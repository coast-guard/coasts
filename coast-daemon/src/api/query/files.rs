use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{
    FileEntry, FileReadResponse, GitFileStatus, GrepMatch, SuccessResponse,
};

use super::{exec_in_coast, resolve_coast_container};
use crate::server::AppState;

// ---------------------------------------------------------------------------
// File browser endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesTreeParams {
    pub project: String,
    pub name: String,
    #[serde(default = "default_workspace_path")]
    pub path: String,
}

fn default_workspace_path() -> String {
    "/workspace".to_string()
}

async fn files_tree(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesTreeParams>,
) -> Result<Json<Vec<FileEntry>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let safe_path = params.path.replace('\'', "'\\''");
    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "ls -1apL --group-directories-first '{}' 2>/dev/null | head -500",
            safe_path
        ),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line == "." || line == ".." || line == "./" || line == "../" {
            continue;
        }
        if line.ends_with('/') {
            let name = line.trim_end_matches('/');
            if !name.is_empty() {
                entries.push(FileEntry {
                    name: name.to_string(),
                    entry_type: "dir".to_string(),
                    size: 0,
                });
            }
        } else {
            entries.push(FileEntry {
                name: line.to_string(),
                entry_type: "file".to_string(),
                size: 0,
            });
        }
    }

    Ok(Json(entries))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesReadParams {
    pub project: String,
    pub name: String,
    pub path: String,
}

async fn files_read(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesReadParams>,
) -> Result<Json<FileReadResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let safe_path = params.path.replace('\'', "'\\''");

    // Check if file is binary (contains null bytes) and get size
    let check_cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("file -b --mime-type '{}' 2>/dev/null", safe_path),
    ];
    let mime = exec_in_coast(&state, container_id, check_cmd)
        .await
        .unwrap_or_default();
    let mime = mime.trim();

    if mime.starts_with("application/octet-stream")
        || mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
    {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "error": "Binary file cannot be displayed", "mime": mime })),
        ));
    }

    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("cat '{}'", safe_path),
    ];

    let content = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    Ok(Json(FileReadResponse {
        content,
        path: params.path,
        mime: mime.to_string(),
    }))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesWriteBody {
    pub project: String,
    pub name: String,
    pub path: String,
    pub content: String,
}

async fn files_write(
    State(state): State<Arc<AppState>>,
    Json(body): Json<FilesWriteBody>,
) -> Result<Json<SuccessResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &body.project, &body.name).await?;
    let container_id = &resolved.container_id;

    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Docker not available" })),
        )
    })?;

    let safe_path = body.path.replace('\'', "'\\''");

    // Use base64 to safely transport content with special characters.
    // Write to a temp file then mv (atomic rename) so that overlayfs
    // generates stronger inotify events (IN_MOVED_TO) which file watchers
    // like chokidar and fsnotify pick up more reliably than in-place writes.
    let encoded = base64_encode(&body.content);
    let cmd_str = format!(
        "echo '{}' | base64 -d > /tmp/.coast-write-tmp && mv /tmp/.coast-write-tmp '{}'",
        encoded, safe_path
    );

    let exec_options = CreateExecOptions {
        cmd: Some(vec!["sh".to_string(), "-c".to_string(), cmd_str]),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("exec create failed: {e}") })),
            )
        })?;

    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    let output = docker
        .start_exec(&exec.id, Some(start_options))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("exec start failed: {e}") })),
            )
        })?;

    let mut stderr_out = String::new();
    if let StartExecResults::Attached { mut output, .. } = output {
        while let Some(chunk) = output.next().await {
            if let Ok(bollard::container::LogOutput::StdErr { message }) = chunk {
                stderr_out.push_str(&String::from_utf8_lossy(&message));
            }
        }
    }

    if !stderr_out.is_empty() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Write failed: {}", stderr_out.trim()) })),
        ));
    }

    Ok(Json(SuccessResponse { success: true }))
}

pub(crate) fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).unwrap();
        encoder.finish().unwrap();
    }
    String::from_utf8(buf).unwrap()
}

struct Base64Encoder<'a> {
    out: &'a mut Vec<u8>,
    buf: Vec<u8>,
}

impl<'a> Base64Encoder<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self {
            out,
            buf: Vec::new(),
        }
    }

    fn finish(self) -> Result<(), std::io::Error> {
        let remaining = self.buf.len();
        if remaining > 0 {
            let mut padded = [0u8; 3];
            padded[..remaining].copy_from_slice(&self.buf);
            let b0 = padded[0];
            let b1 = padded[1];
            let b2 = padded[2];
            let chars: [u8; 4] = [
                B64_TABLE[(b0 >> 2) as usize],
                B64_TABLE[((b0 & 0x03) << 4 | b1 >> 4) as usize],
                if remaining > 1 {
                    B64_TABLE[((b1 & 0x0f) << 2 | b2 >> 6) as usize]
                } else {
                    b'='
                },
                b'=',
            ];
            self.out.extend_from_slice(&chars);
        }
        Ok(())
    }
}

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> std::io::Write for Base64Encoder<'a> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        while self.buf.len() >= 3 {
            let b0 = self.buf[0];
            let b1 = self.buf[1];
            let b2 = self.buf[2];
            let chars: [u8; 4] = [
                B64_TABLE[(b0 >> 2) as usize],
                B64_TABLE[((b0 & 0x03) << 4 | b1 >> 4) as usize],
                B64_TABLE[((b1 & 0x0f) << 2 | b2 >> 6) as usize],
                B64_TABLE[(b2 & 0x3f) as usize],
            ];
            self.out.extend_from_slice(&chars);
            self.buf.drain(..3);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesSearchParams {
    pub project: String,
    pub name: String,
    pub query: String,
}

async fn files_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesSearchParams>,
) -> Result<Json<Vec<String>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let safe_query = params.query.replace('\'', "'\\''");
    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "find /workspace -type f -not -path '*/.git/*' -not -path '*/node_modules/*' -not -path '*/__pycache__/*' -not -path '*/target/*' -name '*{}*' 2>/dev/null | head -50",
            safe_query
        ),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let results: Vec<String> = output
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    Ok(Json(results))
}

// ---------------------------------------------------------------------------
// File index — full file path list for fuzzy search (Ctrl+P)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesIndexParams {
    pub project: String,
    pub name: String,
}

async fn files_index(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesIndexParams>,
) -> Result<Json<Vec<String>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        "if command -v fd >/dev/null 2>&1; then \
           fd --type f \
              --exclude .git --exclude node_modules --exclude __pycache__ \
              --exclude target --exclude .next --exclude dist \
              --exclude .cache --exclude .coasts --exclude .worktrees \
              -E '*.pyc' -E '*.o' -E '*.so' \
              . /workspace 2>/dev/null | head -20000 | sed 's|^/workspace/||'; \
         else \
           find /workspace -type f \
             -not -path '*/.git/*' -not -path '*/node_modules/*' \
             -not -path '*/target/*' -not -path '*/.next/*' \
             -not -path '*/dist/*' -not -path '*/.cache/*' \
             2>/dev/null | head -20000 | sed 's|^/workspace/||'; \
         fi"
        .to_string(),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let paths: Vec<String> = output
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    Ok(Json(paths))
}

// ---------------------------------------------------------------------------
// Content search (grep) — search file contents
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesGrepParams {
    pub project: String,
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub regex: Option<bool>,
}

async fn files_grep(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesGrepParams>,
) -> Result<Json<Vec<GrepMatch>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    if params.query.is_empty() {
        return Ok(Json(vec![]));
    }

    let safe_query = params.query.replace('\'', "'\\''");
    let is_regex = params.regex.unwrap_or(false);

    let rg_flag = if is_regex { "" } else { "--fixed-strings" };
    let grep_flag = if is_regex { "-E" } else { "-F" };
    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "if command -v rg >/dev/null 2>&1; then \
                rg --no-heading --line-number --max-count 5 --max-filesize 1M \
                   --glob '!.git' --glob '!node_modules' --glob '!target' \
                   --glob '!dist' --glob '!.next' --glob '!.cache' \
                   --glob '!*.min.js' --glob '!*.min.css' --glob '!*.map' \
                   {rg_flag} -- '{q}' /workspace 2>/dev/null \
                   | head -200 | sed 's|^/workspace/||'; \
             else \
                grep -rn {grep_flag} '{q}' /workspace \
                     2>/dev/null | grep -v '/\\.git/' | grep -v '/node_modules/' \
                     | grep -v '/target/' | grep -v '/dist/' \
                     | head -200 | sed 's|^/workspace/||'; \
             fi",
            rg_flag = rg_flag,
            grep_flag = grep_flag,
            q = safe_query
        ),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let mut matches = Vec::new();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        // Format: path:line:text
        if let Some(colon1) = line.find(':') {
            let path = &line[..colon1];
            let rest = &line[colon1 + 1..];
            if let Some(colon2) = rest.find(':') {
                let line_num = rest[..colon2].parse::<u32>().unwrap_or(0);
                let text = rest[colon2 + 1..].to_string();
                matches.push(GrepMatch {
                    path: path.to_string(),
                    line: line_num,
                    text: text.trim().to_string(),
                });
            }
        }
    }

    Ok(Json(matches))
}

// ---------------------------------------------------------------------------
// Git status — file change status for the tree
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct FilesGitStatusParams {
    pub project: String,
    pub name: String,
}

async fn files_git_status(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FilesGitStatusParams>,
) -> Result<Json<Vec<GitFileStatus>>, (StatusCode, Json<serde_json::Value>)> {
    let resolved = resolve_coast_container(&state, &params.project, &params.name).await?;
    let container_id = &resolved.container_id;

    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        "cd /workspace && git status --porcelain=v1 2>/dev/null | head -500".to_string(),
    ];

    let output = exec_in_coast(&state, container_id, cmd)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e })),
            )
        })?;

    let mut results = Vec::new();
    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let status = line[..2].trim().to_string();
        let path = line[3..].to_string();
        if !status.is_empty() && !path.is_empty() {
            results.push(GitFileStatus { path, status });
        }
    }

    Ok(Json(results))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/files/tree", get(files_tree))
        .route("/files/read", get(files_read))
        .route("/files/write", post(files_write))
        .route("/files/search", get(files_search))
        .route("/files/index", get(files_index))
        .route("/files/grep", get(files_grep))
        .route("/files/git-status", get(files_git_status))
}
