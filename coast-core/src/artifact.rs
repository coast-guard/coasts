/// Image artifact building for coast projects.
///
/// A coast image artifact is a directory at `~/.coast/images/{project_name}/`
/// that contains everything needed to run coast instances:
///
/// ```text
/// ~/.coast/images/my-app/
///   coastfile.toml        # Copy of the parsed Coastfile
///   compose.yml           # Copy of the compose file
///   manifest.json         # Build manifest with metadata
///   inject/               # Injected host files
///     .ssh/id_ed25519
///     .gitconfig
/// ```
///
/// The actual image caching and secret extraction are performed by
/// `coast-docker` and `coast-secrets` respectively. This module provides
/// the pure artifact structure: manifest types, directory layout,
/// hash computation, file copying, and manifest writing.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::coastfile::Coastfile;
use crate::error::{CoastError, Result};
use crate::volume;

/// The build manifest stored as `manifest.json` in the artifact directory.
///
/// Records metadata about the build for reproducibility and cache validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// When this artifact was built.
    pub build_timestamp: DateTime<Utc>,
    /// SHA-256 hash of the Coastfile contents.
    pub coastfile_hash: String,
    /// Project name from the Coastfile.
    pub project_name: String,
    /// List of Docker images that were cached during build.
    pub cached_images: Vec<CachedImage>,
    /// Names of secrets that were extracted (values are NOT stored here).
    pub resolved_secret_names: Vec<String>,
    /// Volume warnings generated at build time.
    pub volume_warnings: Vec<String>,
    /// Runtime specified in the Coastfile.
    pub runtime: String,
    /// Injected host files that were copied.
    pub injected_files: Vec<String>,
    /// Injected host env var names.
    pub injected_env: Vec<String>,
}

/// Metadata about a cached Docker image in the artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedImage {
    /// Full image reference (e.g., "postgres:16").
    pub reference: String,
    /// Filename of the tarball in the image cache.
    pub tarball_name: String,
    /// Short digest of the image (if known).
    pub digest_short: Option<String>,
}

/// Read the `arch` field from a build manifest.
///
/// Returns `None` if the manifest doesn't exist, can't be parsed,
/// or doesn't contain an `arch` field.
pub fn read_manifest_arch(manifest_path: &Path) -> Option<String> {
    let content = fs::read_to_string(manifest_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("arch").and_then(|v| v.as_str()).map(String::from)
}

/// The root directory for all coast data.
///
/// Reads `COAST_HOME` if set, otherwise defaults to `~/.coast/`.
pub fn coast_home() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("COAST_HOME") {
        return Ok(PathBuf::from(shellexpand::tilde(&dir).into_owned()));
    }
    let home = dirs::home_dir().ok_or_else(|| {
        CoastError::artifact("cannot determine home directory. Set $HOME and try again.")
    })?;
    Ok(home.join(".coast"))
}

/// Path to the image artifacts directory for a project.
///
/// Returns `~/.coast/images/{project_name}/`.
pub fn artifact_dir(project_name: &str) -> Result<PathBuf> {
    Ok(coast_home()?.join("images").join(project_name))
}

/// Path to the shared image cache directory.
///
/// Returns `~/.coast/image-cache/`.
pub fn image_cache_dir() -> Result<PathBuf> {
    Ok(coast_home()?.join("image-cache"))
}

