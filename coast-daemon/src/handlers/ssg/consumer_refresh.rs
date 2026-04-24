//! Refresh consumer coast shared-service proxies after an SSG
//! lifecycle verb (`run` / `start` / `restart`) reallocates dynamic
//! host ports.
//!
//! Phase: ssg-phase-11. See `coast-ssg/DESIGN.md §0 Phase 11` +
//! `§17-38 SETTLED`.
//!
//! The problem: when `coast ssg rm --with-data` + `coast ssg run`
//! picks fresh dynamic host ports, already-running consumer coasts
//! keep forwarding canonical ports (e.g. `postgres:5432`) through
//! socat listeners inside their OWN dind to
//! `host.docker.internal:<OLD-port>`. The consumer's socat processes
//! are configured once at provision time and never refreshed until
//! the consumer itself is rebuilt, so `psql` / `redis-cli` start
//! failing silently after a rebuild of the SSG.
//!
//! This module re-plans and re-ensures the socat forwarders for every
//! local running consumer whose artifact Coastfile has at least one
//! `from_group = true` reference, using the current `ssg_services`
//! dynamic ports.
//!
//! Remote coasts (shadow instances with `remote_host.is_some()`) are
//! intentionally skipped — they use the reverse-tunnel path, which
//! is re-established by the shadow's own run cycle.
//!
//! Errors are logged and never propagated; one broken consumer must
//! not fail the whole SSG lifecycle verb.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{info, warn};

use coast_core::coastfile::Coastfile;
use coast_core::types::InstanceStatus;

use coast_docker::shared_service_routing::{
    ensure_shared_service_proxies, plan_shared_service_routing,
};

use crate::handlers::run::paths::project_images_dir;
use crate::server::AppState;

/// One local running consumer that references at least one SSG
/// service and so needs its socat forwarders refreshed after an SSG
/// lifecycle verb.
#[derive(Debug, Clone)]
struct ConsumerToRefresh {
    project: String,
    name: String,
    container_id: String,
    coastfile: Coastfile,
}

/// Refresh shared-service socat forwarders for every local running
/// consumer whose artifact Coastfile has `from_group = true`
/// references. Returns the list of `project/instance` strings that
/// were successfully refreshed, which callers append to the lifecycle
/// response message. Failures are logged but never propagated.
///
/// Per-project SSG (§23): each consumer's `ssg_services` lookup is
/// scoped to that consumer's own project, so this function is safe
/// to call after any project's SSG lifecycle verb.
pub(crate) async fn refresh_consumer_proxies_after_lifecycle(state: &Arc<AppState>) -> Vec<String> {
    let Some(docker) = state.docker.as_ref() else {
        return Vec::new();
    };

    let consumers = match gather_eligible_consumers(state).await {
        Some(list) if !list.is_empty() => list,
        _ => return Vec::new(),
    };

    // Phase 23: each consumer reads its own project's SSG manifest.
    // Previously a single global `latest` manifest was loaded up
    // front and applied to every consumer; that leaked across
    // projects.
    apply_refresh_to_each(state, &docker, &consumers).await
}

async fn gather_eligible_consumers(state: &Arc<AppState>) -> Option<Vec<ConsumerToRefresh>> {
    match collect_consumers_to_refresh(state).await {
        Ok(list) => Some(list),
        Err(err) => {
            warn!(error = %err, "consumer refresh: failed to enumerate local consumers; skipping");
            None
        }
    }
}

async fn apply_refresh_to_each(
    state: &Arc<AppState>,
    docker: &bollard::Docker,
    consumers: &[ConsumerToRefresh],
) -> Vec<String> {
    let mut refreshed = Vec::with_capacity(consumers.len());
    for consumer in consumers {
        if let Some(label) = refresh_one_consumer(state, docker, consumer).await {
            refreshed.push(label);
        }
    }
    refreshed
}

/// Refresh a single consumer. Returns the successfully-refreshed
/// `project/instance` label when the refresh succeeded, `None`
/// otherwise (all failure modes are logged and swallowed — callers
/// never see an error from consumer refresh).
async fn refresh_one_consumer(
    state: &Arc<AppState>,
    docker: &bollard::Docker,
    consumer: &ConsumerToRefresh,
) -> Option<String> {
    let label = format!("{}/{}", consumer.project, consumer.name);
    let (manifest, services) = resolve_consumer_ssg(state, consumer, &label).await?;
    execute_refresh(docker, consumer, &manifest, &services, &label).await
}

/// Resolve the manifest + services for a consumer, or log-and-skip.
/// Per-project lookup (§23): consumers route into their own project's
/// SSG, so both the manifest and the `ssg_services` list are scoped
/// by this consumer's project.
async fn resolve_consumer_ssg(
    state: &Arc<AppState>,
    consumer: &ConsumerToRefresh,
    label: &str,
) -> Option<(
    coast_ssg::build::artifact::SsgManifest,
    Vec<coast_ssg::state::SsgServiceRecord>,
)> {
    match load_project_ssg_for_consumer(state, consumer).await {
        Ok(Some(pair)) => Some(pair),
        Ok(None) => {
            info!(
                consumer = %label,
                "consumer refresh: no SSG build for project; skipping",
            );
            None
        }
        Err(err) => {
            warn!(
                consumer = %label,
                error = %err,
                "consumer refresh: failed to load per-project SSG manifest; skipping",
            );
            None
        }
    }
}

