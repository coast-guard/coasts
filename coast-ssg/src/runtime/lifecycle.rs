//! SSG lifecycle orchestrators.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.3`, `§9.4`.
//!
//! Each verb (`run`, `stop`, `start`, `restart`, `rm`, `logs`, `exec`,
//! `ports`) has a single entry function in this module. The daemon's
//! `handlers/ssg.rs` adapter is a thin switch that acquires the
//! process-global `ssg_mutex` (for mutating verbs), locks `StateDb`,
//! and dispatches here.
//!
//! The singleton container is literally named `coast-ssg` (`DESIGN.md §4`)
//! — we rely on `ContainerConfig.container_name_override` to bypass the
//! default `{project}-coasts-{instance}` naming.
//!
//! All verbs are expected to be called while the daemon holds the
//! SSG-wide serialization lock. This module does not enforce that
//! itself; the daemon handler is responsible for acquiring
//! `AppState.ssg_mutex` before calling any mutating function.

use std::collections::HashSet;

use bollard::Docker;
use tokio::sync::mpsc::Sender;
use tracing::{info, warn};

use coast_core::artifact;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, SsgPortInfo, SsgResponse, SsgServiceInfo};
use coast_docker::container::ContainerManager;
use coast_docker::dind::{build_dind_config, DindConfigParams, DindRuntime};
use coast_docker::runtime::{PortPublish, Runtime};

use crate::build::artifact::SsgManifest;
use crate::coastfile::SsgCoastfile;
use crate::paths;
use crate::runtime::bind_mounts::{ensure_host_bind_dirs_exist, outer_bind_mounts};
use crate::runtime::ports::{allocate_service_ports, SsgServicePortPlan};
use crate::state::{SsgRecord, SsgServiceRecord, SsgStateExt};

/// Canonical singleton container name per `DESIGN.md §4`.
pub const SSG_CONTAINER_NAME: &str = "coast-ssg";

