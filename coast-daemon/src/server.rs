/// Unix domain socket server for the coast daemon.
///
/// Accepts connections on `~/.coast/coastd.sock`, reads JSON requests,
/// dispatches them to handlers, and writes JSON responses back.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    self, BuildProgressEvent, CoastEvent, ErrorResponse, LogsResponse, Request, Response,
};

use crate::analytics::{self, AnalyticsClient, CommandSource};
use crate::api::streaming::spawn_agent_shell_if_configured;
use crate::handlers;
use crate::state::StateDb;

/// Shared application state accessible from all handler tasks.
pub struct AppState {
    /// The SQLite state database.
    pub db: Mutex<StateDb>,
    /// Bollard Docker client connected to the host daemon.
    /// None in test environments where Docker is not available.
    pub docker: Option<bollard::Docker>,
    /// Broadcast channel for WebSocket event notifications.
    pub event_bus: tokio::sync::broadcast::Sender<CoastEvent>,
    /// Persistent PTY sessions for the host terminal feature.
    pub pty_sessions:
        Mutex<std::collections::HashMap<String, crate::api::ws_host_terminal::PtySession>>,
    /// Persistent exec sessions for coast instance terminals.
    pub exec_sessions:
        Mutex<std::collections::HashMap<String, crate::api::ws_host_terminal::PtySession>>,
    /// Persistent exec sessions for inner compose service terminals.
    pub service_exec_sessions:
        Mutex<std::collections::HashMap<String, crate::api::ws_host_terminal::PtySession>>,
    /// Ring buffer of recent stats per container (keyed by "project:name").
    pub stats_history:
        Mutex<std::collections::HashMap<String, std::collections::VecDeque<serde_json::Value>>>,
    /// Live stats broadcast channels per container (keyed by "project:name").
    pub stats_broadcasts:
        Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<serde_json::Value>>>,
    /// Background stats collector task handles per container (keyed by "project:name").
    pub stats_collectors: Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Ring buffer of recent stats per inner service (keyed by "project:name:service").
    pub service_stats_history:
        Mutex<std::collections::HashMap<String, std::collections::VecDeque<serde_json::Value>>>,
    /// Live service stats broadcast channels (keyed by "project:name:service").
    pub service_stats_broadcasts:
        Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<serde_json::Value>>>,
    /// Background service stats collector task handles (keyed by "project:name:service").
    pub service_stats_collectors:
        Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Active LSP server sessions (keyed by "project:name:language").
    pub lsp_sessions: Mutex<std::collections::HashMap<String, crate::api::ws_lsp::LspSession>>,
    /// Cached shared services responses (keyed by project name).
    /// Each entry stores the response and the time it was computed.
    pub shared_services_cache: Mutex<
        std::collections::HashMap<
            String,
            (tokio::time::Instant, coast_core::protocol::SharedResponse),
        >,
    >,
    /// Cached count of non-running inner services per instance (keyed by "project:name").
    pub service_health_cache: Mutex<std::collections::HashMap<String, u32>>,
    /// Per-project operation semaphores. Mutating operations (run, assign, start,
    /// stop, rm, rebuild) acquire a permit before proceeding, serializing heavy
    /// Docker workflows within the same project.
    pub project_ops: Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// Current display language. Updated when the user sets a language via the
    /// CLI or API. Handlers read from the `watch::Receiver` side.
    pub language_tx: tokio::sync::watch::Sender<String>,
    /// Receiver side for the language watch channel.
    pub language_rx: tokio::sync::watch::Receiver<String>,
    /// Non-blocking analytics client. Events are batched and flushed to PostHog.
    pub analytics: AnalyticsClient,
    /// Watch channel sender for the analytics toggle. The background worker
    /// checks this at flush time.
    pub analytics_enabled_tx: tokio::sync::watch::Sender<bool>,
}

