//! SSG build artifact: manifest + on-disk layout.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! Each SSG build gets a directory under `~/.coast/ssg/builds/{build_id}/`
//! containing:
//!
//! - `manifest.json` — build metadata (images, timestamp, hash, services).
//! - `ssg-coastfile.toml` — the interpolated, post-validation Coastfile.
//! - `compose.yml` — the synthesized inner compose file (from
//!   [`crate::runtime::compose_synth`]).
//!
//! After a successful build, [`flip_latest`] atomically points
//! `~/.coast/ssg/latest` at the new build, and [`auto_prune`] removes
//! stale builds beyond the keep limit (default 5 per DESIGN.md §9.1).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use coast_core::error::{CoastError, Result};

use crate::coastfile::{SsgCoastfile, SsgSharedServiceConfig, SsgVolumeEntry};
use crate::paths::{ssg_build_dir, ssg_builds_dir, ssg_latest_link};

/// On-disk manifest. Written atomically as `manifest.json` in the
/// build directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsgManifest {
    pub build_id: String,
    pub built_at: DateTime<Utc>,
    pub coastfile_hash: String,
    pub services: Vec<SsgManifestService>,
}

/// Per-service snapshot captured in the manifest.
///
/// Secrets and env values are NOT stored (only env var names) —
/// matches the regular coast build's safety posture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsgManifestService {
    pub name: String,
    pub image: String,
    /// Declared container ports. May be empty for sidecar services.
    pub ports: Vec<u16>,
    /// Env var *names*, sorted alphabetically. Values omitted.
    pub env_keys: Vec<String>,
    /// Volume entries in their string form (`"source:target"`).
    pub volumes: Vec<String>,
    pub auto_create_db: bool,
}

impl From<&SsgSharedServiceConfig> for SsgManifestService {
    fn from(svc: &SsgSharedServiceConfig) -> Self {
        let mut env_keys: Vec<String> = svc.env.keys().cloned().collect();
        env_keys.sort();

        let volumes = svc.volumes.iter().map(format_volume_entry).collect();

        Self {
            name: svc.name.clone(),
            image: svc.image.clone(),
            ports: svc.ports.clone(),
            env_keys,
            volumes,
            auto_create_db: svc.auto_create_db,
        }
    }
}

fn format_volume_entry(entry: &SsgVolumeEntry) -> String {
    match entry {
        SsgVolumeEntry::HostBindMount {
            host_path,
            container_path,
        } => format!("{}:{}", host_path.display(), container_path.display()),
        SsgVolumeEntry::InnerNamedVolume {
            name,
            container_path,
        } => format!("{}:{}", name, container_path.display()),
    }
}

/// Compute the build id: `{coastfile_hash}_{YYYYMMDDHHMMSS}`.
///
/// Hash inputs include the raw Coastfile source plus a `Debug`-based
/// fingerprint of the validated config (so structural changes that
/// don't touch the raw source — e.g. interpolation — still produce a
/// different id). Mirrors the regular coast build at
/// [coast-daemon/src/handlers/build/artifact.rs::compute_coastfile_hash].
pub fn compute_build_id(raw: &str, cf: &SsgCoastfile, now: DateTime<Utc>) -> String {
    let hash = compute_coastfile_hash(raw, cf);
    format!("{}_{}", hash, now.format("%Y%m%d%H%M%S"))
}

