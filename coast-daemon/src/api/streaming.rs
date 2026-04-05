use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::post;
use axum::{Json, Router};
use futures_util::stream::Stream;
use tokio::sync::mpsc;

use coast_core::protocol::{
    AssignRequest, BuildProgressEvent, BuildRequest, CoastEvent, RerunExtractorsRequest,
    RmBuildRequest, RunRequest, UnassignRequest,
};
use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

use crate::handlers;
use crate::server::{AppState, UpdateOperationKind};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/build", post(build_sse))
        .route("/rerun-extractors", post(rerun_extractors_sse))
        .route("/run", post(run_sse))
        .route("/assign", post(assign_sse))
        .route("/unassign", post(unassign_sse))
        .route("/rm-build", post(rm_build_sse))
}

async fn rerun_extractors_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RerunExtractorsRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::RerunExtractors,
            Some(&req.project),
            None,
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        let sem = state_clone.project_semaphore(&req.project).await;
        if sem.available_permits() == 0 {
            let _ = tx.try_send(BuildProgressEvent::item(
                "Queued",
                "Waiting for another operation to finish",
                "started",
            ));
        }
        let _permit = sem.acquire().await;
        let result = handlers::handle_rerun_extractors_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = rerun_extractors_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn rerun_extractors_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::RerunExtractorsResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

async fn build_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BuildRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let project_name = coast_core::coastfile::Coastfile::from_file(&req.coastfile_path)
            .map(|cf| cf.name)
            .unwrap_or_default();
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::Build,
            (!project_name.is_empty()).then_some(project_name.as_str()),
            None,
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        let sem = if !project_name.is_empty() {
            Some(state_clone.project_semaphore(&project_name).await)
        } else {
            None
        };
        if let Some(ref s) = sem {
            if s.available_permits() == 0 {
                let _ = tx.try_send(BuildProgressEvent::item(
                    "Queued",
                    "Waiting for another operation to finish",
                    "started",
                ));
            }
        }
        let _permit = match &sem {
            Some(s) => Some(s.acquire().await),
            None => None,
        };
        let result = handlers::handle_build_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = build_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn build_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::BuildResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

async fn run_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::Run,
            Some(&req.project),
            Some(&req.name),
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        {
            let db = state_clone.db.lock().await;
            let enqueued_inst = CoastInstance {
                name: req.name.clone(),
                project: req.project.clone(),
                status: InstanceStatus::Enqueued,
                branch: req.branch.clone(),
                commit_sha: req.commit_sha.clone(),
                container_id: None,
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: req.build_id.clone(),
                coastfile_type: req.coastfile_type.clone(),
                remote_name: None,
            };
            if let Err(e) = db.insert_instance(&enqueued_inst) {
                let _ = result_tx.send(Err(e));
                return;
            }
        }
        state_clone.emit_event(CoastEvent::InstanceStatusChanged {
            name: req.name.clone(),
            project: req.project.clone(),
            status: "enqueued".to_string(),
        });

        let sem = state_clone.project_semaphore(&req.project).await;
        if sem.available_permits() == 0 {
            let _ = tx.try_send(BuildProgressEvent::item(
                "Queued",
                "Waiting for another operation to finish",
                "started",
            ));
        }
        let _permit = sem.acquire().await;

        {
            let db = state_clone.db.lock().await;
            let still_exists = db.get_instance(&req.project, &req.name).ok().flatten();
            if still_exists.is_none() {
                return;
            }
        }

        let project = req.project.clone();
        let name = req.name.clone();
        let coastfile_type = req.coastfile_type.clone();
        let result = handlers::handle_run_with_progress(req, &state_clone, tx).await;
        if let Ok(ref resp) = result {
            spawn_agent_shell_if_configured(
                &state_clone,
                &project,
                &name,
                &resp.container_id,
                coastfile_type.as_deref(),
            )
            .await;
        }
        let _ = result_tx.send(result);
    });

    let stream = run_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn run_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::RunResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

async fn assign_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AssignRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::Assign,
            Some(&req.project),
            Some(&req.name),
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        let sem = state_clone.project_semaphore(&req.project).await;
        if sem.available_permits() == 0 {
            let _ = tx.try_send(BuildProgressEvent::item(
                "Queued",
                "Waiting for another operation to finish",
                "started",
            ));
        }
        let _permit = sem.acquire().await;
        let result = handlers::handle_assign_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = assign_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn unassign_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnassignRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::Unassign,
            Some(&req.project),
            Some(&req.name),
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        let sem = state_clone.project_semaphore(&req.project).await;
        if sem.available_permits() == 0 {
            let _ = tx.try_send(BuildProgressEvent::item(
                "Queued",
                "Waiting for another operation to finish",
                "started",
            ));
        }
        let _permit = sem.acquire().await;
        let result = handlers::handle_unassign_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = unassign_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn unassign_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::UnassignResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

fn assign_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::AssignResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

