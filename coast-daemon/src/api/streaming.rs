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
        .route("/remote-build", post(remote_build_sse))
        .route("/ssg-build", post(ssg_build_sse))
        .route("/ssg-run", post(ssg_run_sse))
        // Phase 33: SSG-native re-extract. Mirrors `rerun-extractors`
        // for regular instances but targets the SSG `[secrets.*]`
        // block in `Coastfile.shared_service_groups`. Backs the
        // SsgSecretsTab "Re-run extractors" button.
        .route("/ssg-rerun-extractors", post(ssg_rerun_extractors_sse))
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
        let project_name = if let Some(ref content) = req.coastfile_content {
            let root = req
                .working_dir
                .as_deref()
                .or_else(|| req.coastfile_path.parent())
                .unwrap_or_else(|| std::path::Path::new("."));
            coast_core::coastfile::Coastfile::parse(content, root)
                .map(|cf| cf.name)
                .unwrap_or_default()
        } else {
            coast_core::coastfile::Coastfile::from_file(&req.coastfile_path)
                .map(|cf| cf.name)
                .unwrap_or_default()
        };
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

async fn remote_build_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BuildRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let result = handlers::handle_remote_build_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = build_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Request shape for `POST /api/v1/stream/ssg-build`.
///
/// Mirrors the `Build` action variant of [`coast_core::protocol::SsgAction`]
/// but flat-shaped for easier JSON construction in TS clients. The
/// daemon resolves the SSG `Coastfile.shared_service_groups` from
/// `working_dir` (or from the project's existing build manifest's
/// `project_root` if `working_dir` is omitted).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SsgBuildSseRequest {
    /// Project name. Required — used to scope the build outcome
    /// (`set_latest_build_id`) and to resolve `project_root` when
    /// `working_dir` is omitted.
    pub project: String,
    /// Optional explicit path to a `Coastfile.shared_service_groups`
    /// (or a custom-named SSG Coastfile). When `None`, the daemon
    /// uses standard `Coastfile.shared_service_groups` discovery in
    /// `working_dir`.
    #[serde(default)]
    pub file: Option<std::path::PathBuf>,
    /// Optional working directory — the project root containing the
    /// SSG Coastfile. When `None`, the daemon resolves it from the
    /// project's most recent regular build manifest (`project_root`
    /// field). The latter falls back to the daemon's own cwd if no
    /// such manifest exists.
    #[serde(default)]
    pub working_dir: Option<std::path::PathBuf>,
    /// Optional inline TOML overrides (forwarded to
    /// [`coast_ssg::daemon_integration::SsgBuildInputs::config`]).
    #[serde(default)]
    pub config: Option<String>,
}

async fn ssg_build_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgBuildSseRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let result = run_ssg_build_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = ssg_build_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Drive an SSG build to completion, emitting progress events to
/// `tx`. Returns the final [`coast_core::protocol::SsgResponse`]
/// payload (the same shape the socket-protocol path emits) on
/// success.
///
/// This intentionally duplicates a minimal slice of
/// `handle_ssg_build_streaming` (server.rs) rather than refactoring
/// the streaming-protocol entry point — the socket path and the SSE
/// path have different cancellation + framing semantics, and
/// extracting a shared helper would force both paths to converge on
/// the lowest-common-denominator API. We can revisit if a third
/// caller appears.
async fn run_ssg_build_with_progress(
    req: SsgBuildSseRequest,
    state: &AppState,
    tx: mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<coast_core::protocol::SsgResponse> {
    use coast_core::error::CoastError;

    if req.project.is_empty() {
        return Err(CoastError::protocol("ssg-build: 'project' is required"));
    }

    let _operation_guard = state.begin_update_operation(
        crate::server::UpdateOperationKind::Build,
        Some(&req.project),
        None,
    )?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| {
            CoastError::docker(
                "coast ssg build requires Docker to be available on the host daemon. \
                 Start Docker Desktop / Colima / OrbStack and restart coastd.",
            )
        })?
        .clone();

    let working_dir = req
        .working_dir
        .clone()
        .or_else(|| resolve_project_root_for(&req.project));

    let inputs = coast_ssg::daemon_integration::SsgBuildInputs {
        project: req.project.clone(),
        file: req.file.clone(),
        working_dir,
        config: req.config.clone(),
    };

    // Pre-read pinned build ids so `auto_prune` inside `build_ssg`
    // can preserve them. Scoped block — guard must drop before any
    // `.await` we don't own.
    let pinned_build_ids: std::collections::HashSet<String> = {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        db.list_ssg_consumer_pins()?
            .into_iter()
            .map(|p| p.build_id)
            .collect()
    };

    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker);
    let outcome =
        coast_ssg::daemon_integration::build_ssg(inputs, &ops, pinned_build_ids, tx).await?;

    // Mirror `handle_ssg_build_streaming`: record the new build as
    // the project's `latest_build_id`. Failure to record is an
    // error — consumers would otherwise hit "no SSG build for
    // project" right after the SPA's "build complete" toast.
    {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        db.set_latest_build_id(&outcome.project, &outcome.build_id)
            .map_err(|err| {
                CoastError::state(format!(
                    "build succeeded but failed to record latest_build_id for project '{}': {err}",
                    outcome.project,
                ))
            })?;
    }

    Ok(outcome.response)
}

