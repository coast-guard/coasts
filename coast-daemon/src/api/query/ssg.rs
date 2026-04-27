//! HTTP endpoints for SSG (Shared Service Group) introspection.
//!
//! These endpoints back the SPA's SSG-aware UI surfaces:
//! - `GET /api/v1/ssg/builds?project=<p>` — lists SSG build artifacts
//!   for a single project. Mirrors the shape of `/api/v1/builds`
//!   (the regular coast image builds list) but scoped to SSG
//!   artifacts under `~/.coast/ssg/<project>/builds/`.
//!
//! Every operation here runs through [`crate::handlers::ssg`] so the
//! socket protocol path and the HTTP path share the same
//! [`coast_core::protocol::SsgRequest`] dispatch + the same
//! response shape. See `coast-ssg/DESIGN.md` for the SSG protocol.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::{
    ImageInspectResponse, RevealSecretResponse, SecretInfo, ServiceInspectResponse, SsgAction,
    SsgBuildEntry, SsgPortInfo, SsgRequest, SsgServiceInfo, VolumeInspectResponse,
};

use crate::handlers;
use crate::server::AppState;

/// Resolve the outer DinD container id for `project`'s SSG.
///
/// Returns:
/// - `Ok(container_id)` when the SSG has a tracked container (running
///   or stopped — the lifecycle of the container itself is the
///   caller's concern).
/// - `Err((StatusCode::NOT_FOUND, json))` when there's no `ssg` row
///   for the project at all (project never built / ssg never run).
/// - `Err((StatusCode::CONFLICT, json))` when the row exists but
///   `container_id` is NULL (built but never run).
///
/// Shared by every endpoint in this module that needs to talk to
/// the SSG container (HTTP `images`/`volumes`, the WebSocket
/// terminal/logs/stats endpoints).
pub(crate) async fn resolve_ssg_container_id(
    state: &AppState,
    project: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    if project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project'"
            })),
        ));
    }
    use coast_ssg::state::SsgStateExt;
    let db = state.db.lock().await;
    let row = db.get_ssg(project).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("state lookup failed: {e}"),
            })),
        )
    })?;
    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("no SSG for project '{project}'"),
            })),
        ));
    };
    let Some(cid) = row.container_id else {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "SSG for project '{project}' has no running container; \
                     run `coast ssg run` to start it",
                ),
            })),
        ));
    };
    Ok(cid)
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgBuildsLsParams {
    pub project: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildsLsHttpResponse {
    pub project: String,
    pub builds: Vec<SsgBuildEntry>,
}

/// Query params for `GET /api/v1/ssg/builds/inspect`. `build_id`
/// must be supplied explicitly — there's no implicit "latest"
/// here, since the SHARED SERVICE GROUPS list always knows the id
/// of the row the user clicked.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgBuildInspectParams {
    pub project: String,
    pub build_id: String,
}

/// One service from the SSG manifest, surfaced for the detail
/// page. Includes `auto_create_db` since that's a non-obvious
/// flag the manifest carries.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildInspectService {
    pub name: String,
    pub image: String,
    pub ports: Vec<u16>,
    pub env_keys: Vec<String>,
    pub volumes: Vec<String>,
    pub auto_create_db: bool,
}

/// Phase 33: one declared `[secrets.<name>]` injection target,
/// surfaced for the SSG Secrets tab. Mirrors
/// [`coast_ssg::build::artifact::SsgManifestSecretInject`] —
/// values are NOT included (they live encrypted in the keystore;
/// only names + inject targets are safe to surface in the UI).
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildInspectSecretInject {
    pub secret_name: String,
    /// `"env"` or `"file"`.
    pub inject_type: String,
    /// Env var name or absolute container path.
    pub inject_target: String,
    pub services: Vec<String>,
}

/// Response payload for `GET /api/v1/ssg/builds/inspect`. Combines
/// the manifest contents with the on-disk `ssg-coastfile.toml` +
/// `compose.yml` so the detail page can render everything in one
/// fetch (no N+1).
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildInspectResponse {
    pub project: String,
    pub build_id: String,
    pub coastfile_hash: String,
    /// RFC3339 string from the manifest `built_at` field. Empty
    /// when the manifest predates that field.
    pub built_at: String,
    /// Same as `built_at` but as Unix epoch seconds for clients
    /// that prefer arithmetic over string parsing. `0` when the
    /// timestamp is missing or unparseable.
    pub built_at_unix: i64,
    /// Absolute path of the build directory (for display).
    pub artifact_path: String,
    pub services: Vec<SsgBuildInspectService>,
    /// Phase 33: declared `[secrets.<name>]` injection targets.
    /// Empty when the manifest predates Phase 33 or the Coastfile
    /// has no `[secrets]` block. Values are deliberately NOT
    /// included — they live encrypted in the keystore.
    #[serde(default)]
    pub secret_injects: Vec<SsgBuildInspectSecretInject>,
    /// Raw `ssg-coastfile.toml` from the artifact dir. `None`
    /// when the file is missing on disk (older builds).
    pub coastfile: Option<String>,
    /// Raw `compose.yml` from the artifact dir. `None` when the
    /// file is missing on disk.
    pub compose: Option<String>,
    /// `true` if this is the project's `latest_build_id`.
    pub latest: bool,
    /// `true` if this is the project's currently-pinned SSG build.
    pub pinned: bool,
}