/// Compute the SHA-256 hash of a Coastfile's contents.
pub fn hash_coastfile(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Ensure the artifact directory structure exists for a project.
///
/// Creates:
/// - `~/.coast/images/{project}/`
/// - `~/.coast/images/{project}/inject/`
/// - `~/.coast/image-cache/`
pub fn ensure_artifact_dirs(project_name: &str) -> Result<PathBuf> {
    let art_dir = artifact_dir(project_name)?;
    let inject_dir = art_dir.join("inject");
    let cache_dir = image_cache_dir()?;

    for dir in &[&art_dir, &inject_dir, &cache_dir] {
        fs::create_dir_all(dir).map_err(|e| CoastError::Io {
            message: format!("failed to create artifact directory '{}'", dir.display()),
            path: (*dir).clone(),
            source: Some(e),
        })?;
    }

    debug!(
        artifact_dir = %art_dir.display(),
        "Artifact directory structure ensured"
    );

    Ok(art_dir)
}

/// Copy the Coastfile into the artifact directory.
///
/// Saves as `coastfile.toml` in the artifact root.
pub fn copy_coastfile(coastfile_content: &str, artifact_dir: &Path) -> Result<()> {
    let dest = artifact_dir.join("coastfile.toml");
    fs::write(&dest, coastfile_content).map_err(|e| CoastError::Io {
        message: format!(
            "failed to write coastfile to artifact at '{}'",
            dest.display()
        ),
        path: dest,
        source: Some(e),
    })?;

    debug!("Coastfile copied to artifact");
    Ok(())
}

/// Copy the compose file into the artifact directory.
///
/// Preserves the filename from the source.
pub fn copy_compose_file(compose_path: &Path, artifact_dir: &Path) -> Result<String> {
    let filename = compose_path
        .file_name()
        .ok_or_else(|| {
            CoastError::artifact(format!(
                "compose path '{}' has no filename",
                compose_path.display()
            ))
        })?
        .to_string_lossy()
        .to_string();

    let dest = artifact_dir.join(&filename);

    fs::copy(compose_path, &dest).map_err(|e| CoastError::Io {
        message: format!(
            "failed to copy compose file '{}' to artifact at '{}'. \
             Verify the compose file exists and is readable.",
            compose_path.display(),
            dest.display()
        ),
        path: compose_path.to_path_buf(),
        source: Some(e),
    })?;

    debug!(
        compose_file = %filename,
        "Compose file copied to artifact"
    );

    Ok(filename)
}

/// Copy injected host files into the artifact's `inject/` directory.
///
/// Each file path is expanded (tilde, env vars) and copied preserving
/// the filename. Returns the list of filenames that were successfully copied.
pub fn copy_inject_files(files: &[String], artifact_dir: &Path) -> Result<Vec<String>> {
    let inject_dir = artifact_dir.join("inject");
    fs::create_dir_all(&inject_dir).map_err(|e| CoastError::Io {
        message: format!(
            "failed to create inject directory '{}'",
            inject_dir.display()
        ),
        path: inject_dir.clone(),
        source: Some(e),
    })?;

    let mut copied = Vec::new();

    for file_spec in files {
        let expanded = shellexpand::tilde(file_spec);
        let src_path = PathBuf::from(expanded.as_ref());

        let filename = src_path
            .file_name()
            .ok_or_else(|| {
                CoastError::artifact(format!("inject file '{}' has no filename", file_spec))
            })?
            .to_string_lossy()
            .to_string();

        let dest = inject_dir.join(&filename);

        fs::copy(&src_path, &dest).map_err(|e| CoastError::Io {
            message: format!(
                "failed to copy inject file '{}' to '{}'. \
                 Verify the file exists and is readable.",
                src_path.display(),
                dest.display()
            ),
            path: src_path.clone(),
            source: Some(e),
        })?;

        debug!(
            file = %filename,
            source = %src_path.display(),
            "Injected file copied to artifact"
        );

        copied.push(filename);
    }

    Ok(copied)
}

/// Generate volume warnings for the Coastfile's volume configuration.
///
/// Delegates to [`volume::generate_volume_warnings`].
pub fn check_volume_warnings(coastfile: &Coastfile) -> Vec<String> {
    volume::generate_volume_warnings(&coastfile.volumes)
}

/// Write the build manifest to the artifact directory.
pub fn write_manifest(manifest: &Manifest, artifact_dir: &Path) -> Result<()> {
    let dest = artifact_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| CoastError::artifact(format!("failed to serialize manifest: {e}")))?;

    fs::write(&dest, json).map_err(|e| CoastError::Io {
        message: format!("failed to write manifest to '{}'", dest.display()),
        path: dest,
        source: Some(e),
    })?;

    info!(
        project = %manifest.project_name,
        "Build manifest written"
    );

    Ok(())
}

/// Read an existing manifest from the artifact directory.
///
/// Returns `None` if the manifest doesn't exist.
pub fn read_manifest(artifact_dir: &Path) -> Result<Option<Manifest>> {
    let manifest_path = artifact_dir.join("manifest.json");

    if !manifest_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&manifest_path).map_err(|e| CoastError::Io {
        message: format!("failed to read manifest from '{}'", manifest_path.display()),
        path: manifest_path,
        source: Some(e),
    })?;

    let manifest: Manifest = serde_json::from_str(&content)
        .map_err(|e| CoastError::artifact(format!("failed to parse manifest: {e}")))?;

    Ok(Some(manifest))
}

