/// Self-update logic: download tarball, extract, and atomically replace binaries.
use crate::error::UpdateError;
use semver::Version;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Detect if the current binary was installed via Homebrew.
///
/// Checks if the executable path contains `/Cellar/` or `/opt/homebrew/`.
pub fn is_homebrew_install() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let path_str = exe.to_string_lossy();
    path_str.contains("/Cellar/") || path_str.contains("/opt/homebrew/")
}

/// Find the `coastd` binary path, using the same resolution logic as the CLI.
///
/// Looks for `coastd` next to the current `coast` executable first,
/// then falls back to `coastd` on PATH.
pub fn resolve_coastd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("coastd");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("coastd")
}

/// Detect the current platform and return (os, arch) strings matching
/// the release tarball naming convention.
pub fn current_platform() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        "unknown"
    };

    (os, arch)
}

/// Download a release tarball to a temporary file and return its path.
pub async fn download_release(
    version: &Version,
    timeout: Duration,
) -> Result<PathBuf, UpdateError> {
    if is_homebrew_install() {
        return Err(UpdateError::HomebrewInstall);
    }

    let (os, arch) = current_platform();
    let url = crate::checker::release_tarball_url(version, os, arch);

    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent("coast-cli")
        .build()
        .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(UpdateError::DownloadFailed(format!(
            "HTTP {} from {url}",
            resp.status()
        )));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;

    let tmp_dir = std::env::temp_dir().join("coast-update");
    std::fs::create_dir_all(&tmp_dir)?;
    let tarball_path = tmp_dir.join(format!("coast-v{version}-{os}-{arch}.tar.gz"));
    std::fs::write(&tarball_path, &bytes)?;

    Ok(tarball_path)
}

/// Extract the tarball and atomically replace the `coast` and `coastd` binaries.
///
/// The replacement strategy:
/// 1. Extract to a temp directory
/// 2. For each binary, rename the old one to `.old`, move the new one in place
/// 3. Remove the `.old` files
///
/// This is as close to atomic as we can get on most filesystems.
pub fn apply_update(tarball_path: &Path) -> Result<(), UpdateError> {
    let coast_path = std::env::current_exe()
        .map_err(|e| UpdateError::ApplyFailed(format!("Cannot determine current exe: {e}")))?;
    let coastd_path = resolve_coastd_path();

    let extract_dir = tarball_path
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("extracted");
    std::fs::create_dir_all(&extract_dir)?;

    // Extract using tar (available on macOS and Linux)
    let status = std::process::Command::new("tar")
        .args(["xzf", &tarball_path.to_string_lossy(), "-C"])
        .arg(&extract_dir)
        .status()
        .map_err(|e| UpdateError::ApplyFailed(format!("Failed to run tar: {e}")))?;

    if !status.success() {
        return Err(UpdateError::ApplyFailed(
            "tar extraction failed".to_string(),
        ));
    }

    // Find extracted binaries
    let new_coast = extract_dir.join("coast");
    let new_coastd = extract_dir.join("coastd");

    if !new_coast.exists() {
        return Err(UpdateError::ApplyFailed(
            "Tarball does not contain 'coast' binary".to_string(),
        ));
    }
    if !new_coastd.exists() {
        return Err(UpdateError::ApplyFailed(
            "Tarball does not contain 'coastd' binary".to_string(),
        ));
    }

    // Replace coast binary
    replace_binary(&new_coast, &coast_path)?;

    // Replace coastd binary (only if it's a real path, not just "coastd" from PATH)
    if coastd_path.is_absolute() || coastd_path.exists() {
        replace_binary(&new_coastd, &coastd_path)?;
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(tarball_path);

    Ok(())
}

/// Replace a single binary atomically using rename.
fn replace_binary(new_path: &Path, target_path: &Path) -> Result<(), UpdateError> {
    let backup = target_path.with_extension("old");

    // Move current binary out of the way
    if target_path.exists() {
        std::fs::rename(target_path, &backup).map_err(|e| {
            UpdateError::ApplyFailed(format!("Failed to backup {}: {e}", target_path.display()))
        })?;
    }

    // Move new binary into place
    if let Err(e) = std::fs::rename(new_path, target_path) {
        // Try to restore backup
        if backup.exists() {
            let _ = std::fs::rename(&backup, target_path);
        }
        return Err(UpdateError::ApplyFailed(format!(
            "Failed to install new binary at {}: {e}",
            target_path.display()
        )));
    }

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target_path, std::fs::Permissions::from_mode(0o755));
    }

    // Remove backup
    let _ = std::fs::remove_file(&backup);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_platform_valid() {
        let (os, arch) = current_platform();
        assert!(
            os == "darwin" || os == "linux" || os == "unknown",
            "unexpected os: {os}"
        );
        assert!(
            arch == "arm64" || arch == "amd64" || arch == "unknown",
            "unexpected arch: {arch}"
        );
    }

    #[test]
    fn test_resolve_coastd_path_returns_something() {
        let path = resolve_coastd_path();
        // Should return either an absolute path or "coastd"
        assert!(
            path.is_absolute() || path == PathBuf::from("coastd"),
            "unexpected coastd path: {}",
            path.display()
        );
    }

    #[test]
    fn test_replace_binary_with_temp_files() {
        let dir = tempfile::tempdir().unwrap();

        let old_binary = dir.path().join("coast");
        std::fs::write(&old_binary, b"old-binary").unwrap();

        let new_binary = dir.path().join("coast-new");
        std::fs::write(&new_binary, b"new-binary").unwrap();

        replace_binary(&new_binary, &old_binary).unwrap();

        let content = std::fs::read_to_string(&old_binary).unwrap();
        assert_eq!(content, "new-binary");
        assert!(!new_binary.exists(), "source file should be renamed away");
    }

    #[test]
    fn test_replace_binary_no_existing_target() {
        let dir = tempfile::tempdir().unwrap();

        let target = dir.path().join("coast");
        let new_binary = dir.path().join("coast-new");
        std::fs::write(&new_binary, b"fresh-binary").unwrap();

        replace_binary(&new_binary, &target).unwrap();

        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, "fresh-binary");
    }

    #[test]
    fn test_replace_binary_restore_on_failure() {
        let dir = tempfile::tempdir().unwrap();

        let target = dir.path().join("coast");
        std::fs::write(&target, b"original").unwrap();

        // new_binary doesn't exist — rename will fail
        let new_binary = dir.path().join("nonexistent");

        let result = replace_binary(&new_binary, &target);
        assert!(result.is_err());

        // Original should be restored from backup
        let content = std::fs::read_to_string(&target).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn test_apply_update_missing_tarball() {
        let result = apply_update(Path::new("/nonexistent/coast.tar.gz"));
        assert!(result.is_err());
    }

    #[test]
    fn test_is_homebrew_install_not_homebrew() {
        // In dev/test environments, the binary won't be under /Cellar/ or /opt/homebrew/
        // This is a best-effort test — we just verify it returns a bool without panicking
        let _result = is_homebrew_install();
    }
}