/// `GET /api/v1/ssg/builds?project=<p>` — list SSG build artifacts for
/// `project`. Returns `200 { project, builds: [] }` for projects that
/// have no SSG builds yet (matching the daemon-side handler's
/// no-error semantics for missing directories).
async fn ssg_builds_ls(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgBuildsLsParams>,
) -> Result<Json<SsgBuildsLsHttpResponse>, (StatusCode, Json<serde_json::Value>)> {
    let project = params.project.clone();
    if project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project'"
            })),
        ));
    }

    let req = SsgRequest {
        project: project.clone(),
        action: SsgAction::BuildsLs,
    };

    match handlers::ssg::handle(state, req).await {
        Ok(resp) => Ok(Json(SsgBuildsLsHttpResponse {
            project,
            builds: resp.builds,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

/// `GET /api/v1/ssg/builds/inspect?project=<p>&build_id=<id>` —
/// return the artifact's manifest contents + raw
/// `ssg-coastfile.toml` + `compose.yml`. Backs the SPA's per-SSG-
/// build detail page.
async fn ssg_builds_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgBuildInspectParams>,
) -> Result<Json<SsgBuildInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let project = params.project.clone();
    let build_id = params.build_id.clone();
    if project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project'"
            })),
        ));
    }
    if build_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'build_id'"
            })),
        ));
    }

    let build_dir = ssg_build_dir(&build_id);
    let manifest_path = build_dir.join("manifest.json");

    let raw_manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!("SSG build '{build_id}' not found"),
                })),
            ));
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to read manifest: {e}"),
                })),
            ));
        }
    };

    let manifest: serde_json::Value = serde_json::from_str(&raw_manifest).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("malformed manifest.json: {e}"),
            })),
        )
    })?;

    let coastfile_hash = manifest
        .get("coastfile_hash")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let built_at = manifest
        .get("built_at")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let built_at_unix = chrono::DateTime::parse_from_rfc3339(&built_at)
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let services = manifest
        .get("services")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|svc| SsgBuildInspectService {
                    name: svc
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    image: svc
                        .get("image")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    ports: svc
                        .get("ports")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|p| p.as_u64().and_then(|n| u16::try_from(n).ok()))
                                .collect()
                        })
                        .unwrap_or_default(),
                    env_keys: svc
                        .get("env_keys")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|k| k.as_str().map(std::string::ToString::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                    volumes: svc
                        .get("volumes")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m.as_str().map(std::string::ToString::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                    auto_create_db: svc
                        .get("auto_create_db")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();

    let secret_injects = parse_secret_injects(&manifest);

    let coastfile = std::fs::read_to_string(build_dir.join("ssg-coastfile.toml")).ok();
    let compose = std::fs::read_to_string(build_dir.join("compose.yml")).ok();

    let (latest, pinned) = {
        let db = state.db.lock().await;
        use coast_ssg::state::SsgStateExt;
        let latest = db
            .get_ssg(&project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id)
            .as_deref()
            == Some(build_id.as_str());
        let pinned = db
            .get_ssg_consumer_pin(&project)
            .ok()
            .flatten()
            .map(|p| p.build_id)
            .as_deref()
            == Some(build_id.as_str());
        (latest, pinned)
    };

    Ok(Json(SsgBuildInspectResponse {
        project,
        build_id,
        coastfile_hash,
        built_at,
        built_at_unix,
        artifact_path: build_dir.to_string_lossy().into_owned(),
        services,
        secret_injects,
        coastfile,
        compose,
        latest,
        pinned,
    }))
}

/// Phase 33: extract `manifest.json`'s `secret_injects` array into the
/// HTTP response shape. Pure: no fallible I/O. Returns an empty vec
/// for manifests that predate Phase 33 (the field is absent).
fn parse_secret_injects(manifest: &serde_json::Value) -> Vec<SsgBuildInspectSecretInject> {
    let Some(arr) = manifest.get("secret_injects").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .map(|s| SsgBuildInspectSecretInject {
            secret_name: s
                .get("secret_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            inject_type: s
                .get("inject_type")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            inject_target: s
                .get("inject_target")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            services: s
                .get("services")
                .and_then(|v| v.as_array())
                .map(|svcs| {
                    svcs.iter()
                        .filter_map(|n| n.as_str().map(std::string::ToString::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// Resolve `~/.coast/ssg/builds/<build_id>/` for the running daemon.
fn ssg_build_dir(build_id: &str) -> std::path::PathBuf {
    crate::handlers::run::paths::active_coast_home()
        .join("ssg")
        .join("builds")
        .join(build_id)
}

/// Request body for `POST /api/v1/ssg/builds/rm`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgBuildsRmRequest {
    /// Project the builds belong to. Used to look up the consumer
    /// pin so we don't delete a build the project depends on. (We
    /// don't currently scope artifacts by project on disk —
    /// `coastfile_hash` is the project anchor — but pin-protection
    /// still needs the project name.)
    pub project: String,
    /// Build ids to remove. Empty list is a no-op.
    pub build_ids: Vec<String>,
}

/// Per-build result for a removal request. The endpoint always
/// returns `200 OK` with this structured outcome — partial failures
/// (one bad id among several good ones) don't poison the whole
/// batch.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildsRmResponse {
    pub project: String,
    /// Build ids whose artifact directory was removed.
    pub removed: Vec<String>,
    /// Build ids skipped because they're the project's currently-
    /// pinned SSG build. Renderers should surface these so users
    /// know the operation wasn't fully silent.
    pub skipped_pinned: Vec<String>,
    /// Build ids that failed to remove for other reasons (filesystem
    /// errors, missing artifact dir, etc.). The error string is
    /// human-readable.
    pub errors: Vec<SsgBuildsRmError>,
    /// `true` if the project's `latest_build_id` was removed and
    /// has been cleared (i.e., next `ssg run` won't try to use a
    /// deleted artifact). Renderers can show a hint about
    /// rebuilding.
    pub cleared_latest: bool,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgBuildsRmError {
    pub build_id: String,
    pub error: String,
}

/// `POST /api/v1/ssg/builds/rm` — remove SSG build artifacts.
///
/// For each `build_id`:
/// 1. If it's the project's consumer pin, skip with a note.
/// 2. Otherwise, `rm -rf ~/.coast/ssg/builds/<build_id>/`.
/// 3. If we removed the project's `latest_build_id`, clear it in
///    `state.db.ssg.<project>` so subsequent operations don't
///    reference a missing artifact.
async fn ssg_builds_rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgBuildsRmRequest>,
) -> Result<Json<SsgBuildsRmResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required field 'project'"
            })),
        ));
    }

    // Read pin + latest BEFORE the filesystem ops so we can decide
    // skip-pinned per-id and detect whether to clear `latest`.
    let (pinned_build_id, latest_build_id) = {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        let pinned = db
            .get_ssg_consumer_pin(&req.project)
            .ok()
            .flatten()
            .map(|p| p.build_id);
        let latest = db
            .get_ssg(&req.project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id);
        (pinned, latest)
    };

    let mut removed: Vec<String> = Vec::new();
    let mut skipped_pinned: Vec<String> = Vec::new();
    let mut errors: Vec<SsgBuildsRmError> = Vec::new();

    for build_id in &req.build_ids {
        if Some(build_id) == pinned_build_id.as_ref() {
            skipped_pinned.push(build_id.clone());
            continue;
        }
        let dir = ssg_build_dir(build_id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => removed.push(build_id.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already gone — treat as success so the operation
                // is idempotent.
                removed.push(build_id.clone());
            }
            Err(e) => errors.push(SsgBuildsRmError {
                build_id: build_id.clone(),
                error: format!("remove failed: {e}"),
            }),
        }
    }

    // If we removed the project's `latest_build_id`, clear it so
    // `ssg run` doesn't try to start a missing artifact. The `ssg`
    // row itself is preserved (other fields like container_id might
    // still be valid for a stopped-but-existing container).
    let cleared_latest = if latest_build_id
        .as_ref()
        .map(|id| removed.iter().any(|r| r == id))
        .unwrap_or(false)
    {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        db.clear_latest_build_id(&req.project).unwrap_or(false)
    } else {
        false
    };

    Ok(Json(SsgBuildsRmResponse {
        project: req.project,
        removed,
        skipped_pinned,
        errors,
        cleared_latest,
    }))
}

