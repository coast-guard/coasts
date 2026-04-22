//! `coast ssg import-host-volume` handler.
//!
//! Phase: ssg-phase-15. See `coast-ssg/DESIGN.md §10.7`.
//!
//! Thin adapter that (1) asks bollard for the host Docker volume's
//! `Mountpoint`, (2) resolves the SSG Coastfile via the standard
//! discovery triplet (`file` / `working_dir` / inline `config`),
//! and (3) delegates to
//! [`coast_ssg::daemon_integration::run_import`] for the actual
//! edit logic. All validation and file-writing lives in the pure
//! orchestrator so the handler stays unit-test-free and trivial.

use std::path::PathBuf;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::SsgResponse;

use crate::server::AppState;

/// Grouped arguments for [`handle_import_host_volume`]. The daemon
/// dispatcher destructures the `SsgRequest::ImportHostVolume` variant
/// into this struct so the handler stays under clippy's
/// `too_many_arguments` threshold.
pub(super) struct ImportHostVolumeArgs {
    pub volume: String,
    pub service: String,
    pub mount: PathBuf,
    pub file: Option<PathBuf>,
    pub working_dir: Option<PathBuf>,
    pub config: Option<String>,
    pub apply: bool,
}

pub(super) async fn handle_import_host_volume(
    state: &std::sync::Arc<AppState>,
    args: ImportHostVolumeArgs,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot import host volume."))?;

    // Resolve the host volume's Mountpoint via bollard. The daemon
    // already holds the Docker handle; no subprocess needed.
    let vol = docker.inspect_volume(&args.volume).await.map_err(|e| {
        CoastError::docker(format!(
            "Host Docker has no volume named '{volume}' (or inspection failed): {e}. \
             Create it with `docker volume create {volume}` first, or pass an existing \
             volume name.",
            volume = args.volume,
        ))
    })?;
    let mountpoint = PathBuf::from(vol.mountpoint);

    // Resolve the SSG Coastfile source. Shared helper with `build_ssg`
    // so both verbs accept the same flag triplet.
    let (coastfile_path, coastfile_raw) =
        coast_ssg::daemon_integration::resolve_ssg_coastfile_source(
            args.file.as_deref(),
            args.working_dir.as_deref(),
            args.config.as_deref(),
        )?;

    coast_ssg::daemon_integration::run_import(
        coast_ssg::daemon_integration::HostVolumeImportInputs {
            volume: args.volume,
            service: args.service,
            mount: args.mount,
            mountpoint,
            coastfile_path,
            coastfile_raw,
            apply: args.apply,
        },
    )
}
