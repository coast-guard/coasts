/// OCI image tarball management for coast containers.
///
/// Handles pulling Docker images, saving them as OCI tarballs for caching,
/// and loading them into the inner Docker daemon inside coast containers.
/// Uses content-addressable naming for cross-instance sharing.
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::{debug, info};

use coast_core::error::{CoastError, Result};

/// Default image cache directory relative to the coast home.
pub const IMAGE_CACHE_DIR: &str = "image-cache";

/// Get the default image cache directory path.
///
/// Returns `~/.coast/image-cache/`.
pub fn default_cache_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| CoastError::docker("Could not determine home directory for image cache"))?;
    Ok(home.join(".coast").join(IMAGE_CACHE_DIR))
}

/// Generate a content-addressable tarball filename for an image.
///
/// Format: `{image_name}_{tag}_{digest_short}.tar`
///
/// The image reference is parsed to extract name and tag, and a short
/// hash of the full reference is appended for uniqueness.
pub fn tarball_filename(image_ref: &str) -> String {
    let (name, tag) = parse_image_ref(image_ref);

    // Sanitize the name for filesystem use (replace / and : with _)
    let safe_name = name.replace(['/', ':'], "_");
    let safe_tag = tag.replace(['/', ':'], "_");

    // Generate a short digest of the full reference for uniqueness
    let digest = short_digest(image_ref);

    format!("{safe_name}_{safe_tag}_{digest}.tar")
}

/// Parse an image reference into (name, tag).
///
/// Handles formats like:
/// - `postgres:16` -> ("postgres", "16")
/// - `postgres` -> ("postgres", "latest")
/// - `docker.io/library/postgres:16` -> ("docker.io/library/postgres", "16")
/// - `myregistry.com/myapp:v1.2.3` -> ("myregistry.com/myapp", "v1.2.3")
pub fn parse_image_ref(image_ref: &str) -> (String, String) {
    // Handle digest references (image@sha256:...)
    if let Some(at_pos) = image_ref.rfind('@') {
        let name = &image_ref[..at_pos];
        let digest = &image_ref[at_pos + 1..];
        return (name.to_string(), digest.to_string());
    }

    // Split on the last colon, but only if it's after any slashes
    // (to handle registry:port/image cases)
    match image_ref.rfind(':') {
        Some(colon_pos) => {
            let after_colon = &image_ref[colon_pos + 1..];
            // If after the colon contains a slash, it's likely a port, not a tag
            if after_colon.contains('/') {
                (image_ref.to_string(), "latest".to_string())
            } else {
                let name = &image_ref[..colon_pos];
                let tag = after_colon;
                (name.to_string(), tag.to_string())
            }
        }
        None => (image_ref.to_string(), "latest".to_string()),
    }
}

/// Generate a short hex digest of a string (first 12 chars of SHA-256).
pub fn short_digest(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..6]) // 6 bytes = 12 hex chars
}

/// Get the full path to a tarball in the cache directory.
pub fn tarball_path(cache_dir: &Path, image_ref: &str) -> PathBuf {
    cache_dir.join(tarball_filename(image_ref))
}

/// Check if a tarball exists in the cache for the given image.
pub fn cache_hit(cache_dir: &Path, image_ref: &str) -> bool {
    tarball_path(cache_dir, image_ref).exists()
}

/// Build the `docker save` command for saving an image to a tarball.
///
/// Returns the command arguments as a vector of strings.
pub fn docker_save_cmd(image_ref: &str, output_path: &Path) -> Vec<String> {
    vec![
        "docker".to_string(),
        "save".to_string(),
        "-o".to_string(),
        output_path.display().to_string(),
        image_ref.to_string(),
    ]
}

/// Build the `docker load` command for loading a tarball into a daemon.
///
/// Returns the command arguments as a vector of strings.
/// This command is meant to be executed inside a coast container via exec.
pub fn docker_load_cmd(tarball_path: &str) -> Vec<String> {
    vec![
        "docker".to_string(),
        "load".to_string(),
        "-i".to_string(),
        tarball_path.to_string(),
    ]
}