/// Query params for `GET /api/v1/ssg/state`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgStateParams {
    pub project: String,
}

/// Combined runtime view of a project's SSG. Backs the `/project/<p>/ssg`
/// SPA tab. Stitches together [`SsgAction::Ps`] (per-service runtime
/// info + container status) and [`SsgAction::Ports`] (canonical /
/// dynamic / virtual port mapping) plus the consumer pin so the SPA
/// can render everything in a single fetch.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgStateResponse {
    pub project: String,
    /// Human-readable summary message from `handle_ps` ("SSG is
    /// running with N service(s)" / "No SSG for project X" / etc.).
    pub message: String,
    /// SSG container status: `running`, `stopped`, `built`,
    /// `created`, etc. `None` when the project has no `ssg` row at
    /// all. Keep the field a string for forward-compat with new
    /// statuses (no `enum` constraint on the wire).
    pub status: Option<String>,
    /// `latest_build_id` if recorded. Empty when the project has
    /// never built.
    pub latest_build_id: Option<String>,
    /// Currently-pinned consumer build_id (if any).
    pub pinned_build_id: Option<String>,
    /// Per-service runtime info (one row per service from the
    /// active build's manifest).
    pub services: Vec<SsgServiceInfo>,
    /// Per-service port mapping. May be a different (potentially
    /// shorter) length than `services` when the SSG hasn't run yet
    /// — `Ports` returns whatever rows exist in `ssg_services`.
    pub ports: Vec<SsgPortInfo>,
}

/// `GET /api/v1/ssg/state?project=<p>` — combined Ps + Ports view.
async fn ssg_state(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgStateParams>,
) -> Result<Json<SsgStateResponse>, (StatusCode, Json<serde_json::Value>)> {
    let project = params.project.clone();
    if project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project'"
            })),
        ));
    }

    let ps = handlers::ssg::handle(
        Arc::clone(&state),
        SsgRequest {
            project: project.clone(),
            action: SsgAction::Ps,
        },
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    let ports = handlers::ssg::handle(
        Arc::clone(&state),
        SsgRequest {
            project: project.clone(),
            action: SsgAction::Ports,
        },
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
    })?;

    let (latest_build_id, pinned_build_id) = {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        let latest = db
            .get_ssg(&project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id);
        let pinned = db
            .get_ssg_consumer_pin(&project)
            .ok()
            .flatten()
            .map(|p| p.build_id);
        (latest, pinned)
    };

    Ok(Json(SsgStateResponse {
        project,
        message: ps.message,
        status: ps.status,
        latest_build_id,
        pinned_build_id,
        services: ps.services,
        ports: ports.ports,
    }))
}

// -----------------------------------------------------------------------------
// /ssg/images, /ssg/volumes — surfaces the inner Docker daemon's images +
// volumes by `docker exec`-ing into the outer DinD container. Backs the
// SSG SPA's Images + Volumes tabs.
// -----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgImagesParams {
    pub project: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgVolumesParams {
    pub project: String,
}

/// One inner-DinD image, parsed from `docker image ls --format '{{json .}}'`.
/// Field names match docker's JSON output verbatim where possible
/// (`Repository`/`Tag`/`ID`/`Size`/`CreatedSince`) but we re-cast
/// to snake_case for TS clients.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgImageInfo {
    pub repository: String,
    pub tag: String,
    pub id: String,
    /// Human-readable size string from `docker image ls` (e.g.
    /// "445MB"). Docker doesn't give us a stable byte count via
    /// `--format '{{json .}}'`, so we keep the textual form.
    pub size: String,
    /// Human-readable created-relative string (e.g. "3 weeks ago").
    pub created: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgVolumeInfo {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub scope: String,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgImagesHttpResponse {
    pub project: String,
    pub images: Vec<SsgImageInfo>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct SsgVolumesHttpResponse {
    pub project: String,
    pub volumes: Vec<SsgVolumeInfo>,
}

/// Run a command inside the outer DinD via bollard exec, capturing
/// the combined stdout output as a `String`. Used by `/ssg/images`
/// and `/ssg/volumes` to issue one-shot `docker image ls` /
/// `docker volume ls` calls.
async fn exec_in_ssg_container_capture_stdout(
    state: &AppState,
    container_id: &str,
    cmd: Vec<&str>,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
    use futures_util::StreamExt;

    let docker = state.docker.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Docker is unavailable on the host daemon",
            })),
        )
    })?;

    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(
                    cmd.into_iter()
                        .map(std::string::ToString::to_string)
                        .collect(),
                ),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("create_exec failed: {e}"),
                })),
            )
        })?;

    let started = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("start_exec failed: {e}"),
                })),
            )
        })?;

    let mut stdout = String::new();
    if let StartExecResults::Attached { mut output, .. } = started {
        while let Some(chunk) = output.next().await {
            if let Ok(bollard::container::LogOutput::StdOut { message }) = chunk {
                stdout.push_str(&String::from_utf8_lossy(&message));
            }
        }
    }
    Ok(stdout)
}

/// Tolerant case-insensitive lookup. Docker's JSON keys are
/// PascalCase (`Repository`, `Tag`, `ID`, `Size`, `CreatedSince`,
/// `Scope`, etc.); we accept either to stay forward-compatible
/// with future docker versions.
fn json_str_ci(v: &serde_json::Value, key: &str) -> String {
    let lower = key.to_lowercase();
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            if k.to_lowercase() == lower {
                if let Some(s) = val.as_str() {
                    return s.to_string();
                }
            }
        }
    }
    String::new()
}