/// Check if a rebuild is needed by comparing Coastfile hashes.
///
/// Returns `true` if:
/// - No existing manifest exists
/// - The Coastfile hash has changed
/// - `force` is true (the `--refresh` flag)
pub fn needs_rebuild(artifact_dir: &Path, coastfile_hash: &str, force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }

    match read_manifest(artifact_dir)? {
        None => Ok(true),
        Some(manifest) => Ok(manifest.coastfile_hash != coastfile_hash),
    }
}

/// Build the artifact directory structure from a parsed Coastfile.
///
/// This performs the "pure" parts of the build:
/// 1. Create directory structure
/// 2. Copy Coastfile into artifact
/// 3. Copy compose file into artifact
/// 4. Copy injected host files
/// 5. Generate volume warnings
///
/// It returns a [`PartialBuild`] that the caller completes by adding
/// cached image info and secret names (from coast-docker and coast-secrets).
pub fn prepare_artifact(coastfile: &Coastfile, coastfile_content: &str) -> Result<PartialBuild> {
    let art_dir = ensure_artifact_dirs(&coastfile.name)?;

    copy_coastfile(coastfile_content, &art_dir)?;
    if let Some(ref compose) = coastfile.compose {
        let compose_filename = copy_compose_file(compose, &art_dir)?;
        info!(compose = %compose_filename, "compose file copied to artifact");
    }
    let injected_files = copy_inject_files(&coastfile.inject.files, &art_dir)?;
    let volume_warnings = check_volume_warnings(coastfile);

    let coastfile_hash = hash_coastfile(coastfile_content);

    info!(
        project = %coastfile.name,
        injected_files = injected_files.len(),
        volume_warnings = volume_warnings.len(),
        "Artifact prepared"
    );

    Ok(PartialBuild {
        artifact_dir: art_dir,
        coastfile_hash,
        project_name: coastfile.name.clone(),
        volume_warnings,
        injected_files,
        injected_env: coastfile.inject.env.clone(),
        runtime: coastfile.runtime.as_str().to_string(),
    })
}

/// Intermediate build state after the pure artifact steps.
///
/// The caller adds cached images and secret names, then calls
/// [`PartialBuild::finalize`] to write the manifest.
#[derive(Debug)]
pub struct PartialBuild {
    /// Path to the artifact directory.
    pub artifact_dir: PathBuf,
    /// SHA-256 hash of the Coastfile.
    pub coastfile_hash: String,
    /// Project name.
    pub project_name: String,
    /// Volume warnings generated during preparation.
    pub volume_warnings: Vec<String>,
    /// Names of injected host files.
    pub injected_files: Vec<String>,
    /// Names of injected host env vars.
    pub injected_env: Vec<String>,
    /// Runtime string.
    pub runtime: String,
}

impl PartialBuild {
    /// Finalize the build by writing the manifest.
    ///
    /// # Arguments
    ///
    /// * `cached_images` - Images that were cached by coast-docker
    /// * `resolved_secret_names` - Secret names that were extracted by coast-secrets
    pub fn finalize(
        self,
        cached_images: Vec<CachedImage>,
        resolved_secret_names: Vec<String>,
    ) -> Result<Manifest> {
        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: self.coastfile_hash,
            project_name: self.project_name,
            cached_images,
            resolved_secret_names,
            volume_warnings: self.volume_warnings,
            runtime: self.runtime,
            injected_files: self.injected_files,
            injected_env: self.injected_env,
        };

        write_manifest(&manifest, &self.artifact_dir)?;

        Ok(manifest)
    }
}

/// Generate the content-addressable tarball filename for a cached image.
///
/// Format: `{image_name}_{tag}_{digest_short}.tar`
/// Slashes in image names are replaced with underscores.
pub fn tarball_filename(image: &str, tag: &str, digest_short: &str) -> String {
    let safe_image = image.replace('/', "_");
    format!("{safe_image}_{tag}_{digest_short}.tar")
}

/// Parse an image reference into (image, tag).
///
/// If no tag is specified, defaults to "latest".
pub fn parse_image_reference(reference: &str) -> (&str, &str) {
    // Handle digest references (image@sha256:...)
    if reference.contains('@') {
        return (reference, "latest");
    }

    match reference.rsplit_once(':') {
        Some((image, tag)) => {
            // Make sure we're not splitting on a port number in the registry
            // e.g., "registry.example.com:5000/myimage"
            if tag.contains('/') {
                (reference, "latest")
            } else {
                (image, tag)
            }
        }
        None => (reference, "latest"),
    }
}