async fn rm_build_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RmBuildRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let _operation_guard = match state_clone.begin_update_operation(
            UpdateOperationKind::RmBuild,
            Some(&req.project),
            None,
        ) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = result_tx.send(Err(error));
                return;
            }
        };
        let sem = state_clone.project_semaphore(&req.project).await;
        if sem.available_permits() == 0 {
            let _ = tx.try_send(BuildProgressEvent::item(
                "Queued",
                "Waiting for another operation to finish",
                "started",
            ));
        }
        let _permit = sem.acquire().await;
        let result = handlers::handle_rm_build_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = rm_build_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn rm_build_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::RmBuildResponse>,
    >,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        while let Some(event) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&event) {
                yield Ok(Event::default().event("progress").data(data));
            }
        }

        match result_rx.await {
            Ok(Ok(resp)) => {
                if let Ok(data) = serde_json::to_string(&resp) {
                    yield Ok(Event::default().event("complete").data(data));
                }
            }
            Ok(Err(e)) => {
                let err = serde_json::json!({ "error": e.to_string() });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
            Err(_) => {
                let err = serde_json::json!({ "error": "handler dropped unexpectedly" });
                yield Ok(Event::default().event("error").data(err.to_string()));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SpawnedAgentShell {
    pub session_id: String,
    pub shell_id: i64,
    pub is_active_agent: bool,
}

pub(crate) fn resolve_agent_shell_command(
    project: &str,
    build_id: Option<&str>,
    coastfile_type: Option<&str>,
) -> Option<String> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);

    let manifest_path = build_id
        .map(|bid| project_dir.join(bid).join("manifest.json"))
        .filter(|p| p.exists())
        .or_else(|| {
            let latest_build_id =
                crate::handlers::run::resolve_latest_build_id(project, coastfile_type);
            latest_build_id
                .map(|bid| project_dir.join(bid).join("manifest.json"))
                .filter(|p| p.exists())
        })
        .or_else(|| {
            let p = project_dir.join("manifest.json");
            p.exists().then_some(p)
        })?;

    let content = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("agent_shell")
        .and_then(|a| a.get("command"))
        .and_then(|c| c.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn wrap_agent_shell_command(command: &str) -> String {
    let escaped_command = command.replace('\'', "'\\''");
    format!(
        "adduser -D -h /home/coast -s /bin/sh coast 2>/dev/null; \
         echo 'coast ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers 2>/dev/null; \
         addgroup coast wheel 2>/dev/null; \
         mkdir -p /home/coast/.claude 2>/dev/null; \
         [ ! -d /home/coast/.claude/.claude ] || rm -rf /home/coast/.claude/.claude 2>/dev/null; \
         cp -a /root/.claude/. /home/coast/.claude/ 2>/dev/null; \
         cp -f /root/.claude.json /home/coast/.claude.json 2>/dev/null; \
         chown -R coast:coast /home/coast 2>/dev/null; \
         chmod 777 /workspace 2>/dev/null; \
         exec su -s /bin/sh coast -c \
         'export HOME=/home/coast \
         PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/go/bin \
         GIT_PAGER=cat PAGER=cat LESS=-FRX; \
         cd /workspace 2>/dev/null; \
         {escaped_command}'"
    )
}

pub(crate) async fn spawn_agent_shell(
    state: &Arc<AppState>,
    project: &str,
    instance_name: &str,
    container_id: &str,
    command: &str,
    set_active: bool,
) -> Result<SpawnedAgentShell, String> {
    let wrapped_command = wrap_agent_shell_command(command);
    let composite_key = format!("{project}:{instance_name}");
    let session_id = super::ws_exec::create_exec_session(
        state,
        &composite_key,
        container_id,
        Some(&wrapped_command),
    )
    .await?;

    let db = state.db.lock().await;
    let row_id = db
        .create_agent_shell(project, instance_name, command)
        .map_err(|e| format!("failed to create agent shell row: {e}"))?;
    if set_active {
        db.set_active_agent_shell(project, instance_name, row_id)
            .map_err(|e| format!("failed to set active agent shell: {e}"))?;
    }
    db.update_agent_shell_session_id(row_id, &session_id)
        .map_err(|e| format!("failed to update agent shell session id: {e}"))?;
    let shell_id = db
        .get_agent_shell_by_id(row_id)
        .map_err(|e| format!("failed to get created agent shell row: {e}"))?
        .map(|s| s.shell_id)
        .ok_or_else(|| "created agent shell row missing".to_string())?;

    state.emit_event(coast_core::protocol::CoastEvent::AgentShellSpawned {
        name: instance_name.to_string(),
        project: project.to_string(),
        shell_id,
    });

    Ok(SpawnedAgentShell {
        session_id,
        shell_id,
        is_active_agent: set_active,
    })
}

/// Read the agent_shell config from the build artifact and spawn a PTY session.
pub(crate) async fn spawn_agent_shell_if_configured(
    state: &Arc<AppState>,
    project: &str,
    instance_name: &str,
    container_id: &str,
    coastfile_type: Option<&str>,
) {
    let Some(command) = resolve_agent_shell_command(project, None, coastfile_type) else {
        return;
    };

    tracing::info!(project = %project, instance = %instance_name, "spawning agent shell");
    match spawn_agent_shell(state, project, instance_name, container_id, &command, true).await {
        Ok(spawned) => {
            tracing::info!(
                shell_id = spawned.shell_id,
                session_id = %spawned.session_id,
                "agent shell spawned and set as active"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to spawn agent shell PTY session");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_agent_shell_command;

    #[test]
    fn test_wrap_agent_shell_command_syncs_credentials_in_place() {
        let wrapped = wrap_agent_shell_command("claude --dangerously-skip-permissions");
        assert!(wrapped.contains("mkdir -p /home/coast/.claude"));
        assert!(wrapped.contains(
            "[ ! -d /home/coast/.claude/.claude ] || rm -rf /home/coast/.claude/.claude"
        ));
        assert!(wrapped.contains("cp -a /root/.claude/. /home/coast/.claude/"));
        assert!(wrapped.contains("cp -f /root/.claude.json /home/coast/.claude.json"));
    }

    #[test]
    fn test_wrap_agent_shell_command_escapes_single_quotes() {
        let wrapped = wrap_agent_shell_command("echo 'hello'");
        assert!(wrapped.contains("echo '\\''hello'\\''"));
    }
}
