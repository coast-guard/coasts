//! SSG lifecycle orchestrators.
//!
//! Phase 3 introduced these verbs backed by raw `&bollard::Docker` +
//! `DindRuntime`. Phase 12 rewrote every orchestrator to take
//! `&dyn SsgDockerOps` (see [`crate::docker_ops`]) so the async
//! sequencing is unit-testable without Docker. Real impl delegation
//! lives entirely in `BollardSsgDockerOps`.
//!
//! Each verb (`run`, `stop`, `start`, `restart`, `rm`, `logs`, `exec`,
//! `ports`) has a single entry function in this module. The daemon's
//! `handlers/ssg.rs` adapter is a thin switch that acquires the
//! process-global `ssg_mutex` (for mutating verbs), locks `StateDb`,
//! constructs a `BollardSsgDockerOps`, and dispatches here.
//!
//! The singleton container is literally named `coast-ssg` (`DESIGN.md §4`)
//! — we rely on `ContainerConfig.container_name_override` to bypass the
//! default `{project}-coasts-{instance}` naming.
//!
//! All verbs are expected to be called while the daemon holds the
//! SSG-wide serialization lock. This module does not enforce that
//! itself; the daemon handler is responsible for acquiring
//! `AppState.ssg_mutex` before calling any mutating function.

use tokio::sync::mpsc::Sender;
use tracing::warn;

use coast_core::artifact;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, SsgPortInfo, SsgResponse, SsgServiceInfo};
use coast_docker::dind::{build_dind_config, DindConfigParams};
use coast_docker::runtime::PortPublish;

use crate::build::artifact::SsgManifest;
use crate::coastfile::SsgCoastfile;
use crate::docker_ops::SsgDockerOps;
use crate::paths;
use crate::runtime::bind_mounts::{ensure_host_bind_dirs_exist, outer_bind_mounts};
use crate::runtime::ports::{allocate_service_ports, SsgServicePortPlan};
use crate::state::{SsgRecord, SsgServiceRecord, SsgStateExt};

/// Per-project SSG container name (`DESIGN.md §23`). E.g.
/// `ssg_container_name("cg") == "cg-ssg"`.
pub fn ssg_container_name(project: &str) -> String {
    format!("{project}-ssg")
}

/// Per-project inner compose project label used inside the SSG DinD.
/// Matches [`ssg_container_name`] so `docker volume ls --filter
/// label=com.docker.compose.project=<x>` and `docker exec <ssg>
/// docker compose -p <x>` reference the same string everywhere.
pub fn ssg_compose_project(project: &str) -> String {
    format!("{project}-ssg")
}

/// Per-project Docker label filter used by `remove_inner_volumes`
/// to scope `docker volume rm` to this SSG's inner named volumes.
pub fn inner_volume_label_filter(project: &str) -> String {
    format!("com.docker.compose.project={project}-ssg")
}

/// Inner path the artifact directory is mounted at (see
/// [`DindConfigParams::artifact_dir`]).
pub(crate) const INNER_ARTIFACT_DIR: &str = "/coast-artifact";

/// Inner path the host image cache is mounted at (see
/// [`DindConfigParams::image_cache_path`]).
const INNER_IMAGE_CACHE_DIR: &str = "/image-cache";

/// Inner daemon readiness timeout. Matches the default used for
/// regular coasts.
const INNER_DAEMON_TIMEOUT_SECS: u64 = 120;

/// Total steps in the `run_ssg` progress plan.
///
/// Fixed: prepare (1), create container (2), start container (3), wait
/// for daemon (4), load images (5), compose up (6).
const RUN_STEPS: u32 = 6;

/// Result of `run_ssg`: all data the caller needs to write state and
/// build a final `SsgResponse`. Separating this from the async Docker
/// work lets the daemon handler apply state writes outside any
/// `await` that would otherwise require `&dyn SsgStateExt: Sync`
/// (and `StateDb` is `!Sync` because `rusqlite::Connection` is).
#[derive(Debug, Clone)]
pub struct SsgRunOutcome {
    pub build_id: String,
    pub container_id: String,
    pub port_plans: Vec<SsgServicePortPlan>,
    pub manifest: SsgManifest,
}

impl SsgRunOutcome {
    /// Apply the post-run state writes and build the final
    /// [`SsgResponse`] to return to the caller. Must be called after
    /// all Docker work is complete.
    pub fn apply_to_state_and_response(
        &self,
        project: &str,
        state: &dyn SsgStateExt,
        status: &str,
        message: String,
    ) -> Result<SsgResponse> {
        state.upsert_ssg(
            project,
            status,
            Some(&self.container_id),
            Some(&self.build_id),
        )?;
        state.clear_ssg_services(project)?;
        for plan in &self.port_plans {
            state.upsert_ssg_service(&SsgServiceRecord {
                project: project.to_string(),
                service_name: plan.service.clone(),
                container_port: plan.container_port,
                dynamic_host_port: plan.dynamic_host_port,
                status: status.to_string(),
            })?;
        }
        Ok(build_response(
            &self.manifest,
            &self.port_plans,
            Some(status),
            Some(&self.container_id),
            message,
        ))
    }
}

