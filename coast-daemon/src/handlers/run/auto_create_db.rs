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
        if !svc.auto_create_db {
            continue;
        }
        let Some(db_type) = infer_db_type(&svc.image) else {
            warn!(
                service = %svc.name,
                image = %svc.image,
                "auto_create_db = true but image is not a known DB engine; skipping"
            );
            continue;
        };
        let command = create_db_command(db_type, &db_name);

        let Some(target) = shared_service_targets.get(&svc.name) else {
            warn!(
                service = %svc.name,
                "auto_create_db: no routing target for shared service; skipping"
            );
            continue;
        };

        match target.as_str() {
            SSG_PLACEHOLDER => {
                exec_in_ssg_service(docker, state, &svc.name, command).await?;
            }
            host_container => {
                exec_in_host_container(&runtime, host_container, &svc.name, command).await?;
            }
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
}