async fn ssg_images(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgImagesParams>,
) -> Result<Json<SsgImagesHttpResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;

    let raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec!["docker", "image", "ls", "--format", "{{json .}}"],
    )
    .await?;

    let mut images: Vec<SsgImageInfo> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        images.push(SsgImageInfo {
            repository: json_str_ci(&entry, "Repository"),
            tag: json_str_ci(&entry, "Tag"),
            id: json_str_ci(&entry, "ID"),
            size: json_str_ci(&entry, "Size"),
            created: json_str_ci(&entry, "CreatedSince"),
        });
    }

    Ok(Json(SsgImagesHttpResponse {
        project: params.project,
        images,
    }))
}

async fn ssg_volumes(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgVolumesParams>,
) -> Result<Json<SsgVolumesHttpResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;

    let raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec!["docker", "volume", "ls", "--format", "{{json .}}"],
    )
    .await?;

    let mut volumes: Vec<SsgVolumeInfo> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        volumes.push(SsgVolumeInfo {
            name: json_str_ci(&entry, "Name"),
            driver: json_str_ci(&entry, "Driver"),
            mountpoint: json_str_ci(&entry, "Mountpoint"),
            scope: json_str_ci(&entry, "Scope"),
        });
    }

    Ok(Json(SsgVolumesHttpResponse {
        project: params.project,
        volumes,
    }))
}

// -----------------------------------------------------------------------------
// /ssg/images/inspect, /ssg/volumes/inspect — runs `docker inspect` /
// `docker volume inspect` inside the SSG outer DinD. Same response
// shape as the per-instance variants in `query/images.rs` /
// `query/volumes.rs` so the SPA's `ImageDetailPage` /
// `VolumeDetailPage` can be reused 1:1 for SSG by just routing them
// at a different URL.
// -----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgImageInspectParams {
    pub project: String,
    pub image: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgVolumeInspectParams {
    pub project: String,
    pub volume: String,
}

async fn ssg_image_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgImageInspectParams>,
) -> Result<Json<ImageInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;

    let inspect_raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec!["docker", "inspect", &params.image],
    )
    .await?;

    let inspect: serde_json::Value = serde_json::from_str(inspect_raw.trim()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to parse inspect output: {e}")
            })),
        )
    })?;

    // List every container (including stopped) that uses this
    // image. Mirrors the per-instance handler so the SPA can show
    // "Used by Services" rows.
    let ancestor_filter = format!("ancestor={}", params.image);
    let containers_raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec![
            "docker",
            "ps",
            "-a",
            "--filter",
            &ancestor_filter,
            "--format",
            "{{json .}}",
        ],
    )
    .await
    .unwrap_or_default();

    let mut containers: Vec<serde_json::Value> = Vec::new();
    for line in containers_raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            containers.push(val);
        }
    }

    Ok(Json(ImageInspectResponse {
        inspect,
        containers,
    }))
}

async fn ssg_volume_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgVolumeInspectParams>,
) -> Result<Json<VolumeInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    let cid = resolve_ssg_container_id(&state, &params.project).await?;

    let inspect_raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec!["docker", "volume", "inspect", &params.volume],
    )
    .await?;

    let inspect: serde_json::Value = serde_json::from_str(inspect_raw.trim()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to parse volume inspect: {e}")
            })),
        )
    })?;

    let volume_filter = format!("volume={}", params.volume);
    let containers_raw = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec![
            "docker",
            "ps",
            "-a",
            "--filter",
            &volume_filter,
            "--format",
            "{{json .}}",
        ],
    )
    .await
    .unwrap_or_default();

    let mut containers: Vec<serde_json::Value> = Vec::new();
    for line in containers_raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            containers.push(val);
        }
    }

    // Walk the SSG Coastfile (`Coastfile.shared_service_groups`) of
    // the active build to find a `[shared_services.<svc>] volumes
    // = ["<vol>:<mount>"]` declaration that owns this volume. The
    // SPA's `VolumeDetailPage` renders a "Configuration" section
    // when this is non-null. Compose prefixes named volumes with
    // the project name (e.g. `cg-ssg_cg_postgres_data`); the
    // `com.docker.compose.volume` label exposes the un-prefixed
    // form, which is what the Coastfile declares.
    let compose_volume_label = inspect
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("Labels"))
        .and_then(|l| l.get("com.docker.compose.volume"))
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string);
    let lookup_name = compose_volume_label
        .as_deref()
        .unwrap_or(params.volume.as_str());
    let coastfile = ssg_coastfile_volume_config(&state, &params.project, lookup_name).await;

    Ok(Json(VolumeInspectResponse {
        inspect,
        containers,
        coastfile,
    }))
}

/// Look up the `[shared_services.<svc>]` declaration that owns
/// `volume_name` in the project's active SSG Coastfile, returning
/// the same shape the per-instance volume-inspect handler emits
/// (`{name, strategy, service, mount, snapshot_source}`). Best-
/// effort: returns `None` on any error so the SPA degrades to the
/// "not configured" state cleanly. Strategy is always reported as
/// `shared` for SSG-owned volumes since they're singletons keyed
/// by the project.
async fn ssg_coastfile_volume_config(
    state: &AppState,
    project: &str,
    volume_name: &str,
) -> Option<serde_json::Value> {
    use coast_ssg::state::SsgStateExt;

    let build_id = {
        let db = state.db.lock().await;
        let pin = db
            .get_ssg_consumer_pin(project)
            .ok()
            .flatten()
            .map(|p| p.build_id);
        let latest = db
            .get_ssg(project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id);
        pin.or(latest)?
    };
    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id).ok()?;
    let cf_path = build_dir.join("ssg-coastfile.toml");
    let cf = coast_ssg::coastfile::SsgCoastfile::from_file(&cf_path).ok()?;

    for svc in &cf.services {
        for vol in &svc.volumes {
            if let coast_ssg::coastfile::SsgVolumeEntry::InnerNamedVolume {
                name,
                container_path,
            } = vol
            {
                if name == volume_name {
                    return Some(serde_json::json!({
                        "name": name,
                        "strategy": "shared",
                        "service": svc.name,
                        "mount": container_path.display().to_string(),
                        "snapshot_source": serde_json::Value::Null,
                    }));
                }
            }
        }
    }
    None
}

