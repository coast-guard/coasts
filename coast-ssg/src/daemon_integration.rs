//! Public hooks the `coast-daemon` crate calls into.
//!
//! Phase 2 landed `build_ssg` (streaming build orchestrator) and
//! `ps_ssg` (one-shot manifest reader). Phase 3 extends this module
//! with runtime verbs: `run_ssg`, `stop_ssg`, `start_ssg`,
//! `restart_ssg`, `rm_ssg`, `logs_ssg`, `exec_ssg`, `ports_ssg`.
//! Phase 3.5+ adds `ensure_ready_for_instance` and
//! `synthesize_shared_service_configs`.
//!
//! This is the *only* public surface `coast-daemon` consumes for SSG
//! runtime integration. Keeping the contract narrow here lets an
//! agent follow the daemon call graph from a single adapter file
//! (`coast-daemon/src/handlers/ssg.rs`) directly into this crate. See
//! `DESIGN.md §4.1` ("adapter-file pattern").

use std::path::{Path, PathBuf};

use tokio::sync::mpsc::Sender;

use coast_core::artifact;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, SsgResponse, SsgServiceInfo};

use crate::build::artifact as build_artifact;
use crate::build::images::pull_and_cache_ssg_images;
use crate::coastfile::SsgCoastfile;
use crate::paths;
use crate::runtime::compose_synth::synth_inner_compose;

/// Inputs for a `coast ssg build` request (mirrors
/// [`coast_core::protocol::SsgRequest::Build`]).
#[derive(Debug, Clone)]
pub struct SsgBuildInputs {
    pub file: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub config: Option<String>,
}

/// Total build steps emitted in the progress plan.
///
/// Fixed plan: parse (1) + resolve build id (2) + synth compose (3) +
/// write artifact (4) + flip latest (5) + prune (6), then one per
/// image.
fn total_steps(num_services: usize) -> u32 {
    6 + num_services as u32
}

/// Resolve the SSG Coastfile path from the request inputs.
///
/// Precedence:
/// 1. `config` (inline TOML) — parsed directly without reading disk.
/// 2. `file` — use as-is.
/// 3. `working_dir` — look for `Coastfile.shared_service_groups[.toml]`.
/// 4. cwd — same lookup.
fn load_ssg_coastfile(inputs: &SsgBuildInputs) -> Result<(SsgCoastfile, String)> {
    if let Some(ref inline) = inputs.config {
        let root = inputs
            .working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let cf = SsgCoastfile::parse(inline, &root)?;
        return Ok((cf, inline.clone()));
    }

    let path = if let Some(ref p) = inputs.file {
        p.clone()
    } else {
        let base = inputs
            .working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        find_coastfile_in(&base).ok_or_else(|| {
            CoastError::coastfile(format!(
                "no Coastfile.shared_service_groups found in '{}' (looked for both the plain and .toml forms)",
                base.display()
            ))
        })?
    };

    let raw = std::fs::read_to_string(&path).map_err(|e| CoastError::Io {
        message: format!("failed to read SSG Coastfile '{}': {e}", path.display()),
        path: path.clone(),
        source: Some(e),
    })?;
    let cf = SsgCoastfile::from_file(&path)?;
    Ok((cf, raw))
}

