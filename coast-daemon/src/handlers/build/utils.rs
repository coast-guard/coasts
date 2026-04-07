use std::path::{Path, PathBuf};

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};

pub(super) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Remove old builds from a project directory, keeping the most recent `keep` builds.
/// Builds whose IDs appear in `in_use` are never pruned regardless of the keep limit.
/// Pruning is partitioned by `(coastfile_type, arch)` so builds for different
/// architectures don't compete with each other.
pub(crate) fn auto_prune_builds(
    project_dir: &Path,
    keep: usize,
    in_use: &std::collections::HashSet<String>,
    coastfile_type: Option<&str>,
) {
    let Ok(entries) = std::fs::read_dir(project_dir) else {
        return;
    };

    let mut arch_groups: std::collections::HashMap<Option<String>, Vec<(String, String)>> =
        std::collections::HashMap::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("latest") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
        let Some(ref manifest) = manifest else {
            continue;
        };
        let build_type = manifest
            .get("coastfile_type")
            .and_then(|value| value.as_str())
            .map(std::string::ToString::to_string);
        if build_type.as_deref() != coastfile_type {
            continue;
        }
        let arch = manifest
            .get("arch")
            .and_then(|value| value.as_str())
            .map(std::string::ToString::to_string);
        let timestamp = manifest
            .get("build_timestamp")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        arch_groups.entry(arch).or_default().push((name, timestamp));
    }

    for builds in arch_groups.values_mut() {
        builds.sort_by(|a, b| b.1.cmp(&a.1));
        prune_sorted_builds(project_dir, builds, keep, in_use);
    }
}

fn prune_sorted_builds(
    project_dir: &Path,
    builds: &[(String, String)],
    keep: usize,
    in_use: &std::collections::HashSet<String>,
) {
    for (dirname, _) in builds.iter().skip(keep) {
        if in_use.contains(dirname) {
            info!(
                build_id = %dirname,
                "skipping prune of build — still in use by running instance(s)"
            );
            continue;
        }
        let path = project_dir.join(dirname);
        if let Err(error) = std::fs::remove_dir_all(&path) {
            warn!(path = %path.display(), error = %error, "failed to prune old build");
        } else {
            info!(build_id = %dirname, "pruned old build");
        }
    }
}

/// Parse a TTL duration string (e.g., "1h", "30m", "3600s", "3600") into seconds.
///
/// Supported suffixes: `s` (seconds), `m` (minutes), `h` (hours), `d` (days).
/// If no suffix is provided, the value is treated as seconds.
/// Returns `None` if the string cannot be parsed.
pub(super) fn parse_ttl_to_seconds(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix('s') {
        num.trim().parse::<i64>().ok()
    } else if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<i64>().ok().map(|n| n * 60)
    } else if let Some(num) = s.strip_suffix('h') {
        num.trim().parse::<i64>().ok().map(|n| n * 3600)
    } else if let Some(num) = s.strip_suffix('d') {
        num.trim().parse::<i64>().ok().map(|n| n * 86400)
    } else {
        s.parse::<i64>().ok()
    }
}

