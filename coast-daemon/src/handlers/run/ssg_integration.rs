//! SSG auto-start hook for `coast run`.
//!
//! Phase: ssg-phase-3.5. See `coast-ssg/DESIGN.md §11.1` and `§16`.
//!
//! Single public entry point: [`ensure_ready_for_consumer`]. Called
//! from [`crate::handlers::run::provision::load_coastfile_resources`]
//! once the parsed artifact Coastfile is in hand. When the consumer
//! Coastfile references SSG services via `[shared_services.<name>]
//! from_group = true` (Phase 1 populated
//! `Coastfile::shared_service_group_refs`), this hook:
//!
//! 1. Short-circuits if there are no refs (no-op, zero cost).
//! 2. Errors with the DESIGN.md §11.1 verbatim message when no SSG
//!    build exists on disk.
//! 3. Acquires `AppState.ssg_mutex` to serialize with explicit
//!    `coast ssg` verbs + other consumer runs that race to auto-start.
//! 4. Dispatches against the current state:
//!    - `None` (never run) -> `run_ssg` (create + start the DinD).
//!    - `stopped` -> `start_ssg` (reuse existing container + ports).
//!    - `running` -> no-op, just emit the progress event.
//! 5. Emits `BuildProgressEvent` steps (`Ensure SSG ready`) on the
//!    consumer's run progress channel so the CLI shows inline status.
//! 6. Emits `CoastEvent::SsgStarting` / `SsgStarted` on the daemon
//!    event bus so Coastguard's WebSocket subscribers see it.
//!
//! This file is the only `*ssg*`-named file under `coast-daemon/src/
//! handlers/run/`. It is the adapter required by the DESIGN.md §4.2
//! "banned patterns" rule: SSG logic must NEVER live in a file whose
//! name does not contain `ssg`.

use tokio::sync::mpsc::Sender;
use tracing::{info, warn};

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent};
use coast_ssg::runtime::ports::SsgServicePortPlan;
use coast_ssg::state::SsgStateExt;

use crate::server::AppState;

/// Auto-start the Shared Service Group if the consumer Coastfile
/// references SSG services and the SSG is not already running.
///
/// Emits `Ensure SSG ready` progress steps on `progress` plus
/// `CoastEvent::SsgStarting` / `SsgStarted` on the daemon event bus.
/// Returns `Ok(())` on success or when the refs list is empty;
/// otherwise returns a `CoastError::coastfile` with DESIGN.md §11.1
/// verbatim wording when no SSG build exists.
pub async fn ensure_ready_for_consumer(
    state: &AppState,
    project: &str,
    coastfile: &Coastfile,
    progress: &Sender<BuildProgressEvent>,
) -> Result<()> {
    if coastfile.shared_service_group_refs.is_empty() {
        return Ok(());
    }

    let referenced_service_names: Vec<String> = coastfile
        .shared_service_group_refs
        .iter()
        .map(|r| r.name.clone())
        .collect();

    // Phase 16: resolve the effective build for this consumer. If a
    // pin exists, validate its build dir exists on disk. If a pin is
    // set but the build is gone, the pinned-build-missing hard-error
    // surfaces here so the user notices BEFORE we try to start the
    // SSG under it.
    let pin = {
        let db = state.db.lock().await;
        db.get_ssg_consumer_pin(project)?
    };
    let pinned_build_id: Option<String> = match pin {
        Some(p) => {
            let build_dir = coast_ssg::paths::ssg_build_dir(&p.build_id)?;
            if !build_dir.is_dir() {
                return Err(coast_ssg::runtime::pinning::pinned_build_missing_error(
                    &p.build_id,
                ));
            }
            Some(p.build_id)
        }
        None => None,
    };

    // Precondition: an SSG build must exist. DESIGN.md §11.1 specifies
    // the verbatim error the user sees when it doesn't.
    // With a valid pin, that check is already satisfied (the pin
    // points at a dir we just verified). Without a pin, require
    // `latest` to resolve.
    if pinned_build_id.is_none() && coast_ssg::paths::resolve_latest_build_id().is_none() {
        return Err(missing_ssg_build_error(project, &referenced_service_names));
    }

    let Some(docker) = state.docker.as_ref() else {
        return Err(CoastError::docker(
            "SSG auto-start requires Docker to be available on the host daemon. \
             Start Docker Desktop / Colima / OrbStack and retry.",
        ));
    };

    emit_started(progress);

    // Serialize with explicit `coast ssg` verbs and other consumer-run
    // auto-start paths. The guard is held for the duration of the
    // dispatch so two racing `coast run`s can't both decide to
    // `run_ssg` and end up with two containers.
    let _ssg_guard = state.ssg_mutex.lock().await;

    // Read the current SSG record with the state lock held briefly.
    // Per-project SSG (§23): scoped to the consumer's own project.
    let record = {
        let db = state.db.lock().await;
        db.get_ssg(project)?
    };

    // DESIGN.md §11.1 says "Emit `CoastEvent::SsgStarting` /
    // `SsgStarted` on the run progress channel so Coastguard can
    // show boot progress inline." — that only makes sense if the
    // `Starting` event precedes the actual start work. Emit it now,
    // before `run_and_apply`/`start_and_apply`, using a best-effort
    // build_id from the existing record (or "pending" when we're
    // about to create one fresh). Emit the `Started` event after
    // the dispatch completes, always, so subscribers can rely on
    // the pair as a handshake (SETTLED #16).
    let starting_build_id = record
        .as_ref()
        .and_then(|r| r.build_id.clone())
        .unwrap_or_else(|| "pending".to_string());
    state.emit_event(CoastEvent::SsgStarting {
        project: project.to_string(),
        build_id: starting_build_id,
    });

    let outcome = match record {
        None => DispatchOutcome::Created(
            run_and_apply(project, state, &docker, pinned_build_id.as_deref(), progress).await?,
        ),
        Some(r) if r.status == "running" => DispatchOutcome::AlreadyRunning(
            r.build_id.clone().unwrap_or_else(|| "unknown".to_string()),
        ),
        Some(r) => DispatchOutcome::Started(
            start_and_apply(project, state, &docker, r, progress).await?,
        ),
    };

    let build_id = outcome.build_id().to_string();

    emit_done(progress, &outcome);
    state.emit_event(CoastEvent::SsgStarted {
        project: project.to_string(),
        build_id,
    });

    info!(
        project = %project,
        ssg_build = %outcome.build_id(),
        transition = %outcome.transition_label(),
        "SSG auto-start complete for consumer"
    );

    Ok(())
}

