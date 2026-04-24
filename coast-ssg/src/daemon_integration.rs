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
use coast_core::protocol::{BuildProgressEvent, SsgPortInfo, SsgResponse, SsgServiceInfo};

use crate::build::artifact as build_artifact;
use crate::build::images::pull_and_cache_ssg_images;
use crate::coastfile::SsgCoastfile;
use crate::docker_ops::SsgDockerOps;
use crate::paths;
use crate::runtime::compose_synth::synth_inner_compose;

/// Inputs for a `coast ssg build` request (mirrors
/// [`coast_core::protocol::SsgAction::Build`]).
#[derive(Debug, Clone)]
pub struct SsgBuildInputs {
    /// Consumer project name (from the sibling Coastfile's
    /// `[coast].name`). Per-project SSGs key all state by this
    /// value (`coast-ssg/DESIGN.md §23`).
    pub project: String,
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
///
/// `pinned_build_ids` is the set of `ssg_consumer_pins.build_id`
/// values loaded by the daemon before this call; any build matching
/// one of these ids survives the auto-prune pass (Phase 16). Pass an
/// empty set when pin-aware pruning is not desired (e.g. first build
/// ever, tests).
pub async fn build_ssg(
    inputs: SsgBuildInputs,
    ops: &dyn SsgDockerOps,
    pinned_build_ids: std::collections::HashSet<String>,
    progress: Sender<BuildProgressEvent>,
) -> Result<SsgResponse> {
    // --- Step 1: parse ---
    let (cf, _raw) = {
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
    // Phase 17: hash the flattened (post-inheritance) standalone
    // form rather than the raw top-level bytes. This is the
    // correctness fix for `extends` / `includes`: a parent-only
    // change must invalidate the build cache, and the artifact
    // directory already stores the same flattened form on disk via
    // `write_artifact`. See `DESIGN.md §17 SETTLED #42`.
    let flattened = cf.to_standalone_toml();
    let build_id = build_artifact::compute_build_id(&flattened, &cf, now);
    let coastfile_hash = build_artifact::coastfile_hash_for(&flattened, &cf);
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
    pull_and_cache_ssg_images(ops, &cf.services, &cache_dir, &progress, 4, total).await?;

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
    let pruned = build_artifact::auto_prune_preserving(5, &pinned_build_ids)?;
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
/// When `state` is `Some`, merges live runtime data from
/// `ssg_services` so callers see actual dynamic host ports and
/// per-service status (`running` / `stopped` / etc.) alongside the
/// built configuration. When `state` is `None`, falls back to the
/// manifest-only view (`dynamic_host_port = 0`, `status = "built"`)
/// used by pre-Phase-9 callers.
pub fn ps_ssg(
    project: &str,
    state: Option<&dyn crate::state::SsgStateExt>,
) -> Result<SsgResponse> {
    let Some((build_id, manifest)) = load_latest_ssg_manifest_with_id()? else {
        return Ok(SsgResponse {
            message: "No SSG build found. Run `coast ssg build` first.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
            findings: Vec::new(),
        });
    };

    let (container_status, service_rows, ports): (
        Option<String>,
        Vec<crate::state::SsgServiceRecord>,
        Vec<SsgPortInfo>,
    ) = match state {
        Some(db) => {
            let ssg_status = db.get_ssg(project)?.map(|r| r.status);
            let services = db.list_ssg_services(project)?;
            let ports: Vec<SsgPortInfo> = services
                .iter()
                .map(|s| SsgPortInfo {
                    service: s.service_name.clone(),
                    canonical_port: s.container_port,
                    dynamic_host_port: s.dynamic_host_port,
                    checked_out: false,
                })
                .collect();
            (ssg_status, services, ports)
        }
        None => (None, Vec::new(), Vec::new()),
    };

    let services: Vec<SsgServiceInfo> = manifest
        .services
        .iter()
        .map(|svc| {
            let inner_port = svc.ports.first().copied().unwrap_or(0);
            let live = service_rows.iter().find(|row| row.service_name == svc.name);
            SsgServiceInfo {
                name: svc.name.clone(),
                image: svc.image.clone(),
                inner_port,
                dynamic_host_port: live.map(|r| r.dynamic_host_port).unwrap_or(0),
                container_id: None,
                status: live
                    .map(|r| r.status.clone())
                    .unwrap_or_else(|| "built".to_string()),
            }
        })
        .collect();

    let message = match &container_status {
        Some(s) => format!("SSG build: {build_id}  ({s})"),
        None => format!("SSG build: {build_id}"),
    };

    Ok(SsgResponse {
        message,
        status: container_status,
        services,
        ports,
        findings: Vec::new(),
    })
}

/// Load the active SSG build's `(build_id, manifest)` or `None` when
/// no build exists. Shared helper for any daemon handler that wants
/// to read the artifact without reimplementing the disk-read dance.
pub fn load_latest_ssg_manifest_with_id() -> Result<Option<(String, build_artifact::SsgManifest)>> {
    let Some(build_id) = paths::resolve_latest_build_id() else {
        return Ok(None);
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
    Ok(Some((build_id, manifest)))
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
    exec_ssg, logs_ssg, ports_ssg, restart_ssg, rm_ssg, run_ssg, run_ssg_with_build_id, start_ssg,
    stop_ssg, SsgRunOutcome, SsgStartOutcome, SsgStopOutcome,
};

// --- Phase 15: host-volume import orchestrator ------------------------------

pub use crate::runtime::host_volume_import::{run_import, HostVolumeImportInputs};

/// Resolve an SSG Coastfile from the standard discovery triplet
/// (`file` / `working_dir` / inline `config`) and return both the
/// on-disk path (when the source is a file) and the raw TOML text.
///
/// Shared by `build_ssg` and `coast ssg import-host-volume` so both
/// verbs follow the exact same resolution rules and error messages.
/// Inline `config` returns `(None, config_text)` — callers that
/// need an on-disk path (e.g. `--apply`) hard-error on `None`
/// themselves with a phase-specific message.
pub fn resolve_ssg_coastfile_source(
    file: Option<&Path>,
    working_dir: Option<&Path>,
    config: Option<&str>,
) -> Result<(Option<PathBuf>, String)> {
    if let Some(inline) = config {
        return Ok((None, inline.to_string()));
    }
    let path = if let Some(p) = file {
        p.to_path_buf()
    } else {
        let base = working_dir
            .map(Path::to_path_buf)
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
    Ok((Some(path), raw))
}

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

/// Synthesize one `SharedServicePortForward` per declared container
/// port of every `from_group = true` service the consumer references.
///
/// Phase: ssg-phase-4.5. See `DESIGN.md §20.2`.
///
/// The remote `coast-service` consumes the resulting list to (a) strip
/// those service names from the inner compose and (b) add
/// `extra_hosts: <name> -> host-gateway` so app containers resolve
/// e.g. `postgres:5432` back through the reverse SSH tunnel.
///
/// Unlike the local-path `synthesize_shared_service_configs`, this
/// returns the thinner `SharedServicePortForward` protocol type —
/// `coast-service` only needs `{name, port}` and is SSG-agnostic.
///
/// Errors with the DESIGN.md §6.1 missing-service wording (shared
/// with the Phase 4 consumer-wiring path via
/// [`missing_ssg_service_error`]) when the consumer names a service
/// the active SSG does not publish.
pub fn synthesize_remote_forwards_for_consumer(
    coastfile: &coast_core::coastfile::Coastfile,
    manifest: &build_artifact::SsgManifest,
    services: &[crate::state::SsgServiceRecord],
) -> Result<Vec<coast_core::protocol::SharedServicePortForward>> {
    if coastfile.shared_service_group_refs.is_empty() {
        return Ok(Vec::new());
    }

    let mut forwards = Vec::new();

    for consumer_ref in &coastfile.shared_service_group_refs {
        // Validate the ref against the active SSG build. Reuses the
        // same error shape as the local-path synthesizer so users see
        // consistent messaging regardless of which code path runs.
        let _manifest_svc = manifest
            .services
            .iter()
            .find(|s| s.name == consumer_ref.name)
            .ok_or_else(|| missing_ssg_service_error(&consumer_ref.name, manifest))?;

        for svc in services
            .iter()
            .filter(|s| s.service_name == consumer_ref.name)
        {
            // `remote_port` is placeholder-zero here; the daemon allocates
            // a real port on top of this synthesized list in Phase 18 step 3.
            forwards.push(coast_core::protocol::SharedServicePortForward {
                name: consumer_ref.name.clone(),
                port: svc.container_port,
                remote_port: 0,
            });
        }
    }

    Ok(forwards)
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

// --- Phase 5 auto_create_db nested-exec bridge -----------------------------

/// Execute `command` inside the inner `service_name` container of the
/// SSG singleton DinD, treating any non-zero exit as an error.
///
/// Phase 5 uses this to run a psql/mysql CREATE-DATABASE command
/// against an SSG-backed DB service on behalf of a consumer coast.
/// The `command` vector is whatever
/// [`coast_daemon::shared_services::create_db_command`] returned — the
/// SQL builder lives in `coast-daemon` (inline path's home) and is
/// shared verbatim with the nested path. See `DESIGN.md §13`.
///
/// Errors include the service name and the captured stderr so
/// troubleshooting doesn't require crawling the daemon logs.
pub async fn create_instance_db_for_consumer(
    ops: &dyn SsgDockerOps,
    ssg_record: &crate::state::SsgRecord,
    service_name: &str,
    command: Vec<String>,
) -> Result<()> {
    let result =
        crate::runtime::auto_create_db::exec_in_ssg_service(ops, ssg_record, service_name, command)
            .await?;
    if !result.success() {
        return Err(CoastError::docker(format!(
            "auto_create_db failed inside SSG service '{service_name}': exit {code}. \
             stderr: {stderr}",
            code = result.exit_code,
            stderr = result.stderr.trim(),
        )));
    }
    Ok(())
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
            project: "test-proj".to_string(),
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
    fn synthesize_auto_create_db_false_override_disables_ssg_default() {
        // DESIGN.md §6 three-valued override: Some(false) on the
        // consumer disables auto_create_db even when the SSG service
        // has it enabled.
        let cf = coastfile_with_refs(vec![SharedServiceGroupRef {
            name: "postgres".to_string(),
            auto_create_db: Some(false),
            inject: None,
        }]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], true)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_shared_service_configs(&cf, &manifest, &services).unwrap();
        assert!(
            !result[0].auto_create_db,
            "Some(false) must override SSG auto_create_db = true"
        );
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

    // --- synthesize_remote_forwards_for_consumer (Phase 4.5) ---

    #[test]
    fn synthesize_remote_forwards_empty_when_no_refs() {
        let cf = coastfile_with_refs(vec![]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_remote_forwards_for_consumer(&cf, &manifest, &services).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn synthesize_remote_forwards_single_service_uses_container_port() {
        let cf = coastfile_with_refs(vec![simple_ref("postgres")]);
        let manifest = sample_manifest(vec![("postgres", "postgres:16-alpine", vec![5432], false)]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let result = synthesize_remote_forwards_for_consumer(&cf, &manifest, &services).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "postgres");
        // The forward carries the CANONICAL (container) port, not the
        // dynamic one — the local-side rewrite happens later via
        // `rewrite_reverse_tunnel_pairs`.
        assert_eq!(result[0].port, 5432);
    }

    #[test]
    fn synthesize_remote_forwards_multi_port_emits_one_entry_per_port() {
        let cf = coastfile_with_refs(vec![simple_ref("kafka")]);
        let manifest = sample_manifest(vec![("kafka", "kafka:3", vec![9092, 9093, 9094], false)]);
        let services = vec![
            sample_record("kafka", 9092, 60010),
            sample_record("kafka", 9093, 60011),
            sample_record("kafka", 9094, 60012),
        ];
        let result = synthesize_remote_forwards_for_consumer(&cf, &manifest, &services).unwrap();
        let ports: Vec<u16> = result.iter().map(|f| f.port).collect();
        assert_eq!(ports, vec![9092, 9093, 9094]);
        assert!(result.iter().all(|f| f.name == "kafka"));
    }

    #[test]
    fn synthesize_remote_forwards_multi_ref_preserves_order() {
        let cf = coastfile_with_refs(vec![simple_ref("postgres"), simple_ref("redis")]);
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16-alpine", vec![5432], false),
            ("redis", "redis:7-alpine", vec![6379], false),
        ]);
        let services = vec![
            sample_record("postgres", 5432, 60001),
            sample_record("redis", 6379, 60002),
        ];
        let result = synthesize_remote_forwards_for_consumer(&cf, &manifest, &services).unwrap();
        let names: Vec<&str> = result.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["postgres", "redis"]);
    }

    #[test]
    fn synthesize_remote_forwards_missing_service_errors_with_available_list() {
        let cf = coastfile_with_refs(vec![simple_ref("mongo")]);
        let manifest = sample_manifest(vec![
            ("postgres", "postgres:16-alpine", vec![5432], false),
            ("redis", "redis:7-alpine", vec![6379], false),
        ]);
        let services = vec![sample_record("postgres", 5432, 60001)];
        let err = synthesize_remote_forwards_for_consumer(&cf, &manifest, &services).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Consumer references SSG service 'mongo'"));
        assert!(message.contains("[postgres, redis]"));
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
        findings: Vec::new(),
    }
}