// --- run -------------------------------------------------------------------

/// Create the project's SSG DinD, wait for the inner daemon, load
/// cached images, and run `docker compose up -d` on the active
/// build's compose file.
///
/// Does NOT touch the daemon's state DB — the caller is responsible
/// for writing [`SsgRunOutcome`] into state (typically via
/// [`SsgRunOutcome::apply_to_state_and_response`]). See the module
/// doc for the rationale.
///
/// Preconditions:
///
/// - A specific `build_id` for `project` has been resolved by the
///   caller (from a pin or `ssg.latest_build_id`).
/// - No SSG container for `project` is currently running — the caller
///   is expected to check and either short-circuit or error out.
///
/// Phase 23: callers MUST pre-resolve the build id from the daemon
/// state (`ssg_consumer_pins` > `ssg.latest_build_id`). There is no
/// global `~/.coast/ssg/latest` fallback any more — that used to
/// leak another project's build into this project's runtime. When
/// `build_id` is `None` the function hard-errors immediately.
///
/// Phase 16 pinning continues to work through the caller:
/// `ensure_ready_for_consumer` passes the pinned id when present,
/// else the project's own `latest_build_id` from state.
pub async fn run_ssg_with_build_id(
    project: &str,
    ops: &dyn SsgDockerOps,
    build_id: Option<&str>,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgRunOutcome> {
    emit(&progress, "Preparing SSG", 1, RUN_STEPS).await;

    let build_id = build_id
        .ok_or_else(|| {
            CoastError::coastfile(format!(
                "no SSG build found for project '{project}'. Run `coast ssg build` in \
                 the directory containing the project's Coastfile.shared_service_groups \
                 before `coast ssg run`."
            ))
        })?
        .to_string();
    let build_dir = paths::ssg_build_dir(&build_id)?;
    let manifest = read_manifest(&build_dir)?;
    let coastfile = load_coastfile(&build_dir)?;

    ensure_host_bind_dirs_exist(&coastfile)?;

    let port_plans = allocate_service_ports(&manifest)?;

    done(&progress, "Preparing SSG", &build_id).await;

    // --- create container ---
    emit(&progress, "Creating SSG container", 2, RUN_STEPS).await;
    let cache_dir = artifact::image_cache_dir()?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| CoastError::Io {
        message: format!(
            "failed to create image cache dir '{}': {e}",
            cache_dir.display()
        ),
        path: cache_dir.clone(),
        source: Some(e),
    })?;

    let container_id =
        create_ssg_container(project, ops, &build_dir, &cache_dir, &coastfile, &port_plans)
            .await?;
    done(&progress, "Creating SSG container", &container_id).await;

    // --- start container ---
    emit(&progress, "Starting SSG container", 3, RUN_STEPS).await;
    ops.start_container(&container_id).await?;
    done(&progress, "Starting SSG container", "ok").await;

    // --- wait for inner daemon ---
    emit(&progress, "Waiting for inner daemon", 4, RUN_STEPS).await;
    ops.wait_for_inner_daemon(&container_id, INNER_DAEMON_TIMEOUT_SECS)
        .await?;
    done(&progress, "Waiting for inner daemon", "ready").await;

    // --- load cached images ---
    emit(&progress, "Loading cached images", 5, RUN_STEPS).await;
    let loaded = load_ssg_images_into_inner(ops, &container_id, &manifest).await?;
    done(
        &progress,
        "Loading cached images",
        &format!("{loaded} loaded"),
    )
    .await;

    // --- compose up ---
    emit(&progress, "Starting inner services", 6, RUN_STEPS).await;
    ops.inner_compose_up(
        &container_id,
        &inner_compose_path(),
        &ssg_compose_project(project),
    )
    .await?;
    done(&progress, "Starting inner services", "ok").await;

    Ok(SsgRunOutcome {
        build_id,
        container_id,
        port_plans,
        manifest,
    })
}

// --- stop ------------------------------------------------------------------

/// Outcome of a stop operation. No state-write closure here — the
/// caller just writes status = "stopped" on the record + each service.
#[derive(Debug, Clone)]
pub struct SsgStopOutcome {
    /// The existing container id that was stopped, if any. `None` when
    /// called on an SSG that was never run. Callers propagate this
    /// unchanged into the new `ssg` row.
    pub container_id: Option<String>,
    pub build_id: Option<String>,
}