/// What the dispatch decided to do. Carries the resulting build id
/// for progress + event payloads.
#[derive(Debug)]
enum DispatchOutcome {
    /// SSG wasn't present before; we created + started it.
    Created(String),
    /// SSG existed but was stopped; we restarted it.
    Started(String),
    /// SSG was already running; we only emitted progress.
    AlreadyRunning(String),
}

impl DispatchOutcome {
    fn build_id(&self) -> &str {
        match self {
            Self::Created(id) | Self::Started(id) | Self::AlreadyRunning(id) => id,
        }
    }

    fn transition_label(&self) -> &'static str {
        match self {
            Self::Created(_) => "created",
            Self::Started(_) => "started",
            Self::AlreadyRunning(_) => "already running",
        }
    }
}

async fn run_and_apply(
    project: &str,
    state: &AppState,
    docker: &bollard::Docker,
    pinned_build_id: Option<&str>,
    progress: &Sender<BuildProgressEvent>,
) -> Result<String> {
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);
    let forward_to = progress.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = inner_rx.recv().await {
            let prefixed = prefix_inner_event(event);
            if forward_to.send(prefixed).await.is_err() {
                break;
            }
        }
    });

    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    let outcome = coast_ssg::daemon_integration::run_ssg_with_build_id(
        project,
        &ops,
        pinned_build_id,
        inner_tx,
    )
    .await?;

    // Ensure the forwarder drains after the inner sender drops.
    let _ = forwarder.await;

    let build_id = outcome.build_id.clone();
    let db = state.db.lock().await;
    let _ = outcome.apply_to_state_and_response(
        project,
        &*db,
        "running",
        format!("SSG running on build {build_id}"),
    )?;
    Ok(build_id)
}

async fn start_and_apply(
    project: &str,
    state: &AppState,
    docker: &bollard::Docker,
    record: coast_ssg::state::SsgRecord,
    progress: &Sender<BuildProgressEvent>,
) -> Result<String> {
    // Re-hydrate existing port plans from ssg_services so start_ssg
    // can re-publish them on the outer DinD.
    let plans: Vec<SsgServicePortPlan> = {
        let db = state.db.lock().await;
        db.list_ssg_services(project)?
            .into_iter()
            .map(|s| SsgServicePortPlan {
                service: s.service_name,
                container_port: s.container_port,
                dynamic_host_port: s.dynamic_host_port,
            })
            .collect()
    };

    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(64);
    let forward_to = progress.clone();
    let forwarder = tokio::spawn(async move {
        while let Some(event) = inner_rx.recv().await {
            let prefixed = prefix_inner_event(event);
            if forward_to.send(prefixed).await.is_err() {
                break;
            }
        }
    });

    let ops = coast_ssg::docker_ops::BollardSsgDockerOps::new(docker.clone());
    let outcome = coast_ssg::daemon_integration::start_ssg(&ops, &record, plans, inner_tx)
        .await
        .inspect_err(|error| {
            warn!(error = %error, "SSG auto-start via start_ssg failed");
        })?;
    let _ = forwarder.await;

    let build_id = outcome.build_id.clone();
    let db = state.db.lock().await;
    let _ = outcome.apply_to_state_and_response(
        project,
        &*db,
        format!("SSG started on build {build_id}"),
    )?;
    Ok(build_id)
}