/// Call `refresh_one` and log the outcome. Returns `Some(label)` on
/// success so the caller can append it to the refreshed list.
async fn execute_refresh(
    docker: &bollard::Docker,
    consumer: &ConsumerToRefresh,
    manifest: &coast_ssg::build::artifact::SsgManifest,
    services: &[coast_ssg::state::SsgServiceRecord],
    label: &str,
) -> Option<String> {
    match refresh_one(docker, consumer, manifest, services).await {
        Ok(()) => {
            info!(consumer = %label, "consumer refresh: proxies updated for new SSG ports");
            Some(label.to_string())
        }
        Err(err) => {
            warn!(
                consumer = %label,
                error = %err,
                "consumer refresh: failed; leaving old proxies in place",
            );
            None
        }
    }
}

/// Enumerate local running consumers whose artifact Coastfile carries
/// at least one `from_group = true` reference. Best-effort: instances
/// with missing or unparseable artifacts are skipped.
async fn collect_consumers_to_refresh(
    state: &Arc<AppState>,
) -> coast_core::error::Result<Vec<ConsumerToRefresh>> {
    let rows = {
        let db = state.db.lock().await;
        db.list_instances()?
    };

    let mut result = Vec::new();
    for inst in rows {
        if !is_eligible_for_refresh(&inst) {
            continue;
        }
        let Some(container_id) = inst.container_id.clone() else {
            continue;
        };
        let coastfile_path = artifact_coastfile_path(&inst.project, inst.build_id.as_deref());
        let Some(coastfile) = load_artifact_coastfile(&coastfile_path) else {
            continue;
        };
        if coastfile.shared_service_group_refs.is_empty() {
            continue;
        }
        result.push(ConsumerToRefresh {
            project: inst.project,
            name: inst.name,
            container_id,
            coastfile,
        });
    }
    Ok(result)
}

/// Running, local-only instances are eligible for a refresh. Remote
/// shadows are skipped (they go through the reverse-tunnel path and
/// re-establish on their own run cycle).
fn is_eligible_for_refresh(inst: &coast_core::types::CoastInstance) -> bool {
    inst.remote_host.is_none() && inst.status == InstanceStatus::Running
}

/// Resolve the artifact `coastfile.toml` path mirroring
/// `provision::resolve_artifact_dir`'s fallback: try the recorded
/// build first, then fall back to the `latest` symlink.
fn artifact_coastfile_path(project: &str, build_id: Option<&str>) -> PathBuf {
    let project_dir = project_images_dir(project);
    if let Some(bid) = build_id {
        let resolved = project_dir.join(bid).join("coastfile.toml");
        if resolved.exists() {
            return resolved;
        }
    }
    project_dir.join("latest").join("coastfile.toml")
}

fn load_artifact_coastfile(path: &Path) -> Option<Coastfile> {
    if !path.exists() {
        return None;
    }
    match Coastfile::from_file(path) {
        Ok(cf) => Some(cf),
        Err(err) => {
            warn!(
                path = %path.display(),
                error = %err,
                "consumer refresh: skipping instance with unparseable artifact Coastfile",
            );
            None
        }
    }
}

/// Resolve `(manifest, services)` for a consumer's own project's
/// SSG. Returns `Ok(None)` when the project has no `ssg build` in
/// state — the caller skips this consumer. Errors on state or
/// filesystem access failures (the caller logs and skips).
async fn load_project_ssg_for_consumer(
    state: &Arc<AppState>,
    consumer: &ConsumerToRefresh,
) -> coast_core::error::Result<
    Option<(
        coast_ssg::build::artifact::SsgManifest,
        Vec<coast_ssg::state::SsgServiceRecord>,
    )>,
> {
    use coast_ssg::state::SsgStateExt;

    let (build_id, services) = {
        let db = state.db.lock().await;
        let pin = db.get_ssg_consumer_pin(&consumer.project)?;
        let latest = db
            .get_ssg(&consumer.project)?
            .and_then(|r| r.latest_build_id);
        let services = db.list_ssg_services(&consumer.project)?;
        (pin.map(|p| p.build_id).or(latest), services)
    };
    let Some(build_id) = build_id else {
        return Ok(None);
    };

    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id)?;
    let manifest_path = build_dir.join("manifest.json");
    let content =
        std::fs::read_to_string(&manifest_path).map_err(|e| coast_core::error::CoastError::Io {
            message: format!(
                "failed to read SSG manifest '{}': {e}",
                manifest_path.display()
            ),
            path: manifest_path.clone(),
            source: Some(e),
        })?;
    let manifest: coast_ssg::build::artifact::SsgManifest = serde_json::from_str(&content)
        .map_err(|e| {
            coast_core::error::CoastError::artifact(format!(
                "failed to parse SSG manifest '{}': {e}",
                manifest_path.display()
            ))
        })?;
    Ok(Some((manifest, services)))
}

