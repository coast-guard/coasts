//! Image pull + tarball cache orchestration for SSG builds.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! Thin wrapper over
//! [`coast_docker::image_cache::pull_and_cache_image`] (lifted in this
//! PR from `coast-daemon/src/handlers/build/utils.rs` so both crates
//! can share the implementation without a dependency cycle). Adds:
//!
//! - Per-image progress events on the `BuildProgressEvent` stream.
//! - Skip-on-cache-hit behavior so repeat builds are fast.

use std::path::{Path, PathBuf};

use tokio::sync::mpsc::Sender;

use coast_core::error::Result;
use coast_core::protocol::BuildProgressEvent;

use crate::coastfile::SsgSharedServiceConfig;

/// Pure helper: compute the tarball path inside `cache_dir` for a
/// given image reference. Matches the naming convention
/// [`coast_docker::image_cache::pull_and_cache_image`] uses, so
/// cache hits from the regular `coast build` pipeline are also hits
/// for the SSG pipeline.
pub fn tarball_path_for(cache_dir: &Path, image: &str) -> PathBuf {
    let safe_name = image.replace(['/', ':'], "_");
    cache_dir.join(format!("{safe_name}.tar"))
}

/// Pure helper: format the per-image progress step label. Extracted
/// so tests can pin the format without simulating channel I/O.
pub fn pull_step_label(image: &str) -> String {
    format!("Pull {image}")
}

/// Pull every service's image, caching tarballs in `cache_dir`.
///
/// Emits one `started` + one `done` progress event per image. Skips
/// pulls when the tarball already exists in the cache; that's a
/// significant speedup on repeat builds since image cache is shared
/// with the regular coast build pipeline.
///
/// `total_steps` and `step_start` are used to place these per-image
/// events correctly in the caller's overall progress plan.
pub async fn pull_and_cache_ssg_images(
    docker: &bollard::Docker,
    services: &[SsgSharedServiceConfig],
    cache_dir: &Path,
    progress: &Sender<BuildProgressEvent>,
    step_start: u32,
    total_steps: u32,
) -> Result<Vec<PathBuf>> {
    let mut tarballs = Vec::with_capacity(services.len());
    for (idx, svc) in services.iter().enumerate() {
        let step_number = step_start + idx as u32;
        let step_label = pull_step_label(&svc.image);

        let _ = progress
            .send(BuildProgressEvent::started(
                step_label.clone(),
                step_number,
                total_steps,
            ))
            .await;

        // If the tarball already exists from a prior build, skip the
        // pull.
        let tarball_path = tarball_path_for(cache_dir, &svc.image);

        if tarball_path.exists() {
            tarballs.push(tarball_path);
            let _ = progress
                .send(BuildProgressEvent::done(step_label, "cached"))
                .await;
            continue;
        }

        let path =
            coast_docker::image_cache::pull_and_cache_image(docker, &svc.image, cache_dir).await?;
        tarballs.push(path);
        let _ = progress
            .send(BuildProgressEvent::done(step_label, "ok"))
            .await;
    }
    Ok(tarballs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tarball_path_for ---

    #[test]
    fn tarball_path_replaces_slash_and_colon() {
        let p = tarball_path_for(Path::new("/cache"), "ghcr.io/library/postgres:16-alpine");
        assert_eq!(
            p,
            PathBuf::from("/cache/ghcr.io_library_postgres_16-alpine.tar"),
        );
    }

    #[test]
    fn tarball_path_for_simple_image_has_no_slashes_in_filename() {
        let p = tarball_path_for(Path::new("/c"), "postgres:16");
        assert_eq!(p, PathBuf::from("/c/postgres_16.tar"));
    }

    #[test]
    fn tarball_path_for_tagless_image() {
        // No colon -> no `:` character to replace. The file still
        // lives at `{image}.tar` since the underlying cache helper
        // treats missing tags as "latest" internally.
        let p = tarball_path_for(Path::new("/c"), "postgres");
        assert_eq!(p, PathBuf::from("/c/postgres.tar"));
    }

    #[test]
    fn tarball_path_is_deterministic() {
        // Regression against any future non-deterministic mangling.
        let a = tarball_path_for(Path::new("/c"), "mongo:7");
        let b = tarball_path_for(Path::new("/c"), "mongo:7");
        assert_eq!(a, b);
    }

    // --- pull_step_label ---

    #[test]
    fn pull_step_label_format_is_stable() {
        // Coastguard and CLI renderers key off this exact prefix
        // ("Pull ") to group per-image progress rows. If we ever
        // rename this string, audit the UI layer too.
        assert_eq!(pull_step_label("postgres:16"), "Pull postgres:16");
        assert_eq!(
            pull_step_label("ghcr.io/coast/tester:v1"),
            "Pull ghcr.io/coast/tester:v1"
        );
    }
}