fn emit_started(progress: &Sender<BuildProgressEvent>) {
    // `Ensure SSG ready` uses a single-step progress plan on the
    // consumer run stream (we don't know the exact sub-step count
    // from run_ssg / start_ssg, and nesting them under one outer step
    // keeps the consumer's progress plan stable).
    let _ = progress.try_send(BuildProgressEvent::started("Ensure SSG ready", 1, 1));
}

fn emit_done(progress: &Sender<BuildProgressEvent>, outcome: &DispatchOutcome) {
    let detail = outcome.transition_label();
    let _ = progress.try_send(BuildProgressEvent::done("Ensure SSG ready", detail));
}

/// Prefix an inner `run_ssg` / `start_ssg` progress event so the
/// consumer's run output shows clearly that these steps came from the
/// auto-start, not from the consumer's own compose/image pipeline.
fn prefix_inner_event(mut event: BuildProgressEvent) -> BuildProgressEvent {
    if !event.step.is_empty() && !event.step.starts_with("SSG: ") {
        event.step = format!("SSG: {}", event.step);
    }
    event
}

/// Synthesize `SharedServiceConfig` entries for every
/// `from_group = true` reference in the consumer Coastfile.
///
/// Phase: ssg-phase-4. See `coast-ssg/DESIGN.md §11` for the overall
/// wiring contract. Returns an empty vec when the consumer has no
/// SSG references; otherwise reads the active SSG build manifest
/// from disk + the `ssg_services` rows from the state DB and
/// delegates the pure synthesis work to
/// `coast_ssg::daemon_integration::synthesize_shared_service_configs`.
///
/// Errors with the DESIGN.md §6.1 missing-service wording (via
/// `coast-ssg`) when the consumer names a service the active SSG
/// does not publish.
pub async fn synthesize_configs_for_consumer(
    state: &AppState,
    coastfile: &Coastfile,
) -> Result<Vec<coast_core::types::SharedServiceConfig>> {
    if coastfile.shared_service_group_refs.is_empty() {
        return Ok(Vec::new());
    }

    let build_id = coast_ssg::paths::resolve_latest_build_id().ok_or_else(|| {
        CoastError::coastfile(
            "no active SSG build found while synthesizing consumer shared services. \
             Run `coast ssg build` in the directory containing your \
             Coastfile.shared_service_groups.",
        )
    })?;
    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id)?;
    let manifest_path = build_dir.join("manifest.json");
    let manifest_contents =
        std::fs::read_to_string(&manifest_path).map_err(|e| CoastError::Io {
            message: format!(
                "failed to read SSG manifest '{}': {e}",
                manifest_path.display()
            ),
            path: manifest_path.clone(),
            source: Some(e),
        })?;
    let manifest: coast_ssg::build::artifact::SsgManifest =
        serde_json::from_str(&manifest_contents).map_err(|e| {
            CoastError::artifact(format!(
                "failed to parse SSG manifest '{}': {e}",
                manifest_path.display()
            ))
        })?;

    let services = {
        let db = state.db.lock().await;
        // Per-project SSG (§23): look up the consumer's own project.
        db.list_ssg_services(&coastfile.name)?
    };

    coast_ssg::daemon_integration::synthesize_shared_service_configs(
        coastfile, &manifest, &services,
    )
}

/// DESIGN.md §11.1 verbatim wording, customized with the consumer
/// project name and the list of referenced SSG service names. A
/// single error type (`CoastError::coastfile`) is used so the
/// run-progress error path surfaces it unchanged to the CLI.
fn missing_ssg_build_error(project: &str, referenced_services: &[String]) -> CoastError {
    let service_list = if referenced_services.len() == 1 {
        format!("shared service '{}'", referenced_services[0])
    } else {
        let names: Vec<String> = referenced_services
            .iter()
            .map(|n| format!("'{n}'"))
            .collect();
        format!("shared services {}", names.join(", "))
    };
    CoastError::coastfile(format!(
        "Project '{project}' references {service_list} from the Shared Service Group, \
         but no SSG build exists. Run `coast ssg build` in the directory containing your \
         Coastfile.shared_service_groups."
    ))
}

