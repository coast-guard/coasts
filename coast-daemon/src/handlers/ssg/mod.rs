//! Handler for `coast ssg *` requests (non-streaming variants).
//!
//! Phase 2 landed `Ps`. Phase 3 lands the full lifecycle:
//! `Stop`, `Rm`, `Logs` (non-follow), `Exec`, `Ports`. The streaming
//! variants (`Build`, `Run`, `Start`, `Restart`, `Logs { follow: true }`)
//! never reach this handler — they are intercepted by the streaming
//! routers in `server.rs`.
//!
//! Mutating verbs acquire `AppState.ssg_mutex` before dispatching
//! into `coast_ssg::daemon_integration`. Read-only verbs do not.
//! See `coast-ssg/DESIGN.md §17-5` for mutex scope.
//!
//! Lifecycle functions do not touch the SQLite state DB themselves —
//! this handler reads the current state before the async Docker
//! section and applies writes afterwards. That split exists because
//! `StateDb` wraps a `!Sync` `rusqlite::Connection`, which would
//! otherwise reject the `Send` bound on streaming futures.

// ssg-phase-6: checkout / uncheckout orchestrator (host-side canonical
// port binding via socat). Lives in a sibling file to keep `mod.rs`
// focused on the dispatcher + non-checkout lifecycle verbs.
pub mod checkout;

// ssg-phase-8: host bind-mount permission doctor. See `doctor.rs` +
// `coast-ssg/src/doctor.rs` for the pure evaluator.
pub mod doctor;

// ssg-phase-26 / 28 (§24.5): stable virtual-port allocator.
// Wired into production by Phase 28's host_socat lifecycle hooks.
pub(crate) mod virtual_port_allocator;

// ssg-phase-27 / 28 (§24): daemon-managed host socat supervisor. One
// long-lived process per `(project, service_name, container_port)`
// pair, bound to the stable virtual port from Phase 26, forwarding
// to the current SSG dynamic port. Phase 28 wired this into SSG
// lifecycle verbs (run/start/restart/stop/rm) plus daemon-start
// reconciliation. Replaces the legacy `consumer_refresh` machinery
// (deleted in Phase 28).
pub(crate) mod host_socat;

// ssg-phase-15: `coast ssg import-host-volume` — zero-copy migration
// of existing host Docker named volumes into SSG bind-mount entries.
// See `coast-ssg/DESIGN.md §10.7`.
pub mod host_volume_import;

// ssg-phase-16: consumer pinning — `coast ssg checkout-build`,
// `uncheckout-build`, and `show-pin`. See
// `coast-ssg/DESIGN.md §17-9` (SETTLED — Phase 16).
pub mod pin;

use std::sync::Arc;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SsgAction, SsgListing, SsgRequest, SsgResponse};
use coast_ssg::state::{SsgRecord, SsgStateExt};

use crate::server::AppState;

/// Dispatch a non-streaming SSG request.
///
/// `Build`, `Run`, `Start`, `Restart`, and `Logs { follow: true }`
/// never reach this handler — they are intercepted upstream.
pub async fn handle(state: Arc<AppState>, req: SsgRequest) -> Result<SsgResponse> {
    let SsgRequest { project, action } = req;
    match action {
        SsgAction::Ps => {
            let db = state.db.lock().await;
            coast_ssg::daemon_integration::ps_ssg(&project, Some(&*db as &dyn SsgStateExt))
        }
        SsgAction::Ports => {
            let db = state.db.lock().await;
            coast_ssg::daemon_integration::ports_ssg(&project, &*db)
        }

        SsgAction::Stop { force } => handle_stop(&project, &state, force).await,
        SsgAction::Rm { with_data, force } => handle_rm(&project, &state, with_data, force).await,

        SsgAction::Logs {
            service,
            tail,
            follow,
        } => {
            if follow {
                unreachable!(
                    "SsgAction::Logs {{ follow: true }} handled by handle_ssg_logs_streaming"
                )
            }
            handle_logs(&project, &state, service, tail).await
        }

        SsgAction::Exec { service, command } => {
            handle_exec(&project, &state, service, command).await
        }

        SsgAction::Run => {
            unreachable!("SsgAction::Run handled by handle_ssg_lifecycle_streaming")
        }
        SsgAction::Start => {
            unreachable!("SsgAction::Start handled by handle_ssg_lifecycle_streaming")
        }
        SsgAction::Restart => {
            unreachable!("SsgAction::Restart handled by handle_ssg_lifecycle_streaming")
        }
        SsgAction::Build { .. } => {
            unreachable!("SsgAction::Build handled by handle_ssg_build_streaming")
        }

        SsgAction::Checkout { service, all } => {
            checkout::handle_checkout(&project, &state, service, all).await
        }
        SsgAction::Uncheckout { service, all } => {
            checkout::handle_uncheckout(&project, &state, service, all).await
        }

        SsgAction::Doctor => doctor::handle_doctor(&project, &state).await,

        SsgAction::CheckoutBuild { build_id } => {
            pin::handle_checkout_build(&state, project, build_id).await
        }
        SsgAction::UncheckoutBuild => pin::handle_uncheckout_build(&state, project).await,
        SsgAction::ShowPin => pin::handle_show_pin(&state, project).await,

        SsgAction::ImportHostVolume {
            volume,
            service,
            mount,
            file,
            working_dir,
            config,
            apply,
        } => {
            host_volume_import::handle_import_host_volume(
                &project,
                &state,
                host_volume_import::ImportHostVolumeArgs {
                    volume,
                    service,
                    mount,
                    file,
                    working_dir,
                    config,
                    apply,
                },
            )
            .await
        }

        // Cross-project verb: ignore `project` (CLI sends empty string).
        // See `coast-ssg/DESIGN.md §23` — Phase 22.
        SsgAction::Ls => handle_ls(&state).await,

        // Project-scoped: list build artifacts under
        // `~/.coast/ssg/<project>/builds/`. Backs the SPA's "SHARED
        // SERVICE GROUPS" subsection on the project detail page.
        SsgAction::BuildsLs => handle_builds_ls(&project, &state).await,
    }
}

