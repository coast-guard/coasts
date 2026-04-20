//! Per-instance database creation for consumer coasts.
//!
//! Phase: ssg-phase-5. See `coast-ssg/DESIGN.md §13`.
//!
//! Every shared service with `auto_create_db = true` gets a
//! `{instance}_{project}` database created inside it *before* the
//! consumer's inner compose starts. Two dispatch paths:
//!
//! - **Inline** shared services: `docker exec <host-container> psql ...`
//!   against the host-daemon container.
//! - **SSG** services: nested `docker compose exec -T <service> psql ...`
//!   via [`coast_ssg::daemon_integration::create_instance_db_for_consumer`].
//!
//! Dispatch is driven by the `shared_service_targets` placeholder
//! convention from Phase 4: SSG-backed services have the literal string
//! `"coast-ssg"` as their target. Any other value is assumed to be the
//! inline host container name.
//!
//! The SQL is built by
//! [`crate::shared_services::create_db_command`] regardless of path,
//! so the inline and nested flows emit byte-identical DDL.

use std::collections::HashMap;

use bollard::Docker;
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::types::SharedServiceConfig;
use coast_docker::dind::DindRuntime;
use coast_docker::runtime::{ExecResult, Runtime};
use coast_ssg::state::SsgStateExt;

use crate::server::AppState;
use crate::shared_services::{consumer_db_name, create_db_command, infer_db_type};

/// Sentinel value written into `shared_service_targets` by Phase 4 to
/// indicate an SSG-backed service. See
/// [`coast_daemon::handlers::run::ssg_integration`] and DESIGN §4.
const SSG_PLACEHOLDER: &str = "coast-ssg";

/// Classification of an `auto_create_db = true` shared service into
/// the dispatch path we should take for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AutoCreateDbDispatch {
    /// Skip: service doesn't have `auto_create_db = true`.
    SkipAutoCreateDbOff,
    /// Skip: image isn't a known DB engine (redis/nginx/etc).
    SkipUnknownDbImage,
    /// Skip: no routing target — Phase 4 merger should have populated it.
    SkipNoRoutingTarget,
    /// Dispatch: SSG-backed service, use nested compose exec.
    Ssg,
    /// Dispatch: inline host-daemon container; connect via the
    /// given container name.
    Inline(String),
}

/// Pure helper: decide how to handle a single shared service for
/// `auto_create_db`. Extracted from the main loop so tests can
/// drive the decision table without needing a real Docker client.
pub(super) fn classify_service_for_auto_create_db(
    svc: &SharedServiceConfig,
    shared_service_targets: &HashMap<String, String>,
) -> AutoCreateDbDispatch {
    if !svc.auto_create_db {
        return AutoCreateDbDispatch::SkipAutoCreateDbOff;
    }
    if infer_db_type(&svc.image).is_none() {
        return AutoCreateDbDispatch::SkipUnknownDbImage;
    }
    let Some(target) = shared_service_targets.get(&svc.name) else {
        return AutoCreateDbDispatch::SkipNoRoutingTarget;
    };
    if target == SSG_PLACEHOLDER {
        AutoCreateDbDispatch::Ssg
    } else {
        AutoCreateDbDispatch::Inline(target.clone())
    }
}

/// Run `auto_create_db` for every shared service flagged on this
/// consumer coast. Skipped silently for:
///
/// - Services without `auto_create_db = true`.
/// - Images we can't classify (non-postgres/mysql/mariadb).
/// - Services with no routing target (shouldn't happen after Phase 4
///   merging; treated as "unreachable" and logged).
///
/// Called from `provision.rs` between shared-service routing setup
/// and the consumer's inner `docker compose up -d`.
pub(super) async fn run_auto_create_dbs(
    docker: &Docker,
    state: &AppState,
    services: &[SharedServiceConfig],
    shared_service_targets: &HashMap<String, String>,
    project: &str,
    instance: &str,
) -> Result<()> {
    let db_name = consumer_db_name(instance, project);
    let runtime = DindRuntime::with_client(docker.clone());

    for svc in services {
        let dispatch = classify_service_for_auto_create_db(svc, shared_service_targets);
        dispatch_one(docker, state, &runtime, svc, dispatch, &db_name).await?;
    }

    Ok(())
}