impl AppState {
    /// Create a new `AppState` with the given state database and Docker client.
    pub fn new(db: StateDb) -> Self {
        let docker = bollard::Docker::connect_with_local_defaults().ok();
        let (event_bus, _) = tokio::sync::broadcast::channel(256);
        let initial_lang = db.get_language().unwrap_or_else(|_| "en".to_string());
        let (language_tx, language_rx) = tokio::sync::watch::channel(initial_lang);

        // Analytics: read or generate a stable anonymous ID, read current toggle
        let anonymous_id = match db.get_user_config("anonymous_id") {
            Ok(Some(id)) => id,
            _ => {
                let id = uuid::Uuid::new_v4().to_string();
                let _ = db.set_user_config("anonymous_id", &id);
                id
            }
        };
        let analytics_initial = db.get_analytics_enabled().unwrap_or(true);
        let (analytics_enabled_tx, analytics_enabled_rx) =
            tokio::sync::watch::channel(analytics_initial);
        let analytics_client = analytics::spawn_worker(anonymous_id, analytics_enabled_rx);

        Self {
            db: Mutex::new(db),
            docker,
            event_bus,
            pty_sessions: Mutex::new(std::collections::HashMap::new()),
            exec_sessions: Mutex::new(std::collections::HashMap::new()),
            service_exec_sessions: Mutex::new(std::collections::HashMap::new()),
            stats_history: Mutex::new(std::collections::HashMap::new()),
            stats_broadcasts: Mutex::new(std::collections::HashMap::new()),
            stats_collectors: Mutex::new(std::collections::HashMap::new()),
            service_stats_history: Mutex::new(std::collections::HashMap::new()),
            service_stats_broadcasts: Mutex::new(std::collections::HashMap::new()),
            service_stats_collectors: Mutex::new(std::collections::HashMap::new()),
            lsp_sessions: Mutex::new(std::collections::HashMap::new()),
            shared_services_cache: Mutex::new(std::collections::HashMap::new()),
            service_health_cache: Mutex::new(std::collections::HashMap::new()),
            project_ops: Mutex::new(std::collections::HashMap::new()),
            language_tx,
            language_rx,
            analytics: analytics_client,
            analytics_enabled_tx,
        }
    }

    /// Create a new `AppState` for testing (no Docker client).
    ///
    /// Port availability and socat sections are skipped (`docker` is `None`).
    #[cfg(test)]
    pub fn new_for_testing(db: StateDb) -> Self {
        let (event_bus, _) = tokio::sync::broadcast::channel(256);
        let (language_tx, language_rx) = tokio::sync::watch::channel("en".to_string());
        let (analytics_enabled_tx, _analytics_enabled_rx) = tokio::sync::watch::channel(true);
        Self {
            db: Mutex::new(db),
            docker: None,
            event_bus,
            pty_sessions: Mutex::new(std::collections::HashMap::new()),
            exec_sessions: Mutex::new(std::collections::HashMap::new()),
            service_exec_sessions: Mutex::new(std::collections::HashMap::new()),
            stats_history: Mutex::new(std::collections::HashMap::new()),
            stats_broadcasts: Mutex::new(std::collections::HashMap::new()),
            stats_collectors: Mutex::new(std::collections::HashMap::new()),
            service_stats_history: Mutex::new(std::collections::HashMap::new()),
            service_stats_broadcasts: Mutex::new(std::collections::HashMap::new()),
            service_stats_collectors: Mutex::new(std::collections::HashMap::new()),
            lsp_sessions: Mutex::new(std::collections::HashMap::new()),
            shared_services_cache: Mutex::new(std::collections::HashMap::new()),
            service_health_cache: Mutex::new(std::collections::HashMap::new()),
            project_ops: Mutex::new(std::collections::HashMap::new()),
            language_tx,
            language_rx,
            analytics: AnalyticsClient::noop(),
            analytics_enabled_tx,
        }
    }

    /// Create a new `AppState` for testing with a Docker client stub.
    ///
    /// Points at `/dev/null` so no real Docker socket is contacted and Docker
    /// Desktop is not woken up. The client is never called — it only needs to
    /// exist so that `state.docker.is_some()` returns true for code paths like
    /// port availability checks.
    #[cfg(test)]
    pub fn new_for_testing_with_docker(db: StateDb) -> Self {
        let mut s = Self::new_for_testing(db);
        s.docker = Some(
            bollard::Docker::connect_with_http(
                "http://127.0.0.1:0",
                1,
                bollard::API_DEFAULT_VERSION,
            )
            .expect("bollard stub client creation should not fail"),
        );
        s
    }