// -----------------------------------------------------------------------------
// Lifecycle: /ssg/{run,start,stop,restart,rm} — non-streaming POST wrappers
// around the same daemon orchestration the CLI socket protocol uses.
// Backs the SSG list-panel toolbar buttons (Start/Stop/Remove).
// -----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgLifecycleRequest {
    pub project: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgStopRequest {
    pub project: String,
    /// Tear down remote-shadow tunnels first instead of refusing
    /// when other instances reference the SSG. See
    /// `coast-ssg/DESIGN.md §20.6`.
    #[serde(default)]
    pub force: bool,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgRmRequest {
    pub project: String,
    /// Remove the SSG's inner named volumes (postgres WAL etc.)
    /// in addition to the container itself. Bind-mount data on
    /// the host is unaffected.
    #[serde(default)]
    pub with_data: bool,
    /// Same shadow-coast bypass as `SsgStopRequest::force`.
    #[serde(default)]
    pub force: bool,
}

/// `POST /api/v1/ssg/run` — drive the SSG's `Run` lifecycle verb
/// inline (no SSE) with progress events discarded. Consumers
/// poll `/ssg/state` for the post-run status. The orchestration
/// (build_id resolution, ssg_mutex, run_ssg_with_build_id, state
/// apply, host-socat refresh) mirrors `server.rs::run_streaming_run`
/// minus the streaming machinery.
async fn ssg_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgLifecycleRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    use coast_core::protocol::BuildProgressEvent;
    use coast_ssg::state::SsgStateExt;
    use tokio::sync::mpsc;

    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }

    let _operation_guard = state
        .begin_update_operation(
            crate::server::UpdateOperationKind::Build,
            Some(&req.project),
            None,
        )
        .map_err(|e| map_coast_err(&e))?;
    let _ssg_lock = state.ssg_mutex.lock().await;

    let resolved_build_id = {
        let db = state.db.lock().await;
        let pin = db
            .get_ssg_consumer_pin(&req.project)
            .ok()
            .flatten()
            .map(|p| p.build_id);
        let latest = db
            .get_ssg(&req.project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id);
        pin.or(latest)
    };
    let Some(build_id) = resolved_build_id else {
        return Err((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!(
                    "no SSG build for project '{}'. Run `coast ssg build` first.",
                    req.project,
                ),
            })),
        ));
    };

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Docker is unavailable on the host daemon",
                })),
            )
        })?
        .clone();

    let (tx, mut rx) = mpsc::channel::<BuildProgressEvent>(64);
    // Drain progress events so the daemon_integration sender doesn't
    // back-pressure on an unread channel. Drop the rx after the
    // run completes.
    let drain = tokio::spawn(async move {
        while rx.recv().await.is_some() {
            // discard
        }
    });

    let outcome = {
        let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker);
        coast_ssg::runtime::lifecycle::run_ssg_with_build_id(
            &req.project,
            &ops,
            Some(build_id.as_str()),
            tx,
        )
        .await
        .map_err(|e| map_coast_err(&e))?
    };
    drain.abort();

    let mut resp = {
        let db = state.db.lock().await;
        outcome
            .apply_to_state_and_response(
                &req.project,
                &*db,
                "running",
                format!("SSG running on build {build_id}"),
            )
            .map_err(|e| map_coast_err(&e))?
    };
    crate::handlers::run::ssg_integration::refresh_host_socats_for_project(&req.project, &state)
        .await;
    resp.message = format!("SSG running on build {build_id}");
    Ok(Json(resp))
}

/// `POST /api/v1/ssg/start` — start a previously-stopped SSG.
/// Like `ssg_run` but routes through `start_ssg` which preserves
/// the existing container-id and dynamic-port assignments.
async fn ssg_start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgLifecycleRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    use coast_core::protocol::BuildProgressEvent;
    use coast_ssg::state::SsgStateExt;
    use tokio::sync::mpsc;

    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let _operation_guard = state
        .begin_update_operation(
            crate::server::UpdateOperationKind::Build,
            Some(&req.project),
            None,
        )
        .map_err(|e| map_coast_err(&e))?;
    let _ssg_lock = state.ssg_mutex.lock().await;

    let (record, plans) = {
        let db = state.db.lock().await;
        let Some(record) = db.get_ssg(&req.project).ok().flatten() else {
            return Err((
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": format!(
                        "SSG for project '{}' has not been created. \
                         Use 'Run' instead of 'Start' for a fresh SSG.",
                        req.project,
                    ),
                })),
            ));
        };
        let services = db
            .list_ssg_services(&req.project)
            .map_err(|e| map_coast_err(&e))?;
        let plans: Vec<coast_ssg::runtime::ports::SsgServicePortPlan> = services
            .into_iter()
            .map(|s| coast_ssg::runtime::ports::SsgServicePortPlan {
                service: s.service_name,
                container_port: s.container_port,
                dynamic_host_port: s.dynamic_host_port,
            })
            .collect();
        (record, plans)
    };

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Docker is unavailable on the host daemon",
                })),
            )
        })?
        .clone();

    let (tx, mut rx) = mpsc::channel::<BuildProgressEvent>(64);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let outcome = {
        let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker);
        coast_ssg::runtime::lifecycle::start_ssg(&ops, &record, plans, tx)
            .await
            .map_err(|e| map_coast_err(&e))?
    };
    drain.abort();

    let resp = {
        let db = state.db.lock().await;
        outcome
            .apply_to_state_and_response(
                &req.project,
                &*db,
                format!("SSG started on build {}", outcome.build_id),
            )
            .map_err(|e| map_coast_err(&e))?
    };
    crate::handlers::run::ssg_integration::refresh_host_socats_for_project(&req.project, &state)
        .await;
    Ok(Json(resp))
}

/// `POST /api/v1/ssg/stop` — non-streaming dispatch to the
/// existing `SsgAction::Stop` handler.
async fn ssg_stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgStopRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let resp = handlers::ssg::handle(
        Arc::clone(&state),
        coast_core::protocol::SsgRequest {
            project: req.project,
            action: coast_core::protocol::SsgAction::Stop { force: req.force },
        },
    )
    .await
    .map_err(|e| map_coast_err(&e))?;
    Ok(Json(resp))
}