/// Resolve `project_root` for `project` from its most recent regular
/// coast image build manifest. Mirrors the resolution logic in
/// `coast-daemon/src/api/query/builds.rs` (`builds_coastfile_types`).
/// Returns `None` when the project has no regular build yet — in
/// which case the SSG build will use `coastd`'s own cwd, matching
/// CLI behaviour.
fn resolve_project_root_for(project: &str) -> Option<std::path::PathBuf> {
    use coast_core::artifact::coast_home;

    let project_dir = coast_home().ok()?.join("images").join(project);

    let manifest_path = std::fs::read_link(project_dir.join("latest"))
        .ok()
        .map(|t| project_dir.join(t).join("manifest.json"))
        .filter(|p| p.exists())
        .or_else(|| {
            let flat = project_dir.join("manifest.json");
            flat.exists().then_some(flat)
        })?;

    let raw = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&raw).ok()?;
    manifest
        .get("project_root")?
        .as_str()
        .map(std::path::PathBuf::from)
}

/// Request shape for `POST /api/v1/stream/ssg-run`.
///
/// Streams progress events while the daemon brings the project's
/// SSG up: pull/create the outer DinD container, wait for the
/// inner Docker daemon, run `docker compose up` for the inner
/// services, refresh host socats. The final `complete` SSE event
/// carries the post-run [`coast_core::protocol::SsgResponse`] so
/// the SPA can replace its cached `/ssg/state` payload without a
/// refetch round-trip.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SsgRunSseRequest {
    /// Project name. Required — used to resolve the build_id from
    /// the SSG runtime row (`pin > latest_build_id`) and to scope
    /// the operation guard / ssg_mutex.
    pub project: String,
}

async fn ssg_run_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgRunSseRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let result = run_ssg_run_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = ssg_build_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Drive an SSG `run` lifecycle to completion, emitting progress
/// events to `tx`. Mirrors `run_ssg_build_with_progress` for the
/// run path: resolve build_id, acquire ssg_mutex, run the outer
/// DinD bring-up, apply the outcome to state, refresh host socats.
/// The synchronous `POST /ssg/run` endpoint in
/// `api/query/ssg.rs::ssg_run` runs the same orchestration with
/// the progress channel discarded — the two callers are kept
/// parallel rather than fused so each has its own cancellation
/// and framing semantics. See `coast-ssg/DESIGN.md §32` for the
/// rm/run cycle preservation rules driven by `latest_build_id`.
async fn run_ssg_run_with_progress(
    req: SsgRunSseRequest,
    state: &Arc<AppState>,
    tx: mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<coast_core::protocol::SsgResponse> {
    use coast_core::error::CoastError;
    use coast_ssg::state::SsgStateExt;

    if req.project.is_empty() {
        return Err(CoastError::protocol("ssg-run: 'project' is required"));
    }

    let _operation_guard = state.begin_update_operation(
        crate::server::UpdateOperationKind::Build,
        Some(&req.project),
        None,
    )?;
    let _ssg_lock = state.ssg_mutex.lock().await;

    let resolved_build_id = {
        let db = state.db.lock().await;
        let pin = db.get_ssg_consumer_pin(&req.project)?.map(|p| p.build_id);
        let latest = db.get_ssg(&req.project)?.and_then(|r| r.latest_build_id);
        pin.or(latest)
    };
    let build_id = resolved_build_id.ok_or_else(|| {
        CoastError::coastfile(format!(
            "no SSG build for project '{}'. Run `coast ssg build` first.",
            req.project,
        ))
    })?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| {
            CoastError::docker(
                "coast ssg run requires Docker to be available on the host daemon. \
                 Start Docker Desktop / Colima / OrbStack and restart coastd.",
            )
        })?
        .clone();

    let outcome = {
        let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker);
        coast_ssg::runtime::lifecycle::run_ssg_with_build_id(
            &req.project,
            &ops,
            Some(build_id.as_str()),
            tx,
        )
        .await?
    };

    let mut resp = {
        let db = state.db.lock().await;
        outcome.apply_to_state_and_response(
            &req.project,
            &*db,
            "running",
            format!("SSG running on build {build_id}"),
        )?
    };
    crate::handlers::run::ssg_integration::refresh_host_socats_for_project(&req.project, state)
        .await;
    resp.message = format!("SSG running on build {build_id}");
    Ok(resp)
}