    /// Get or create the per-project operation semaphore.
    ///
    /// Mutating operations (run, assign, start, stop, rm, rebuild) acquire a
    /// permit from this semaphore before proceeding, ensuring only one heavy
    /// operation runs per project at a time.
    pub async fn project_semaphore(&self, project: &str) -> Arc<tokio::sync::Semaphore> {
        let mut map = self.project_ops.lock().await;
        map.entry(project.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    /// Emit an event to all connected WebSocket clients.
    /// Silently ignores errors (no subscribers connected).
    pub fn emit_event(&self, event: CoastEvent) {
        let _ = self.event_bus.send(event);
    }

    /// Get the current display language.
    pub fn language(&self) -> String {
        self.language_rx.borrow().clone()
    }
}

/// The default socket path: `$COAST_HOME/coastd.sock`.
pub fn default_socket_path() -> Result<PathBuf> {
    Ok(coast_core::artifact::coast_home()?.join("coastd.sock"))
}

/// The default PID file path: `$COAST_HOME/coastd.pid`.
pub fn default_pid_path() -> Result<PathBuf> {
    Ok(coast_core::artifact::coast_home()?.join("coastd.pid"))
}

/// Ensure the coast home directory exists.
pub fn ensure_coast_dir() -> Result<PathBuf> {
    let coast_dir = coast_core::artifact::coast_home()?;
    std::fs::create_dir_all(&coast_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create {} directory: {e}", coast_dir.display()),
        path: coast_dir.clone(),
        source: Some(e),
    })?;
    Ok(coast_dir)
}

/// Start the Unix socket server.
///
/// Listens on the given socket path, accepts connections concurrently,
/// and dispatches requests to handlers via the shared `AppState`.
///
/// The server runs until the `shutdown` signal is received.
#[allow(clippy::cognitive_complexity)]
pub async fn run_server(
    socket_path: &Path,
    state: Arc<AppState>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<()> {
    // Remove stale socket file if it exists
    if socket_path.exists() {
        std::fs::remove_file(socket_path).map_err(|e| CoastError::Io {
            message: format!(
                "failed to remove stale socket file '{}': {e}",
                socket_path.display()
            ),
            path: socket_path.to_path_buf(),
            source: Some(e),
        })?;
    }

    let listener = UnixListener::bind(socket_path).map_err(|e| CoastError::Io {
        message: format!(
            "failed to bind Unix socket at '{}'. \
             Is another coastd instance running? Error: {e}",
            socket_path.display()
        ),
        path: socket_path.to_path_buf(),
        source: Some(e),
    })?;

    info!(socket = %socket_path.display(), "coastd server listening");

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, state).await {
                                error!("connection handler error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        error!("failed to accept connection: {e}");
                    }
                }
            }
            _ = shutdown.recv() => {
                info!("shutdown signal received, stopping server");
                break;
            }
        }
    }

    // Clean up socket file
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    info!("coastd server stopped");
    Ok(())
}