/// `POST /api/v1/ssg/rm` — non-streaming dispatch to the existing
/// `SsgAction::Rm` handler.
async fn ssg_rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgRmRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let resp = handlers::ssg::handle(
        Arc::clone(&state),
        coast_core::protocol::SsgRequest {
            project: req.project,
            action: coast_core::protocol::SsgAction::Rm {
                with_data: req.with_data,
                force: req.force,
            },
        },
    )
    .await
    .map_err(|e| map_coast_err(&e))?;
    Ok(Json(resp))
}

// -----------------------------------------------------------------------------
// /ssg/{restart-services,checkout,uncheckout} — non-streaming POST
// wrappers over the existing daemon orchestration. Back the SSG
// header action buttons (Restart Services / Stop / Start /
// Checkout / Uncheckout) on the SPA, mirroring the per-instance
// header on `InstanceDetailPage`.
// -----------------------------------------------------------------------------

/// `POST /api/v1/ssg/services/restart-all` — restart every inner
/// compose service inside the SSG outer DinD without bouncing the
/// outer DinD itself. Mirrors `useRestartServicesMutation` for
/// regular instances. Implementation: `docker compose -f
/// /coast-artifact/compose.yml -p <project>-ssg restart` exec'd
/// inside the outer DinD via `exec_in_ssg_container_capture_stdout`.
/// We bypass the per-service `SsgDockerOps::inner_compose_service_action`
/// trait method here because that path requires a service name
/// argument; the no-arg form restarts every service in the inner
/// compose stack in one round trip.
async fn ssg_restart_services(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgLifecycleRequest>,
) -> Result<Json<SsgServiceActionResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let cid = resolve_ssg_container_id(&state, &req.project).await?;
    let compose_project = format!("{}-ssg", req.project);

    let _ = exec_in_ssg_container_capture_stdout(
        &state,
        &cid,
        vec![
            "docker",
            "compose",
            "-f",
            "/coast-artifact/compose.yml",
            "-p",
            &compose_project,
            "restart",
        ],
    )
    .await?;

    Ok(Json(SsgServiceActionResponse {
        project: req.project.clone(),
        service: String::new(),
        verb: "restart-all".to_string(),
        message: format!("Restarted all SSG services for '{}'.", req.project),
    }))
}

/// `POST /api/v1/ssg/checkout` — bind every SSG service's
/// canonical port on the host. Toggle counterpart of
/// `/ssg/uncheckout`. Maps to `SsgAction::Checkout {service:
/// None, all: true}`.
async fn ssg_checkout_all(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgLifecycleRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let resp = handlers::ssg::handle(
        Arc::clone(&state),
        coast_core::protocol::SsgRequest {
            project: req.project,
            action: coast_core::protocol::SsgAction::Checkout {
                service: None,
                all: true,
            },
        },
    )
    .await
    .map_err(|e| map_coast_err(&e))?;
    Ok(Json(resp))
}

/// `POST /api/v1/ssg/uncheckout` — release every SSG service's
/// canonical-port binding on the host. Toggle counterpart of
/// `/ssg/checkout`. Maps to `SsgAction::Uncheckout {service:
/// None, all: true}`.
async fn ssg_uncheckout_all(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgLifecycleRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let resp = handlers::ssg::handle(
        Arc::clone(&state),
        coast_core::protocol::SsgRequest {
            project: req.project,
            action: coast_core::protocol::SsgAction::Uncheckout {
                service: None,
                all: true,
            },
        },
    )
    .await
    .map_err(|e| map_coast_err(&e))?;
    Ok(Json(resp))
}

/// Phase 33: query params for `GET /api/v1/ssg/secrets`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSecretsListParams {
    pub project: String,
}

/// Phase 33: query params for `GET /api/v1/ssg/secrets/reveal`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSecretsRevealParams {
    pub project: String,
    pub secret: String,
}

/// Phase 33: request body for `POST /api/v1/ssg/secrets/override`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSecretsOverrideRequest {
    pub project: String,
    pub name: String,
    pub value: String,
}

/// Phase 33: request body for `POST /api/v1/ssg/secrets/clear`.
#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgSecretsClearRequest {
    pub project: String,
}

/// Sentinel keystore namespace for SSG secret overrides set via the
/// SPA. Run-time materialize prefers overrides over base values, the
/// same way regular instance overrides take precedence over base
/// project secrets. See `coast-ssg/DESIGN.md §33`.
fn ssg_secrets_override_image_key(project: &str) -> String {
    format!("ssg:{project}/override")
}

/// `GET /api/v1/ssg/secrets?project=<p>` — list every secret known
/// to the SSG keystore for `project`. Mirrors the regular
/// `/api/v1/secrets` endpoint shape so the SPA's
/// `InstanceSecretsTab`-shaped UI can render it identically.
///
/// Walks two keystore namespaces:
///   - `ssg:<project>` — base values written by `coast ssg build`'s
///     extract step.
///   - `ssg:<project>/override` — values set via the SPA's
///     "Override" button (Phase 33).
///
/// Same merge policy as `merge_secrets` in
/// `coast-daemon/src/handlers/secret.rs`: an override REPLACES the
/// base entry by name and is flagged `is_override = true`.
async fn ssg_secrets_ls(
    Query(params): Query<SsgSecretsListParams>,
) -> Result<Json<Vec<SecretInfo>>, (StatusCode, Json<serde_json::Value>)> {
    if params.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required query param 'project'" })),
        ));
    }
    Ok(Json(load_ssg_secret_info(&params.project)))
}

/// `GET /api/v1/ssg/secrets/reveal?project=<p>&secret=<name>` —
/// returns the decrypted plaintext value for a single secret.
/// Override row wins over base row when both exist.
async fn ssg_secrets_reveal(
    Query(params): Query<SsgSecretsRevealParams>,
) -> Result<Json<RevealSecretResponse>, (StatusCode, Json<serde_json::Value>)> {
    if params.project.is_empty() || params.secret.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project' or 'secret'"
            })),
        ));
    }

    let keystore = open_ssg_keystore_or_404()?;
    let override_key = ssg_secrets_override_image_key(&params.project);
    let base_key = coast_ssg::build::keystore_image_key(&params.project);

    // Override wins.
    let stored = keystore
        .get_secret(&override_key, &params.secret)
        .map_err(internal_error)?
        .or_else(|| {
            keystore
                .get_secret(&base_key, &params.secret)
                .ok()
                .flatten()
        });

    match stored {
        Some(s) => Ok(Json(RevealSecretResponse {
            name: params.secret,
            value: String::from_utf8_lossy(&s.value).into_owned(),
        })),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": format!("Secret '{}' not found for SSG project '{}'", params.secret, params.project)
            })),
        )),
    }
}

