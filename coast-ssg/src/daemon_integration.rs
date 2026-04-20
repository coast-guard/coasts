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

// --- Phase 4 consumer wiring -----------------------------------------------

/// Synthesize a `SharedServiceConfig` per `from_group = true` entry in
/// the consumer Coastfile so the existing `shared_service_routing` +
/// `compose_rewrite` pipeline can consume SSG-backed services the same
/// way it consumes inline ones.
///
/// Inputs are pulled from three places:
/// - `coastfile.shared_service_group_refs` gives us the list of
///   consumer references and their per-project overrides (inject,
///   auto_create_db).
/// - `manifest` (the active SSG build's `manifest.json`) provides the
///   image reference and the default `auto_create_db` for each service.
/// - `services` (the daemon's `ssg_services` rows) provides the dynamic
///   host port per inner container port.
///
/// Returns `Err` with a DESIGN.md §6.1-shaped message listing the
/// actually-available service names when a consumer references a name
/// the active SSG does not publish.
///
/// `volumes` and `env` are left empty: the consumer does not touch the
/// SSG container. They only appear on `SharedServiceConfig` because the
/// same struct is reused for inline services, where they are relevant.
pub fn synthesize_shared_service_configs(
    coastfile: &coast_core::coastfile::Coastfile,
    manifest: &build_artifact::SsgManifest,
    services: &[crate::state::SsgServiceRecord],
) -> Result<Vec<coast_core::types::SharedServiceConfig>> {
    if coastfile.shared_service_group_refs.is_empty() {
        return Ok(Vec::new());
    }

    let mut synthesized = Vec::with_capacity(coastfile.shared_service_group_refs.len());

    for consumer_ref in &coastfile.shared_service_group_refs {
        let manifest_svc = manifest
            .services
            .iter()
            .find(|s| s.name == consumer_ref.name)
            .ok_or_else(|| missing_ssg_service_error(&consumer_ref.name, manifest))?;

        let ports: Vec<coast_core::types::SharedServicePort> = services
            .iter()
            .filter(|s| s.service_name == consumer_ref.name)
            .map(|s| coast_core::types::SharedServicePort {
                host_port: s.dynamic_host_port,
                container_port: s.container_port,
            })
            .collect();

        synthesized.push(coast_core::types::SharedServiceConfig {
            name: consumer_ref.name.clone(),
            image: manifest_svc.image.clone(),
            ports,
            volumes: Vec::new(),
            env: std::collections::HashMap::new(),
            auto_create_db: consumer_ref
                .auto_create_db
                .unwrap_or(manifest_svc.auto_create_db),
            inject: consumer_ref.inject.clone(),
        });
    }

    Ok(synthesized)
}