/// Look for `Coastfile.shared_service_groups.toml` then
/// `Coastfile.shared_service_groups` in `dir`. Matches the existing
/// "`.toml` variant wins" convention used by regular Coastfiles.
fn find_coastfile_in(dir: &Path) -> Option<PathBuf> {
    for name in [
        "Coastfile.shared_service_groups.toml",
        "Coastfile.shared_service_groups",
    ] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Build the SSG: parse Coastfile, pull/cache images, write artifact,
/// flip `latest`, prune old builds.
///
/// Streams progress events through `progress` while running; returns
/// a final [`SsgResponse`] with the per-service summary.
pub async fn build_ssg(
    inputs: SsgBuildInputs,
    docker: &bollard::Docker,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgResponse> {
    // --- Step 1: parse ---
    let (cf, raw) = {
        let _ = progress
            .send(BuildProgressEvent::started("Parse coastfile", 1, 1))
            .await;
        let parsed = load_ssg_coastfile(&inputs)?;
        let _ = progress
            .send(BuildProgressEvent::done("Parse coastfile", "ok"))
            .await;
        parsed
    };

    let total = total_steps(cf.services.len());

    // Re-emit step 1 with a proper total so renderers can plan correctly.
    // (Some CLI displays show the max total_steps they've seen.)
    // Skip the re-emit; the first event already showed "ok".

    // --- Step 2: compute build id ---
    let _ = progress
        .send(BuildProgressEvent::started("Resolve build id", 2, total))
        .await;
    let now = chrono::Utc::now();
    let build_id = build_artifact::compute_build_id(&raw, &cf, now);
    let coastfile_hash = build_artifact::coastfile_hash_for(&raw, &cf);
    let _ = progress
        .send(BuildProgressEvent::done("Resolve build id", &build_id))
        .await;

    // --- Step 3: synthesize inner compose ---
    let _ = progress
        .send(BuildProgressEvent::started("Synthesize compose", 3, total))
        .await;
    let inner_compose = synth_inner_compose(&cf);
    let _ = progress
        .send(BuildProgressEvent::done("Synthesize compose", "ok"))
        .await;

    // --- Steps 4..N: pull images ---
    //
    // Pull steps are numbered starting at 4. Artifact/flip/prune come
    // after all images.
    let cache_dir = artifact::image_cache_dir()?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| CoastError::Io {
        message: format!(
            "failed to create image cache dir '{}': {e}",
            cache_dir.display()
        ),
        path: cache_dir.clone(),
        source: Some(e),
    })?;
    pull_and_cache_ssg_images(docker, &cf.services, &cache_dir, &progress, 4, total).await?;

    let post_pull_step = 4 + cf.services.len() as u32;

    // --- Write artifact ---
    let _ = progress
        .send(BuildProgressEvent::started(
            "Write build artifact",
            post_pull_step,
            total,
        ))
        .await;
    let manifest = build_artifact::build_manifest(&build_id, &coastfile_hash, &cf);
    let build_dir = build_artifact::write_artifact(&manifest, &cf, &inner_compose)?;
    let _ = progress
        .send(BuildProgressEvent::done(
            "Write build artifact",
            &build_dir.display().to_string(),
        ))
        .await;

    // --- Flip latest ---
    let _ = progress
        .send(BuildProgressEvent::started(
            "Flip latest symlink",
            post_pull_step + 1,
            total,
        ))
        .await;
    build_artifact::flip_latest(&build_id)?;
    let _ = progress
        .send(BuildProgressEvent::done("Flip latest symlink", "ok"))
        .await;

    // --- Prune ---
    let _ = progress
        .send(BuildProgressEvent::started(
            "Prune old builds",
            post_pull_step + 2,
            total,
        ))
        .await;
    let pruned = build_artifact::auto_prune(5)?;
    let _ = progress
        .send(BuildProgressEvent::done(
            "Prune old builds",
            &format!("removed {pruned}"),
        ))
        .await;

    Ok(build_response_from_manifest(
        &manifest,
        format!("Build complete: {build_id}"),
    ))
}

/// Read the active SSG build manifest and return service metadata.
///
/// Returns an empty-services response with an explanatory message if
/// no build exists (Phase 3 extends this with runtime status).
pub fn ps_ssg() -> Result<SsgResponse> {
    let Some(build_id) = paths::resolve_latest_build_id() else {
        return Ok(SsgResponse {
            message: "No SSG build found. Run `coast ssg build` first.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
        });
    };

    let build_dir = paths::ssg_build_dir(&build_id)?;
    let manifest_path = build_dir.join("manifest.json");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| CoastError::Io {
        message: format!(
            "failed to read SSG manifest '{}': {e}",
            manifest_path.display()
        ),
        path: manifest_path.clone(),
        source: Some(e),
    })?;
    let manifest: build_artifact::SsgManifest = serde_json::from_str(&content).map_err(|e| {
        CoastError::artifact(format!(
            "failed to parse SSG manifest '{}': {e}",
            manifest_path.display()
        ))
    })?;

    Ok(build_response_from_manifest(
        &manifest,
        format!("SSG build: {build_id}"),
    ))
}

// --- Phase 3 runtime wrappers ----------------------------------------------
//
// Re-exports the types callers need. Lifecycle functions are intentionally
// state-free so the async Docker work doesn't have to satisfy
// `&dyn SsgStateExt: Sync` (and it can't; `StateDb` wraps a
// `rusqlite::Connection` which is `!Sync`). Daemon handlers read the
// current state before the async section, call into this crate, then
// apply writes afterwards.

pub use crate::runtime::lifecycle::{
    exec_ssg, logs_ssg, ports_ssg, restart_ssg, rm_ssg, run_ssg, start_ssg, stop_ssg,
    SsgRunOutcome, SsgStartOutcome, SsgStopOutcome,
};

fn build_response_from_manifest(
    manifest: &build_artifact::SsgManifest,
    message: String,
) -> SsgResponse {
    let services = manifest
        .services
        .iter()
        .map(|s| SsgServiceInfo {
            name: s.name.clone(),
            image: s.image.clone(),
            inner_port: s.ports.first().copied().unwrap_or(0),
            // dynamic_host_port is 0 pre-run — populated in Phase 3 when
            // the SSG DinD is started and ports are allocated.
            dynamic_host_port: 0,
            container_id: None,
            status: "built".to_string(),
        })
        .collect();

    SsgResponse {
        message,
        // status is the SSG container's runtime status (Phase 3).
        // Before the first `coast ssg run`, there is no container, so
        // we leave it None.
        status: None,
        services,
        ports: Vec::new(),
    }
}