/// `POST /api/v1/ssg/secrets/override` — write a single
/// user-supplied value into the override namespace. Subsequent
/// `coast ssg run` / `start` will inject this value (the
/// run-time `materialize_secrets` path prefers overrides over
/// base values). Persists across `coast ssg build` since rebuild
/// only resets the base namespace.
async fn ssg_secrets_override(
    Json(req): Json<SsgSecretsOverrideRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() || req.name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required field 'project' or 'name'"
            })),
        ));
    }

    let keystore = open_ssg_keystore_or_404()?;
    let override_key = ssg_secrets_override_image_key(&req.project);
    let base_key = coast_ssg::build::keystore_image_key(&req.project);

    // Carry the original extractor + inject metadata forward so the
    // override row matches the base row's shape (the run-time
    // materializer reads `inject_type` + `inject_target` to render
    // the compose override). If no base row exists, the override
    // can't be meaningfully injected — reject.
    let base = keystore
        .get_secret(&base_key, &req.name)
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": format!(
                        "Secret '{}' is not declared in the SSG manifest for project '{}'. \
                         Add it to `Coastfile.shared_service_groups` and run `coast ssg build` first.",
                        req.name, req.project
                    )
                })),
            )
        })?;

    keystore
        .store_secret(&coast_secrets::keystore::StoreSecretParams {
            coast_image: &override_key,
            secret_name: &req.name,
            value: req.value.as_bytes(),
            inject_type: &base.inject_type,
            inject_target: &base.inject_target,
            extractor: &base.extractor,
            ttl_seconds: None,
        })
        .map_err(internal_error)?;

    Ok(Json(serde_json::json!({
        "message": format!(
            "Override stored for SSG secret '{}' (project '{}'). Restart the SSG to apply.",
            req.name, req.project
        )
    })))
}

/// Open the global keystore. Returns 404 when the keystore file
/// doesn't exist (no SSG has ever been built — the LIST endpoint
/// degrades to an empty list, but reveal/override demand an
/// existing keystore).
fn open_ssg_keystore_or_404(
) -> Result<coast_secrets::keystore::Keystore, (StatusCode, Json<serde_json::Value>)> {
    let home = coast_core::artifact::coast_home().map_err(internal_error)?;
    let db_path = home.join("keystore.db");
    let key_path = home.join("keystore.key");
    if !db_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "no SSG keystore found. Run `coast ssg build` first."
            })),
        ));
    }
    coast_secrets::keystore::Keystore::open(&db_path, &key_path).map_err(internal_error)
}

/// Load SSG secret info for the LIST endpoint. Pure: returns an
/// empty vec on any error (missing keystore, decrypt failure,
/// etc.) so the SPA renders an empty table instead of a hard
/// failure.
fn load_ssg_secret_info(project: &str) -> Vec<SecretInfo> {
    let Ok(home) = coast_core::artifact::coast_home() else {
        return Vec::new();
    };
    let db_path = home.join("keystore.db");
    let key_path = home.join("keystore.key");
    if !db_path.exists() {
        return Vec::new();
    }
    let Ok(keystore) = coast_secrets::keystore::Keystore::open(&db_path, &key_path) else {
        return Vec::new();
    };

    let base_key = coast_ssg::build::keystore_image_key(project);
    let override_key = ssg_secrets_override_image_key(project);
    let base = keystore.get_all_secrets(&base_key).unwrap_or_default();
    let overrides = keystore.get_all_secrets(&override_key).unwrap_or_default();

    let mut out: Vec<SecretInfo> = base
        .into_iter()
        .map(|s| SecretInfo {
            name: s.secret_name,
            extractor: s.extractor,
            inject: format!("{}:{}", s.inject_type, s.inject_target),
            is_override: false,
        })
        .collect();
    // Overrides REPLACE the base entry by name; mirrors the merge
    // policy in `handlers::secret::merge_secrets`.
    for ov in overrides {
        out.retain(|existing| existing.name != ov.secret_name);
        out.push(SecretInfo {
            name: ov.secret_name,
            extractor: ov.extractor,
            inject: format!("{}:{}", ov.inject_type, ov.inject_target),
            is_override: true,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn internal_error<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": e.to_string() })),
    )
}

/// `POST /api/v1/ssg/secrets/clear` — drop every keystore entry
/// whose `coast_image == "ssg:<project>"`. Idempotent. See
/// `coast-ssg/DESIGN.md §33`. Backs the SsgSecretsTab "Clear
/// secrets" button on the SPA.
async fn ssg_secrets_clear(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgSecretsClearRequest>,
) -> Result<Json<coast_core::protocol::SsgResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    let resp = handlers::ssg::handle(
        Arc::clone(&state),
        coast_core::protocol::SsgRequest {
            project: req.project,
            action: coast_core::protocol::SsgAction::SecretsClear,
        },
    )
    .await
    .map_err(|e| map_coast_err(&e))?;
    Ok(Json(resp))
}

// -----------------------------------------------------------------------------
// Per-service inner-compose control: /ssg/services/{stop,start,restart,rm}
// Backs the toolbar buttons on the SSG → Services tab. Each endpoint
// resolves the SSG outer-DinD container id from `state.db`, then runs
// `docker exec <ssg> docker compose -f /coast-artifact/compose.yml \
//  -p <project>-ssg <verb> <service>` inside it.
// -----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceActionRequest {
    pub project: String,
    pub service: String,
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceActionResponse {
    pub project: String,
    pub service: String,
    pub verb: String,
    pub message: String,
}

/// Run a single compose verb against one inner SSG service. Shared
/// implementation behind the four endpoints so the route handlers
/// stay slim.
async fn run_inner_service_action(
    state: &AppState,
    project: &str,
    service: &str,
    verb: &'static str,
) -> Result<SsgServiceActionResponse, (StatusCode, Json<serde_json::Value>)> {
    if project.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'project'" })),
        ));
    }
    if service.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing required field 'service'" })),
        ));
    }

    let container_id = resolve_ssg_container_id(state, project).await?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Docker is unavailable on the host daemon",
                })),
            )
        })?
        .clone();

    use coast_ssg::docker_ops::SsgDockerOps;
    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker);
    let compose_path = coast_ssg::runtime::lifecycle::inner_compose_path();
    let compose_project = coast_ssg::runtime::lifecycle::ssg_compose_project(project);
    ops.inner_compose_service_action(
        &container_id,
        &compose_path,
        &compose_project,
        verb,
        service,
    )
    .await
    .map_err(|e| map_coast_err(&e))?;

    Ok(SsgServiceActionResponse {
        project: project.to_string(),
        service: service.to_string(),
        verb: verb.to_string(),
        message: format!("Service '{service}' {verb} ok"),
    })
}

