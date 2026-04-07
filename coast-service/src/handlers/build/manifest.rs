use std::path::Path;

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use tracing::{info, warn};

const MAX_BUILDS_KEPT: usize = 5;

pub fn write_manifest(
    coastfile: &Coastfile,
    build_id: &str,
    artifact_path: &Path,
    images_cached: usize,
    images_built: usize,
) -> Result<()> {
    let manifest = serde_json::json!({
        "build_id": build_id,
        "project": coastfile.name,
        "coastfile_type": coastfile.coastfile_type,
        "arch": std::env::consts::ARCH,
        "build_timestamp": chrono::Utc::now().to_rfc3339(),
        "images_cached": images_cached,
        "images_built": images_built,
    });

    let manifest_path = artifact_path.join("manifest.json");
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| CoastError::state(format!("failed to serialize manifest: {e}")))?;
    std::fs::write(&manifest_path, &content).map_err(|e| CoastError::Io {
        message: format!("failed to write manifest.json: {e}"),
        path: manifest_path.clone(),
        source: Some(e),
    })?;
    info!(path = %manifest_path.display(), "wrote manifest.json");

    let project_dir = artifact_path
        .parent()
        .ok_or_else(|| CoastError::state("artifact path has no parent"))?;

    create_latest_symlink(project_dir, build_id, coastfile.coastfile_type.as_deref())?;
    prune_old_builds(project_dir)?;

    Ok(())
}

fn create_latest_symlink(
    project_dir: &Path,
    build_id: &str,
    coastfile_type: Option<&str>,
) -> Result<()> {
    let link_name = match coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let link_path = project_dir.join(&link_name);

    if link_path.exists() || link_path.is_symlink() {
        std::fs::remove_file(&link_path).map_err(|e| CoastError::Io {
            message: format!("failed to remove old symlink: {e}"),
            path: link_path.clone(),
            source: Some(e),
        })?;
    }

    std::os::unix::fs::symlink(build_id, &link_path).map_err(|e| CoastError::Io {
        message: format!("failed to create latest symlink: {e}"),
        path: link_path.clone(),
        source: Some(e),
    })?;
    info!(link = %link_name, target = build_id, "created latest symlink");

    Ok(())
}

fn prune_old_builds(project_dir: &Path) -> Result<()> {
    let mut builds: Vec<(String, String)> = Vec::new();

    let Ok(entries) = std::fs::read_dir(project_dir) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with("latest") || entry.path().is_symlink() {
            continue;
        }

        if !entry.path().is_dir() {
            continue;
        }

        let manifest_path = entry.path().join("manifest.json");
        let timestamp = match std::fs::read_to_string(&manifest_path) {
            Ok(content) => {
                let parsed: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                parsed["build_timestamp"].as_str().unwrap_or("").to_string()
            }
            Err(_) => continue,
        };

        builds.push((name, timestamp));
    }

    builds.sort_by(|a, b| b.1.cmp(&a.1));

    if builds.len() <= MAX_BUILDS_KEPT {
        return Ok(());
    }

    for (name, _) in &builds[MAX_BUILDS_KEPT..] {
        let path = project_dir.join(name);
        if let Err(e) = std::fs::remove_dir_all(&path) {
            warn!(path = %path.display(), error = %e, "failed to prune old build");
        } else {
            info!(build = %name, "pruned old build");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::coastfile::Coastfile;

    fn minimal_coastfile(dir: &Path) -> Coastfile {
        let toml = "[coast]\nname = \"test-project\"\n";
        Coastfile::parse(toml, dir).unwrap()
    }

    #[test]
    fn test_manifest_json_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let artifact_path = tmp.path().join("images/test-project/build_001");
        std::fs::create_dir_all(&artifact_path).unwrap();

        let cf = minimal_coastfile(tmp.path());
        write_manifest(&cf, "build_001", &artifact_path, 3, 1).unwrap();

        let content = std::fs::read_to_string(artifact_path.join("manifest.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["build_id"], "build_001");
        assert_eq!(parsed["project"], "test-project");
        assert_eq!(parsed["images_cached"], 3);
        assert_eq!(parsed["images_built"], 1);
        assert!(parsed["build_timestamp"].as_str().is_some());
    }

    #[test]
    fn test_latest_symlink_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("images/test-project");
        let artifact_path = project_dir.join("build_002");
        std::fs::create_dir_all(&artifact_path).unwrap();

        let cf = minimal_coastfile(tmp.path());
        write_manifest(&cf, "build_002", &artifact_path, 0, 0).unwrap();

        let link = project_dir.join("latest");
        assert!(link.is_symlink());
        assert_eq!(
            std::fs::read_link(&link).unwrap().to_str().unwrap(),
            "build_002"
        );
    }

    #[test]
    fn test_latest_symlink_with_type() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("images/test-project");
        let artifact_path = project_dir.join("build_003");
        std::fs::create_dir_all(&artifact_path).unwrap();

        let toml = "[coast]\nname = \"test-project\"\n";
        let mut cf = Coastfile::parse(toml, tmp.path()).unwrap();
        cf.coastfile_type = Some("light".to_string());

        write_manifest(&cf, "build_003", &artifact_path, 0, 0).unwrap();

        let link = project_dir.join("latest-light");
        assert!(link.is_symlink());
    }

    #[test]
    fn test_prune_keeps_latest_five() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("images/test-project");

        for i in 0..7 {
            let build_id = format!("build_{i:03}");
            let build_dir = project_dir.join(&build_id);
            std::fs::create_dir_all(&build_dir).unwrap();

            let timestamp = format!("2026-01-{:02}T00:00:00Z", i + 1);
            let manifest = serde_json::json!({
                "build_id": build_id,
                "project": "test-project",
                "build_timestamp": timestamp,
                "images_cached": 0,
                "images_built": 0,
            });
            std::fs::write(
                build_dir.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
        }

        prune_old_builds(&project_dir).unwrap();

        let remaining: Vec<_> = std::fs::read_dir(&project_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();

        assert_eq!(remaining.len(), 5);
        assert!(!project_dir.join("build_000").exists());
        assert!(!project_dir.join("build_001").exists());
        assert!(project_dir.join("build_006").exists());
    }
}