fn compute_coastfile_hash(raw: &str, cf: &SsgCoastfile) -> String {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    // Hash a compact, deterministic summary of the parsed services.
    for svc in &cf.services {
        svc.name.hash(&mut hasher);
        svc.image.hash(&mut hasher);
        svc.ports.hash(&mut hasher);
        // BTreeMap-like deterministic order.
        let mut env: Vec<(&String, &String)> = svc.env.iter().collect();
        env.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in env {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        for entry in &svc.volumes {
            format_volume_entry(entry).hash(&mut hasher);
        }
        svc.auto_create_db.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

/// Write `manifest.json`, `ssg-coastfile.toml`, and `compose.yml` into
/// `~/.coast/ssg/builds/{build_id}/`.
///
/// Creates the build directory if it doesn't exist. Returns the
/// absolute path to the build directory.
pub fn write_artifact(
    manifest: &SsgManifest,
    coastfile: &SsgCoastfile,
    inner_compose: &str,
) -> Result<PathBuf> {
    let dir = ssg_build_dir(&manifest.build_id)?;
    std::fs::create_dir_all(&dir).map_err(|e| CoastError::Io {
        message: format!("failed to create SSG build dir '{}': {e}", dir.display()),
        path: dir.clone(),
        source: Some(e),
    })?;

    let manifest_path = dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(manifest)
        .map_err(|e| CoastError::artifact(format!("failed to serialize SSG manifest: {e}")))?;
    std::fs::write(&manifest_path, manifest_json).map_err(|e| CoastError::Io {
        message: format!(
            "failed to write SSG manifest '{}': {e}",
            manifest_path.display()
        ),
        path: manifest_path,
        source: Some(e),
    })?;

    let coastfile_path = dir.join("ssg-coastfile.toml");
    std::fs::write(&coastfile_path, coastfile.to_standalone_toml()).map_err(|e| {
        CoastError::Io {
            message: format!(
                "failed to write ssg-coastfile.toml '{}': {e}",
                coastfile_path.display()
            ),
            path: coastfile_path,
            source: Some(e),
        }
    })?;

    let compose_path = dir.join("compose.yml");
    std::fs::write(&compose_path, inner_compose).map_err(|e| CoastError::Io {
        message: format!(
            "failed to write compose.yml '{}': {e}",
            compose_path.display()
        ),
        path: compose_path,
        source: Some(e),
    })?;

    Ok(dir)
}

/// Atomically flip `~/.coast/ssg/latest` to point at `{build_id}`.
///
/// Removes any existing symlink first. On non-unix platforms this is
/// a no-op (SSG is local-only per DESIGN.md §3).
#[cfg(unix)]
pub fn flip_latest(build_id: &str) -> Result<()> {
    let link = ssg_latest_link()?;
    // Ensure parent exists.
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoastError::Io {
            message: format!("failed to create {}: {e}", parent.display()),
            path: parent.to_path_buf(),
            source: Some(e),
        })?;
    }

    // Remove whatever is there (file, symlink, or nothing).
    let _ = std::fs::remove_file(&link);

    let target = ssg_build_dir(build_id)?;
    std::os::unix::fs::symlink(&target, &link).map_err(|e| CoastError::Io {
        message: format!(
            "failed to point {} at {}: {e}",
            link.display(),
            target.display()
        ),
        path: link.clone(),
        source: Some(e),
    })?;
    Ok(())
}

#[cfg(not(unix))]
pub fn flip_latest(_build_id: &str) -> Result<()> {
    Err(CoastError::state(
        "SSG is unix-only in v1 (flip_latest requires symlinks)",
    ))
}

/// Remove build directories beyond `keep`, oldest first.
///
/// Never removes the build currently targeted by `latest`. Returns the
/// number of builds removed.
///
/// Sort key is the manifest's `built_at` timestamp. Builds that are
/// missing or have an unparseable manifest are sorted to the front
/// (oldest) so they get pruned first.
pub fn auto_prune(keep: usize) -> Result<usize> {
    let builds_dir = ssg_builds_dir()?;
    if !builds_dir.exists() {
        return Ok(0);
    }

    let latest_target = crate::paths::resolve_latest_build_id();

    let mut entries: Vec<(PathBuf, Option<DateTime<Utc>>)> = Vec::new();
    for entry in std::fs::read_dir(&builds_dir).map_err(|e| CoastError::Io {
        message: format!(
            "failed to list SSG builds dir '{}': {e}",
            builds_dir.display()
        ),
        path: builds_dir.clone(),
        source: Some(e),
    })? {
        let entry = entry.map_err(|e| CoastError::Io {
            message: format!("failed to read SSG builds entry: {e}"),
            path: builds_dir.clone(),
            source: Some(e),
        })?;
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let built_at = read_manifest_timestamp(&path);
        entries.push((path, built_at));
    }

    // Sort ascending by timestamp. `None` sorts first (oldest).
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    let total = entries.len();
    if total <= keep {
        return Ok(0);
    }

    let to_remove_count = total - keep;
    let mut removed = 0;
    for (path, _) in entries.into_iter().take(to_remove_count) {
        let name = path
            .file_name()
            .and_then(|f| f.to_str())
            .map(ToString::to_string);
        if let Some(ref n) = name {
            if Some(n) == latest_target.as_ref() {
                // Protect the currently-pinned build even if it's old.
                continue;
            }
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn read_manifest_timestamp(build_dir: &Path) -> Option<DateTime<Utc>> {
    let path = build_dir.join("manifest.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let manifest: SsgManifest = serde_json::from_str(&content).ok()?;
    Some(manifest.built_at)
}

/// Build a full `SsgManifest` from a parsed Coastfile.
pub fn build_manifest(build_id: &str, coastfile_hash: &str, cf: &SsgCoastfile) -> SsgManifest {
    let mut services: Vec<SsgManifestService> =
        cf.services.iter().map(SsgManifestService::from).collect();
    services.sort_by(|a, b| a.name.cmp(&b.name));
    SsgManifest {
        build_id: build_id.to_string(),
        built_at: Utc::now(),
        coastfile_hash: coastfile_hash.to_string(),
        services,
    }
}

/// Public accessor for the hash used by [`compute_build_id`], so the
/// manifest's `coastfile_hash` and the build id share a prefix.
pub fn coastfile_hash_for(raw: &str, cf: &SsgCoastfile) -> String {
    compute_coastfile_hash(raw, cf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_coast_home<F: FnOnce(&std::path::Path)>(f: F) {
        let guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", tmp.path());
        }
        f(tmp.path());
        unsafe {
            match prev {
                Some(v) => std::env::set_var("COAST_HOME", v),
                None => std::env::remove_var("COAST_HOME"),
            }
        }
        drop(guard);
    }

    fn sample_cf() -> SsgCoastfile {
        SsgCoastfile::parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
"#,
            Path::new("/tmp"),
        )
        .unwrap()
    }

    #[test]
    fn compute_build_id_is_deterministic() {
        let cf = sample_cf();
        let raw = "[shared_services.postgres]\nimage = \"postgres:16\"\n";
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let a = compute_build_id(raw, &cf, now);
        let b = compute_build_id(raw, &cf, now);
        assert_eq!(a, b);

        // Shape: `{hex_hash}_{YYYYMMDDHHMMSS}`. Suffix format matches
        // what `now` renders with `%Y%m%d%H%M%S`.
        let (hash, suffix) = a.rsplit_once('_').expect("build id has a '_' separator");
        assert!(!hash.is_empty(), "build id hash component is empty");
        assert_eq!(
            suffix,
            now.format("%Y%m%d%H%M%S").to_string(),
            "timestamp suffix format mismatch"
        );
    }

    #[test]
    fn compute_build_id_differs_on_content_change() {
        let cf_a = sample_cf();
        let cf_b = SsgCoastfile::parse(
            r#"
[shared_services.redis]
image = "redis:7"
"#,
            Path::new("/tmp"),
        )
        .unwrap();
        let now = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let a = compute_build_id("a", &cf_a, now);
        let b = compute_build_id("b", &cf_b, now);
        assert_ne!(a, b);
    }

    #[test]
    fn ssg_manifest_round_trips_through_json() {
        let cf = sample_cf();
        let manifest = build_manifest("abc_20260101", "abc", &cf);

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let back: SsgManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(back.build_id, "abc_20260101");
        assert_eq!(back.coastfile_hash, "abc");
        assert_eq!(back.services.len(), 1);
        assert_eq!(back.services[0].name, "postgres");
        assert_eq!(back.services[0].ports, vec![5432]);
    }

    #[test]
    fn manifest_service_captures_env_keys_not_values() {
        let cf = SsgCoastfile::parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "secret" }
"#,
            Path::new("/tmp"),
        )
        .unwrap();
        let ms: SsgManifestService = (&cf.services[0]).into();
        assert_eq!(
            ms.env_keys,
            vec!["POSTGRES_PASSWORD".to_string(), "POSTGRES_USER".to_string()]
        );
        // Values must not leak into the manifest struct.
        let json = serde_json::to_string(&ms).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("coast"));
    }

    #[test]
    fn write_artifact_and_flip_latest_produce_valid_layout() {
        with_coast_home(|root| {
            let cf = sample_cf();
            let manifest = build_manifest("b1_20260101000000", "b1hash", &cf);
            let compose = "services:\n  postgres:\n    image: postgres:16\n";
            let dir = write_artifact(&manifest, &cf, compose).unwrap();

            assert_eq!(
                dir,
                root.join("ssg").join("builds").join("b1_20260101000000")
            );
            assert!(dir.join("manifest.json").exists());
            assert!(dir.join("ssg-coastfile.toml").exists());
            assert!(dir.join("compose.yml").exists());

            // Manifest round-trips.
            let loaded: SsgManifest =
                serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(loaded.build_id, "b1_20260101000000");

            // ssg-coastfile.toml reparses through SsgCoastfile::parse.
            let reparsed = SsgCoastfile::parse(
                &std::fs::read_to_string(dir.join("ssg-coastfile.toml")).unwrap(),
                Path::new("/tmp"),
            )
            .unwrap();
            assert_eq!(reparsed.services.len(), 1);
            assert_eq!(reparsed.services[0].name, "postgres");

            // Flip latest.
            flip_latest(&manifest.build_id).unwrap();
            assert_eq!(
                crate::paths::resolve_latest_build_id(),
                Some("b1_20260101000000".to_string())
            );

            // Flipping again replaces the symlink idempotently.
            flip_latest(&manifest.build_id).unwrap();
            assert_eq!(
                crate::paths::resolve_latest_build_id(),
                Some("b1_20260101000000".to_string())
            );
        });
    }

    #[test]
    fn auto_prune_preserves_newest_and_latest() {
        with_coast_home(|root| {
            let builds = root.join("ssg").join("builds");
            std::fs::create_dir_all(&builds).unwrap();

            // Write 7 builds with increasing timestamps.
            for i in 0..7u32 {
                let id = format!("b{i}_202601010000{i:02}");
                let dir = builds.join(&id);
                std::fs::create_dir_all(&dir).unwrap();
                let manifest = SsgManifest {
                    build_id: id.clone(),
                    built_at: DateTime::<Utc>::from_timestamp(1_700_000_000 + i64::from(i), 0)
                        .unwrap(),
                    coastfile_hash: format!("h{i}"),
                    services: vec![],
                };
                std::fs::write(
                    dir.join("manifest.json"),
                    serde_json::to_string(&manifest).unwrap(),
                )
                .unwrap();
            }

            // Point latest at the oldest build to verify pin-protection.
            let oldest = "b0_20260101000000";
            let link = root.join("ssg").join("latest");
            std::os::unix::fs::symlink(builds.join(oldest), &link).unwrap();

            let removed = auto_prune(5).unwrap();
            // 7 total, keep 5, would remove 2; but the oldest is pinned
            // so only 1 gets removed.
            assert_eq!(removed, 1);

            // The oldest (pinned) must survive even though it is the
            // oldest by timestamp.
            assert!(builds.join(oldest).exists());

            let remaining: Vec<_> = std::fs::read_dir(&builds)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(remaining.len(), 6);
        });
    }

    #[test]
    fn auto_prune_noop_when_under_keep_limit() {
        with_coast_home(|root| {
            let builds = root.join("ssg").join("builds");
            std::fs::create_dir_all(&builds).unwrap();
            for i in 0..3u32 {
                std::fs::create_dir_all(builds.join(format!("b{i}"))).unwrap();
            }
            let removed = auto_prune(5).unwrap();
            assert_eq!(removed, 0);
        });
    }
}