/// Stop the SSG DinD (inner compose down + outer container stop).
///
/// Does NOT write state — the caller writes status = "stopped" on the
/// `ssg` and `ssg_services` rows after this returns.
pub async fn stop_ssg(ops: &dyn SsgDockerOps, record: &SsgRecord) -> Result<SsgStopOutcome> {
    if let Some(ref cid) = record.container_id {
        if let Err(e) = ops
            .inner_compose_down(
                cid,
                &inner_compose_path(),
                &ssg_compose_project(&record.project),
            )
            .await
        {
            warn!(error = %e, container_id = %cid, "inner compose down failed; continuing");
        }
        if let Err(e) = ops.stop_container(cid).await {
            warn!(error = %e, container_id = %cid, "stop_container failed; continuing");
        }
    }

    Ok(SsgStopOutcome {
        container_id: record.container_id.clone(),
        build_id: record.build_id.clone(),
    })
}

// --- start -----------------------------------------------------------------

/// Outcome of starting an already-created SSG.
#[derive(Debug, Clone)]
pub struct SsgStartOutcome {
    pub container_id: String,
    pub build_id: String,
    pub port_plans: Vec<SsgServicePortPlan>,
    pub manifest: SsgManifest,
}

impl SsgStartOutcome {
    /// Apply post-start state writes and build the response.
    pub fn apply_to_state_and_response(
        &self,
        project: &str,
        state: &dyn SsgStateExt,
        message: String,
    ) -> Result<SsgResponse> {
        state.upsert_ssg(
            project,
            "running",
            Some(&self.container_id),
            Some(&self.build_id),
        )?;
        for svc in state.list_ssg_services(project)? {
            state.update_ssg_service_status(project, &svc.service_name, "running")?;
        }
        Ok(build_response(
            &self.manifest,
            &self.port_plans,
            Some("running"),
            Some(&self.container_id),
            message,
        ))
    }
}