// --- Phase 7: SSG drift detection -------------------------------------------
//
// `coast build` embeds an `ssg` block in `manifest.json` recording the
// active SSG build_id + image refs for every `from_group = true`
// service. At `coast run` time we validate that snapshot against the
// current SSG. See `coast-ssg/DESIGN.md §6.1`.
//
// Three outcomes:
//   Match             -> proceed silently.
//   SameImageWarn     -> emit a warn progress event + proceed.
//   HardError         -> fail `coast run` with the DESIGN §6.1
//                        verbatim message plus a specific suffix.
//
// When the consumer has no refs, or the artifact manifest has no
// `ssg` block (pre-Phase-7 build), or the active SSG has been
// removed entirely, we fall through to the existing
// `ensure_ready_for_consumer` flow which handles those cases.

/// Verbatim DESIGN.md §6.1 hard-error sentence. Do not edit without
/// updating DESIGN.md in the same commit.
const DRIFT_DESIGN_SENTENCE: &str = "SSG has changed since this coast was built. \
    Re-run `coast build` to pick up the new SSG, or pin the SSG to the old build.";

/// Validate that the coast build's recorded SSG snapshot still
/// matches the active SSG. No-op when the coastfile has no SSG
/// refs, or when the artifact manifest lacks an `ssg` block
/// (pre-Phase-7 build).
///
/// Emits a single `Checking SSG drift` progress step so the CLI
/// surfaces the check. Warns via a `warn` status when build ids
/// differ but all image refs match; errors with the DESIGN §6.1
/// verbatim sentence when any referenced service's image changed
/// or is missing.
pub async fn validate_ssg_drift(
    project: &str,
    coastfile: &Coastfile,
    manifest_path: &std::path::Path,
    state: &AppState,
    progress: &Sender<BuildProgressEvent>,
) -> Result<()> {
    // Phase 16: the "active" manifest is either the pinned build or
    // `latest`, depending on whether the consumer's project has a pin
    // in `ssg_consumer_pins`. Read the pin once here and hand the
    // resolution closure to the inner helper.
    let pin = {
        let db = state.db.lock().await;
        db.get_ssg_consumer_pin(project)?
    };
    let loader_pin = pin.map(|p| coast_ssg::runtime::pinning::PinRecord {
        project: p.project,
        build_id: p.build_id,
    });
    validate_ssg_drift_with_loader(coastfile, manifest_path, progress, move || {
        // Returns `Ok(Some(manifest))` on hit, `Ok(None)` when no
        // pin + no latest (fall through to existing
        // `drift_missing_ssg_error`), or propagates a hard error
        // when a pin is set but its build dir is missing
        // (pin-pruned, per §17-41 SETTLED).
        coast_ssg::runtime::pinning::resolve_effective_manifest(loader_pin.as_ref())
            .map(|opt| opt.map(|(_, manifest)| manifest))
    })
}

