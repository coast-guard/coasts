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

use coast_core::protocol::{SsgAction, SsgBuildEntry, SsgPortInfo, SsgRequest, SsgServiceInfo};

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
        coastfile,
        compose,
        latest,
        pinned,
    }))
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
        .route("/ssg/volumes", get(ssg_volumes))
        .route("/ssg/run", post(ssg_run))
        .route("/ssg/start", post(ssg_start))
        .route("/ssg/stop", post(ssg_stop))
        .route("/ssg/rm", post(ssg_rm))
        .route("/ssg/services/stop", post(ssg_service_stop))
        .route("/ssg/services/start", post(ssg_service_start))
        .route("/ssg/services/restart", post(ssg_service_restart))
        .route("/ssg/services/rm", post(ssg_service_rm))
}
