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
        let step_label = format!("Pull {}", svc.image);

        let _ = progress
            .send(BuildProgressEvent::started(
                step_label.clone(),
                step_number,
                total_steps,
            ))
            .await;

        // If the tarball already exists from a prior build, skip the
        // pull. The tarball naming convention matches
        // `coast_docker::image_cache::pull_and_cache_image`.
        let safe_name = svc.image.replace(['/', ':'], "_");
        let tarball_path = cache_dir.join(format!("{safe_name}.tar"));

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