/// `coast ssg ls` — list every per-project SSG known to the daemon.
///
/// Cross-project: enumerates every row in the `ssg` table and folds
/// in a per-project service count from `ssg_services`. Returns an
/// empty `listings` vec when no project has run `coast ssg run` yet.
async fn handle_ls(state: &Arc<AppState>) -> Result<SsgResponse> {
    let (rows, listings) = {
        let db = state.db.lock().await;
        let rows = db.list_ssgs()?;
        let mut listings: Vec<SsgListing> = Vec::with_capacity(rows.len());
        for rec in &rows {
            let svc_count = match db.list_ssg_services(&rec.project) {
                Ok(list) => list.len() as u32,
                Err(_) => 0,
            };
            listings.push(SsgListing {
                project: rec.project.clone(),
                status: rec.status.clone(),
                build_id: rec.build_id.clone(),
                container_id: rec.container_id.clone(),
                service_count: svc_count,
                created_at: rec.created_at.clone(),
            });
        }
        (rows, listings)
    };

    let message = if rows.is_empty() {
        "No SSGs registered. Run `coast ssg run` from a project to create one.".to_string()
    } else {
        format!("{} SSG(s) across {} project(s).", rows.len(), rows.len())
    };

    Ok(SsgResponse {
        message,
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings,
        builds: Vec::new(),
    })
}

/// `coast ssg builds-ls` — list SSG build artifacts for `project`.
///
/// Walks `~/.coast/ssg/<project>/builds/<build_id>/manifest.json`,
/// parses each manifest, and returns one [`SsgBuildEntry`] per
/// build_id sorted by `created_at_unix` descending. Cross-references
/// `state.db` for `latest_build_id` and the project's SSG pin to
/// populate the `latest` / `pinned` flags.
///
/// Backs `GET /api/v1/ssg/builds?project=<p>` (the SPA's "SHARED
/// SERVICE GROUPS" subsection).
async fn handle_builds_ls(project: &str, state: &Arc<AppState>) -> Result<SsgResponse> {
    let entries = collect_ssg_build_entries(project, state).await?;
    let count = entries.len();
    let message = if count == 0 {
        format!("No SSG builds for project '{project}'.")
    } else {
        format!("{count} SSG build(s) for project '{project}'.")
    };
    Ok(SsgResponse {
        message,
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: entries,
    })
}