/// Pull a Docker image and save it as a tarball in the cache directory.
pub(super) async fn pull_and_cache_image(
    docker: &bollard::Docker,
    image: &str,
    cache_dir: &Path,
) -> Result<PathBuf> {
    use bollard::image::CreateImageOptions;
    use futures_util::StreamExt;

    let (name, tag) = if let Some(pos) = image.rfind(':') {
        (&image[..pos], &image[pos + 1..])
    } else {
        (image, "latest")
    };

    info!(image = %image, "pulling image for cache");

    let options = CreateImageOptions {
        from_image: name,
        tag,
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(options), None, None);
    while let Some(result) = stream.next().await {
        match result {
            Ok(info) => {
                if let Some(status) = info.status {
                    tracing::debug!(status = %status, "pull progress");
                }
            }
            Err(error) => {
                return Err(CoastError::docker(format!(
                    "failed to pull image '{}': {}",
                    image, error
                )));
            }
        }
    }

    let safe_name = image.replace(['/', ':'], "_");
    let tarball_path = cache_dir.join(format!("{safe_name}.tar"));

    let mut export_stream = docker.export_image(image);
    let mut tarball_data = Vec::new();
    while let Some(chunk) = export_stream.next().await {
        match chunk {
            Ok(bytes) => tarball_data.extend_from_slice(&bytes),
            Err(error) => {
                return Err(CoastError::docker(format!(
                    "failed to export image '{}': {}",
                    image, error
                )));
            }
        }
    }

    std::fs::write(&tarball_path, &tarball_data).map_err(|error| CoastError::Io {
        message: format!("failed to write image tarball: {error}"),
        path: tarball_path.clone(),
        source: Some(error),
    })?;

    info!(image = %image, path = %tarball_path.display(), "image cached");

    Ok(tarball_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, timestamp: &str, coastfile_type: Option<&str>) {
        write_manifest_with_arch(dir, timestamp, coastfile_type, None);
    }

    fn write_manifest_with_arch(
        dir: &Path,
        timestamp: &str,
        coastfile_type: Option<&str>,
        arch: Option<&str>,
    ) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::json!({
                "build_timestamp": timestamp,
                "coastfile_type": coastfile_type,
                "arch": arch,
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn test_shell_single_quote_escapes_embedded_quotes() {
        assert_eq!(shell_single_quote("abc"), "'abc'");
        assert_eq!(shell_single_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_single_quote(""), "''");
    }

    #[test]
    fn test_parse_ttl_to_seconds_supports_suffixes() {
        assert_eq!(parse_ttl_to_seconds("45"), Some(45));
        assert_eq!(parse_ttl_to_seconds("45s"), Some(45));
        assert_eq!(parse_ttl_to_seconds("2m"), Some(120));
        assert_eq!(parse_ttl_to_seconds("3h"), Some(10800));
        assert_eq!(parse_ttl_to_seconds("1d"), Some(86400));
        assert_eq!(parse_ttl_to_seconds(""), None);
        assert_eq!(parse_ttl_to_seconds("bad"), None);
    }

    #[test]
    fn test_auto_prune_builds_keeps_newest_and_preserves_in_use() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(&dir.path().join("build-old"), "2024-01-01T00:00:00Z", None);
        write_manifest(&dir.path().join("build-mid"), "2024-02-01T00:00:00Z", None);
        write_manifest(&dir.path().join("build-new"), "2024-03-01T00:00:00Z", None);

        let in_use: std::collections::HashSet<String> =
            ["build-old".to_string()].into_iter().collect();
        auto_prune_builds(dir.path(), 2, &in_use, None);

        assert!(dir.path().join("build-old").exists());
        assert!(dir.path().join("build-mid").exists());
        assert!(dir.path().join("build-new").exists());
    }

    #[test]
    fn test_auto_prune_builds_filters_by_coastfile_type() {
        let dir = tempfile::tempdir().unwrap();
        write_manifest(
            &dir.path().join("default-old"),
            "2024-01-01T00:00:00Z",
            None,
        );
        write_manifest(
            &dir.path().join("default-new"),
            "2024-03-01T00:00:00Z",
            None,
        );
        write_manifest(
            &dir.path().join("dev-old"),
            "2024-01-15T00:00:00Z",
            Some("dev"),
        );
        write_manifest(
            &dir.path().join("dev-new"),
            "2024-04-01T00:00:00Z",
            Some("dev"),
        );

        auto_prune_builds(
            dir.path(),
            1,
            &std::collections::HashSet::new(),
            Some("dev"),
        );

        assert!(dir.path().join("default-old").exists());
        assert!(dir.path().join("default-new").exists());
        assert!(!dir.path().join("dev-old").exists());
        assert!(dir.path().join("dev-new").exists());
    }

    #[test]
    fn test_auto_prune_builds_remote_type() {
        let dir = tempfile::tempdir().unwrap();
        for i in 1..=7 {
            write_manifest(
                &dir.path().join(format!("remote-build-{i:02}")),
                &format!("2024-0{i}-01T00:00:00Z"),
                Some("remote"),
            );
        }
        write_manifest(
            &dir.path().join("local-build-01"),
            "2024-01-15T00:00:00Z",
            None,
        );

        auto_prune_builds(
            dir.path(),
            5,
            &std::collections::HashSet::new(),
            Some("remote"),
        );

        assert!(
            !dir.path().join("remote-build-01").exists(),
            "oldest remote pruned"
        );
        assert!(
            !dir.path().join("remote-build-02").exists(),
            "2nd oldest remote pruned"
        );
        assert!(
            dir.path().join("remote-build-03").exists(),
            "3rd oldest kept"
        );
        assert!(dir.path().join("remote-build-07").exists(), "newest kept");
        assert!(
            dir.path().join("local-build-01").exists(),
            "local build untouched"
        );
    }

    #[test]
    fn test_auto_prune_builds_partitions_by_arch() {
        let dir = tempfile::tempdir().unwrap();

        for i in 1..=7 {
            write_manifest_with_arch(
                &dir.path().join(format!("x86-build-{i:02}")),
                &format!("2024-0{i}-01T00:00:00Z"),
                Some("remote"),
                Some("x86_64"),
            );
        }
        for i in 1..=3 {
            write_manifest_with_arch(
                &dir.path().join(format!("arm-build-{i:02}")),
                &format!("2024-0{i}-01T00:00:00Z"),
                Some("remote"),
                Some("aarch64"),
            );
        }

        auto_prune_builds(
            dir.path(),
            5,
            &std::collections::HashSet::new(),
            Some("remote"),
        );

        assert!(
            !dir.path().join("x86-build-01").exists(),
            "oldest x86 pruned"
        );
        assert!(
            !dir.path().join("x86-build-02").exists(),
            "2nd oldest x86 pruned"
        );
        assert!(
            dir.path().join("x86-build-03").exists(),
            "3rd oldest x86 kept (within keep=5)"
        );
        assert!(dir.path().join("x86-build-07").exists(), "newest x86 kept");

        assert!(
            dir.path().join("arm-build-01").exists(),
            "oldest aarch64 kept (only 3 total, under keep=5)"
        );
        assert!(dir.path().join("arm-build-02").exists(), "2nd aarch64 kept");
        assert!(
            dir.path().join("arm-build-03").exists(),
            "newest aarch64 kept"
        );
    }
}