/// Build the `podman load` command for loading a tarball into Podman.
pub fn podman_load_cmd(tarball_path: &str) -> Vec<String> {
    vec![
        "podman".to_string(),
        "load".to_string(),
        "-i".to_string(),
        tarball_path.to_string(),
    ]
}

/// Build the inner-daemon load command path for a tarball.
///
/// When the image cache is mounted at `/image-cache` inside the coast
/// container, this generates the path to a specific tarball.
pub fn inner_cache_path(image_ref: &str) -> String {
    format!("/image-cache/{}", tarball_filename(image_ref))
}

/// Manager for the OCI image tarball cache.
///
/// Handles saving images from the host Docker daemon into tarballs and
/// loading tarballs into coast containers' inner daemons.
pub struct ImageCache {
    /// Path to the cache directory.
    cache_dir: PathBuf,
}

impl ImageCache {
    /// Create a new image cache at the given directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Create a new image cache at the default location.
    pub fn default_location() -> Result<Self> {
        let cache_dir = default_cache_dir()?;
        Ok(Self { cache_dir })
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Check if an image is cached.
    pub fn is_cached(&self, image_ref: &str) -> bool {
        cache_hit(&self.cache_dir, image_ref)
    }

    /// Get the tarball path for an image.
    pub fn get_tarball_path(&self, image_ref: &str) -> PathBuf {
        tarball_path(&self.cache_dir, image_ref)
    }

    /// Get the path where this tarball will be available inside a coast container.
    pub fn inner_path(&self, image_ref: &str) -> String {
        inner_cache_path(image_ref)
    }

    /// Ensure the cache directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| CoastError::Io {
            message: format!("Failed to create image cache directory: {e}"),
            path: self.cache_dir.clone(),
            source: Some(e),
        })?;
        Ok(())
    }

    /// List all cached tarballs.
    pub fn list_cached(&self) -> Result<Vec<PathBuf>> {
        if !self.cache_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.cache_dir).map_err(|e| CoastError::Io {
            message: format!("Failed to read image cache directory: {e}"),
            path: self.cache_dir.clone(),
            source: Some(e),
        })?;

        let mut tarballs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| CoastError::Io {
                message: format!("Failed to read cache directory entry: {e}"),
                path: self.cache_dir.clone(),
                source: Some(e),
            })?;

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("tar") {
                tarballs.push(path);
            }
        }

        Ok(tarballs)
    }

    /// Get the `docker save` command for saving an image to the cache.
    pub fn save_command(&self, image_ref: &str) -> Vec<String> {
        let path = self.get_tarball_path(image_ref);
        docker_save_cmd(image_ref, &path)
    }

    /// Get the `docker load` command for loading a cached image
    /// inside a coast container.
    pub fn load_command(&self, image_ref: &str) -> Vec<String> {
        let path = self.inner_path(image_ref);
        docker_load_cmd(&path)
    }

    /// Get the `podman load` command for loading a cached image.
    pub fn podman_load_command(&self, image_ref: &str) -> Vec<String> {
        let path = self.inner_path(image_ref);
        podman_load_cmd(&path)
    }

    /// Build the load commands for all cached images.
    ///
    /// Returns a list of (image_ref_approx, load_cmd) pairs.
    /// Note: we cannot reconstruct the exact image ref from the filename,
    /// so these commands use the tarball path directly.
    pub fn load_all_commands(&self, use_podman: bool) -> Result<Vec<Vec<String>>> {
        let tarballs = self.list_cached()?;
        let mut commands = Vec::new();

        for tarball in &tarballs {
            let filename = tarball.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let inner_path = format!("/image-cache/{filename}");

            if use_podman {
                commands.push(podman_load_cmd(&inner_path));
            } else {
                commands.push(docker_load_cmd(&inner_path));
            }

            debug!(tarball = %tarball.display(), "Queued image for loading");
        }

        info!(count = commands.len(), "Prepared image load commands");
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_image_ref_with_tag() {
        let (name, tag) = parse_image_ref("postgres:16");
        assert_eq!(name, "postgres");
        assert_eq!(tag, "16");
    }

    #[test]
    fn test_parse_image_ref_no_tag() {
        let (name, tag) = parse_image_ref("postgres");
        assert_eq!(name, "postgres");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_with_registry() {
        let (name, tag) = parse_image_ref("docker.io/library/postgres:16");
        assert_eq!(name, "docker.io/library/postgres");
        assert_eq!(tag, "16");
    }

    #[test]
    fn test_parse_image_ref_with_registry_no_tag() {
        let (name, tag) = parse_image_ref("docker.io/library/postgres");
        assert_eq!(name, "docker.io/library/postgres");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_with_port() {
        let (name, tag) = parse_image_ref("myregistry.com:5000/myapp:v1");
        assert_eq!(name, "myregistry.com:5000/myapp");
        assert_eq!(tag, "v1");
    }

    #[test]
    fn test_parse_image_ref_with_port_no_tag() {
        let (name, tag) = parse_image_ref("myregistry.com:5000/myapp");
        assert_eq!(name, "myregistry.com:5000/myapp");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_with_digest() {
        let (name, tag) = parse_image_ref("postgres@sha256:abc123");
        assert_eq!(name, "postgres");
        assert_eq!(tag, "sha256:abc123");
    }

    #[test]
    fn test_tarball_filename_simple() {
        let filename = tarball_filename("postgres:16");
        assert!(filename.starts_with("postgres_16_"));
        assert!(filename.ends_with(".tar"));
    }

    #[test]
    fn test_tarball_filename_no_tag() {
        let filename = tarball_filename("redis");
        assert!(filename.starts_with("redis_latest_"));
        assert!(filename.ends_with(".tar"));
    }

    #[test]
    fn test_tarball_filename_with_registry() {
        let filename = tarball_filename("docker.io/library/node:20");
        assert!(filename.starts_with("docker.io_library_node_20_"));
        assert!(filename.ends_with(".tar"));
        // No slashes in filename
        assert!(!filename.contains('/'));
    }

    #[test]
    fn test_tarball_filename_deterministic() {
        let f1 = tarball_filename("postgres:16");
        let f2 = tarball_filename("postgres:16");
        assert_eq!(f1, f2);
    }

    #[test]
    fn test_tarball_filename_different_for_different_images() {
        let f1 = tarball_filename("postgres:16");
        let f2 = tarball_filename("postgres:15");
        assert_ne!(f1, f2);
    }

    #[test]
    fn test_short_digest() {
        let d1 = short_digest("postgres:16");
        assert_eq!(d1.len(), 12);
        // Should be deterministic
        let d2 = short_digest("postgres:16");
        assert_eq!(d1, d2);
    }

    #[test]
    fn test_short_digest_different_inputs() {
        let d1 = short_digest("postgres:16");
        let d2 = short_digest("postgres:15");
        assert_ne!(d1, d2);
    }

    #[test]
    fn test_tarball_path() {
        let cache = Path::new("/home/user/.coast/image-cache");
        let path = tarball_path(cache, "postgres:16");
        assert!(path.starts_with("/home/user/.coast/image-cache"));
        assert!(path.to_string_lossy().ends_with(".tar"));
    }

    #[test]
    fn test_cache_hit_nonexistent() {
        let cache = Path::new("/tmp/nonexistent-coast-cache-12345");
        assert!(!cache_hit(cache, "postgres:16"));
    }

    #[test]
    fn test_cache_hit_with_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let filename = tarball_filename("postgres:16");
        std::fs::write(dir.path().join(&filename), b"fake tarball").unwrap();

        assert!(cache_hit(dir.path(), "postgres:16"));
        assert!(!cache_hit(dir.path(), "postgres:15"));
    }

    #[test]
    fn test_docker_save_cmd() {
        let cmd = docker_save_cmd("postgres:16", Path::new("/cache/pg.tar"));
        assert_eq!(
            cmd,
            vec!["docker", "save", "-o", "/cache/pg.tar", "postgres:16"]
        );
    }

    #[test]
    fn test_docker_load_cmd() {
        let cmd = docker_load_cmd("/image-cache/pg.tar");
        assert_eq!(cmd, vec!["docker", "load", "-i", "/image-cache/pg.tar"]);
    }

    #[test]
    fn test_podman_load_cmd() {
        let cmd = podman_load_cmd("/image-cache/pg.tar");
        assert_eq!(cmd, vec!["podman", "load", "-i", "/image-cache/pg.tar"]);
    }

    #[test]
    fn test_inner_cache_path() {
        let path = inner_cache_path("postgres:16");
        assert!(path.starts_with("/image-cache/"));
        assert!(path.ends_with(".tar"));
    }

    #[test]
    fn test_image_cache_new() {
        let cache = ImageCache::new(PathBuf::from("/tmp/test-cache"));
        assert_eq!(cache.cache_dir(), Path::new("/tmp/test-cache"));
    }

    #[test]
    fn test_image_cache_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(dir.path().to_path_buf());

        assert!(!cache.is_cached("postgres:16"));

        // Create the tarball file
        let filename = tarball_filename("postgres:16");
        std::fs::write(dir.path().join(&filename), b"fake").unwrap();

        assert!(cache.is_cached("postgres:16"));
    }

    #[test]
    fn test_image_cache_ensure_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("nested").join("cache");
        let cache = ImageCache::new(cache_dir.clone());

        assert!(!cache_dir.exists());
        cache.ensure_dir().unwrap();
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_image_cache_list_cached_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(dir.path().to_path_buf());

        let tarballs = cache.list_cached().unwrap();
        assert!(tarballs.is_empty());
    }

    #[test]
    fn test_image_cache_list_cached() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgres_16_abc123.tar"), b"fake").unwrap();
        std::fs::write(dir.path().join("redis_7_def456.tar"), b"fake").unwrap();
        std::fs::write(dir.path().join("not-a-tarball.txt"), b"ignore").unwrap();

        let cache = ImageCache::new(dir.path().to_path_buf());
        let tarballs = cache.list_cached().unwrap();
        assert_eq!(tarballs.len(), 2);
        assert!(tarballs.iter().all(|t| t.extension().unwrap() == "tar"));
    }

    #[test]
    fn test_image_cache_list_cached_nonexistent_dir() {
        let cache = ImageCache::new(PathBuf::from("/tmp/nonexistent-coast-cache-xyz"));
        let tarballs = cache.list_cached().unwrap();
        assert!(tarballs.is_empty());
    }

    #[test]
    fn test_image_cache_save_command() {
        let cache = ImageCache::new(PathBuf::from("/home/user/.coast/image-cache"));
        let cmd = cache.save_command("postgres:16");
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "save");
        assert_eq!(cmd[2], "-o");
        assert!(cmd[3].starts_with("/home/user/.coast/image-cache/"));
        assert_eq!(cmd[4], "postgres:16");
    }

    #[test]
    fn test_image_cache_load_command() {
        let cache = ImageCache::new(PathBuf::from("/home/user/.coast/image-cache"));
        let cmd = cache.load_command("postgres:16");
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "load");
        assert_eq!(cmd[2], "-i");
        assert!(cmd[3].starts_with("/image-cache/"));
    }

    #[test]
    fn test_image_cache_podman_load_command() {
        let cache = ImageCache::new(PathBuf::from("/home/user/.coast/image-cache"));
        let cmd = cache.podman_load_command("redis:7");
        assert_eq!(cmd[0], "podman");
        assert_eq!(cmd[1], "load");
    }

    #[test]
    fn test_image_cache_load_all_commands() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgres_16_abc.tar"), b"fake").unwrap();
        std::fs::write(dir.path().join("redis_7_def.tar"), b"fake").unwrap();

        let cache = ImageCache::new(dir.path().to_path_buf());
        let commands = cache.load_all_commands(false).unwrap();
        assert_eq!(commands.len(), 2);
        for cmd in &commands {
            assert_eq!(cmd[0], "docker");
            assert_eq!(cmd[1], "load");
        }
    }

    #[test]
    fn test_image_cache_load_all_commands_podman() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("postgres_16_abc.tar"), b"fake").unwrap();

        let cache = ImageCache::new(dir.path().to_path_buf());
        let commands = cache.load_all_commands(true).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0][0], "podman");
    }

    #[test]
    fn test_image_cache_load_all_commands_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(dir.path().to_path_buf());
        let commands = cache.load_all_commands(false).unwrap();
        assert!(commands.is_empty());
    }

    #[test]
    fn test_image_cache_dir_constant() {
        assert_eq!(IMAGE_CACHE_DIR, "image-cache");
    }
}
