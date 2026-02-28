/// Image loading for the Coast daemon.
///
/// Handles loading cached OCI image tarballs into the inner Docker daemon
/// running inside a coast container. After a coast container starts and the
/// inner daemon is ready, this module provides the commands needed to load
/// pre-pulled images from the shared cache.
///
/// The shared image cache at `~/.coast/image-cache/` is bind-mounted
/// read-only into each coast container, allowing cross-instance sharing
/// of cached images without re-pulling.
use std::path::Path;

use tracing::debug;

use coast_core::error::{CoastError, Result};

/// Default mount path for the image cache inside a coast container.
pub const IMAGE_CACHE_CONTAINER_PATH: &str = "/image-cache";

/// Generate the `docker load` command for a single OCI tarball.
///
/// # Arguments
///
/// * `tarball_path` - Absolute path to the tarball inside the container
///   (typically under `/image-cache/`).
///
/// # Returns
///
/// A command vector: `["docker", "load", "-i", tarball_path]`
pub fn load_command(tarball_path: &str) -> Vec<String> {
    vec![
        "docker".to_string(),
        "load".to_string(),
        "-i".to_string(),
        tarball_path.to_string(),
    ]
}

/// Generate the bind mount pair for the image cache directory.
///
/// Returns a tuple of `(host_path, container_path)` suitable for creating
/// a read-only bind mount that gives the coast container access to the
/// shared image cache.
///
/// # Arguments
///
/// * `cache_dir` - Absolute path to the image cache on the host
///   (typically `~/.coast/image-cache/`).
///
/// # Returns
///
/// A tuple `(host_path_string, container_mount_path)`.
pub fn image_cache_mount(cache_dir: &Path) -> (String, String) {
    (
        cache_dir.to_string_lossy().to_string(),
        IMAGE_CACHE_CONTAINER_PATH.to_string(),
    )
}

/// Generate the command to check inner daemon readiness.
///
/// Returns the `docker info` command that is used to poll whether the
/// inner Docker daemon inside a coast container is ready to accept
/// commands.
///
/// # Returns
///
/// A command vector: `["docker", "info"]`
pub fn wait_for_inner_daemon_command() -> Vec<String> {
    vec!["docker".to_string(), "info".to_string()]
}

/// Generate load commands for all cached image tarballs.
///
/// For each tarball name, produces a `docker load -i` command using the
/// given mount path prefix.
///
/// # Arguments
///
/// * `tarball_names` - List of tarball filenames (e.g., `["postgres_16_abc123.tar", "node_20_def456.tar"]`).
/// * `cache_mount_path` - The path where the cache is mounted inside the container
///   (typically `/image-cache`).
///
/// # Returns
///
/// A vector of command vectors, one per tarball.
pub fn load_all_images_commands(
    tarball_names: &[String],
    cache_mount_path: &str,
) -> Vec<Vec<String>> {
    tarball_names
        .iter()
        .map(|name| {
            let full_path = format!("{cache_mount_path}/{name}");
            debug!(tarball = %name, path = %full_path, "Generating load command");
            load_command(&full_path)
        })
        .collect()
}

/// Validate that a cache directory exists and is readable.
///
/// # Arguments
///
/// * `cache_dir` - Path to the image cache directory.
///
/// # Errors
///
/// Returns an error if the directory does not exist or is not readable.
pub fn validate_cache_dir(cache_dir: &Path) -> Result<()> {
    if !cache_dir.exists() {
        return Err(CoastError::io(
            format!(
                "Image cache directory '{}' does not exist. \
                 Run `coast build` to create it and cache images.",
                cache_dir.display()
            ),
            cache_dir,
        ));
    }

    if !cache_dir.is_dir() {
        return Err(CoastError::io(
            format!(
                "Image cache path '{}' exists but is not a directory.",
                cache_dir.display()
            ),
            cache_dir,
        ));
    }

    Ok(())
}