/// Dispatch one pre-classified service. Extracted to keep
/// `run_auto_create_dbs`'s cognitive complexity under the clippy gate.
async fn dispatch_one(
    docker: &Docker,
    state: &AppState,
    runtime: &DindRuntime,
    svc: &SharedServiceConfig,
    dispatch: AutoCreateDbDispatch,
    db_name: &str,
) -> Result<()> {
    if let Some(target) = match &dispatch {
        AutoCreateDbDispatch::SkipAutoCreateDbOff => None,
        AutoCreateDbDispatch::SkipUnknownDbImage => {
            log_skip_unknown_db_image(svc);
            None
        }
        AutoCreateDbDispatch::SkipNoRoutingTarget => {
            log_skip_no_routing_target(svc);
            None
        }
        AutoCreateDbDispatch::Ssg => Some("coast-ssg".to_string()),
        AutoCreateDbDispatch::Inline(host_container) => Some(host_container.clone()),
    } {
        let command = build_create_db_command(svc, db_name);
        match dispatch {
            AutoCreateDbDispatch::Ssg => {
                exec_in_ssg_service(docker, state, &svc.name, command).await?;
            }
            AutoCreateDbDispatch::Inline(ref host) => {
                exec_in_host_container(runtime, host, &svc.name, command).await?;
            }
            _ => unreachable!("skip variants produced None above"),
        }
        info!(
            service = %svc.name,
            db = %db_name,
            target = %target,
            "auto_create_db: per-instance database created"
        );
    }
    Ok(())
}

fn log_skip_unknown_db_image(svc: &SharedServiceConfig) {
    warn!(
        service = %svc.name,
        image = %svc.image,
        "auto_create_db = true but image is not a known DB engine; skipping"
    );
}

fn log_skip_no_routing_target(svc: &SharedServiceConfig) {
    warn!(
        service = %svc.name,
        "auto_create_db: no routing target for shared service; skipping"
    );
}

/// Build the CREATE DATABASE command for `svc`. The classifier
/// already validated that `infer_db_type` returns Some, so the
/// `expect` is an invariant check, not a user-facing error.
fn build_create_db_command(svc: &SharedServiceConfig, db_name: &str) -> Vec<String> {
    let db_type = infer_db_type(&svc.image).expect("classifier already confirmed known DB type");
    create_db_command(db_type, db_name)
}

/// Nested exec into an SSG inner service via the singleton DinD.
async fn exec_in_ssg_service(
    docker: &Docker,
    state: &AppState,
    service_name: &str,
    command: Vec<String>,
) -> Result<()> {
    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };
    let Some(record) = record else {
        return Err(CoastError::state(
            "auto_create_db requested on an SSG-backed service but no SSG is registered. \
             This should not happen — `ensure_ready_for_consumer` runs earlier in the \
             provision pipeline.",
        ));
    };

    coast_ssg::daemon_integration::create_instance_db_for_consumer(
        docker,
        &record,
        service_name,
        command,
    )
    .await
}