/// Inner drift validator with the active-SSG-manifest loader injected.
/// Keeps the public [`validate_ssg_drift`] a one-liner while letting
/// unit tests pass a deterministic closure instead of mutating
/// `COAST_HOME`, which races with other test modules that touch the
/// same env var.
///
/// The loader now returns `Result<Option<SsgManifest>>` (Phase 16):
/// - `Ok(Some(manifest))` — an effective SSG build is available.
/// - `Ok(None)` — no build exists (pre-existing "missing SSG" path).
/// - `Err(e)` — pinned build has been pruned; surface as-is.
fn validate_ssg_drift_with_loader<F>(
    coastfile: &Coastfile,
    manifest_path: &std::path::Path,
    progress: &Sender<BuildProgressEvent>,
    active_loader: F,
) -> Result<()>
where
    F: FnOnce() -> Result<Option<coast_ssg::build::artifact::SsgManifest>>,
{
    if coastfile.shared_service_group_refs.is_empty() {
        return Ok(());
    }

    let Some(recorded) = read_recorded_ssg_ref(manifest_path) else {
        // Pre-Phase-7 artifact or coast-service-built without the
        // local patch. Fall through to the existing auto-start path;
        // it will still error if the SSG is missing entirely.
        return Ok(());
    };

    // Recorded manifest says there was an SSG at build time; resolve
    // the effective (pin-aware) manifest. Pinned-build-pruned
    // propagates as a hard error; missing-entirely uses the
    // pre-existing §6.1 wording.
    let Some(active) = active_loader()? else {
        return Err(drift_missing_ssg_error(&recorded));
    };

    let referenced: Vec<String> = coastfile
        .shared_service_group_refs
        .iter()
        .map(|r| r.name.clone())
        .collect();

    let _ = progress.try_send(BuildProgressEvent::started("Checking SSG drift", 1, 1));

    match coast_ssg::evaluate_drift(&recorded, &active, &referenced) {
        coast_ssg::DriftOutcome::Match => {
            let _ = progress.try_send(BuildProgressEvent::done("Checking SSG drift", "ok"));
            Ok(())
        }
        coast_ssg::DriftOutcome::SameImageWarn {
            old_build_id,
            new_build_id,
        } => {
            let detail = format!(
                "SSG build differs (was {old_build_id}, now {new_build_id}) but image refs \
                 still match for every referenced service. Proceeding."
            );
            warn!(
                old = %old_build_id,
                new = %new_build_id,
                "SSG drift: same-image warn"
            );
            // Use a `warn`-status event carrying the long message in
            // `detail` so the CLI's progress renderer surfaces it as
            // a warning line under the step, rather than swallowing
            // the text as an unrecognized status.
            let _ = progress.try_send(BuildProgressEvent::item(
                "Checking SSG drift",
                detail,
                "warn",
            ));
            let _ = progress.try_send(BuildProgressEvent::done("Checking SSG drift", "warn"));
            Ok(())
        }
        coast_ssg::DriftOutcome::HardError { reason } => Err(drift_hard_error(&reason)),
    }
}

/// Read the recorded SSG ref from a coast build's `manifest.json`.
/// Returns `None` for pre-Phase-7 manifests, unreadable files, or
/// malformed JSON.
fn read_recorded_ssg_ref(manifest_path: &std::path::Path) -> Option<coast_ssg::RecordedSsgRef> {
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    let ssg = manifest.get("ssg")?;
    serde_json::from_value(ssg.clone()).ok()
}

fn drift_missing_ssg_error(recorded: &coast_ssg::RecordedSsgRef) -> CoastError {
    CoastError::coastfile(format!(
        "{DRIFT_DESIGN_SENTENCE} (this coast was built against SSG build {build}, but no \
         SSG build exists now; run `coast ssg build` and then rebuild this coast.)",
        build = recorded.build_id,
    ))
}