/// Collect SSG build artifacts for `project`.
///
/// SSG build artifacts live in a globally-shared pool at
/// `~/.coast/ssg/builds/<build_id>/`. Project ownership is encoded
/// via the manifest's `coastfile_hash` field (the same hash that
/// forms the `<build_id>` prefix). Two projects whose SSG
/// `Coastfile.shared_service_groups` content matches share artifacts.
///
/// Filter strategy: look up the project's `latest_build_id` from
/// `state.db.ssg.<project>`, read its manifest to get the project's
/// canonical `coastfile_hash`, then return every build whose
/// manifest's `coastfile_hash` matches. The currently-pinned
/// build_id (if any) is always included, even if its hash drifted
/// (rare — usually you only pin within the same hash family).
///
/// Returns an empty list when the project has never built.
async fn collect_ssg_build_entries(
    project: &str,
    state: &Arc<AppState>,
) -> Result<Vec<coast_core::protocol::SsgBuildEntry>> {
    use coast_core::protocol::SsgBuildEntry;

    let builds_root = ssg_builds_root();
    let read_dir = match std::fs::read_dir(&builds_root) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(CoastError::io(
                format!("Failed to read SSG builds directory: {e}"),
                builds_root,
            ));
        }
    };

    let (latest_build_id, pinned_build_id) = {
        let db = state.db.lock().await;
        let latest = db
            .get_ssg(project)
            .ok()
            .flatten()
            .and_then(|r| r.latest_build_id.clone());
        let pinned = db
            .get_ssg_consumer_pin(project)
            .ok()
            .flatten()
            .map(|p| p.build_id);
        (latest, pinned)
    };

    // Resolve the project's canonical `coastfile_hash` from its
    // `latest_build_id`'s manifest. If the project has never built
    // (no `latest_build_id`) AND has no pin, there's nothing to
    // anchor the filter against — return empty. If only a pin is
    // set, anchor on the pin's hash so freshly-pinned-but-never-built
    // projects still surface their pinned build.
    let project_hash: Option<String> = match latest_build_id
        .as_deref()
        .or(pinned_build_id.as_deref())
    {
        Some(anchor_id) => read_coastfile_hash(&builds_root.join(anchor_id).join("manifest.json")),
        None => return Ok(Vec::new()),
    };

    let mut entries: Vec<SsgBuildEntry> = Vec::new();
    for dir_entry in read_dir.flatten() {
        let build_dir = dir_entry.path();
        if !build_dir.is_dir() {
            continue;
        }
        let manifest_path = build_dir.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let Some(build_id) = build_dir.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };

        let entry_hash = manifest
            .get("coastfile_hash")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let is_pinned = pinned_build_id.as_deref() == Some(build_id);
        // Include the build IF it matches the project's hash, OR
        // it's the project's pin (covers the rare cross-hash pin
        // edge case).
        let belongs_to_project = match &project_hash {
            Some(h) => &entry_hash == h,
            None => false,
        };
        if !belongs_to_project && !is_pinned {
            continue;
        }

        let services: Vec<String> = manifest
            .get("services")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|svc| {
                        svc.get("name")
                            .and_then(|n| n.as_str())
                            .map(std::string::ToString::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Manifest writes `built_at` (RFC3339); fall back to
        // `created_at` for forward-compat with any future schema
        // change, then to `created_at_unix` if a Unix epoch was
        // pre-computed elsewhere.
        let created_at_unix = manifest
            .get("built_at")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp())
            .or_else(|| {
                manifest
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.timestamp())
            })
            .or_else(|| {
                manifest
                    .get("created_at_unix")
                    .and_then(serde_json::Value::as_i64)
            })
            .unwrap_or(0);

        let services_count = u32::try_from(services.len()).unwrap_or(u32::MAX);
        let latest = latest_build_id.as_deref() == Some(build_id);

        entries.push(SsgBuildEntry {
            build_id: build_id.to_string(),
            project: project.to_string(),
            created_at_unix,
            services,
            services_count,
            pinned: is_pinned,
            latest,
        });
    }

    entries.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix));
    Ok(entries)
}

/// Read a manifest's `coastfile_hash` field, returning `None` if
/// the file is missing, malformed, or lacks the field.
fn read_coastfile_hash(manifest_path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&raw).ok()?;
    manifest
        .get("coastfile_hash")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
}

/// Resolve `~/.coast/ssg/builds/` for the running daemon. SSG
/// artifacts are stored globally (project-agnostic on the
/// filesystem); per-project filtering happens via the manifest's
/// `coastfile_hash`. See [`collect_ssg_build_entries`].
fn ssg_builds_root() -> std::path::PathBuf {
    crate::handlers::run::paths::active_coast_home()
        .join("ssg")
        .join("builds")
}

async fn handle_stop(project: &str, state: &Arc<AppState>, force: bool) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot stop the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg(project)?
    };
    let Some(record) = record else {
        return Ok(build_stop_response_missing_record(project));
    };

    // Phase 4.5 gate: refuse to stop while remote shadow coasts are
    // currently consuming the SSG unless `--force` is set. With
    // `--force`, kill the reverse-tunnel ssh children first so the
    // shadow coast doesn't leak stale ssh processes.
    enforce_shadow_gate_and_maybe_tear_down(state, force, "stop").await?;

    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    coast_ssg::daemon_integration::stop_ssg(&ops, &record).await?;

    {
        let db = state.db.lock().await;
        db.upsert_ssg(
            project,
            "stopped",
            record.container_id.as_deref(),
            record.build_id.as_deref(),
        )?;
        for svc in db.list_ssg_services(project)? {
            db.update_ssg_service_status(project, &svc.service_name, "stopped")?;
        }
    }

    // Phase 6: preserve `ssg_port_checkouts` rows but null their
    // socat_pid columns and kill the live socats. Next `run / start`
    // re-spawns against the new dynamic ports.
    checkout::kill_active_checkout_socats_preserve_rows(project, state).await;

    // Phase 28: kill the per-service host socats so consumers
    // attempting to reach a stopped SSG fail fast with
    // `ECONNREFUSED` (no orphan socat forwarding traffic into a
    // dead dyn port). Virtual port allocations stay in
    // `ssg_virtual_ports` — the same numbers come back on the next
    // `ssg run/start` so consumer in-DinD socats never need to
    // change.
    kill_host_socats_for_project_services(project, state).await;

    Ok(build_stop_response_success())
}