/// Phase 33: request body for `POST /api/v1/stream/ssg-rerun-extractors`.
///
/// Re-runs the SSG's `[secrets.*]` extractor pass against the
/// Coastfile baked into the active build artifact. Mirrors the
/// regular `RerunExtractorsRequest` but doesn't take a `build_id`
/// since the SSG always re-extracts against the current
/// `latest_build_id` (per-project SSG, §23).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SsgRerunExtractorsSseRequest {
    pub project: String,
}

async fn ssg_rerun_extractors_sse(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgRerunExtractorsSseRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<BuildProgressEvent>(64);

    let state_clone = Arc::clone(&state);
    let (result_tx, result_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let result = run_ssg_rerun_extractors_with_progress(req, &state_clone, tx).await;
        let _ = result_tx.send(result);
    });

    let stream = ssg_rerun_extractors_event_stream(rx, result_rx);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Drive the SSG re-extract to completion, emitting a 2-step plan
/// ("Resolving cached SSG Coastfile" + "Extracting secrets") so
/// the SPA's `SsgRunModal`-style checklist renderer displays a
/// proper progress trail. The actual extraction runs through the
/// shared `coast_ssg::build::secrets::extract_ssg_secrets` helper,
/// keyed on `coast_image = "ssg:<project>"`. Overrides written via
/// the SPA's "Override" button live in the parallel
/// `ssg:<project>/override` namespace and are NOT touched here —
/// the run-time `materialize_secrets` path merges them back in.
async fn run_ssg_rerun_extractors_with_progress(
    req: SsgRerunExtractorsSseRequest,
    state: &Arc<AppState>,
    tx: mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<coast_core::protocol::RerunExtractorsResponse> {
    use coast_core::error::CoastError;
    use coast_ssg::state::SsgStateExt;

    if req.project.is_empty() {
        return Err(CoastError::protocol(
            "ssg-rerun-extractors: 'project' is required",
        ));
    }

    let _operation_guard = state.begin_update_operation(
        crate::server::UpdateOperationKind::RerunExtractors,
        Some(&req.project),
        None,
    )?;
    let _ssg_lock = state.ssg_mutex.lock().await;

    let plan = vec![
        "Resolving cached SSG Coastfile".to_string(),
        "Extracting secrets".to_string(),
    ];
    let total_steps = plan.len() as u32;
    let _ = tx.try_send(BuildProgressEvent::build_plan(plan));

    // --- Step 1: resolve build ---
    let _ = tx
        .send(BuildProgressEvent::started(
            "Resolving cached SSG Coastfile",
            1,
            total_steps,
        ))
        .await;

    let resolved_build_id = {
        let db = state.db.lock().await;
        let pin = db.get_ssg_consumer_pin(&req.project)?.map(|p| p.build_id);
        let latest = db.get_ssg(&req.project)?.and_then(|r| r.latest_build_id);
        pin.or(latest)
    };
    let build_id = resolved_build_id.ok_or_else(|| {
        CoastError::coastfile(format!(
            "no SSG build for project '{}'. Run `coast ssg build` first.",
            req.project,
        ))
    })?;
    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id)?;
    let coastfile_path = build_dir.join("ssg-coastfile.toml");
    if !coastfile_path.exists() {
        return Err(CoastError::coastfile(format!(
            "build artifact missing `ssg-coastfile.toml` at '{}'. \
             Re-run `coast ssg build`.",
            coastfile_path.display()
        )));
    }
    let cf = coast_ssg::coastfile::SsgCoastfile::from_file(&coastfile_path)?;
    let _ = tx
        .send(
            BuildProgressEvent::done("Resolving cached SSG Coastfile", "ok")
                .with_verbose(coastfile_path.display().to_string()),
        )
        .await;

    // --- Step 2: extract ---
    if cf.secrets.is_empty() {
        let _ = tx
            .send(BuildProgressEvent::skip(
                "Extracting secrets",
                2,
                total_steps,
            ))
            .await;
        return Ok(coast_core::protocol::RerunExtractorsResponse {
            project: req.project,
            secrets_extracted: 0,
            warnings: vec![
                "No `[secrets]` declared in this SSG's Coastfile.shared_service_groups."
                    .to_string(),
            ],
        });
    }

    // `extract_ssg_secrets` emits its own `started` + per-item +
    // `done` events for the "Extracting secrets" step.
    let outcome =
        coast_ssg::build::extract_ssg_secrets(&req.project, &cf, &tx, 2, total_steps).await;

    Ok(coast_core::protocol::RerunExtractorsResponse {
        project: req.project,
        secrets_extracted: outcome.secrets_extracted,
        warnings: outcome.warnings,
    })
}

fn ssg_rerun_extractors_event_stream(
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

fn ssg_build_event_stream(
    mut rx: mpsc::Receiver<BuildProgressEvent>,
    result_rx: tokio::sync::oneshot::Receiver<
        coast_core::error::Result<coast_core::protocol::SsgResponse>,
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
                remote_host: None,
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