fn drift_hard_error(reason: &coast_ssg::DriftHardErrorReason) -> CoastError {
    let suffix = match reason {
        coast_ssg::DriftHardErrorReason::ImageChanged {
            service,
            old_image,
            new_image,
        } => format!("(service '{service}' image changed: {old_image} -> {new_image})"),
        coast_ssg::DriftHardErrorReason::ServiceMissing { service, available } => {
            let available_list = if available.is_empty() {
                "(none)".to_string()
            } else {
                format!("[{}]", available.join(", "))
            };
            format!(
                "(service '{service}' is no longer in the active SSG; available: {available_list})"
            )
        }
    };
    CoastError::coastfile(format!("{DRIFT_DESIGN_SENTENCE} {suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::sync::Mutex;

    use coast_core::coastfile::Coastfile;

    /// Serialize tests that mutate the process-wide `COAST_HOME`
    /// env var. Without this, concurrent tests race: one test seeds
    /// an SSG build and another test that expects "no SSG build"
    /// accidentally observes it.
    static COAST_HOME_LOCK: Mutex<()> = Mutex::new(());

    fn empty_coastfile() -> Coastfile {
        // Minimal TOML that parses to a Coastfile with no SSG refs.
        Coastfile::parse(
            r#"
[coast]
name = "noop"
"#,
            Path::new("/tmp"),
        )
        .expect("minimal Coastfile should parse")
    }

    fn coastfile_with_group_refs(names: &[&str]) -> Coastfile {
        let refs_toml = names
            .iter()
            .map(|name| format!("[shared_services.{name}]\nfrom_group = true\n"))
            .collect::<Vec<_>>()
            .join("\n");
        let toml = format!(
            r#"
[coast]
name = "consumer"

{refs_toml}"#
        );
        Coastfile::parse(&toml, Path::new("/tmp")).expect("Coastfile with group refs should parse")
    }

    #[test]
    fn missing_ssg_build_error_single_service_uses_singular() {
        let err = missing_ssg_build_error("my-app", &["postgres".to_string()]);
        let message = err.to_string();
        assert!(message.contains("Project 'my-app' references shared service 'postgres'"));
        assert!(message.contains("no SSG build exists"));
        assert!(message.contains("Coastfile.shared_service_groups"));
    }

    #[test]
    fn missing_ssg_build_error_multiple_services_uses_plural() {
        let err = missing_ssg_build_error("my-app", &["postgres".to_string(), "redis".to_string()]);
        let message = err.to_string();
        assert!(
            message.contains("shared services 'postgres', 'redis'"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn prefix_inner_event_adds_ssg_namespace_once() {
        let event = BuildProgressEvent::started("Loading cached images", 5, 6);
        let prefixed = prefix_inner_event(event);
        assert_eq!(prefixed.step, "SSG: Loading cached images");
    }

    #[test]
    fn prefix_inner_event_is_idempotent() {
        let event = BuildProgressEvent::started("SSG: Loading cached images", 5, 6);
        let prefixed = prefix_inner_event(event);
        assert_eq!(prefixed.step, "SSG: Loading cached images");
    }

    #[test]
    fn dispatch_outcome_build_id_accessor() {
        assert_eq!(
            DispatchOutcome::Created("abc".to_string()).build_id(),
            "abc"
        );
        assert_eq!(
            DispatchOutcome::Started("def".to_string()).build_id(),
            "def"
        );
        assert_eq!(
            DispatchOutcome::AlreadyRunning("ghi".to_string()).build_id(),
            "ghi"
        );
    }

    #[test]
    fn dispatch_outcome_transition_label() {
        assert_eq!(
            DispatchOutcome::Created("abc".to_string()).transition_label(),
            "created"
        );
        assert_eq!(
            DispatchOutcome::Started("abc".to_string()).transition_label(),
            "started"
        );
        assert_eq!(
            DispatchOutcome::AlreadyRunning("abc".to_string()).transition_label(),
            "already running"
        );
    }

    #[tokio::test]
    async fn ensure_ready_for_consumer_is_noop_when_no_refs() {
        // No SSG-related code is exercised when the Coastfile has no
        // `from_group = true` entries; the function short-circuits
        // before touching `state` at all. We construct a minimal fake
        // AppState via `new_for_testing` to prove the short-circuit
        // path doesn't need Docker or a running daemon.
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = crate::server::AppState::new_for_testing(db);

        let coastfile = empty_coastfile();
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        ensure_ready_for_consumer(&state, "noop-project", &coastfile, &tx)
            .await
            .expect("noop short-circuit should succeed");
    }

    #[tokio::test]
    async fn ensure_ready_for_consumer_errors_when_no_ssg_build_exists() {
        // Point COAST_HOME at an empty tempdir so
        // `coast_ssg::paths::resolve_latest_build_id()` returns None
        // regardless of the developer's real ~/.coast.
        let _guard = COAST_HOME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().unwrap();
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = crate::server::AppState::new_for_testing(db);

        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", tmp.path());
        }

        let result = ensure_ready_for_consumer(&state, "my-app", &coastfile, &tx).await;

        unsafe {
            match prev {
                Some(v) => std::env::set_var("COAST_HOME", v),
                None => std::env::remove_var("COAST_HOME"),
            }
        }

        let err = result.expect_err("missing build must error");
        let message = err.to_string();
        assert!(
            message.contains("Project 'my-app' references shared service 'postgres'"),
            "unexpected message: {message}"
        );
        assert!(message.contains("no SSG build exists"));
        assert!(message.contains("Coastfile.shared_service_groups"));
    }

    #[tokio::test]
    async fn synthesize_configs_for_consumer_returns_empty_when_no_refs() {
        // Consumer has no `from_group = true` entries — short-circuits
        // before any disk/DB read, so no SSG build need exist.
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = crate::server::AppState::new_for_testing(db);

        let coastfile = empty_coastfile();
        let result = synthesize_configs_for_consumer(&state, &coastfile)
            .await
            .expect("empty refs must succeed");
        assert!(result.is_empty());
    }

    /// Write a minimal SSG artifact tree at `$root/ssg/` so
    /// `coast_ssg::paths::resolve_latest_build_id()` finds it.
    fn seed_ssg_build(root: &Path, build_id: &str, services: &[(&str, &str)]) {
        let ssg_home = root.join("ssg");
        let build_dir = ssg_home.join("builds").join(build_id);
        std::fs::create_dir_all(&build_dir).unwrap();
        let manifest = serde_json::json!({
            "build_id": build_id,
            "built_at": "2026-04-20T00:00:00Z",
            "coastfile_hash": "fake-hash",
            "services": services
                .iter()
                .map(|(name, image)| serde_json::json!({
                    "name": name,
                    "image": image,
                    "ports": [5432],
                    "env_keys": [],
                    "volumes": [],
                    "auto_create_db": false,
                }))
                .collect::<Vec<_>>(),
        });
        std::fs::write(
            build_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let latest = ssg_home.join("latest");
        let _ = std::fs::remove_file(&latest);
        #[cfg(unix)]
        std::os::unix::fs::symlink(Path::new("builds").join(build_id), &latest).unwrap();
    }

    #[tokio::test]
    async fn ensure_ready_for_consumer_errors_when_docker_unavailable() {
        // Consumer has `from_group = true`, an SSG build exists, but
        // `state.docker` is None (test harness default). We should
        // surface a clear "Docker is unavailable" error rather than
        // panicking or progressing into the lock section.
        let _guard = COAST_HOME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().unwrap();
        seed_ssg_build(
            tmp.path(),
            "b9_20260420000000",
            &[("postgres", "postgres:16")],
        );

        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = crate::server::AppState::new_for_testing(db);

        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", tmp.path());
        }

        let result = ensure_ready_for_consumer(&state, "my-app", &coastfile, &tx).await;

        unsafe {
            match prev {
                Some(v) => std::env::set_var("COAST_HOME", v),
                None => std::env::remove_var("COAST_HOME"),
            }
        }

        let err = result.expect_err("missing Docker must error");
        let msg = err.to_string();
        assert!(
            msg.contains("Docker to be available"),
            "error must cite missing Docker; got: {msg}"
        );
        assert!(
            msg.contains("Docker Desktop"),
            "error must hint at Docker Desktop / Colima / OrbStack; got: {msg}"
        );
    }

    #[tokio::test]
    async fn missing_ssg_build_error_contains_all_referenced_services() {
        // Regression: the error message for `ensure_ready_for_consumer`
        // must enumerate every referenced service, not just the first.
        let err = missing_ssg_build_error(
            "many-refs",
            &[
                "postgres".to_string(),
                "redis".to_string(),
                "mongo".to_string(),
            ],
        );
        let msg = err.to_string();
        for svc in ["postgres", "redis", "mongo"] {
            assert!(
                msg.contains(&format!("'{svc}'")),
                "error must mention '{svc}'; got: {msg}"
            );
        }
        // Plural form used when multiple services are referenced.
        assert!(
            msg.contains("shared services"),
            "multi-service message should use plural; got: {msg}"
        );
    }

    // --- Phase 7: validate_ssg_drift ---

    fn write_manifest_with_ssg_block(dir: &Path, build_id: &str, images: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let images_map: serde_json::Value = images
            .iter()
            .map(|(n, i)| {
                (
                    (*n).to_string(),
                    serde_json::Value::String((*i).to_string()),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into();
        let services: Vec<&&str> = images.iter().map(|(n, _)| n).collect();
        let manifest = serde_json::json!({
            "build_id": "coast-build-id",
            "project": "consumer",
            "ssg": {
                "build_id": build_id,
                "services": services,
                "images": images_map,
            },
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn fake_active_manifest(
        build_id: &str,
        services: &[(&str, &str)],
    ) -> coast_ssg::build::artifact::SsgManifest {
        use chrono::TimeZone;
        coast_ssg::build::artifact::SsgManifest {
            build_id: build_id.to_string(),
            built_at: chrono::Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap(),
            coastfile_hash: "fake-hash".to_string(),
            services: services
                .iter()
                .map(|(n, i)| coast_ssg::build::artifact::SsgManifestService {
                    name: (*n).to_string(),
                    image: (*i).to_string(),
                    ports: vec![5432],
                    env_keys: Vec::new(),
                    volumes: Vec::new(),
                    auto_create_db: false,
                })
                .collect(),
        }
    }

    #[test]
    fn validate_drift_is_noop_when_consumer_has_no_refs() {
        let coastfile = empty_coastfile();
        let tmp = tempfile::tempdir().unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);
        // No refs: must short-circuit even if the active loader would error.
        validate_ssg_drift_with_loader(&coastfile, &tmp.path().join("missing.json"), &tx, || {
            panic!("active loader must not be called when there are no refs")
        })
        .expect("no-ref consumer should succeed silently");
    }

    #[test]
    fn validate_drift_is_noop_when_manifest_has_no_ssg_block() {
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let tmp = tempfile::tempdir().unwrap();
        // Write a pre-Phase-7-style manifest: no `ssg` key.
        std::fs::write(
            tmp.path().join("manifest.json"),
            r#"{"build_id":"coast-build-id","project":"consumer"}"#,
        )
        .unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);
        validate_ssg_drift_with_loader(&coastfile, &tmp.path().join("manifest.json"), &tx, || {
            panic!("active loader must not be called when manifest lacks the ssg block")
        })
        .expect("pre-Phase-7 manifest should fall through to auto-start");
    }

    #[test]
    fn validate_drift_match_succeeds_silently() {
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let artifact_tmp = tempfile::tempdir().unwrap();
        write_manifest_with_ssg_block(
            artifact_tmp.path(),
            "build-A",
            &[("postgres", "postgres:16-alpine")],
        );
        let manifest_path = artifact_tmp.path().join("manifest.json");
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        validate_ssg_drift_with_loader(&coastfile, &manifest_path, &tx, || {
            Ok(Some(fake_active_manifest(
                "build-A",
                &[("postgres", "postgres:16-alpine")],
            )))
        })
        .expect("matching build ids should succeed");
    }

    #[test]
    fn validate_drift_hard_error_contains_design_sentence() {
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let artifact_tmp = tempfile::tempdir().unwrap();
        // Recorded snapshot has postgres:16; active has postgres:17.
        write_manifest_with_ssg_block(
            artifact_tmp.path(),
            "build-A",
            &[("postgres", "postgres:16-alpine")],
        );
        let manifest_path = artifact_tmp.path().join("manifest.json");
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        let err = validate_ssg_drift_with_loader(&coastfile, &manifest_path, &tx, || {
            Ok(Some(fake_active_manifest(
                "build-B",
                &[("postgres", "postgres:17-alpine")],
            )))
        })
        .expect_err("image change must hard-error");
        let msg = err.to_string();
        assert!(
            msg.contains("SSG has changed since this coast was built"),
            "missing DESIGN sentence: {msg}"
        );
        assert!(
            msg.contains("postgres:16-alpine -> postgres:17-alpine"),
            "missing drift suffix: {msg}"
        );
    }

    #[test]
    fn validate_drift_missing_ssg_build_is_hard_error() {
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let artifact_tmp = tempfile::tempdir().unwrap();
        write_manifest_with_ssg_block(
            artifact_tmp.path(),
            "build-A",
            &[("postgres", "postgres:16-alpine")],
        );
        let manifest_path = artifact_tmp.path().join("manifest.json");
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        // Active loader returns None -> SSG removed between build and run.
        let err = validate_ssg_drift_with_loader(&coastfile, &manifest_path, &tx, || Ok(None))
            .expect_err("missing active SSG must hard-error");
        let msg = err.to_string();
        assert!(msg.contains("SSG has changed since this coast was built"));
        assert!(
            msg.contains("build-A"),
            "error should mention recorded build id: {msg}"
        );
    }

    // --- Phase 16: pin-aware drift loader ---

    #[test]
    fn validate_drift_loader_error_propagates_pin_pruned() {
        // The loader closure returns `Err(..)` when a pin is set but
        // the pinned build dir is gone. Drift validator must surface
        // that error verbatim so users see the Phase 16 pin-pruned
        // message.
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let artifact_tmp = tempfile::tempdir().unwrap();
        write_manifest_with_ssg_block(
            artifact_tmp.path(),
            "build-A",
            &[("postgres", "postgres:16-alpine")],
        );
        let manifest_path = artifact_tmp.path().join("manifest.json");
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        let err = validate_ssg_drift_with_loader(&coastfile, &manifest_path, &tx, || {
            Err(coast_ssg::runtime::pinning::pinned_build_missing_error(
                "b_pinned",
            ))
        })
        .expect_err("loader error must surface unchanged");
        let msg = err.to_string();
        assert!(msg.contains("no longer exists"), "got: {msg}");
        assert!(msg.contains("b_pinned"));
    }

    #[test]
    fn validate_drift_loader_match_against_pinned_manifest_succeeds() {
        // Consumer recorded build-A; the effective manifest closure
        // returns build-A (same build_id, same image refs). Drift
        // check should pass regardless of whether build-A came from
        // `latest` or a pin — the loader abstracts that away.
        let coastfile = coastfile_with_group_refs(&["postgres"]);
        let artifact_tmp = tempfile::tempdir().unwrap();
        write_manifest_with_ssg_block(
            artifact_tmp.path(),
            "build-A",
            &[("postgres", "postgres:16-alpine")],
        );
        let manifest_path = artifact_tmp.path().join("manifest.json");
        let (tx, _rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);

        validate_ssg_drift_with_loader(&coastfile, &manifest_path, &tx, || {
            Ok(Some(fake_active_manifest(
                "build-A",
                &[("postgres", "postgres:16-alpine")],
            )))
        })
        .expect("pinned-manifest match should succeed");
    }
}