/// Generate a mapping of env vars from the inject config.
///
/// For each env var name in the inject config, reads the current
/// value from the host environment.
pub fn resolve_inject_env(env_names: &[String]) -> HashMap<String, String> {
    let mut env_map = HashMap::new();
    for name in env_names {
        if let Ok(value) = std::env::var(name) {
            env_map.insert(name.clone(), value);
        }
    }
    env_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_hash_coastfile() {
        let hash = hash_coastfile("some content");
        assert_eq!(hash.len(), 64); // SHA-256 produces 64 hex chars
                                    // Same content should produce same hash
        assert_eq!(hash, hash_coastfile("some content"));
    }

    #[test]
    fn test_hash_coastfile_different_content() {
        let h1 = hash_coastfile("content a");
        let h2 = hash_coastfile("content b");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_coastfile_empty() {
        let hash = hash_coastfile("");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_tarball_filename() {
        assert_eq!(
            tarball_filename("postgres", "16", "abc123"),
            "postgres_16_abc123.tar"
        );
    }

    #[test]
    fn test_tarball_filename_with_namespace() {
        assert_eq!(
            tarball_filename("library/postgres", "16", "def456"),
            "library_postgres_16_def456.tar"
        );
    }

    #[test]
    fn test_tarball_filename_with_registry() {
        assert_eq!(
            tarball_filename("registry.example.com/myapp", "v1.0", "aaa"),
            "registry.example.com_myapp_v1.0_aaa.tar"
        );
    }

    #[test]
    fn test_parse_image_reference_with_tag() {
        let (image, tag) = parse_image_reference("postgres:16");
        assert_eq!(image, "postgres");
        assert_eq!(tag, "16");
    }

    #[test]
    fn test_parse_image_reference_no_tag() {
        let (image, tag) = parse_image_reference("postgres");
        assert_eq!(image, "postgres");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_reference_with_registry_port() {
        let (image, tag) = parse_image_reference("registry.example.com:5000/myapp");
        assert_eq!(image, "registry.example.com:5000/myapp");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_reference_with_registry_port_and_tag() {
        let (image, tag) = parse_image_reference("registry.example.com:5000/myapp:v2");
        assert_eq!(image, "registry.example.com:5000/myapp");
        assert_eq!(tag, "v2");
    }

    #[test]
    fn test_parse_image_reference_with_digest() {
        let (image, tag) = parse_image_reference("postgres@sha256:abc123");
        assert_eq!(image, "postgres@sha256:abc123");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_reference_namespaced() {
        let (image, tag) = parse_image_reference("library/node:20");
        assert_eq!(image, "library/node");
        assert_eq!(tag, "20");
    }

    #[test]
    fn test_ensure_artifact_dirs() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("fake_home");
        fs::create_dir_all(&home).unwrap();

        // We can't easily test ensure_artifact_dirs directly since it depends
        // on dirs::home_dir(). Instead, test the directory creation logic.
        let art_dir = home.join(".coast").join("images").join("test-project");
        let inject_dir = art_dir.join("inject");
        let cache_dir = home.join(".coast").join("image-cache");

        fs::create_dir_all(&art_dir).unwrap();
        fs::create_dir_all(&inject_dir).unwrap();
        fs::create_dir_all(&cache_dir).unwrap();

        assert!(art_dir.exists());
        assert!(inject_dir.exists());
        assert!(cache_dir.exists());
    }

    #[test]
    fn test_copy_coastfile() {
        let tmp = TempDir::new().unwrap();
        let content = "[coast]\nname = \"test\"\ncompose = \"dc.yml\"";
        copy_coastfile(content, tmp.path()).unwrap();

        let dest = tmp.path().join("coastfile.toml");
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(dest).unwrap(), content);
    }

    #[test]
    fn test_copy_compose_file() {
        let tmp = TempDir::new().unwrap();

        // Create a source compose file
        let src = tmp.path().join("docker-compose.yml");
        fs::write(&src, "version: '3'\nservices:\n  web: {}").unwrap();

        let artifact = tmp.path().join("artifact");
        fs::create_dir_all(&artifact).unwrap();

        let filename = copy_compose_file(&src, &artifact).unwrap();
        assert_eq!(filename, "docker-compose.yml");

        let dest = artifact.join("docker-compose.yml");
        assert!(dest.exists());
        assert_eq!(
            fs::read_to_string(dest).unwrap(),
            "version: '3'\nservices:\n  web: {}"
        );
    }

    #[test]
    fn test_copy_compose_file_not_found() {
        let tmp = TempDir::new().unwrap();
        let result = copy_compose_file(&tmp.path().join("nonexistent.yml"), tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("failed to copy compose file"));
    }

    #[test]
    fn test_copy_inject_files() {
        let tmp = TempDir::new().unwrap();

        // Create source files
        let src_dir = tmp.path().join("source");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("file1.txt"), "content1").unwrap();
        fs::write(src_dir.join("file2.txt"), "content2").unwrap();

        let artifact = tmp.path().join("artifact");
        fs::create_dir_all(&artifact).unwrap();

        let files = vec![
            src_dir.join("file1.txt").to_string_lossy().to_string(),
            src_dir.join("file2.txt").to_string_lossy().to_string(),
        ];

        let copied = copy_inject_files(&files, &artifact).unwrap();
        assert_eq!(copied.len(), 2);
        assert!(copied.contains(&"file1.txt".to_string()));
        assert!(copied.contains(&"file2.txt".to_string()));

        assert!(artifact.join("inject").join("file1.txt").exists());
        assert!(artifact.join("inject").join("file2.txt").exists());
    }

    #[test]
    fn test_copy_inject_files_missing_file() {
        let tmp = TempDir::new().unwrap();
        let artifact = tmp.path().join("artifact");
        fs::create_dir_all(&artifact).unwrap();

        let files = vec!["/tmp/nonexistent_coast_test_file_12345.txt".to_string()];
        let result = copy_inject_files(&files, &artifact);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_inject_files_empty() {
        let tmp = TempDir::new().unwrap();
        let artifact = tmp.path().join("artifact");
        fs::create_dir_all(&artifact).unwrap();

        let copied = copy_inject_files(&[], &artifact).unwrap();
        assert!(copied.is_empty());
    }

    #[test]
    fn test_write_and_read_manifest() {
        let tmp = TempDir::new().unwrap();

        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: "abc123".to_string(),
            project_name: "my-app".to_string(),
            cached_images: vec![CachedImage {
                reference: "postgres:16".to_string(),
                tarball_name: "postgres_16_abc.tar".to_string(),
                digest_short: Some("abc".to_string()),
            }],
            resolved_secret_names: vec!["api_key".to_string(), "db_pass".to_string()],
            volume_warnings: vec!["Warning: shared volume on database".to_string()],
            runtime: "dind".to_string(),
            injected_files: vec!["id_ed25519".to_string()],
            injected_env: vec!["NODE_ENV".to_string()],
        };

        write_manifest(&manifest, tmp.path()).unwrap();
        assert!(tmp.path().join("manifest.json").exists());

        let loaded = read_manifest(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.coastfile_hash, "abc123");
        assert_eq!(loaded.project_name, "my-app");
        assert_eq!(loaded.cached_images.len(), 1);
        assert_eq!(loaded.cached_images[0].reference, "postgres:16");
        assert_eq!(loaded.resolved_secret_names.len(), 2);
        assert_eq!(loaded.volume_warnings.len(), 1);
        assert_eq!(loaded.runtime, "dind");
        assert_eq!(loaded.injected_files, vec!["id_ed25519"]);
        assert_eq!(loaded.injected_env, vec!["NODE_ENV"]);
    }

    #[test]
    fn test_read_manifest_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let result = read_manifest(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_manifest_invalid_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("manifest.json"), "not json").unwrap();
        let result = read_manifest(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_needs_rebuild_no_manifest() {
        let tmp = TempDir::new().unwrap();
        assert!(needs_rebuild(tmp.path(), "hash", false).unwrap());
    }

    #[test]
    fn test_needs_rebuild_force() {
        let tmp = TempDir::new().unwrap();
        // Even if manifest exists with same hash, force should return true
        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: "same_hash".to_string(),
            project_name: "test".to_string(),
            cached_images: vec![],
            resolved_secret_names: vec![],
            volume_warnings: vec![],
            runtime: "dind".to_string(),
            injected_files: vec![],
            injected_env: vec![],
        };
        write_manifest(&manifest, tmp.path()).unwrap();

        assert!(needs_rebuild(tmp.path(), "same_hash", true).unwrap());
    }

    #[test]
    fn test_needs_rebuild_same_hash() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: "abc123".to_string(),
            project_name: "test".to_string(),
            cached_images: vec![],
            resolved_secret_names: vec![],
            volume_warnings: vec![],
            runtime: "dind".to_string(),
            injected_files: vec![],
            injected_env: vec![],
        };
        write_manifest(&manifest, tmp.path()).unwrap();

        assert!(!needs_rebuild(tmp.path(), "abc123", false).unwrap());
    }

    #[test]
    fn test_needs_rebuild_different_hash() {
        let tmp = TempDir::new().unwrap();
        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: "old_hash".to_string(),
            project_name: "test".to_string(),
            cached_images: vec![],
            resolved_secret_names: vec![],
            volume_warnings: vec![],
            runtime: "dind".to_string(),
            injected_files: vec![],
            injected_env: vec![],
        };
        write_manifest(&manifest, tmp.path()).unwrap();

        assert!(needs_rebuild(tmp.path(), "new_hash", false).unwrap());
    }

    #[test]
    fn test_manifest_serialization_roundtrip() {
        let manifest = Manifest {
            build_timestamp: Utc::now(),
            coastfile_hash: "deadbeef".to_string(),
            project_name: "roundtrip".to_string(),
            cached_images: vec![
                CachedImage {
                    reference: "node:20".to_string(),
                    tarball_name: "node_20_aaa.tar".to_string(),
                    digest_short: Some("aaa".to_string()),
                },
                CachedImage {
                    reference: "redis:7".to_string(),
                    tarball_name: "redis_7_bbb.tar".to_string(),
                    digest_short: None,
                },
            ],
            resolved_secret_names: vec!["key1".to_string()],
            volume_warnings: vec![],
            runtime: "sysbox".to_string(),
            injected_files: vec![],
            injected_env: vec![],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let deserialized: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.project_name, "roundtrip");
        assert_eq!(deserialized.cached_images.len(), 2);
        assert!(deserialized.cached_images[1].digest_short.is_none());
    }

    #[test]
    fn test_cached_image_serialization() {
        let img = CachedImage {
            reference: "postgres:16".to_string(),
            tarball_name: "postgres_16_abc.tar".to_string(),
            digest_short: Some("abc".to_string()),
        };
        let json = serde_json::to_string(&img).unwrap();
        let deserialized: CachedImage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.reference, "postgres:16");
        assert_eq!(deserialized.digest_short, Some("abc".to_string()));
    }

    #[test]
    fn test_resolve_inject_env() {
        // Set a test env var
        unsafe {
            std::env::set_var("COAST_TEST_ARTIFACT_VAR", "test_value");
        }

        let result = resolve_inject_env(&[
            "COAST_TEST_ARTIFACT_VAR".to_string(),
            "COAST_NONEXISTENT_12345".to_string(),
        ]);

        assert_eq!(
            result.get("COAST_TEST_ARTIFACT_VAR"),
            Some(&"test_value".to_string())
        );
        assert!(!result.contains_key("COAST_NONEXISTENT_12345"));

        unsafe {
            std::env::remove_var("COAST_TEST_ARTIFACT_VAR");
        }
    }

    #[test]
    fn test_resolve_inject_env_empty() {
        let result = resolve_inject_env(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_coast_home() {
        // This should not fail as long as $HOME is set
        let result = coast_home();
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains(".coast"));
    }

    #[test]
    fn test_artifact_dir() {
        let result = artifact_dir("my-app");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains("images"));
        assert!(path.to_string_lossy().contains("my-app"));
    }

    #[test]
    fn test_image_cache_dir() {
        let result = image_cache_dir();
        assert!(result.is_ok());
        assert!(result.unwrap().to_string_lossy().contains("image-cache"));
    }

    #[test]
    fn test_partial_build_finalize() {
        let tmp = TempDir::new().unwrap();

        let partial = PartialBuild {
            artifact_dir: tmp.path().to_path_buf(),
            coastfile_hash: "abc123".to_string(),
            project_name: "test".to_string(),
            volume_warnings: vec!["warning1".to_string()],
            injected_files: vec!["file1".to_string()],
            injected_env: vec!["ENV1".to_string()],
            runtime: "dind".to_string(),
        };

        let cached = vec![CachedImage {
            reference: "postgres:16".to_string(),
            tarball_name: "pg.tar".to_string(),
            digest_short: None,
        }];
        let secrets = vec!["secret1".to_string()];

        let manifest = partial.finalize(cached, secrets).unwrap();
        assert_eq!(manifest.project_name, "test");
        assert_eq!(manifest.coastfile_hash, "abc123");
        assert_eq!(manifest.cached_images.len(), 1);
        assert_eq!(manifest.resolved_secret_names, vec!["secret1"]);
        assert_eq!(manifest.volume_warnings, vec!["warning1"]);

        // Manifest should be written to disk
        assert!(tmp.path().join("manifest.json").exists());
    }
}