/// Start the previously-created SSG DinD (after `coast ssg stop`).
///
/// Reuses the already-allocated dynamic ports provided in
/// `existing_plans` from `ssg_services`. Does not write state.
pub async fn start_ssg(
    ops: &dyn SsgDockerOps,
    record: &SsgRecord,
    existing_plans: Vec<SsgServicePortPlan>,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgStartOutcome> {
    let container_id = record.container_id.clone().ok_or_else(|| {
        CoastError::coastfile(
            "SSG record has no container id. Run `coast ssg run` to re-create it.",
        )
    })?;
    let build_id = record.build_id.clone().ok_or_else(|| {
        CoastError::coastfile("SSG record has no build id. Run `coast ssg run` to re-create it.")
    })?;

    emit(&progress, "Starting SSG container", 1, 3).await;
    ops.start_container(&container_id).await?;
    done(&progress, "Starting SSG container", "ok").await;

    emit(&progress, "Waiting for inner daemon", 2, 3).await;
    ops.wait_for_inner_daemon(&container_id, INNER_DAEMON_TIMEOUT_SECS)
        .await?;
    done(&progress, "Waiting for inner daemon", "ready").await;

    emit(&progress, "Starting inner services", 3, 3).await;
    ops.inner_compose_up(
        &container_id,
        &inner_compose_path(),
        &ssg_compose_project(&record.project),
    )
    .await?;
    done(&progress, "Starting inner services", "ok").await;

    let build_dir = paths::ssg_build_dir(&build_id)?;
    let manifest = read_manifest(&build_dir)?;

    Ok(SsgStartOutcome {
        container_id,
        build_id,
        port_plans: existing_plans,
        manifest,
    })
}

// --- restart ---------------------------------------------------------------

/// Stop then start the SSG (preserves allocated ports and container id).
///
/// Convenience helper; `coast ssg restart` could equivalently call
/// [`stop_ssg`] then [`start_ssg`] from the daemon handler.
pub async fn restart_ssg(
    ops: &dyn SsgDockerOps,
    record: &SsgRecord,
    existing_plans: Vec<SsgServicePortPlan>,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgStartOutcome> {
    stop_ssg(ops, record).await?;
    start_ssg(ops, record, existing_plans, progress).await
}

// --- rm --------------------------------------------------------------------

/// Remove the SSG DinD container. When `with_data`, also removes inner
/// named volumes (postgres WAL, redis AOF, etc.) before tearing down
/// the DinD.
///
/// Host bind mount contents are **never** touched regardless of
/// `with_data` — see `DESIGN.md §10.4`.
///
/// Does not write state — the caller clears `ssg` and `ssg_services`
/// rows after this returns.
pub async fn rm_ssg(ops: &dyn SsgDockerOps, record: &SsgRecord, with_data: bool) -> Result<()> {
    if let Some(ref cid) = record.container_id {
        let was_running = record.status == "running";
        teardown_ssg_container(&record.project, ops, cid, was_running, with_data).await;
    }
    Ok(())
}

/// Teardown sequence for `rm_ssg`: if the container was stopped,
/// transiently start it so we can clean up compose resources, then
/// `docker compose down`, optionally remove inner named volumes, and
/// finally remove the outer DinD container.
///
/// All inner errors are logged as warnings rather than propagated —
/// a partial cleanup is preferable to leaving the state row behind.
async fn teardown_ssg_container(
    project: &str,
    ops: &dyn SsgDockerOps,
    cid: &str,
    was_running: bool,
    with_data: bool,
) {
    if !was_running {
        transient_start_for_cleanup(ops, cid).await;
    }
    inner_compose_down_best_effort(project, ops, cid).await;
    if with_data {
        remove_inner_volumes_best_effort(project, ops, cid).await;
    }
    remove_container_best_effort(ops, cid).await;
}

async fn inner_compose_down_best_effort(project: &str, ops: &dyn SsgDockerOps, cid: &str) {
    if let Err(e) = ops
        .inner_compose_down(cid, &inner_compose_path(), &ssg_compose_project(project))
        .await
    {
        warn!(error = %e, container_id = %cid, "inner compose down during rm failed; continuing");
    }
}

async fn remove_inner_volumes_best_effort(project: &str, ops: &dyn SsgDockerOps, cid: &str) {
    if let Err(e) = ops
        .remove_inner_volumes(cid, &inner_volume_label_filter(project))
        .await
    {
        warn!(error = %e, container_id = %cid, "remove_inner_volumes failed; continuing");
    }
}

async fn remove_container_best_effort(ops: &dyn SsgDockerOps, cid: &str) {
    if let Err(e) = ops.remove_container(cid).await {
        warn!(error = %e, container_id = %cid, "remove_container failed; state will be cleared regardless");
    }
}

async fn transient_start_for_cleanup(ops: &dyn SsgDockerOps, cid: &str) {
    if let Err(e) = ops.start_container(cid).await {
        warn!(error = %e, container_id = %cid, "transient start before rm failed");
        return;
    }
    let _ = ops
        .wait_for_inner_daemon(cid, INNER_DAEMON_TIMEOUT_SECS)
        .await;
}

// --- logs ------------------------------------------------------------------

/// Collect logs from the outer DinD or a specific inner service.
///
/// This is the non-streaming path; follow-mode is handled at the
/// daemon layer via a separate streaming handler (see
/// `coast-daemon/src/server.rs::handle_ssg_logs_streaming`).
///
/// Returns the raw text to return in `SsgResponse.message`.
pub async fn logs_ssg(
    ops: &dyn SsgDockerOps,
    record: &SsgRecord,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<String> {
    let container_id = record
        .container_id
        .clone()
        .ok_or_else(|| CoastError::coastfile("SSG record has no container id; nothing to tail."))?;

    if let Some(ref svc) = service {
        ops.inner_compose_logs(
            &container_id,
            &inner_compose_path(),
            &ssg_compose_project(&record.project),
            svc,
            tail,
        )
        .await
    } else {
        ops.host_container_logs(&container_id, tail).await
    }
}

// --- exec ------------------------------------------------------------------

/// Execute a command inside the outer DinD or a named inner service.
///
/// With `service`, runs `docker compose exec -T <service> <cmd...>`
/// inside the outer DinD. Without, execs directly on the outer DinD.
/// Returns the combined stdout / stderr.
pub async fn exec_ssg(
    ops: &dyn SsgDockerOps,
    record: &SsgRecord,
    service: Option<String>,
    command: Vec<String>,
) -> Result<String> {
    if command.is_empty() {
        return Err(CoastError::coastfile(
            "coast ssg exec requires a command to run (e.g. `coast ssg exec -- psql -U coast`).",
        ));
    }

    let container_id = record.container_id.clone().ok_or_else(|| {
        CoastError::coastfile("SSG record has no container id; nothing to exec against.")
    })?;

    let result = if let Some(svc) = service {
        ops.inner_compose_exec(
            &container_id,
            &inner_compose_path(),
            &ssg_compose_project(&record.project),
            &svc,
            &command,
        )
        .await?
    } else {
        ops.exec_in_container(&container_id, &command).await?
    };

    Ok(if result.stdout.is_empty() {
        result.stderr
    } else if result.stderr.is_empty() {
        result.stdout
    } else {
        format!("{}\n{}", result.stdout, result.stderr)
    })
}

// --- ports -----------------------------------------------------------------

/// Read the current per-service dynamic host ports from state.
///
/// Phase 6 populates `checked_out` from `ssg_port_checkouts` by
/// joining on `canonical_port`. A service whose canonical port has a
/// live checkout row (socat_pid not null) reads as `checked_out =
/// true`; rows whose socat was torn down (e.g. after `coast ssg stop`)
/// keep the row but set `socat_pid = null` and thus read `false`
/// until the next `run` / `start` re-spawns them.
pub fn ports_ssg(project: &str, state: &dyn SsgStateExt) -> Result<SsgResponse> {
    let services = state.list_ssg_services(project)?;
    let record = state.get_ssg(project)?;
    let checkouts = state.list_ssg_port_checkouts(project)?;

    let ports: Vec<SsgPortInfo> = services
        .iter()
        .map(|s| {
            let checked_out = checkouts.iter().any(|c| {
                c.canonical_port == s.container_port
                    && c.service_name == s.service_name
                    && c.socat_pid.is_some()
            });
            SsgPortInfo {
                service: s.service_name.clone(),
                canonical_port: s.container_port,
                dynamic_host_port: s.dynamic_host_port,
                checked_out,
            }
        })
        .collect();

    let message = if ports.is_empty() {
        "No SSG services running. Run `coast ssg run` to allocate ports.".to_string()
    } else {
        format!("{} service port mapping(s).", ports.len())
    };

    Ok(SsgResponse {
        message,
        status: record.map(|r| r.status),
        services: Vec::new(),
        ports,
        findings: Vec::new(),
        listings: Vec::new(),
    })
}

// --- internals -------------------------------------------------------------

pub(crate) fn inner_compose_path() -> String {
    format!("{INNER_ARTIFACT_DIR}/compose.yml")
}

async fn create_ssg_container(
    project: &str,
    ops: &dyn SsgDockerOps,
    build_dir: &std::path::Path,
    cache_dir: &std::path::Path,
    coastfile: &SsgCoastfile,
    plans: &[SsgServicePortPlan],
) -> Result<String> {
    let bind_mounts = outer_bind_mounts(coastfile);

    let mut config = build_dind_config(DindConfigParams {
        bind_mounts,
        artifact_dir: Some(build_dir),
        image_cache_path: Some(cache_dir),
        container_name_override: Some(ssg_container_name(project)),
        // Per-project SSG (§23): the outer Docker compose label uses
        // the consumer project name so Docker Desktop groups the SSG
        // under `{project}-coasts/{project}-ssg`.
        ..DindConfigParams::new(project, "ssg", build_dir)
    });

    for plan in plans {
        config.published_ports.push(PortPublish {
            host_port: plan.dynamic_host_port,
            container_port: plan.container_port,
        });
    }

    ops.create_container(&config).await
}

/// Load one cached tarball per unique image referenced in the manifest
/// into the inner daemon. Returns the number of images actually loaded
/// (already-present images are skipped).
///
/// The tarball naming convention matches
/// [`coast_docker::image_cache::pull_and_cache_image`] so this looks
/// up `{safe_name}.tar` files at `/image-cache/{safe_name}.tar` inside
/// the outer DinD.
async fn load_ssg_images_into_inner(
    ops: &dyn SsgDockerOps,
    container_id: &str,
    manifest: &SsgManifest,
) -> Result<u32> {
    // Query already-loaded images in the inner daemon to skip re-loads
    // (relevant when restarting an existing SSG on a cached DinD volume).
    let existing = ops.list_inner_images(container_id).await?;

    let missing = plan_images_to_load(manifest, &existing);
    if missing.is_empty() {
        return Ok(0);
    }

    let inner_paths: Vec<String> = missing
        .iter()
        .map(|img| {
            let safe_name = img.replace(['/', ':'], "_");
            format!("{INNER_IMAGE_CACHE_DIR}/{safe_name}.tar")
        })
        .collect();

    ops.load_images_into_inner(container_id, &inner_paths).await
}

/// Pure helper: which images from `manifest.services` are not yet
/// loaded in the inner daemon? Preserves manifest order, dedupes by
/// image ref.
fn plan_images_to_load(
    manifest: &SsgManifest,
    already_loaded: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for svc in &manifest.services {
        if !seen.insert(svc.image.clone()) {
            continue;
        }
        if already_loaded.contains(&svc.image) {
            continue;
        }
        out.push(svc.image.clone());
    }
    out
}

fn read_manifest(build_dir: &std::path::Path) -> Result<SsgManifest> {
    let manifest_path = build_dir.join("manifest.json");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| CoastError::Io {
        message: format!(
            "failed to read SSG manifest '{}': {e}",
            manifest_path.display()
        ),
        path: manifest_path.clone(),
        source: Some(e),
    })?;
    serde_json::from_str(&content).map_err(|e| {
        CoastError::artifact(format!(
            "failed to parse SSG manifest '{}': {e}",
            manifest_path.display()
        ))
    })
}

fn load_coastfile(build_dir: &std::path::Path) -> Result<SsgCoastfile> {
    let toml_path = build_dir.join("ssg-coastfile.toml");
    SsgCoastfile::from_file(&toml_path)
}

async fn emit(progress: &Sender<BuildProgressEvent>, step: &str, number: u32, total: u32) {
    let _ = progress
        .send(BuildProgressEvent::started(step, number, total))
        .await;
}

async fn done(progress: &Sender<BuildProgressEvent>, step: &str, status_or_detail: &str) {
    let _ = progress
        .send(BuildProgressEvent::done(step, status_or_detail))
        .await;
}

fn build_response(
    manifest: &SsgManifest,
    plans: &[SsgServicePortPlan],
    status: Option<&str>,
    container_id: Option<&str>,
    message: String,
) -> SsgResponse {
    let mut service_infos: Vec<SsgServiceInfo> = Vec::with_capacity(manifest.services.len());
    for svc in &manifest.services {
        let (inner_port, dynamic_host_port) = plans
            .iter()
            .find(|p| p.service == svc.name)
            .map(|p| (p.container_port, p.dynamic_host_port))
            .unwrap_or((svc.ports.first().copied().unwrap_or(0), 0));
        service_infos.push(SsgServiceInfo {
            name: svc.name.clone(),
            image: svc.image.clone(),
            inner_port,
            dynamic_host_port,
            container_id: container_id.map(str::to_string),
            status: status.unwrap_or("unknown").to_string(),
        });
    }

    let ports: Vec<SsgPortInfo> = plans
        .iter()
        .map(|p| SsgPortInfo {
            service: p.service.clone(),
            canonical_port: p.container_port,
            dynamic_host_port: p.dynamic_host_port,
            checked_out: false,
        })
        .collect();

    SsgResponse {
        message,
        status: status.map(str::to_string),
        services: service_infos,
        ports,
        findings: Vec::new(),
        listings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::artifact::SsgManifestService;
    use crate::docker_ops::{MockCall, MockSsgDockerOps, SsgExecOutput};
    use std::collections::HashSet;

    fn sample_manifest(services: Vec<(&str, &str, Vec<u16>)>) -> SsgManifest {
        SsgManifest {
            build_id: "b_test".to_string(),
            built_at: chrono::Utc::now(),
            coastfile_hash: "h".to_string(),
            services: services
                .into_iter()
                .map(|(name, image, ports)| SsgManifestService {
                    name: name.to_string(),
                    image: image.to_string(),
                    ports,
                    env_keys: vec![],
                    volumes: vec![],
                    auto_create_db: false,
                })
                .collect(),
        }
    }

    fn sample_record(status: &str, cid: Option<&str>) -> SsgRecord {
        SsgRecord {
            project: "test-proj".to_string(),
            status: status.to_string(),
            container_id: cid.map(str::to_string),
            build_id: Some("b_test".to_string()),
            latest_build_id: Some("b_test".to_string()),
            created_at: "2026-04-20T00:00:00Z".to_string(),
        }
    }

    // --- inner_compose_path ---

    #[test]
    fn inner_compose_path_is_under_artifact_dir() {
        assert_eq!(inner_compose_path(), "/coast-artifact/compose.yml");
    }

    #[test]
    fn naming_helpers_derive_from_project() {
        // Per-project SSG (§23): every real Docker label flows from
        // the consumer project name, not a global constant.
        assert_eq!(ssg_container_name("cg"), "cg-ssg");
        assert_eq!(ssg_compose_project("cg"), "cg-ssg");
        assert_eq!(
            inner_volume_label_filter("cg"),
            "com.docker.compose.project=cg-ssg"
        );
        // Project name with hyphens/underscores flows through verbatim.
        assert_eq!(ssg_container_name("my-app_2"), "my-app_2-ssg");
        assert_eq!(
            inner_volume_label_filter("filemap"),
            "com.docker.compose.project=filemap-ssg"
        );
    }

    // --- plan_images_to_load ---

    #[test]
    fn plan_images_skips_already_loaded() {
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16", vec![5432]),
            ("redis", "redis:7", vec![6379]),
        ]);
        let loaded: HashSet<String> = ["postgres:16".to_string()].into_iter().collect();
        let plan = plan_images_to_load(&manifest, &loaded);
        assert_eq!(plan, vec!["redis:7"]);
    }

    #[test]
    fn plan_images_dedupes_duplicates() {
        let manifest = sample_manifest(vec![
            ("a", "postgres:16", vec![5432]),
            ("b", "postgres:16", vec![5433]),
            ("c", "redis:7", vec![6379]),
        ]);
        let plan = plan_images_to_load(&manifest, &HashSet::new());
        assert_eq!(plan, vec!["postgres:16", "redis:7"]);
    }

    #[test]
    fn plan_images_returns_empty_when_all_loaded() {
        let manifest = sample_manifest(vec![("postgres", "postgres:16", vec![5432])]);
        let loaded: HashSet<String> = ["postgres:16".to_string()].into_iter().collect();
        assert!(plan_images_to_load(&manifest, &loaded).is_empty());
    }

    // --- build_response ---

    #[test]
    fn build_response_pairs_plans_with_manifest_services() {
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16", vec![5432]),
            ("redis", "redis:7", vec![6379]),
        ]);
        let plans = vec![
            SsgServicePortPlan {
                service: "postgres".to_string(),
                container_port: 5432,
                dynamic_host_port: 60000,
            },
            SsgServicePortPlan {
                service: "redis".to_string(),
                container_port: 6379,
                dynamic_host_port: 60001,
            },
        ];
        let resp = build_response(
            &manifest,
            &plans,
            Some("running"),
            Some("cid"),
            "ok".to_string(),
        );
        assert_eq!(resp.services.len(), 2);
        assert_eq!(resp.services[0].name, "postgres");
        assert_eq!(resp.services[0].dynamic_host_port, 60000);
        assert_eq!(resp.services[1].name, "redis");
        assert_eq!(resp.services[1].dynamic_host_port, 60001);
        assert_eq!(resp.ports.len(), 2);
    }

    #[test]
    fn build_response_marks_missing_plans_as_zero_port() {
        let manifest = sample_manifest(vec![("sidecar", "alpine:3", vec![])]);
        let resp = build_response(&manifest, &[], Some("running"), None, "ok".to_string());
        assert_eq!(resp.services[0].inner_port, 0);
        assert_eq!(resp.services[0].dynamic_host_port, 0);
        assert!(resp.ports.is_empty());
    }

    // --- stop_ssg ---

    #[tokio::test]
    async fn stop_ssg_calls_compose_down_then_stop_container() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("running", Some("cid-1"));
        let outcome = stop_ssg(&mock, &record).await.unwrap();
        assert_eq!(outcome.container_id.as_deref(), Some("cid-1"));
        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            calls[0],
            MockCall::InnerComposeDown { ref container_id, ref project, .. }
                if container_id == "cid-1" && project == "test-proj-ssg"
        ));
        assert!(matches!(
            calls[1],
            MockCall::StopContainer(ref cid) if cid == "cid-1"
        ));
    }

    #[tokio::test]
    async fn stop_ssg_swallows_compose_down_error_and_still_stops_container() {
        let mock = MockSsgDockerOps::new();
        mock.push_compose_down_result(Err(CoastError::docker("compose down exploded")));
        let record = sample_record("running", Some("cid-1"));
        stop_ssg(&mock, &record).await.unwrap();
        let calls = mock.calls();
        // Both calls happened despite the compose-down error.
        assert!(matches!(calls[0], MockCall::InnerComposeDown { .. }));
        assert!(matches!(calls[1], MockCall::StopContainer(_)));
    }

    #[tokio::test]
    async fn stop_ssg_with_no_container_id_is_noop() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("stopped", None);
        let outcome = stop_ssg(&mock, &record).await.unwrap();
        assert!(outcome.container_id.is_none());
        assert!(mock.calls().is_empty());
    }

    // --- rm_ssg ---

    #[tokio::test]
    async fn rm_ssg_with_data_removes_volumes_then_container() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("running", Some("cid-1"));
        rm_ssg(&mock, &record, /*with_data=*/ true).await.unwrap();
        let calls = mock.calls();
        // running -> no transient start -> compose down -> remove volumes -> remove container.
        assert!(matches!(calls[0], MockCall::InnerComposeDown { .. }));
        assert!(matches!(
            calls[1],
            MockCall::RemoveInnerVolumes { ref label_filter, .. }
                if label_filter == "com.docker.compose.project=test-proj-ssg"
        ));
        assert!(matches!(calls[2], MockCall::RemoveContainer(ref cid) if cid == "cid-1"));
    }

    #[tokio::test]
    async fn rm_ssg_without_data_skips_volume_removal() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("running", Some("cid-1"));
        rm_ssg(&mock, &record, /*with_data=*/ false).await.unwrap();
        let calls = mock.calls();
        assert!(matches!(calls[0], MockCall::InnerComposeDown { .. }));
        assert!(matches!(calls[1], MockCall::RemoveContainer(_)));
        for c in &calls {
            assert!(
                !matches!(c, MockCall::RemoveInnerVolumes { .. }),
                "should not remove volumes without --with-data"
            );
        }
    }

    #[tokio::test]
    async fn rm_ssg_from_stopped_status_transient_starts_first() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("stopped", Some("cid-1"));
        rm_ssg(&mock, &record, /*with_data=*/ false).await.unwrap();
        let calls = mock.calls();
        assert!(matches!(calls[0], MockCall::StartContainer(_)));
        assert!(matches!(calls[1], MockCall::WaitForInnerDaemon { .. }));
        assert!(matches!(calls[2], MockCall::InnerComposeDown { .. }));
        assert!(matches!(calls[3], MockCall::RemoveContainer(_)));
    }

    // --- logs_ssg ---

    #[tokio::test]
    async fn logs_ssg_with_service_uses_inner_compose_logs() {
        let mock = MockSsgDockerOps::new();
        mock.push_compose_logs_result(Ok("compose log".to_string()));
        let record = sample_record("running", Some("cid-1"));
        let out = logs_ssg(&mock, &record, Some("postgres".to_string()), Some(10))
            .await
            .unwrap();
        assert_eq!(out, "compose log");
        match &mock.calls()[0] {
            MockCall::InnerComposeLogs { service, tail, .. } => {
                assert_eq!(service, "postgres");
                assert_eq!(*tail, Some(10));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn logs_ssg_without_service_uses_host_logs() {
        let mock = MockSsgDockerOps::new();
        mock.push_host_logs_result(Ok("host log".to_string()));
        let record = sample_record("running", Some("cid-1"));
        let out = logs_ssg(&mock, &record, None, None).await.unwrap();
        assert_eq!(out, "host log");
        match &mock.calls()[0] {
            MockCall::HostContainerLogs { container_id, .. } => {
                assert_eq!(container_id, "cid-1");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn logs_ssg_missing_container_id_errors() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("stopped", None);
        let err = logs_ssg(&mock, &record, None, None).await.unwrap_err();
        assert!(err.to_string().contains("no container id"));
    }

    // --- exec_ssg ---

    #[tokio::test]
    async fn exec_ssg_with_service_routes_to_inner_compose_exec() {
        let mock = MockSsgDockerOps::new();
        mock.push_compose_exec_result(Ok(SsgExecOutput {
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
        }));
        let record = sample_record("running", Some("cid-1"));
        let out = exec_ssg(
            &mock,
            &record,
            Some("postgres".to_string()),
            vec!["psql".to_string(), "-U".to_string(), "coast".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(out, "ok");
        match &mock.calls()[0] {
            MockCall::InnerComposeExec { service, argv, .. } => {
                assert_eq!(service, "postgres");
                assert_eq!(
                    argv,
                    &vec!["psql".to_string(), "-U".to_string(), "coast".to_string()]
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn exec_ssg_without_service_routes_to_exec_in_container() {
        let mock = MockSsgDockerOps::new();
        mock.push_exec_result(Ok(SsgExecOutput {
            exit_code: 0,
            stdout: "direct".to_string(),
            stderr: String::new(),
        }));
        let record = sample_record("running", Some("cid-1"));
        let out = exec_ssg(&mock, &record, None, vec!["uname".to_string()])
            .await
            .unwrap();
        assert_eq!(out, "direct");
        match &mock.calls()[0] {
            MockCall::ExecInContainer { argv, .. } => {
                assert_eq!(argv, &vec!["uname".to_string()]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn exec_ssg_empty_command_errors() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record("running", Some("cid-1"));
        let err = exec_ssg(&mock, &record, None, Vec::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires a command"));
    }

    // --- load_ssg_images_into_inner ---

    #[tokio::test]
    async fn load_ssg_images_skips_already_loaded_entries() {
        let mock = MockSsgDockerOps::new();
        let already: HashSet<String> = ["postgres:16".to_string()].into_iter().collect();
        mock.push_list_images_result(Ok(already));
        mock.push_load_result(Ok(1));

        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16", vec![5432]),
            ("redis", "redis:7", vec![6379]),
        ]);
        let loaded = load_ssg_images_into_inner(&mock, "cid", &manifest)
            .await
            .unwrap();
        assert_eq!(loaded, 1);

        // Only the missing `redis:7` tarball is passed to load.
        match &mock.calls()[1] {
            MockCall::LoadImagesIntoInner { tarballs, .. } => {
                assert_eq!(tarballs, &vec!["/image-cache/redis_7.tar".to_string()]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn load_ssg_images_empty_when_nothing_missing() {
        let mock = MockSsgDockerOps::new();
        let already: HashSet<String> = ["postgres:16".to_string()].into_iter().collect();
        mock.push_list_images_result(Ok(already));
        let manifest = sample_manifest(vec![("postgres", "postgres:16", vec![5432])]);
        let loaded = load_ssg_images_into_inner(&mock, "cid", &manifest)
            .await
            .unwrap();
        assert_eq!(loaded, 0);
        // Only list was called; no load invoked.
        assert_eq!(mock.calls().len(), 1);
        assert!(matches!(mock.calls()[0], MockCall::ListInnerImages { .. }));
    }
}