/// Generate a Docker bind mount string for the image cache.
///
/// Produces a string in the format `host_path:container_path:ro` suitable
/// for passing to Docker's `-v` flag or the bind mount configuration.
///
/// # Arguments
///
/// * `cache_dir` - Absolute path to the image cache on the host.
pub fn image_cache_bind_mount_string(cache_dir: &Path) -> String {
    format!("{}:{}:ro", cache_dir.display(), IMAGE_CACHE_CONTAINER_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -----------------------------------------------------------
    // load_command tests
    // -----------------------------------------------------------

    #[test]
    fn test_load_command_basic() {
        let cmd = load_command("/image-cache/postgres_16_abc123.tar");
        assert_eq!(cmd.len(), 4);
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "load");
        assert_eq!(cmd[2], "-i");
        assert_eq!(cmd[3], "/image-cache/postgres_16_abc123.tar");
    }

    #[test]
    fn test_load_command_different_path() {
        let cmd = load_command("/mnt/cache/node_20_def456.tar");
        assert_eq!(cmd[3], "/mnt/cache/node_20_def456.tar");
    }

    #[test]
    fn test_load_command_with_spaces_in_path() {
        let cmd = load_command("/image cache/my image.tar");
        assert_eq!(cmd[3], "/image cache/my image.tar");
    }

    #[test]
    fn test_load_command_empty_path() {
        let cmd = load_command("");
        assert_eq!(cmd[3], "");
    }

    // -----------------------------------------------------------
    // image_cache_mount tests
    // -----------------------------------------------------------

    #[test]
    fn test_image_cache_mount_basic() {
        let cache_dir = Path::new("/home/user/.coast/image-cache");
        let (host, container) = image_cache_mount(cache_dir);
        assert_eq!(host, "/home/user/.coast/image-cache");
        assert_eq!(container, "/image-cache");
    }

    #[test]
    fn test_image_cache_mount_different_path() {
        let cache_dir = Path::new("/var/coast/cache");
        let (host, container) = image_cache_mount(cache_dir);
        assert_eq!(host, "/var/coast/cache");
        assert_eq!(container, IMAGE_CACHE_CONTAINER_PATH);
    }

    #[test]
    fn test_image_cache_mount_container_path_is_constant() {
        let cache_dir = Path::new("/any/path");
        let (_, container) = image_cache_mount(cache_dir);
        assert_eq!(container, "/image-cache");
    }

    #[test]
    fn test_image_cache_mount_with_trailing_slash() {
        let cache_dir = Path::new("/home/user/.coast/image-cache/");
        let (host, _) = image_cache_mount(cache_dir);
        // PathBuf normalizes trailing slashes
        assert!(host.starts_with("/home/user/.coast/image-cache"));
    }

    // -----------------------------------------------------------
    // wait_for_inner_daemon_command tests
    // -----------------------------------------------------------

    #[test]
    fn test_wait_for_inner_daemon_command() {
        let cmd = wait_for_inner_daemon_command();
        assert_eq!(cmd.len(), 2);
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "info");
    }

    #[test]
    fn test_wait_for_inner_daemon_command_is_consistent() {
        let cmd1 = wait_for_inner_daemon_command();
        let cmd2 = wait_for_inner_daemon_command();
        assert_eq!(cmd1, cmd2);
    }

    // -----------------------------------------------------------
    // load_all_images_commands tests
    // -----------------------------------------------------------

    #[test]
    fn test_load_all_images_commands_single() {
        let tarballs = vec!["postgres_16_abc123.tar".to_string()];
        let commands = load_all_images_commands(&tarballs, "/image-cache");

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0][0], "docker");
        assert_eq!(commands[0][1], "load");
        assert_eq!(commands[0][2], "-i");
        assert_eq!(commands[0][3], "/image-cache/postgres_16_abc123.tar");
    }

    #[test]
    fn test_load_all_images_commands_multiple() {
        let tarballs = vec![
            "postgres_16_abc123.tar".to_string(),
            "node_20_def456.tar".to_string(),
            "redis_7_ghi789.tar".to_string(),
        ];
        let commands = load_all_images_commands(&tarballs, "/image-cache");

        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0][3], "/image-cache/postgres_16_abc123.tar");
        assert_eq!(commands[1][3], "/image-cache/node_20_def456.tar");
        assert_eq!(commands[2][3], "/image-cache/redis_7_ghi789.tar");
    }

    #[test]
    fn test_load_all_images_commands_empty() {
        let tarballs: Vec<String> = vec![];
        let commands = load_all_images_commands(&tarballs, "/image-cache");
        assert!(commands.is_empty());
    }

    #[test]
    fn test_load_all_images_commands_custom_mount_path() {
        let tarballs = vec!["image.tar".to_string()];
        let commands = load_all_images_commands(&tarballs, "/custom/cache");

        assert_eq!(commands[0][3], "/custom/cache/image.tar");
    }

    #[test]
    fn test_load_all_images_commands_preserves_order() {
        let tarballs = vec![
            "c.tar".to_string(),
            "a.tar".to_string(),
            "b.tar".to_string(),
        ];
        let commands = load_all_images_commands(&tarballs, "/cache");

        assert_eq!(commands[0][3], "/cache/c.tar");
        assert_eq!(commands[1][3], "/cache/a.tar");
        assert_eq!(commands[2][3], "/cache/b.tar");
    }

    #[test]
    fn test_load_all_images_commands_each_is_valid_load() {
        let tarballs = vec!["img1.tar".to_string(), "img2.tar".to_string()];
        let commands = load_all_images_commands(&tarballs, "/image-cache");

        for cmd in &commands {
            assert_eq!(cmd.len(), 4);
            assert_eq!(cmd[0], "docker");
            assert_eq!(cmd[1], "load");
            assert_eq!(cmd[2], "-i");
            assert!(cmd[3].starts_with("/image-cache/"));
        }
    }

    // -----------------------------------------------------------
    // validate_cache_dir tests
    // -----------------------------------------------------------

    #[test]
    fn test_validate_cache_dir_exists() {
        let dir = TempDir::new().unwrap();
        let result = validate_cache_dir(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_cache_dir_not_exists() {
        let result = validate_cache_dir(Path::new("/nonexistent/path/image-cache"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
        assert!(err.contains("coast build"));
    }

    #[test]
    fn test_validate_cache_dir_is_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("not-a-dir");
        std::fs::write(&file_path, "data").unwrap();

        let result = validate_cache_dir(&file_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a directory"));
    }

    // -----------------------------------------------------------
    // image_cache_bind_mount_string tests
    // -----------------------------------------------------------

    #[test]
    fn test_image_cache_bind_mount_string_basic() {
        let cache_dir = Path::new("/home/user/.coast/image-cache");
        let mount_str = image_cache_bind_mount_string(cache_dir);
        assert_eq!(mount_str, "/home/user/.coast/image-cache:/image-cache:ro");
    }

    #[test]
    fn test_image_cache_bind_mount_string_is_read_only() {
        let cache_dir = Path::new("/any/path");
        let mount_str = image_cache_bind_mount_string(cache_dir);
        assert!(mount_str.ends_with(":ro"));
    }

    #[test]
    fn test_image_cache_bind_mount_string_contains_container_path() {
        let cache_dir = Path::new("/cache");
        let mount_str = image_cache_bind_mount_string(cache_dir);
        assert!(mount_str.contains(IMAGE_CACHE_CONTAINER_PATH));
    }

    // -----------------------------------------------------------
    // Constant tests
    // -----------------------------------------------------------

    #[test]
    fn test_image_cache_container_path_constant() {
        assert_eq!(IMAGE_CACHE_CONTAINER_PATH, "/image-cache");
    }
}