/// Compose project name used inside the SSG DinD's inner daemon. All
/// inner service containers and named volumes get labeled with this.
pub const SSG_COMPOSE_PROJECT: &str = "coast-ssg";

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
        state: &dyn SsgStateExt,
        status: &str,
        message: String,
    ) -> Result<SsgResponse> {
        state.upsert_ssg(status, Some(&self.container_id), Some(&self.build_id))?;
        state.clear_ssg_services()?;
        for plan in &self.port_plans {
            state.upsert_ssg_service(&SsgServiceRecord {
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

/// Create the SSG singleton DinD, wait for the inner daemon, load
/// cached images, and run `docker compose up -d` on the active build's
/// compose file.
///
/// Does NOT touch the daemon's state DB — the caller is responsible
/// for writing [`SsgRunOutcome`] into state (typically via
/// [`SsgRunOutcome::apply_to_state_and_response`]). See the module
/// doc for the rationale.
///
/// Preconditions:
///
/// - An SSG build exists (`~/.coast/ssg/latest` resolves).
/// - No SSG container is currently running — the caller is expected to
///   check and either short-circuit or error out.
pub async fn run_ssg(
    docker: &Docker,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgRunOutcome> {
    emit(&progress, "Preparing SSG", 1, RUN_STEPS).await;

    let build_id = paths::resolve_latest_build_id().ok_or_else(|| {
        CoastError::coastfile("no SSG build found. Run `coast ssg build` before `coast ssg run`.")
    })?;
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
        create_ssg_container(docker, &build_dir, &cache_dir, &coastfile, &port_plans).await?;
    done(&progress, "Creating SSG container", &container_id).await;

    // --- start container ---
    emit(&progress, "Starting SSG container", 3, RUN_STEPS).await;
    let runtime = DindRuntime::with_client(docker.clone());
    runtime.start_coast_container(&container_id).await?;
    done(&progress, "Starting SSG container", "ok").await;

    // --- wait for inner daemon ---
    emit(&progress, "Waiting for inner daemon", 4, RUN_STEPS).await;
    let manager = ContainerManager::with_timeout(
        DindRuntime::with_client(docker.clone()),
        INNER_DAEMON_TIMEOUT_SECS,
    );
    manager.wait_for_inner_daemon(&container_id).await?;
    done(&progress, "Waiting for inner daemon", "ready").await;

    // --- load cached images ---
    emit(&progress, "Loading cached images", 5, RUN_STEPS).await;
    let loaded = load_ssg_images_into_inner(docker, &container_id, &manifest).await?;
    done(
        &progress,
        "Loading cached images",
        &format!("{loaded} loaded"),
    )
    .await;

    // --- compose up ---
    emit(&progress, "Starting inner services", 6, RUN_STEPS).await;
    run_inner_compose_up(docker, &container_id).await?;
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
pub async fn stop_ssg(docker: &Docker, record: &SsgRecord) -> Result<SsgStopOutcome> {
    let runtime = DindRuntime::with_client(docker.clone());

    if let Some(ref cid) = record.container_id {
        let _ = runtime
            .exec_in_coast(
                cid,
                &[
                    "docker",
                    "compose",
                    "-f",
                    &inner_compose_path(),
                    "-p",
                    SSG_COMPOSE_PROJECT,
                    "down",
                ],
            )
            .await;
        if let Err(e) = runtime.stop_coast_container(cid).await {
            warn!(error = %e, container_id = %cid, "stop_coast_container failed; continuing");
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
        state: &dyn SsgStateExt,
        message: String,
    ) -> Result<SsgResponse> {
        state.upsert_ssg("running", Some(&self.container_id), Some(&self.build_id))?;
        for svc in state.list_ssg_services()? {
            state.update_ssg_service_status(&svc.service_name, "running")?;
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
    docker: &Docker,
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
    let runtime = DindRuntime::with_client(docker.clone());
    runtime.start_coast_container(&container_id).await?;
    done(&progress, "Starting SSG container", "ok").await;

    emit(&progress, "Waiting for inner daemon", 2, 3).await;
    let manager = ContainerManager::with_timeout(
        DindRuntime::with_client(docker.clone()),
        INNER_DAEMON_TIMEOUT_SECS,
    );
    manager.wait_for_inner_daemon(&container_id).await?;
    done(&progress, "Waiting for inner daemon", "ready").await;

    emit(&progress, "Starting inner services", 3, 3).await;
    run_inner_compose_up(docker, &container_id).await?;
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
    docker: &Docker,
    record: &SsgRecord,
    existing_plans: Vec<SsgServicePortPlan>,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgStartOutcome> {
    stop_ssg(docker, record).await?;
    start_ssg(docker, record, existing_plans, progress).await
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
pub async fn rm_ssg(docker: &Docker, record: &SsgRecord, with_data: bool) -> Result<()> {
    if let Some(ref cid) = record.container_id {
        let runtime = DindRuntime::with_client(docker.clone());
        let was_running = record.status == "running";
        teardown_ssg_container(docker, &runtime, cid, was_running, with_data).await;
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
    docker: &Docker,
    runtime: &DindRuntime,
    cid: &str,
    was_running: bool,
    with_data: bool,
) {
    if !was_running {
        transient_start_for_cleanup(docker, runtime, cid).await;
    }

    let _ = runtime
        .exec_in_coast(
            cid,
            &[
                "docker",
                "compose",
                "-f",
                &inner_compose_path(),
                "-p",
                SSG_COMPOSE_PROJECT,
                "down",
            ],
        )
        .await;

    if with_data {
        remove_inner_named_volumes(runtime, cid).await;
    }

    if let Err(e) = runtime.remove_coast_container(cid).await {
        warn!(error = %e, container_id = %cid, "remove_coast_container failed; state will be cleared regardless");
    }
}

async fn transient_start_for_cleanup(docker: &Docker, runtime: &DindRuntime, cid: &str) {
    if let Err(e) = runtime.start_coast_container(cid).await {
        warn!(error = %e, container_id = %cid, "transient start before rm failed");
        return;
    }
    let manager = ContainerManager::with_timeout(
        DindRuntime::with_client(docker.clone()),
        INNER_DAEMON_TIMEOUT_SECS,
    );
    let _ = manager.wait_for_inner_daemon(cid).await;
}

async fn remove_inner_named_volumes(runtime: &DindRuntime, cid: &str) {
    let list = match runtime
        .exec_in_coast(
            cid,
            &[
                "docker",
                "volume",
                "ls",
                "-q",
                "--filter",
                &format!("label=com.docker.compose.project={SSG_COMPOSE_PROJECT}"),
            ],
        )
        .await
    {
        Ok(out) => out,
        Err(e) => {
            warn!(error = %e, "failed to list inner named volumes; skipping volume removal");
            return;
        }
    };
    for vol in list.stdout.lines().filter(|l| !l.trim().is_empty()) {
        let vol = vol.trim();
        if let Err(e) = runtime
            .exec_in_coast(cid, &["docker", "volume", "rm", vol])
            .await
        {
            warn!(error = %e, volume = %vol, "failed to remove inner named volume");
        }
    }
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
    docker: &Docker,
    record: &SsgRecord,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<String> {
    let container_id = record
        .container_id
        .clone()
        .ok_or_else(|| CoastError::coastfile("SSG record has no container id; nothing to tail."))?;
    let runtime = DindRuntime::with_client(docker.clone());

    let tail_value = tail.unwrap_or(200).to_string();

    let output = if let Some(ref svc) = service {
        let compose_file = inner_compose_path();
        runtime
            .exec_in_coast(
                &container_id,
                &[
                    "docker",
                    "compose",
                    "-f",
                    &compose_file,
                    "-p",
                    SSG_COMPOSE_PROJECT,
                    "logs",
                    "--tail",
                    &tail_value,
                    svc,
                ],
            )
            .await?
    } else {
        let args: Vec<String> = vec![
            "logs".to_string(),
            "--tail".to_string(),
            tail_value.clone(),
            container_id.clone(),
        ];
        run_host_docker(&args)?
    };

    Ok(if output.stdout.is_empty() {
        output.stderr
    } else {
        output.stdout
    })
}

// --- exec ------------------------------------------------------------------

/// Execute a command inside the outer DinD or a named inner service.
///
/// With `service`, runs `docker compose exec -T <service> <cmd...>`
/// inside the outer DinD. Without, execs directly on the outer DinD.
/// Returns the combined stdout / stderr.
pub async fn exec_ssg(
    docker: &Docker,
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
    let runtime = DindRuntime::with_client(docker.clone());

    let result = if let Some(svc) = service {
        let compose_file = inner_compose_path();
        let mut full: Vec<String> = vec![
            "docker".to_string(),
            "compose".to_string(),
            "-f".to_string(),
            compose_file,
            "-p".to_string(),
            SSG_COMPOSE_PROJECT.to_string(),
            "exec".to_string(),
            "-T".to_string(),
            svc,
        ];
        full.extend(command);
        let refs: Vec<&str> = full.iter().map(String::as_str).collect();
        runtime.exec_in_coast(&container_id, &refs).await?
    } else {
        let refs: Vec<&str> = command.iter().map(String::as_str).collect();
        runtime.exec_in_coast(&container_id, &refs).await?
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
/// `checked_out` is always `false` in Phase 3 — Phase 6 adds real
/// checkout accounting via `ssg_port_checkouts`.
pub fn ports_ssg(state: &dyn SsgStateExt) -> Result<SsgResponse> {
    let services = state.list_ssg_services()?;
    let record = state.get_ssg()?;

    let ports: Vec<SsgPortInfo> = services
        .iter()
        .map(|s| SsgPortInfo {
            service: s.service_name.clone(),
            canonical_port: s.container_port,
            dynamic_host_port: s.dynamic_host_port,
            checked_out: false,
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
    })
}

// --- internals -------------------------------------------------------------

pub(crate) fn inner_compose_path() -> String {
    format!("{INNER_ARTIFACT_DIR}/compose.yml")
}

async fn create_ssg_container(
    docker: &Docker,
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
        container_name_override: Some(SSG_CONTAINER_NAME.to_string()),
        ..DindConfigParams::new("coast", "ssg", build_dir)
    });

    for plan in plans {
        config.published_ports.push(PortPublish {
            host_port: plan.dynamic_host_port,
            container_port: plan.container_port,
        });
    }

    let runtime = DindRuntime::with_client(docker.clone());
    runtime.create_coast_container(&config).await
}

async fn run_inner_compose_up(docker: &Docker, container_id: &str) -> Result<()> {
    let runtime = DindRuntime::with_client(docker.clone());
    let compose_file = inner_compose_path();
    let result = runtime
        .exec_in_coast(
            container_id,
            &[
                "docker",
                "compose",
                "-f",
                &compose_file,
                "-p",
                SSG_COMPOSE_PROJECT,
                "up",
                "-d",
                "--remove-orphans",
            ],
        )
        .await?;
    if !result.success() {
        return Err(CoastError::docker(format!(
            "docker compose up -d failed (exit {}). stderr: {}",
            result.exit_code, result.stderr
        )));
    }
    Ok(())
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
    docker: &Docker,
    container_id: &str,
    manifest: &SsgManifest,
) -> Result<usize> {
    let runtime = DindRuntime::with_client(docker.clone());

    // Query already-loaded images in the inner daemon to skip re-loads
    // (relevant when restarting an existing SSG on a cached DinD volume).
    let existing = query_existing_inner_images(&runtime, container_id).await;

    let mut loaded = 0usize;
    let mut seen_images: HashSet<String> = HashSet::new();

    for svc in &manifest.services {
        if !seen_images.insert(svc.image.clone()) {
            continue;
        }
        if existing.contains(&svc.image) {
            info!(image = %svc.image, "image already in inner daemon; skipping load");
            continue;
        }

        let safe_name = svc.image.replace(['/', ':'], "_");
        let inner_tarball_path = format!("{INNER_IMAGE_CACHE_DIR}/{safe_name}.tar");

        let result = runtime
            .exec_in_coast(container_id, &["docker", "load", "-i", &inner_tarball_path])
            .await?;
        if !result.success() {
            return Err(CoastError::docker(format!(
                "docker load failed for image '{}' (exit {}). stderr: {}",
                svc.image, result.exit_code, result.stderr
            )));
        }
        loaded += 1;
    }

    Ok(loaded)
}

async fn query_existing_inner_images(runtime: &DindRuntime, container_id: &str) -> HashSet<String> {
    match runtime
        .exec_in_coast(
            container_id,
            &["docker", "images", "--format", "{{.Repository}}:{{.Tag}}"],
        )
        .await
    {
        Ok(result) if result.success() => result
            .stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && l != "<none>:<none>")
            .collect(),
        _ => HashSet::new(),
    }
}

/// Run `docker <args>` on the host Docker daemon in a blocking context.
///
/// Used for reading outer DinD logs without round-tripping through the
/// inner daemon.
fn run_host_docker(args: &[String]) -> Result<coast_docker::runtime::ExecResult> {
    let output = std::process::Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| CoastError::Docker {
            message: format!("failed to spawn `docker {}`: {e}", args.join(" ")),
            source: Some(Box::new(e)),
        })?;
    Ok(coast_docker::runtime::ExecResult {
        exit_code: i64::from(output.status.code().unwrap_or(-1)),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests focus on the small pure helpers. Lifecycle orchestration
    // is covered by the Phase 3 integration tests (test_ssg_run_lifecycle,
    // test_ssg_bind_mount_symmetric, test_ssg_named_volume_persists).

    #[test]
    fn inner_compose_path_is_under_artifact_dir() {
        assert_eq!(inner_compose_path(), "/coast-artifact/compose.yml");
    }

    #[test]
    fn build_response_pairs_plans_with_manifest_services() {
        let manifest = SsgManifest {
            build_id: "b_20260101000000".to_string(),
            built_at: chrono::Utc::now(),
            coastfile_hash: "h".to_string(),
            services: vec![
                crate::build::artifact::SsgManifestService {
                    name: "postgres".to_string(),
                    image: "postgres:16".to_string(),
                    ports: vec![5432],
                    env_keys: vec![],
                    volumes: vec![],
                    auto_create_db: false,
                },
                crate::build::artifact::SsgManifestService {
                    name: "redis".to_string(),
                    image: "redis:7".to_string(),
                    ports: vec![6379],
                    env_keys: vec![],
                    volumes: vec![],
                    auto_create_db: false,
                },
            ],
        };
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
        let manifest = SsgManifest {
            build_id: "b_20260101000000".to_string(),
            built_at: chrono::Utc::now(),
            coastfile_hash: "h".to_string(),
            services: vec![crate::build::artifact::SsgManifestService {
                name: "sidecar".to_string(),
                image: "alpine:3".to_string(),
                ports: vec![],
                env_keys: vec![],
                volumes: vec![],
                auto_create_db: false,
            }],
        };
        let resp = build_response(&manifest, &[], Some("running"), None, "ok".to_string());
        assert_eq!(resp.services[0].inner_port, 0);
        assert_eq!(resp.services[0].dynamic_host_port, 0);
        assert!(resp.ports.is_empty());
    }

    #[test]
    fn constants_are_coast_ssg() {
        assert_eq!(SSG_CONTAINER_NAME, "coast-ssg");
        assert_eq!(SSG_COMPOSE_PROJECT, "coast-ssg");
    }
}