fn missing_ssg_service_error(
    referenced_name: &str,
    manifest: &build_artifact::SsgManifest,
) -> CoastError {
    let mut available: Vec<&str> = manifest.services.iter().map(|s| s.name.as_str()).collect();
    available.sort();
    let available_list = if available.is_empty() {
        "(the active SSG has no services)".to_string()
    } else {
        format!("[{}]", available.join(", "))
    };
    CoastError::coastfile(format!(
        "Consumer references SSG service '{referenced_name}' which does not exist in the active \
         SSG build {build_id}. Available services: {available_list}.",
        build_id = manifest.build_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::artifact::{SsgManifest, SsgManifestService};
    use crate::state::SsgServiceRecord;
    use coast_core::coastfile::Coastfile;
    use coast_core::types::{InjectType, SharedServiceGroupRef};
    use std::path::Path;

    fn sample_manifest(services: Vec<(&str, &str, Vec<u16>, bool)>) -> SsgManifest {
        SsgManifest {
            build_id: "b1_20260420000000".to_string(),
            built_at: chrono::Utc::now(),
            coastfile_hash: "hash".to_string(),
            services: services
                .into_iter()
                .map(|(name, image, ports, auto_create_db)| SsgManifestService {
                    name: name.to_string(),
                    image: image.to_string(),
                    ports,
                    env_keys: Vec::new(),
                    volumes: Vec::new(),
                    auto_create_db,
                })
                .collect(),
        }
    }

    fn sample_record(service: &str, container_port: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            service_name: service.to_string(),
            container_port,
            dynamic_host_port: dynamic,
            status: "running".to_string(),
        }
    }

    fn coastfile_with_refs(refs: Vec<SharedServiceGroupRef>) -> Coastfile {
        let mut cf = Coastfile::parse("[coast]\nname = \"consumer\"\n", Path::new("/tmp"))
            .expect("minimal coastfile parses");
        cf.shared_service_group_refs = refs;
        cf
    }

    fn simple_ref(name: &str) -> SharedServiceGroupRef {
        SharedServiceGroupRef {
            name: name.to_string(),
            auto_create_db: None,
            inject: None,
        }
    }

    #[test]
    fn synthesize_empty_when_no_refs() {
        let cf = coastfile_with_refs(vec![]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60000)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn synthesize_single_service_uses_manifest_image_and_dynamic_port() {
        let cf = coastfile_with_refs(vec![simple_ref("postgres")]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert_eq!(result.len(), 1);
        let cfg = &result[0];
        assert_eq!(cfg.name, "postgres");
        assert_eq!(cfg.image, "postgres:16-alpine");
        assert_eq!(cfg.ports.len(), 1);
        assert_eq!(cfg.ports[0].container_port, 5432);
        assert_eq!(cfg.ports[0].host_port, 60001);
        assert!(cfg.volumes.is_empty(), "volumes are inert for consumers");
        assert!(cfg.env.is_empty(), "env is inert for consumers");
        assert!(!cfg.auto_create_db);
        assert!(cfg.inject.is_none());
    }

    #[test]
    fn synthesize_multi_service_preserves_ref_order() {
        let cf = coastfile_with_refs(vec![simple_ref("postgres"), simple_ref("redis")]);
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16-alpine", vec![5432], false),
            ("redis", "redis:7-alpine", vec![6379], false),
        ]);
        let services = vec![
            sample_record("postgres", 5432, 60001),
            sample_record("redis", 6379, 60002),
        ];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        let names: Vec<&str> = result.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["postgres", "redis"]);
    }

    #[test]
    fn synthesize_missing_service_errors_with_available_list() {
        let cf = coastfile_with_refs(vec![simple_ref("mongo")]);
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16-alpine", vec![5432], false),
            ("redis", "redis:7-alpine", vec![6379], false),
        ]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let err = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("Consumer references SSG service 'mongo'"),
            "unexpected message: {message}"
        );
        assert!(message.contains("does not exist"));
        assert!(message.contains("b1_20260420000000"));
        assert!(
            message.contains("[postgres, redis]"),
            "available list missing or unsorted: {message}"
        );
    }

    #[test]
    fn synthesize_missing_service_handles_empty_manifest() {
        let cf = coastfile_with_refs(vec![simple_ref("mongo")]);
        let manifest = sample_manifest(vec![]);
        let err = synthesize_shared_service_configs(&cf, &manifest, &[]).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("the active SSG has no services"));
    }

    #[test]
    fn synthesize_passes_inject_through_from_ref() {
        let cf = coastfile_with_refs(vec![SharedServiceGroupRef {
            name: "postgres".to_string(),
            auto_create_db: None,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        }]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        match &result[0].inject {
            Some(InjectType::Env(name)) => assert_eq!(name, "DATABASE_URL"),
            other => panic!("expected Env(DATABASE_URL), got {other:?}"),
        }
    }

    #[test]
    fn synthesize_auto_create_db_override_wins_over_manifest() {
        let cf = coastfile_with_refs(vec![SharedServiceGroupRef {
            name: "postgres".to_string(),
            auto_create_db: Some(true),
            inject: None,
        }]);
        // Manifest says false, ref overrides to true.
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert!(result[0].auto_create_db);
    }

    #[test]
    fn synthesize_auto_create_db_inherits_manifest_when_ref_is_none() {
        let cf = coastfile_with_refs(vec![simple_ref("postgres")]);
        // Manifest says true; ref doesn't override.
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], true)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert!(result[0].auto_create_db);
    }

    #[test]
    fn synthesize_multi_port_service_emits_one_entry_per_port() {
        let cf = coastfile_with_refs(vec![simple_ref("kafka")]);
        let manifest = sample_manifest(vec![("kafka", "kafka:3", vec![9092, 9093, 9094], false)]);
        let services = vec![
            sample_record("kafka", 9092, 60010),
            sample_record("kafka", 9093, 60011),
            sample_record("kafka", 9094, 60012),
        ];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert_eq!(result.len(), 1);
        let ports: Vec<(u16, u16)> = result[0]
            .ports
            .iter()
            .map(|p| (p.container_port, p.host_port))
            .collect();
        assert!(ports.contains(&(9092, 60010)));
        assert!(ports.contains(&(9093, 60011)));
        assert!(ports.contains(&(9094, 60012)));
    }
}

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
