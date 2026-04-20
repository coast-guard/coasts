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

    // Precondition: an SSG build must exist. DESIGN.md §11.1 specifies
    // the verbatim error the user sees when it doesn't.
    if coast_ssg::paths::resolve_latest_build_id().is_none() {
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
    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };

    let outcome = match record {
        None => DispatchOutcome::Created(run_and_apply(state, &docker, progress).await?),
        Some(r) if r.status == "running" => DispatchOutcome::AlreadyRunning(
            r.build_id.clone().unwrap_or_else(|| "unknown".to_string()),
        ),
        Some(r) => DispatchOutcome::Started(start_and_apply(state, &docker, r, progress).await?),
    };

    let build_id = outcome.build_id().to_string();

    emit_done(progress, &outcome);
    state.emit_event(CoastEvent::SsgStarting {
        project: project.to_string(),
        build_id: build_id.clone(),
    });
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
    state: &AppState,
    docker: &bollard::Docker,
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

    let outcome = coast_ssg::daemon_integration::run_ssg(docker, inner_tx).await?;

    // Ensure the forwarder drains after the inner sender drops.
    let _ = forwarder.await;

    let build_id = outcome.build_id.clone();
    let db = state.db.lock().await;
    let _ = outcome.apply_to_state_and_response(
        &*db,
        "running",
        format!("SSG running on build {build_id}"),
    )?;
    Ok(build_id)
}

async fn start_and_apply(
    state: &AppState,
    docker: &bollard::Docker,
    record: coast_ssg::state::SsgRecord,
    progress: &Sender<BuildProgressEvent>,
) -> Result<String> {
    // Re-hydrate existing port plans from ssg_services so start_ssg
    // can re-publish them on the outer DinD.
    let plans: Vec<SsgServicePortPlan> = {
        let db = state.db.lock().await;
        db.list_ssg_services()?
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

    let outcome = coast_ssg::daemon_integration::start_ssg(docker, &record, plans, inner_tx)
        .await
        .inspect_err(|error| {
            warn!(error = %error, "SSG auto-start via start_ssg failed");
        })?;
    let _ = forwarder.await;

    let build_id = outcome.build_id.clone();
    let db = state.db.lock().await;
    let _ =
        outcome.apply_to_state_and_response(&*db, format!("SSG started on build {build_id}"))?;
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
        db.list_ssg_services()?
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;

    use coast_core::coastfile::Coastfile;

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
}