async fn refresh_one(
    docker: &bollard::Docker,
    consumer: &ConsumerToRefresh,
    manifest: &coast_ssg::build::artifact::SsgManifest,
    services: &[coast_ssg::state::SsgServiceRecord],
) -> coast_core::error::Result<()> {
    let synthesized = coast_ssg::daemon_integration::synthesize_shared_service_configs(
        &consumer.coastfile,
        manifest,
        services,
    )?;
    if synthesized.is_empty() {
        return Ok(());
    }

    // The socat upstream target is always `host.docker.internal`
    // (the `SOCAT_UPSTREAM_HOST` constant in `shared_service_routing`);
    // the placeholder we insert here just has to exist for the
    // routing planner's existence check. Mirror the Phase 4 flow in
    // `provision.rs`.
    let mut targets: HashMap<String, String> = HashMap::new();
    for cfg in &synthesized {
        targets.insert(cfg.name.clone(), "coast-ssg".to_string());
    }

    let plan =
        plan_shared_service_routing(docker, &consumer.container_id, &synthesized, &targets).await?;
    ensure_shared_service_proxies(docker, &consumer.container_id, &plan).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    fn sample_instance(project: &str, name: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            status,
            project: project.to_string(),
            branch: None,
            commit_sha: None,
            container_id: Some("cid".to_string()),
            runtime: RuntimeType::Dind,
            created_at: Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    #[test]
    fn is_eligible_accepts_local_running() {
        let inst = sample_instance("app", "dev-1", InstanceStatus::Running);
        assert!(is_eligible_for_refresh(&inst));
    }

    #[test]
    fn is_eligible_rejects_stopped() {
        let inst = sample_instance("app", "dev-1", InstanceStatus::Stopped);
        assert!(!is_eligible_for_refresh(&inst));
    }

    #[test]
    fn is_eligible_rejects_remote_shadow() {
        let mut inst = sample_instance("app", "dev-1", InstanceStatus::Running);
        inst.remote_host = Some("host-x".to_string());
        assert!(!is_eligible_for_refresh(&inst));
    }

    #[test]
    fn is_eligible_rejects_non_running_statuses() {
        for status in [
            InstanceStatus::Idle,
            InstanceStatus::CheckedOut,
            InstanceStatus::Provisioning,
            InstanceStatus::Enqueued,
            InstanceStatus::Starting,
            InstanceStatus::Stopping,
            InstanceStatus::Assigning,
            InstanceStatus::Unassigning,
        ] {
            let inst = sample_instance("app", "dev-1", status);
            assert!(
                !is_eligible_for_refresh(&inst),
                "status {:?} should not be refresh-eligible",
                inst.status
            );
        }
    }

    #[test]
    fn artifact_coastfile_path_falls_back_to_latest_when_build_missing() {
        // build_id = None -> always use latest.
        let p = artifact_coastfile_path("nope-project", None);
        assert!(
            p.ends_with("nope-project/latest/coastfile.toml"),
            "expected latest fallback, got {}",
            p.display()
        );
    }

    #[test]
    fn artifact_coastfile_path_falls_back_to_latest_for_missing_build_dir() {
        // build_id provided but the directory doesn't exist -> fall through to latest.
        let p = artifact_coastfile_path("nope-project", Some("b1_nonexistent"));
        assert!(
            p.ends_with("nope-project/latest/coastfile.toml"),
            "expected latest fallback when build_id dir missing, got {}",
            p.display()
        );
    }

    #[test]
    fn load_artifact_coastfile_returns_none_for_missing() {
        let path = std::path::PathBuf::from("/tmp/coast-nonexistent-consumer-refresh.toml");
        assert!(load_artifact_coastfile(&path).is_none());
    }

    #[test]
    fn load_artifact_coastfile_returns_none_for_bad_toml() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Coastfile");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "this is {{ not valid toml").unwrap();
        assert!(load_artifact_coastfile(&path).is_none());
    }

    #[test]
    fn load_artifact_coastfile_parses_valid_consumer_coastfile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Coastfile");
        std::fs::write(
            &path,
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
from_group = true
"#,
        )
        .unwrap();
        let cf = load_artifact_coastfile(&path).expect("parses");
        assert_eq!(cf.shared_service_group_refs.len(), 1);
        assert_eq!(cf.shared_service_group_refs[0].name, "postgres");
    }

    // --- Pattern C: in-memory AppState exercises ---

    fn in_memory_app_state() -> Arc<AppState> {
        use crate::state::StateDb;
        let db = StateDb::open_in_memory().expect("in-memory statedb");
        Arc::new(AppState::new_for_testing(db))
    }

    #[tokio::test]
    async fn refresh_returns_empty_when_no_docker() {
        // `new_for_testing` leaves docker = None, so the function
        // short-circuits before touching any other state.
        let state = in_memory_app_state();
        let refreshed = refresh_consumer_proxies_after_lifecycle(&state).await;
        assert!(refreshed.is_empty());
    }
}