async fn handle_rm(
    project: &str,
    state: &Arc<AppState>,
    with_data: bool,
    force: bool,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot remove the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg(project)?
    };
    let Some(record) = record else {
        return Ok(build_rm_response_missing_record(project));
    };

    enforce_shadow_gate_and_maybe_tear_down(state, force, "remove").await?;

    // Phase 6: tear down checkouts before the SSG itself. Doing it
    // first means if the subsequent Docker rm fails and the user
    // retries, we don't end up with dangling checkout rows pointing
    // at a partially-removed SSG.
    checkout::kill_and_clear_all_checkouts(project, state).await;

    // Phase 28: snapshot the service rows BEFORE the Docker rm runs
    // so we still know which host socats to kill even after
    // `clear_ssg_services` wipes the table below.
    let services_for_socat_teardown = {
        let db = state.db.lock().await;
        db.list_ssg_services(project).unwrap_or_default()
    };

    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    coast_ssg::daemon_integration::rm_ssg(&ops, &record, with_data).await?;

    {
        let db = state.db.lock().await;
        db.clear_ssg_services(project)?;
        if with_data {
            // `--with-data` means the user is wiping identity too.
            // Release the virtual port band slots so they can be
            // reused by other projects on the same host. Wipe the
            // entire `ssg` row including `latest_build_id` so the
            // next `Run` requires a fresh build (matches CLI
            // expectations for `coast ssg rm --with-data`).
            db.clear_ssg(project)?;
            db.clear_ssg_virtual_ports(project)?;
        } else {
            // Without `--with-data`, virtual ports survive — the
            // user is doing a rebuild-style rm/run cycle and
            // consumer socats must keep pointing at the same
            // targets. Preserve `latest_build_id` for the same
            // reason: the next `Run` can re-create the container
            // from the existing build artifact without forcing the
            // user back to the Builds tab. See
            // `coast-ssg/DESIGN.md §32`.
            db.clear_ssg_runtime_only(project)?;
        }
    }

    // Phase 28: kill the per-service host socats. We do this after
    // the Docker rm so a failed rm doesn't leave us with neither a
    // dyn port nor a socat (which would silently hide the failure
    // until the next `run`).
    use crate::handlers::ssg::host_socat;
    for svc in services_for_socat_teardown {
        if let Err(err) = host_socat::kill(project, &svc.service_name, svc.container_port) {
            tracing::warn!(
                project = %project,
                service = %svc.service_name,
                container_port = svc.container_port,
                error = %err,
                "host socat kill failed during ssg rm; pidfile may be stale"
            );
        }
    }

    Ok(build_rm_response_success(with_data))
}

/// Kill every host socat backing a service in `project`. Used by
/// `handle_stop` (preserves virtual port allocations) and called
/// inline from `handle_rm` (which also clears virtual ports under
/// `--with-data`). Reads `ssg_services` to enumerate the
/// `(service, container_port)` pairs.
async fn kill_host_socats_for_project_services(project: &str, state: &Arc<AppState>) {
    use crate::handlers::ssg::host_socat;
    use coast_ssg::state::SsgStateExt;

    let services = {
        let db = state.db.lock().await;
        db.list_ssg_services(project).unwrap_or_default()
    };
    for svc in services {
        if let Err(err) = host_socat::kill(project, &svc.service_name, svc.container_port) {
            tracing::warn!(
                project = %project,
                service = %svc.service_name,
                container_port = svc.container_port,
                error = %err,
                "host socat kill failed during ssg stop; pidfile may be stale"
            );
        }
    }
}

async fn handle_logs(
    project: &str,
    state: &Arc<AppState>,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot tail SSG logs."))?;

    let record = fetch_required_record(project, state).await?;
    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    let text = coast_ssg::daemon_integration::logs_ssg(&ops, &record, service, tail).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    })
}

async fn handle_exec(
    project: &str,
    state: &Arc<AppState>,
    service: Option<String>,
    command: Vec<String>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot exec against the SSG."))?;

    let record = fetch_required_record(project, state).await?;
    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    let text = coast_ssg::daemon_integration::exec_ssg(&ops, &record, service, command).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    })
}

async fn fetch_required_record(project: &str, state: &Arc<AppState>) -> Result<SsgRecord> {
    let db = state.db.lock().await;
    db.get_ssg(project)?.ok_or_else(|| {
        CoastError::coastfile(format!(
            "SSG for project '{project}' has not been created. \
             Run `coast ssg run` first."
        ))
    })
}

/// Identifier for a shadow instance that is currently consuming the SSG.
#[derive(Debug, Clone)]
struct ShadowUsingSsg {
    project: String,
    instance: String,
    remote_host: String,
}

impl std::fmt::Display for ShadowUsingSsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}@{}", self.project, self.instance, self.remote_host)
    }
}