/// Direct `docker exec <host_container> <cmd>` against a host-daemon
/// inline shared service container.
async fn exec_in_host_container(
    runtime: &DindRuntime,
    host_container: &str,
    service_name: &str,
    command: Vec<String>,
) -> Result<()> {
    let refs: Vec<&str> = command.iter().map(String::as_str).collect();
    let result: ExecResult = runtime.exec_in_coast(host_container, &refs).await?;
    if !result.success() {
        return Err(CoastError::docker(format!(
            "auto_create_db failed inside inline shared service '{service_name}' \
             (container '{host_container}'): exit {code}. stderr: {stderr}",
            code = result.exit_code,
            stderr = result.stderr.trim(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::types::{InjectType, SharedServicePort};

    fn cfg_postgres_auto(image: &str) -> SharedServiceConfig {
        SharedServiceConfig {
            name: "postgres".to_string(),
            image: image.to_string(),
            ports: vec![SharedServicePort::same(5432)],
            volumes: vec![],
            env: std::collections::HashMap::new(),
            auto_create_db: true,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        }
    }

    #[test]
    fn ssg_placeholder_constant_matches_phase_4_convention() {
        // Phase 4's `ssg_integration.rs` writes the literal string
        // "coast-ssg" into `shared_service_targets`. If that ever
        // changes, this test catches the drift so both sides stay in
        // sync. Also documented in `DESIGN.md §17.18`.
        assert_eq!(SSG_PLACEHOLDER, "coast-ssg");
    }

    #[test]
    fn services_without_auto_create_db_are_skipped() {
        // With no services flagged, the orchestrator short-circuits.
        // We don't need a live Docker to prove this; call the
        // skip-predicate directly by constructing a services vec.
        let mut svc = cfg_postgres_auto("postgres:16");
        svc.auto_create_db = false;
        let selected: Vec<_> = [svc].into_iter().filter(|s| s.auto_create_db).collect();
        assert!(selected.is_empty());
    }

    #[test]
    fn services_with_unknown_db_type_are_skipped() {
        let svc = cfg_postgres_auto("redis:7");
        // Same check the orchestrator applies: infer_db_type(svc.image).
        assert!(crate::shared_services::infer_db_type(&svc.image).is_none());
    }

    #[test]
    fn db_name_matches_consumer_db_name_helper() {
        // Regression guard: if the orchestrator ever inlines the db
        // name, the {instance}_{project} convention must not drift.
        assert_eq!(
            consumer_db_name("dev-1", "my-project"),
            "dev-1_my-project",
            "auto_create_db and inject must agree on this exact shape"
        );
    }

    // --- Phase 9 coverage: classifier decision table ---

    fn targets(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn classifier_skips_auto_create_db_off() {
        let mut svc = cfg_postgres_auto("postgres:16");
        svc.auto_create_db = false;
        let targets = targets(&[("postgres", "coast-ssg")]);
        assert_eq!(
            classify_service_for_auto_create_db(&svc, &targets),
            AutoCreateDbDispatch::SkipAutoCreateDbOff
        );
    }

    #[test]
    fn classifier_skips_unknown_db_image() {
        // redis is not a known DB engine for auto_create_db.
        let mut svc = cfg_postgres_auto("redis:7-alpine");
        svc.name = "redis".to_string();
        let targets = targets(&[("redis", "coast-ssg")]);
        assert_eq!(
            classify_service_for_auto_create_db(&svc, &targets),
            AutoCreateDbDispatch::SkipUnknownDbImage
        );
    }

    #[test]
    fn classifier_skips_missing_routing_target() {
        let svc = cfg_postgres_auto("postgres:16");
        let targets = HashMap::new();
        assert_eq!(
            classify_service_for_auto_create_db(&svc, &targets),
            AutoCreateDbDispatch::SkipNoRoutingTarget
        );
    }

    #[test]
    fn classifier_dispatches_ssg_for_coast_ssg_placeholder() {
        let svc = cfg_postgres_auto("postgres:16");
        let targets = targets(&[("postgres", "coast-ssg")]);
        assert_eq!(
            classify_service_for_auto_create_db(&svc, &targets),
            AutoCreateDbDispatch::Ssg
        );
    }

    #[test]
    fn classifier_dispatches_inline_for_real_container_name() {
        let svc = cfg_postgres_auto("postgres:16");
        let targets = targets(&[("postgres", "my-project-shared-services-postgres")]);
        assert_eq!(
            classify_service_for_auto_create_db(&svc, &targets),
            AutoCreateDbDispatch::Inline("my-project-shared-services-postgres".to_string())
        );
    }

    #[test]
    fn classifier_handles_mysql_and_mariadb_images() {
        for image in &["mysql:8", "mariadb:10"] {
            let mut svc = cfg_postgres_auto(image);
            svc.name = "db".to_string();
            let targets = targets(&[("db", "coast-ssg")]);
            assert_eq!(
                classify_service_for_auto_create_db(&svc, &targets),
                AutoCreateDbDispatch::Ssg,
                "image {image} should classify as a DB engine"
            );
        }
    }

    // --- Phase 9 Pattern C: in-memory AppState SSG dispatch ---
    //
    // We can't actually exec into a real Docker container in a unit
    // test. But we CAN verify that `run_auto_create_dbs` correctly
    // detects "no SSG registered" when the SSG state row is absent
    // and raises the right error.

    fn in_memory_app_state() -> std::sync::Arc<crate::server::AppState> {
        use crate::state::StateDb;
        let db = StateDb::open_in_memory().expect("in-memory statedb");
        std::sync::Arc::new(crate::server::AppState::new_for_testing(db))
    }

    #[tokio::test]
    async fn auto_create_db_no_services_returns_ok() {
        // Empty service list -> no DB calls attempted -> Ok.
        let state = in_memory_app_state();
        // Connect to the test bollard - docker is None in
        // new_for_testing, but run_auto_create_dbs doesn't touch
        // docker when the services list is empty.
        //
        // We synthesize a placeholder Docker handle by making a fake
        // one that isn't used. Using `connect_to_host_docker` might
        // fail in CI so instead we rely on the empty-services
        // short-circuit: with `services = []`, the function returns
        // Ok without ever touching docker.
        let docker = bollard::Docker::connect_with_local_defaults().ok();
        let Some(docker) = docker else {
            // No docker available in this test env. Skip.
            eprintln!("skipping: no local docker daemon");
            return;
        };
        let targets = HashMap::new();
        let result = run_auto_create_dbs(&docker, &state, &[], &targets, "proj", "inst-1").await;
        assert!(result.is_ok(), "empty services -> Ok; got {result:?}");
    }

    #[tokio::test]
    async fn auto_create_db_ssg_dispatch_errors_when_no_ssg_record() {
        // Service with auto_create_db=true + image=postgres +
        // target="coast-ssg" but no SSG record in state -> we fail
        // fast with a state error pointing at the Phase 3.5 gate.
        let state = in_memory_app_state();
        let docker = bollard::Docker::connect_with_local_defaults().ok();
        let Some(docker) = docker else {
            eprintln!("skipping: no local docker daemon");
            return;
        };
        let svc = cfg_postgres_auto("postgres:16");
        let targets = targets(&[("postgres", "coast-ssg")]);
        let result = run_auto_create_dbs(&docker, &state, &[svc], &targets, "proj", "inst-1").await;
        let err = result.expect_err("expected error, got Ok");
        let msg = err.to_string();
        assert!(
            msg.contains("no SSG is registered"),
            "error must mention missing SSG record; got: {msg}"
        );
        assert!(
            msg.contains("ensure_ready_for_consumer"),
            "error must reference Phase 3.5 gate; got: {msg}"
        );
    }
}