async fn ssg_service_stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgServiceActionRequest>,
) -> Result<Json<SsgServiceActionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resp = run_inner_service_action(&state, &req.project, &req.service, "stop").await?;
    Ok(Json(resp))
}

async fn ssg_service_start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgServiceActionRequest>,
) -> Result<Json<SsgServiceActionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resp = run_inner_service_action(&state, &req.project, &req.service, "start").await?;
    Ok(Json(resp))
}

async fn ssg_service_restart(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgServiceActionRequest>,
) -> Result<Json<SsgServiceActionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resp = run_inner_service_action(&state, &req.project, &req.service, "restart").await?;
    Ok(Json(resp))
}

async fn ssg_service_rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SsgServiceActionRequest>,
) -> Result<Json<SsgServiceActionResponse>, (StatusCode, Json<serde_json::Value>)> {
    let resp = run_inner_service_action(&state, &req.project, &req.service, "rm").await?;
    Ok(Json(resp))
}

// -----------------------------------------------------------------------------
// /ssg/services/inspect — runs `docker inspect <inner_container>` inside
// the SSG outer DinD. Same response shape as the per-instance
// `/api/v1/service/inspect` so the SPA's per-service detail page can
// reuse the existing `ServiceInspectTab` rendering helpers.
// -----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct SsgServiceInspectParams {
    pub project: String,
    pub service: String,
}

/// Resolve the inner compose service's container name (e.g.
/// `cg-ssg-postgres-1`) by running `docker compose ps --format json
/// <service>` inside the outer DinD and pulling the `Name` field.
/// Returns 404 when the inner container is missing (service is
/// stopped or removed) so the SPA can render a friendly empty
/// state.
pub(crate) async fn resolve_ssg_inner_container_name(
    state: &AppState,
    project: &str,
    container_id: &str,
    service: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let compose_project = format!("{project}-ssg");
    let raw = exec_in_ssg_container_capture_stdout(
        state,
        container_id,
        vec![
            "docker",
            "compose",
            "-f",
            "/coast-artifact/compose.yml",
            "-p",
            &compose_project,
            "ps",
            "--format",
            "json",
            service,
        ],
    )
    .await?;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        if let Some(name) = entry
            .get("Name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Ok(name.to_string());
        }
        // Fallback: docker compose ps may emit the field as `name`
        // (lowercase) on some versions.
        if let Some(name) = entry
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Ok(name.to_string());
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": format!(
                "inner service '{service}' not found in SSG '{project}-ssg' (is it running?)"
            ),
        })),
    ))
}

async fn ssg_service_inspect(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsgServiceInspectParams>,
) -> Result<Json<ServiceInspectResponse>, (StatusCode, Json<serde_json::Value>)> {
    if params.project.is_empty() || params.service.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing required query param 'project' or 'service'",
            })),
        ));
    }

    let cid = resolve_ssg_container_id(&state, &params.project).await?;
    let inner_name =
        resolve_ssg_inner_container_name(&state, &params.project, &cid, &params.service).await?;

    let raw =
        exec_in_ssg_container_capture_stdout(&state, &cid, vec!["docker", "inspect", &inner_name])
            .await?;

    let inspect: serde_json::Value = serde_json::from_str(raw.trim()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to parse service inspect output: {e}")
            })),
        )
    })?;

    Ok(Json(ServiceInspectResponse { inspect }))
}

/// Map a `CoastError` to the standard daemon HTTP error tuple.
fn map_coast_err(e: &coast_core::error::CoastError) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": e.to_string() })),
    )
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ssg/builds", get(ssg_builds_ls))
        .route("/ssg/builds/inspect", get(ssg_builds_inspect))
        .route("/ssg/builds/rm", post(ssg_builds_rm))
        .route("/ssg/state", get(ssg_state))
        .route("/ssg/images", get(ssg_images))
        .route("/ssg/images/inspect", get(ssg_image_inspect))
        .route("/ssg/volumes", get(ssg_volumes))
        .route("/ssg/volumes/inspect", get(ssg_volume_inspect))
        .route("/ssg/run", post(ssg_run))
        .route("/ssg/start", post(ssg_start))
        .route("/ssg/stop", post(ssg_stop))
        .route("/ssg/rm", post(ssg_rm))
        .route("/ssg/services/inspect", get(ssg_service_inspect))
        .route("/ssg/services/stop", post(ssg_service_stop))
        .route("/ssg/services/start", post(ssg_service_start))
        .route("/ssg/services/restart", post(ssg_service_restart))
        .route("/ssg/services/restart-all", post(ssg_restart_services))
        .route("/ssg/checkout", post(ssg_checkout_all))
        .route("/ssg/uncheckout", post(ssg_uncheckout_all))
        .route("/ssg/services/rm", post(ssg_service_rm))
        // Phase 33: SSG-native secrets. Build extracts; run injects;
        // clear drops the keystore namespaces `ssg:<project>` and
        // `ssg:<project>/override`. List/reveal/override mirror the
        // regular instance secret endpoints so the SPA's
        // SsgSecretsTab can render the same DataTable + reveal modal
        // as InstanceSecretsTab.
        .route("/ssg/secrets", get(ssg_secrets_ls))
        .route("/ssg/secrets/reveal", get(ssg_secrets_reveal))
        .route("/ssg/secrets/override", post(ssg_secrets_override))
        .route("/ssg/secrets/clear", post(ssg_secrets_clear))
}