/// Enforce the Phase 4.5 §20.6 block: refuse `coast ssg stop/rm` while
/// any remote shadow instance references the SSG, unless `force` is
/// set. With `force`, kill the tracked reverse-tunnel PIDs for each
/// blocking shadow before returning so the caller can proceed.
async fn enforce_shadow_gate_and_maybe_tear_down(
    state: &Arc<AppState>,
    force: bool,
    verb: &str,
) -> Result<()> {
    let shadows = collect_remote_shadows_using_ssg(state).await?;
    if shadows.is_empty() {
        return Ok(());
    }

    if !force {
        return Err(CoastError::state(format_shadow_gate_error(&shadows, verb)));
    }

    // --force: tear down recorded reverse-tunnel PIDs for each shadow.
    let mut map = state.shared_service_tunnel_pids.lock().await;
    for shadow in &shadows {
        if let Some(pids) = map.remove(&(shadow.project.clone(), shadow.instance.clone())) {
            for pid in pids {
                kill_ssh_tunnel_pid(pid);
            }
        }
    }
    Ok(())
}

/// Read shadow instances from the state DB and return those whose
/// artifact Coastfile has at least one `shared_service_group_refs`
/// entry (i.e. those actively consuming SSG services).
///
/// Reads artifact Coastfiles with best-effort IO: if an artifact dir
/// is missing or parse-fails, the shadow is skipped rather than
/// failing the entire gate. This matches `provision::load_coastfile_resources`'s
/// lenient reading behavior (missing artifact -> empty resources).
async fn collect_remote_shadows_using_ssg(state: &Arc<AppState>) -> Result<Vec<ShadowUsingSsg>> {
    let shadow_rows = {
        let db = state.db.lock().await;
        db.list_instances()?
            .into_iter()
            .filter(|inst| inst.remote_host.is_some())
            .map(|inst| {
                (
                    inst.project.clone(),
                    inst.name.clone(),
                    inst.build_id.clone(),
                    inst.remote_host.clone().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
    };

    let mut result = Vec::new();
    for (project, instance, build_id, remote_host) in shadow_rows {
        let artifact_dir = resolve_artifact_dir(&project, build_id.as_deref());
        let coastfile_path = artifact_dir.join("coastfile.toml");
        if !coastfile_path.exists() {
            continue;
        }
        if coastfile_has_ssg_refs(&coastfile_path) {
            result.push(ShadowUsingSsg {
                project,
                instance,
                remote_host,
            });
        }
    }
    Ok(result)
}

/// Minimal TOML scan: does this Coastfile declare at least one
/// `[shared_services.*]` entry with `from_group = true`?
///
/// We cannot use the full `Coastfile::from_file` parser because
/// artifact coastfiles are always written to `coastfile.toml`, which
/// the parser rejects when a `[remote]` section is present (the
/// `[remote]` section is gated on the filename `Coastfile.remote*`).
/// For the shadow-gate we don't need validation — we only need the
/// single boolean "does this consumer reference the SSG?". A tiny
/// custom deserializer avoids that filename coupling.
fn coastfile_has_ssg_refs(path: &std::path::Path) -> bool {
    #[derive(serde::Deserialize)]
    struct MinimalCf {
        #[serde(default)]
        shared_services: std::collections::HashMap<String, MinimalSvc>,
    }
    #[derive(serde::Deserialize)]
    struct MinimalSvc {
        #[serde(default)]
        from_group: bool,
    }

    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(cf) = toml::from_str::<MinimalCf>(&contents) else {
        return false;
    };
    cf.shared_services.values().any(|s| s.from_group)
}

/// Mirror of `provision::resolve_artifact_dir`, kept inline so this
/// handler doesn't need `pub(super)` access into the run submodule.
fn resolve_artifact_dir(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let project_images_dir = crate::handlers::run::paths::project_images_dir(project);
    if let Some(bid) = build_id {
        let resolved = project_images_dir.join(bid);
        if resolved.exists() {
            return resolved;
        }
    }
    project_images_dir.join("latest")
}

/// Send SIGTERM to a reverse-tunnel ssh child PID. Best-effort: a
/// missing PID (already died) is not an error.
pub(crate) fn kill_ssh_tunnel_pid(pid: u32) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    let Ok(signed_pid) = i32::try_from(pid) else {
        tracing::warn!(pid = %pid, "reverse-tunnel PID does not fit in i32; skipping kill");
        return;
    };
    match kill(Pid::from_raw(signed_pid), Signal::SIGTERM) {
        Ok(()) => {
            tracing::info!(pid = %pid, "killed reverse-tunnel ssh child on --force");
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Already gone.
        }
        Err(err) => {
            tracing::warn!(pid = %pid, error = %err, "failed to SIGTERM reverse-tunnel PID");
        }
    }
}

// --- Phase 9 pure response helpers -------------------------------------
//
// Each `handle_*` above is split into (a) state-read + Docker side-
// effects (remain async and Docker-dependent) and (b) pure response
// shaping (these functions). The pure halves are fully unit-tested.

/// Response for `coast ssg stop` when no SSG record exists.
fn build_stop_response_missing_record(project: &str) -> SsgResponse {
    SsgResponse {
        message: format!("SSG for project '{project}' has not been created. Nothing to stop."),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

/// Response for `coast ssg stop` after a successful stop.
fn build_stop_response_success() -> SsgResponse {
    SsgResponse {
        message: "SSG stopped.".to_string(),
        status: Some("stopped".to_string()),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

/// Response for `coast ssg rm` when no SSG record exists.
fn build_rm_response_missing_record(project: &str) -> SsgResponse {
    SsgResponse {
        message: format!("SSG for project '{project}' has not been created. Nothing to remove."),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

/// Response for `coast ssg rm [--with-data]` after a successful remove.
fn build_rm_response_success(with_data: bool) -> SsgResponse {
    let suffix = if with_data { " (with data)" } else { "" };
    SsgResponse {
        message: format!("SSG removed{suffix}."),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

/// Build the error message for the shadow-gate refusal. Pure function
/// of the blocking shadows + the verb being gated. Extracted so
/// tests can assert wording without synthesizing the full
/// `CoastError` + `AppState`.
fn format_shadow_gate_error(shadows: &[ShadowUsingSsg], verb: &str) -> String {
    let list = shadows
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SSG is currently serving remote coast(s) [{list}]. \
         Running `coast ssg {verb}` now will break their shared-service \
         connectivity. Stop those remotes first, or re-run with \
         --force to tear down their reverse tunnels and proceed.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_using_ssg_display_is_project_instance_at_remote() {
        let s = ShadowUsingSsg {
            project: "my-app".to_string(),
            instance: "dev-1".to_string(),
            remote_host: "host-a".to_string(),
        };
        assert_eq!(s.to_string(), "my-app/dev-1@host-a");
    }

    fn write_temp_coastfile(content: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn coastfile_has_ssg_refs_detects_from_group_true() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
from_group = true
"#,
        );
        assert!(coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_tolerates_remote_section() {
        // Artifact coastfiles for remote instances include [remote],
        // but are saved as `coastfile.toml`. The full parser rejects
        // that combination; our minimal scanner must accept it.
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
from_group = true

[remote]
"#,
        );
        assert!(coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_for_inline_only() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
image = "postgres:16-alpine"
"#,
        );
        assert!(!coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_when_no_shared_services() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"
"#,
        );
        assert!(!coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_on_missing_file() {
        let nonexistent = std::path::PathBuf::from("/tmp/coast-nonexistent-xyz-404.toml");
        assert!(!coastfile_has_ssg_refs(&nonexistent));
    }

    // --- Phase 9 pure response helpers ---

    #[test]
    fn stop_response_missing_record_has_nothing_to_stop_message() {
        let r = build_stop_response_missing_record("test-proj");
        assert_eq!(
            r.message,
            "SSG for project 'test-proj' has not been created. Nothing to stop."
        );
        assert!(r.status.is_none());
        assert!(r.services.is_empty());
        assert!(r.ports.is_empty());
        assert!(r.findings.is_empty());
    }

    #[test]
    fn stop_response_success_reports_stopped_status() {
        let r = build_stop_response_success();
        assert_eq!(r.message, "SSG stopped.");
        assert_eq!(r.status.as_deref(), Some("stopped"));
    }

    #[test]
    fn rm_response_missing_record_has_nothing_to_remove_message() {
        let r = build_rm_response_missing_record("test-proj");
        assert_eq!(
            r.message,
            "SSG for project 'test-proj' has not been created. Nothing to remove."
        );
        assert!(r.status.is_none());
    }

    #[test]
    fn rm_response_success_without_data_has_no_suffix() {
        let r = build_rm_response_success(false);
        assert_eq!(r.message, "SSG removed.");
        assert!(r.status.is_none());
    }

    #[test]
    fn rm_response_success_with_data_appends_with_data_suffix() {
        let r = build_rm_response_success(true);
        assert_eq!(r.message, "SSG removed (with data).");
    }

    #[test]
    fn shadow_gate_error_lists_all_blocking_shadows() {
        let shadows = vec![
            ShadowUsingSsg {
                project: "app-a".to_string(),
                instance: "dev-1".to_string(),
                remote_host: "host-x".to_string(),
            },
            ShadowUsingSsg {
                project: "app-b".to_string(),
                instance: "dev-2".to_string(),
                remote_host: "host-y".to_string(),
            },
        ];
        let msg = format_shadow_gate_error(&shadows, "stop");
        assert!(
            msg.contains("app-a/dev-1@host-x"),
            "first shadow missing; got: {msg}"
        );
        assert!(
            msg.contains("app-b/dev-2@host-y"),
            "second shadow missing; got: {msg}"
        );
        assert!(msg.contains("--force"), "must mention --force; got: {msg}");
    }

    #[test]
    fn shadow_gate_error_names_the_verb_via_coast_ssg_cmd() {
        let shadows = vec![ShadowUsingSsg {
            project: "p".to_string(),
            instance: "i".to_string(),
            remote_host: "h".to_string(),
        }];
        let stop_msg = format_shadow_gate_error(&shadows, "stop");
        assert!(stop_msg.contains("`coast ssg stop`"), "got: {stop_msg}");
        let rm_msg = format_shadow_gate_error(&shadows, "remove");
        assert!(rm_msg.contains("`coast ssg remove`"), "got: {rm_msg}");
    }

    #[test]
    fn shadow_gate_error_with_single_shadow_has_no_comma() {
        let shadows = vec![ShadowUsingSsg {
            project: "only".to_string(),
            instance: "one".to_string(),
            remote_host: "h".to_string(),
        }];
        let msg = format_shadow_gate_error(&shadows, "stop");
        // The joined list should not contain ", " since only one shadow.
        let after_bracket = msg.split('[').nth(1).unwrap();
        let before_bracket = after_bracket.split(']').next().unwrap();
        assert_eq!(before_bracket, "only/one@h");
    }

    // -------------------------------------------------------------------
    // handle_builds_ls + collect_ssg_build_entries tests
    // -------------------------------------------------------------------

    /// Test scaffolding shared across the SSG-builds-list tests.
    /// Allocates a tempdir, points `COAST_HOME` at it (under the
    /// crate-wide env lock), and returns `(state, project, tempdir)`.
    /// Drop the tempdir guard last to clean up.
    struct BuildsLsFixture {
        _coast_home_guard: std::sync::MutexGuard<'static, ()>,
        prev_coast_home: Option<std::ffi::OsString>,
        _home: tempfile::TempDir,
        coast_home: std::path::PathBuf,
        state: std::sync::Arc<crate::server::AppState>,
        project: String,
    }

    impl BuildsLsFixture {
        fn new(project: &str) -> Self {
            let guard = crate::test_support::coast_home_env_lock();
            let prev_coast_home = std::env::var_os("COAST_HOME");
            let home = tempfile::tempdir().unwrap();
            let coast_home = home.path().join(".coast");
            std::fs::create_dir_all(&coast_home).unwrap();
            // Safety: serialized by the crate-wide coast_home_env_lock.
            unsafe {
                std::env::set_var("COAST_HOME", &coast_home);
            }

            let db = crate::state::StateDb::open_in_memory().unwrap();
            let state = std::sync::Arc::new(crate::server::AppState::new_for_testing(db));

            Self {
                _coast_home_guard: guard,
                prev_coast_home,
                _home: home,
                coast_home,
                state,
                project: project.to_string(),
            }
        }

        fn builds_dir(&self) -> std::path::PathBuf {
            // Global pool — SSG artifacts are not per-project on disk.
            self.coast_home.join("ssg").join("builds")
        }

        /// Write a fully-formed manifest with `coastfile_hash` +
        /// `built_at` + `services`. The build_id is derived as
        /// `<coastfile_hash>_<built_at_compact>`.
        fn write_manifest_full(
            &self,
            coastfile_hash: &str,
            built_at_rfc3339: &str,
            built_at_compact: &str,
            services: &[&str],
        ) -> String {
            let build_id = format!("{coastfile_hash}_{built_at_compact}");
            let dir = self.builds_dir().join(&build_id);
            std::fs::create_dir_all(&dir).unwrap();
            let services_json: Vec<serde_json::Value> = services
                .iter()
                .map(|name| serde_json::json!({ "name": name }))
                .collect();
            let manifest = serde_json::json!({
                "build_id": build_id,
                "coastfile_hash": coastfile_hash,
                "built_at": built_at_rfc3339,
                "services": services_json,
            });
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            build_id
        }

        fn make_dir_without_manifest(&self, build_id: &str) {
            let dir = self.builds_dir().join(build_id);
            std::fs::create_dir_all(&dir).unwrap();
            // Intentionally no `manifest.json`.
        }
    }

    impl Drop for BuildsLsFixture {
        fn drop(&mut self) {
            // Restore env var so subsequent tests see whatever was there
            // before. Safety: serialized by `_coast_home_guard`.
            match self.prev_coast_home.take() {
                Some(value) => unsafe { std::env::set_var("COAST_HOME", value) },
                None => unsafe { std::env::remove_var("COAST_HOME") },
            }
        }
    }

    #[tokio::test]
    async fn handle_builds_ls_returns_empty_for_missing_project_dir() {
        let fixture = BuildsLsFixture::new("missing-cg");
        // Don't write any manifests — the builds dir doesn't exist.
        let resp = handle_builds_ls(&fixture.project, &fixture.state)
            .await
            .expect("missing dir should be benign, not an error");
        assert!(resp.builds.is_empty());
        assert!(
            resp.message.contains("No SSG builds"),
            "unexpected message: {}",
            resp.message
        );
    }

    #[tokio::test]
    async fn handle_builds_ls_returns_sorted_desc() {
        let fixture = BuildsLsFixture::new("sort-cg");
        // All three builds share `coastfile_hash = "abc"` -> they
        // all belong to the project's SSG Coastfile family.
        let oldest = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let middle = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres", "redis"],
        );
        let newest = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );

        // Anchor the project on hash `abc` via `latest_build_id`.
        {
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &newest).unwrap();
        }

        let resp = handle_builds_ls(&fixture.project, &fixture.state)
            .await
            .unwrap();
        assert_eq!(resp.builds.len(), 3);
        assert_eq!(resp.builds[0].build_id, newest);
        assert_eq!(resp.builds[1].build_id, middle);
        assert_eq!(resp.builds[2].build_id, oldest);
        assert!(
            resp.builds[0].created_at_unix > resp.builds[1].created_at_unix,
            "must be sorted descending by created_at_unix"
        );
        assert_eq!(resp.builds[1].services, vec!["postgres", "redis"]);
        assert_eq!(resp.builds[1].services_count, 2);
        assert!(resp.builds[0].latest);
        assert!(!resp.builds[1].latest);
        assert!(!resp.builds[2].latest);
        for entry in &resp.builds {
            assert!(!entry.pinned);
        }
    }

    #[tokio::test]
    async fn handle_builds_ls_marks_latest_and_pinned() {
        let fixture = BuildsLsFixture::new("flags-cg");
        let a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let b = fixture.write_manifest_full(
            "abc",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["postgres"],
        );
        let c = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres"],
        );

        // Seed `latest_build_id = c` and pin `b`.
        {
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &c).unwrap();
            db.upsert_ssg_consumer_pin(&coast_ssg::state::SsgConsumerPinRecord {
                project: fixture.project.clone(),
                build_id: b.clone(),
                created_at: "2026-04-21T00:01:00+00:00".to_string(),
            })
            .unwrap();
        }

        let resp = handle_builds_ls(&fixture.project, &fixture.state)
            .await
            .unwrap();
        assert_eq!(resp.builds.len(), 3);

        let by_id: std::collections::HashMap<&str, &coast_core::protocol::SsgBuildEntry> = resp
            .builds
            .iter()
            .map(|e| (e.build_id.as_str(), e))
            .collect();
        assert!(by_id[c.as_str()].latest);
        assert!(!by_id[c.as_str()].pinned);
        assert!(by_id[b.as_str()].pinned);
        assert!(!by_id[b.as_str()].latest);
        assert!(!by_id[a.as_str()].latest);
        assert!(!by_id[a.as_str()].pinned);
    }

    #[tokio::test]
    async fn handle_builds_ls_skips_dirs_without_manifest() {
        let fixture = BuildsLsFixture::new("skip-cg");
        let good_a = fixture.write_manifest_full(
            "abc",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        fixture.make_dir_without_manifest("abc_orphan_b");
        let good_c = fixture.write_manifest_full(
            "abc",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["redis"],
        );
        fixture.make_dir_without_manifest("abc_orphan_d");

        // Anchor `latest_build_id` so the hash filter resolves.
        {
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &good_c).unwrap();
        }

        let resp = handle_builds_ls(&fixture.project, &fixture.state)
            .await
            .unwrap();
        let ids: Vec<&str> = resp.builds.iter().map(|e| e.build_id.as_str()).collect();
        assert_eq!(ids, vec![good_c.as_str(), good_a.as_str()]);
    }

    #[tokio::test]
    async fn handle_builds_ls_filters_by_coastfile_hash() {
        // Two distinct SSG Coastfiles (= two distinct
        // `coastfile_hash` values) share one global
        // `~/.coast/ssg/builds/` pool. Listing for a project
        // anchored on hash `aaa` must NOT surface builds with
        // hash `bbb`.
        let fixture = BuildsLsFixture::new("multi-cg");

        let a1 = fixture.write_manifest_full(
            "aaa",
            "2026-04-20T00:00:00+00:00",
            "20260420000000",
            &["postgres"],
        );
        let _b1 = fixture.write_manifest_full(
            "bbb",
            "2026-04-21T00:00:00+00:00",
            "20260421000000",
            &["mongo"],
        );
        let a2 = fixture.write_manifest_full(
            "aaa",
            "2026-04-22T00:00:00+00:00",
            "20260422000000",
            &["postgres", "redis"],
        );

        {
            let db = fixture.state.db.lock().await;
            db.set_latest_build_id(&fixture.project, &a2).unwrap();
        }

        let resp = handle_builds_ls(&fixture.project, &fixture.state)
            .await
            .unwrap();
        let ids: Vec<&str> = resp.builds.iter().map(|e| e.build_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![a2.as_str(), a1.as_str()],
            "only `aaa`-hashed builds should appear; got {ids:?}"
        );
    }
}