/// Handle a single client connection.
///
/// Reads one JSON request line, dispatches to the appropriate handler,
/// and writes the JSON response back.
#[allow(clippy::cognitive_complexity)]
async fn handle_connection(stream: tokio::net::UnixStream, state: Arc<AppState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // Read one line (newline-terminated JSON)
    let bytes_read = buf_reader
        .read_line(&mut line)
        .await
        .map_err(|e| CoastError::io_simple(format!("failed to read request from client: {e}")))?;

    if bytes_read == 0 {
        debug!("client disconnected without sending data");
        return Ok(());
    }

    let trimmed = line.trim();
    if trimmed.is_empty() {
        debug!("received empty request");
        return Ok(());
    }

    debug!(request_bytes = bytes_read, "received request");

    // Decode the request
    let request = match protocol::decode_request(trimmed.as_bytes()) {
        Ok(r) => r,
        Err(e) => {
            warn!("malformed request: {e}");
            let resp = Response::Error(ErrorResponse {
                error: format!("malformed request: {e}"),
            });
            write_response(&mut writer, &resp).await?;
            return Ok(());
        }
    };

    // Capture command name, context, metadata, and start time for analytics
    let request_for_meta = request.clone();
    let command_name = analytics::request_command_name(&request);
    let (ctx_project, ctx_instance) = analytics::request_context(&request);
    let base_metadata = analytics::request_metadata(&request_for_meta);
    let ctx_project = ctx_project.map(String::from);
    let ctx_instance = ctx_instance.map(String::from);
    let start = tokio::time::Instant::now();

    // Helper: record an analytics event for the CLI path
    let track = |success: bool, metadata: analytics::AnalyticsMetadata| {
        state.analytics.track_command_with_context(
            &command_name,
            CommandSource::Cli,
            success,
            start.elapsed().as_millis() as u64,
            ctx_project.as_deref(),
            ctx_instance.as_deref(),
            if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
        );
    };

    // Build and run requests get special streaming treatment: progress events
    // are sent as individual JSON lines before the final response.
    if let Request::Build(req) = request {
        let result = handle_build_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::RerunExtractors(req) = request {
        let result = handle_rerun_extractors_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Run(req) = request {
        let result = handle_run_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Assign(req) = request {
        let result = handle_assign_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Unassign(req) = request {
        let result = handle_unassign_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Start(req) = request {
        let result = handle_start_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Stop(req) = request {
        let result = handle_stop_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::RmBuild(req) = request {
        let result = handle_rm_build_streaming(req, &state, &mut writer).await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::AgentShell(req @ coast_core::protocol::AgentShellRequest::Tty { .. }) = request
    {
        let result =
            handlers::agent_shell::handle_tty_stream(req, &state, &mut buf_reader, &mut writer)
                .await;
        track(result.is_ok(), base_metadata.clone());
        return result;
    }
    if let Request::Logs(req) = request {
        if req.follow {
            let result = handle_logs_streaming(req, &state, &mut writer).await;
            track(result.is_ok(), base_metadata.clone());
            return result;
        }
        let response = handlers::handle_logs(req, &state).await;
        let success = !matches!(&response, Response::Error(_));
        let mut metadata = base_metadata.clone();
        metadata.extend(analytics::response_metadata(&request_for_meta, &response));
        write_response(&mut writer, &response).await?;
        track(success, metadata);
        return Ok(());
    }

    let response = dispatch_request(request, &state).await;
    let success = !matches!(&response, Response::Error(_));
    let mut metadata = base_metadata;
    metadata.extend(analytics::response_metadata(&request_for_meta, &response));
    write_response(&mut writer, &response).await?;
    track(success, metadata);

    Ok(())
}

/// Encode and write a single response line, then flush.
async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    resp: &Response,
) -> Result<()> {
    let bytes = protocol::encode_response(resp)
        .map_err(|e| CoastError::protocol(format!("failed to encode response: {e}")))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|e| CoastError::io_simple(format!("failed to write response to client: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| CoastError::io_simple(format!("failed to flush response to client: {e}")))?;
    Ok(())
}

/// Handle a build request with streaming progress output.
///
/// Creates an mpsc channel, runs the build handler concurrently with a
/// loop that forwards progress events to the client as JSON lines.
#[allow(clippy::cognitive_complexity)]
async fn handle_build_streaming(
    req: coast_core::protocol::BuildRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    // Derive project name from the coastfile to acquire the per-project semaphore.
    let project_name = coast_core::coastfile::Coastfile::from_file(&req.coastfile_path)
        .map(|cf| cf.name)
        .unwrap_or_default();
    let sem = if !project_name.is_empty() {
        Some(state.project_semaphore(&project_name).await)
    } else {
        None
    };
    let _permit = match &sem {
        Some(s) => Some(
            s.acquire()
                .await
                .map_err(|_| CoastError::state("operation queue closed"))?,
        ),
        None => None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut build_future = std::pin::pin!(handlers::handle_build_with_progress(req, state, tx));
    let mut build_done = false;
    let mut build_result: Option<
        std::result::Result<coast_core::protocol::BuildResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if build_done {
            // Drain remaining buffered events after handler finished
            while let Ok(event) = rx.try_recv() {
                let resp = Response::BuildProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send build progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut build_future => {
                build_result = Some(result);
                build_done = true;
                // Don't break yet — drain remaining events in next iteration
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::BuildProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send build progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match build_result.unwrap() {
        Ok(resp) => Response::Build(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle a logs request with streaming output chunks.
async fn handle_logs_streaming(
    req: coast_core::protocol::LogsRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<LogsResponse>(64);

    let mut logs_future = std::pin::pin!(handlers::handle_logs_with_progress(req, state, tx));
    let mut logs_done = false;
    let mut logs_result: Option<
        std::result::Result<coast_core::protocol::LogsResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if logs_done {
            while let Ok(chunk) = rx.try_recv() {
                let resp = Response::LogsProgress(chunk);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send logs progress: {e}");
                    return Ok(());
                }
            }
            break;
        }

        tokio::select! {
            result = &mut logs_future => {
                logs_result = Some(result);
                logs_done = true;
            }
            chunk = rx.recv() => {
                if let Some(chunk) = chunk {
                    let resp = Response::LogsProgress(chunk);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send logs progress: {e}");
                        return Ok(());
                    }
                }
            }
        }
    }

    let final_response = match logs_result.unwrap() {
        Ok(resp) => Response::Logs(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle a run request with streaming progress output.
#[allow(clippy::cognitive_complexity)]
async fn handle_run_streaming(
    req: coast_core::protocol::RunRequest,
    state: &Arc<AppState>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    {
        let db = state.db.lock().await;
        let enqueued_inst = coast_core::types::CoastInstance {
            name: req.name.clone(),
            project: req.project.clone(),
            status: coast_core::types::InstanceStatus::Enqueued,
            branch: req.branch.clone(),
            commit_sha: req.commit_sha.clone(),
            container_id: None,
            runtime: coast_core::types::RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: req.build_id.clone(),
            coastfile_type: req.coastfile_type.clone(),
        };
        db.insert_instance(&enqueued_inst)?;
    }
    state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "enqueued".to_string(),
    });

    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    {
        let db = state.db.lock().await;
        let still_exists = db.get_instance(&req.project, &req.name).ok().flatten();
        if still_exists.is_none() {
            return Ok(());
        }
    }

    let project = req.project.clone();
    let name = req.name.clone();
    let coastfile_type = req.coastfile_type.clone();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut run_future = std::pin::pin!(handlers::handle_run_with_progress(req, state, tx));
    let mut run_done = false;
    let mut run_result: Option<
        std::result::Result<coast_core::protocol::RunResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if run_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::RunProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send run progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut run_future => {
                run_result = Some(result);
                run_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::RunProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send run progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match run_result.unwrap() {
        Ok(resp) => {
            spawn_agent_shell_if_configured(
                state,
                &project,
                &name,
                &resp.container_id,
                coastfile_type.as_deref(),
            )
            .await;
            Response::Run(resp)
        }
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle a rerun-extractors request with streaming progress output.
async fn handle_rerun_extractors_streaming(
    req: coast_core::protocol::RerunExtractorsRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut rerun_future = std::pin::pin!(handlers::handle_rerun_extractors_with_progress(
        req, state, tx
    ));
    let mut rerun_done = false;
    let mut rerun_result: Option<
        std::result::Result<
            coast_core::protocol::RerunExtractorsResponse,
            coast_core::error::CoastError,
        >,
    > = None;

    loop {
        if rerun_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::RerunExtractorsProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send rerun-extractors progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut rerun_future => {
                rerun_result = Some(result);
                rerun_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::RerunExtractorsProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send rerun-extractors progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match rerun_result.unwrap() {
        Ok(resp) => Response::RerunExtractors(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle an assign request with streaming progress output.
async fn handle_assign_streaming(
    req: coast_core::protocol::AssignRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut assign_future = std::pin::pin!(handlers::handle_assign_with_progress(req, state, tx));
    let mut assign_done = false;
    let mut assign_result: Option<
        std::result::Result<coast_core::protocol::AssignResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if assign_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::AssignProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send assign progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut assign_future => {
                assign_result = Some(result);
                assign_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::AssignProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send assign progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match assign_result.unwrap() {
        Ok(resp) => Response::Assign(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle an unassign request with streaming progress output.
async fn handle_unassign_streaming(
    req: coast_core::protocol::UnassignRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut unassign_future =
        std::pin::pin!(handlers::handle_unassign_with_progress(req, state, tx));
    let mut unassign_done = false;
    let mut unassign_result: Option<
        std::result::Result<coast_core::protocol::UnassignResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if unassign_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::UnassignProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send unassign progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut unassign_future => {
                unassign_result = Some(result);
                unassign_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::UnassignProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send unassign progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match unassign_result.unwrap() {
        Ok(resp) => Response::Unassign(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle a start request with streaming progress output.
async fn handle_start_streaming(
    req: coast_core::protocol::StartRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut start_future = std::pin::pin!(handlers::handle_start_with_progress(req, state, tx));
    let mut start_done = false;
    let mut start_result: Option<
        std::result::Result<coast_core::protocol::StartResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if start_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::StartProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send start progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut start_future => {
                start_result = Some(result);
                start_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::StartProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send start progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match start_result.unwrap() {
        Ok(resp) => Response::Start(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle a stop request with streaming progress output.
async fn handle_stop_streaming(
    req: coast_core::protocol::StopRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut stop_future = std::pin::pin!(handlers::handle_stop_with_progress(req, state, tx));
    let mut stop_done = false;
    let mut stop_result: Option<
        std::result::Result<coast_core::protocol::StopResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if stop_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::StopProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send stop progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut stop_future => {
                stop_result = Some(result);
                stop_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::StopProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send stop progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match stop_result.unwrap() {
        Ok(resp) => Response::Stop(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Handle an rm-build request with streaming progress output.
async fn handle_rm_build_streaming(
    req: coast_core::protocol::RmBuildRequest,
    state: &AppState,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) -> Result<()> {
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem
        .acquire()
        .await
        .map_err(|_| CoastError::state("operation queue closed"))?;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);

    let mut rm_future = std::pin::pin!(handlers::handle_rm_build_with_progress(req, state, tx));
    let mut rm_done = false;
    let mut rm_result: Option<
        std::result::Result<coast_core::protocol::RmBuildResponse, coast_core::error::CoastError>,
    > = None;

    loop {
        if rm_done {
            while let Ok(event) = rx.try_recv() {
                let resp = Response::RmBuildProgress(event);
                if let Err(e) = write_response(writer, &resp).await {
                    warn!("failed to send rm-build progress: {e}");
                    break;
                }
            }
            break;
        }

        tokio::select! {
            result = &mut rm_future => {
                rm_result = Some(result);
                rm_done = true;
            }
            event = rx.recv() => {
                if let Some(event) = event {
                    let resp = Response::RmBuildProgress(event);
                    if let Err(e) = write_response(writer, &resp).await {
                        warn!("failed to send rm-build progress: {e}");
                    }
                }
            }
        }
    }

    let final_response = match rm_result.unwrap() {
        Ok(resp) => Response::RmBuild(resp),
        Err(e) => Response::Error(ErrorResponse {
            error: e.to_string(),
        }),
    };
    write_response(writer, &final_response).await
}

/// Dispatch a decoded request to the appropriate handler.
async fn dispatch_request(request: Request, state: &Arc<AppState>) -> Response {
    match request {
        Request::Build(_) => unreachable!("build requests handled by handle_build_streaming"),
        Request::RerunExtractors(_) => {
            unreachable!("rerun-extractors requests handled by handle_rerun_extractors_streaming")
        }
        Request::Run(_) => unreachable!("run requests handled by handle_run_streaming"),
        Request::Stop(_) => unreachable!("stop requests handled by handle_stop_streaming"),
        Request::Start(_) => unreachable!("start requests handled by handle_start_streaming"),
        Request::Rm(req) => handlers::handle_rm(req, state).await,
        Request::Checkout(req) => handlers::handle_checkout(req, state).await,
        Request::Ports(req) => handlers::handle_ports(req, state).await,
        Request::Exec(req) => handlers::handle_exec(req, state).await,
        Request::Logs(req) => handlers::handle_logs(req, state).await,
        Request::Ps(req) => handlers::handle_ps(req, state).await,
        Request::Ls(req) => handlers::handle_ls(req, state).await,
        Request::Lookup(req) => handlers::handle_lookup(req, state).await,
        Request::Docs(req) => handlers::handle_docs(req, state).await,
        Request::SearchDocs(req) => handlers::handle_search_docs(req, state).await,
        Request::Secret(req) => handlers::handle_secret(req, state).await,
        Request::Shared(req) => handlers::handle_shared(req, state).await,
        Request::Assign(_) => unreachable!("assign requests handled by handle_assign_streaming"),
        Request::Unassign(_) => {
            unreachable!("unassign requests handled by handle_unassign_streaming")
        }
        Request::Rebuild(req) => handlers::handle_rebuild(req, state).await,
        Request::RmBuild(_) => {
            unreachable!("rm-build requests handled by handle_rm_build_streaming")
        }
        Request::ArchiveProject(req) => handlers::handle_archive_project(req, state).await,
        Request::UnarchiveProject(req) => handlers::handle_unarchive_project(req, state).await,
        Request::Builds(req) => handlers::handle_builds(req, state).await,
        Request::McpLs(req) => handlers::handle_mcp_ls(req, state).await,
        Request::McpTools(req) => handlers::handle_mcp_tools(req, state).await,
        Request::McpLocations(req) => handlers::handle_mcp_locations(req, state).await,
        Request::AgentShell(req) => handlers::handle_agent_shell(req, state).await,
        Request::SetLanguage(req) => handlers::handle_set_language(req, state).await,
        Request::SetAnalytics(req) => handlers::handle_set_analytics(req, state).await,
    }
}

/// Write the PID file for the daemon process.
pub fn write_pid_file(pid_path: &Path) -> Result<()> {
    let pid = std::process::id();
    std::fs::write(pid_path, pid.to_string()).map_err(|e| CoastError::Io {
        message: format!("failed to write PID file at '{}': {e}", pid_path.display()),
        path: pid_path.to_path_buf(),
        source: Some(e),
    })?;
    debug!(pid = pid, path = %pid_path.display(), "PID file written");
    Ok(())
}

/// Remove the PID file.
pub fn remove_pid_file(pid_path: &Path) -> Result<()> {
    if pid_path.exists() {
        std::fs::remove_file(pid_path).map_err(|e| CoastError::Io {
            message: format!("failed to remove PID file at '{}': {e}", pid_path.display()),
            path: pid_path.to_path_buf(),
            source: Some(e),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_socket_path() {
        let path = default_socket_path().unwrap();
        assert!(path.to_string_lossy().contains(".coast"));
        assert!(path.to_string_lossy().contains("coastd.sock"));
    }

    #[test]
    fn test_default_pid_path() {
        let path = default_pid_path().unwrap();
        assert!(path.to_string_lossy().contains(".coast"));
        assert!(path.to_string_lossy().contains("coastd.pid"));
    }

    #[test]
    fn test_ensure_coast_dir() {
        let path = ensure_coast_dir().unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains(".coast"));
    }

    #[test]
    fn test_write_and_remove_pid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_path = tmp.path().join("coastd.pid");

        write_pid_file(&pid_path).unwrap();
        assert!(pid_path.exists());

        let content = std::fs::read_to_string(&pid_path).unwrap();
        let pid: u32 = content.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_file(&pid_path).unwrap();
        assert!(!pid_path.exists());
    }

    #[test]
    fn test_remove_nonexistent_pid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_path = tmp.path().join("nonexistent.pid");
        // Should not error
        remove_pid_file(&pid_path).unwrap();
    }

    #[tokio::test]
    async fn test_server_accepts_connection() {
        let tmp = tempfile::tempdir().unwrap();
        let socket_path = tmp.path().join("test.sock");
        let db = StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        let server_path = socket_path.clone();
        let server_state = Arc::clone(&state);
        let server_handle =
            tokio::spawn(async move { run_server(&server_path, server_state, shutdown_rx).await });

        // Give the server a moment to start
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Connect and send a request
        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();

        let request = Request::Ls(coast_core::protocol::LsRequest { project: None });
        let encoded = protocol::encode_request(&request).unwrap();
        writer.write_all(&encoded).await.unwrap();
        writer.flush().await.unwrap();

        // Read response
        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await.unwrap();

        let response = protocol::decode_response(response_line.trim().as_bytes()).unwrap();
        match response {
            Response::Ls(ls) => {
                assert!(ls.instances.is_empty());
            }
            Response::Error(e) => {
                // Also acceptable during testing — the handler might
                // return an error if DB isn't fully set up
                debug!("got error response: {}", e.error);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        // Shutdown
        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_server_handles_malformed_request() {
        let tmp = tempfile::tempdir().unwrap();
        let socket_path = tmp.path().join("test_malformed.sock");
        let db = StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        let server_path = socket_path.clone();
        let server_state = Arc::clone(&state);
        let server_handle =
            tokio::spawn(async move { run_server(&server_path, server_state, shutdown_rx).await });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();

        // Send malformed JSON
        writer.write_all(b"not valid json\n").await.unwrap();
        writer.flush().await.unwrap();

        let mut buf_reader = BufReader::new(reader);
        let mut response_line = String::new();
        buf_reader.read_line(&mut response_line).await.unwrap();

        let response = protocol::decode_response(response_line.trim().as_bytes()).unwrap();
        match response {
            Response::Error(e) => {
                assert!(e.error.contains("malformed"));
            }
            other => panic!("expected error response, got: {other:?}"),
        }

        let _ = shutdown_tx.send(());
        let _ = server_handle.await;
    }
}
