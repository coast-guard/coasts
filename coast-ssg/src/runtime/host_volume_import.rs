//! Zero-copy migration helper: merge an existing host Docker named
//! volume into the SSG Coastfile as a bind-mount entry.
//!
//! Phase: ssg-phase-15. See `DESIGN.md §10.7`.
//!
//! The daemon resolves the host volume's `Mountpoint` via
//! `bollard::Docker::inspect_volume` and hands the result here as a
//! plain `PathBuf`. This module is I/O-free except for the optional
//! `--apply` path that writes the rewritten Coastfile + `.bak`
//! backup; that write is behind an `apply` feature of the struct and
//! uses the existing [`SsgCoastfile::to_standalone_toml`] serializer
//! so no custom TOML surgery is required.
//!
//! ## Invariants
//!
//! - The host Docker volume must already exist (resolved by the
//!   daemon before this runs).
//! - `mount` (container path) and `mountpoint` (host path) must both
//!   be absolute.
//! - The target `[shared_services.<name>]` section must already
//!   exist; this helper adds to an existing service, it does not
//!   create new services.
//! - `--apply` is rejected when the Coastfile source is inline
//!   `--config` text (nothing to write back to).
//! - Duplicate mount paths on the same service are hard-errors so
//!   users don't silently shadow an existing volume entry.

use std::path::{Path, PathBuf};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::SsgResponse;

use crate::coastfile::{SsgCoastfile, SsgSharedServiceConfig, SsgVolumeEntry};

/// Inputs to [`run_import`].
///
/// All Docker I/O has already happened upstream (`volume` exists,
/// `mountpoint` resolved). The orchestrator is pure apart from the
/// `apply` branch's file writes.
#[derive(Debug, Clone)]
pub struct HostVolumeImportInputs {
    /// Host Docker named volume name. Used only for messages.
    pub volume: String,
    /// Target `[shared_services.<name>]` section.
    pub service: String,
    /// Absolute container path to bind the volume mountpoint at.
    pub mount: PathBuf,
    /// Host filesystem path (`Mountpoint` from `docker volume
    /// inspect`). Typically `/var/lib/docker/volumes/<vol>/_data`.
    pub mountpoint: PathBuf,
    /// On-disk path of the SSG Coastfile. `None` when the daemon
    /// received inline `--config` text; in that case `apply` must
    /// be `false`.
    pub coastfile_path: Option<PathBuf>,
    /// Raw TOML text of the SSG Coastfile (required either way; used
    /// to re-parse into a mutable `SsgCoastfile` for editing).
    pub coastfile_raw: String,
    /// When `true`, rewrite the SSG Coastfile on disk with a `.bak`
    /// backup. When `false`, emit a TOML snippet to the response
    /// message.
    pub apply: bool,
}

/// Run the import. Returns an [`SsgResponse`] whose `message`
/// contains either the TOML snippet (when `apply = false`) or a
/// human summary of the applied change (when `apply = true`).
pub fn run_import(inputs: HostVolumeImportInputs) -> Result<SsgResponse> {
    validate_inputs(&inputs)?;

    let HostVolumeImportInputs {
        volume,
        service,
        mount,
        mountpoint,
        coastfile_path,
        coastfile_raw,
        apply,
    } = inputs;

    let mut coastfile = SsgCoastfile::parse(
        &coastfile_raw,
        // The parser uses `project_root` for relative-path
        // resolution; since we reject relative bind paths anyway,
        // the value doesn't affect this phase — use cwd as a
        // stable default.
        coastfile_path
            .as_deref()
            .map_or_else(
                || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                |p| {
                    p.parent()
                        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
                },
            )
            .as_path(),
    )?;

    check_service_exists(&coastfile, &service)?;
    check_for_duplicate(&coastfile, &service, &mount)?;

    // Merge the new entry into the target service.
    if let Some(svc) = coastfile.services.iter_mut().find(|s| s.name == service) {
        svc.volumes.push(SsgVolumeEntry::HostBindMount {
            host_path: mountpoint.clone(),
            container_path: mount.clone(),
        });
    } else {
        // Already checked by `check_service_exists`, but keep this
        // defensive for future refactors.
        return Err(service_not_found_error(&service, &coastfile));
    }

    let summary = ImportSummary {
        volume: &volume,
        service: &service,
        mountpoint: &mountpoint,
        mount: &mount,
    };
    if apply {
        let path = coastfile_path
            .as_deref()
            .expect("validate_inputs rejects apply without coastfile_path");
        apply_to_disk(&coastfile, path, &coastfile_raw, summary)
    } else {
        Ok(SsgResponse {
            message: render_snippet(&coastfile, summary),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
            findings: Vec::new(),
            listings: Vec::new(),
            builds: Vec::new(),
        })
    }
}

/// Shared view of the successful import fields. Grouped so
/// `apply_to_disk` and `render_snippet` stay under clippy's
/// `too_many_arguments` threshold. `Copy` because every field is a
/// cheap reference; this keeps call sites free of explicit
/// borrow-or-move decisions.
#[derive(Clone, Copy)]
struct ImportSummary<'a> {
    volume: &'a str,
    service: &'a str,
    mountpoint: &'a Path,
    mount: &'a Path,
}

// --- validation ---

fn validate_inputs(inputs: &HostVolumeImportInputs) -> Result<()> {
    if inputs.volume.trim().is_empty() {
        return Err(CoastError::coastfile(
            "coast ssg import-host-volume: VOLUME must not be empty.",
        ));
    }
    if inputs.service.trim().is_empty() {
        return Err(CoastError::coastfile(
            "coast ssg import-host-volume: --service must not be empty.",
        ));
    }
    if !inputs.mount.is_absolute() {
        return Err(CoastError::coastfile(format!(
            "coast ssg import-host-volume: --mount must be an absolute container path (got '{}').",
            inputs.mount.display(),
        )));
    }
    if !inputs.mountpoint.is_absolute() {
        return Err(CoastError::coastfile(format!(
            "coast ssg import-host-volume: the host volume's Mountpoint ('{}') must be absolute; \
             this is a Docker-reported path and should never be relative. Check the Docker version.",
            inputs.mountpoint.display(),
        )));
    }
    if inputs.apply && inputs.coastfile_path.is_none() {
        return Err(CoastError::coastfile(
            "coast ssg import-host-volume --apply requires an on-disk Coastfile (use -f / \
             --working-dir); cannot rewrite inline --config input.",
        ));
    }
    Ok(())
}

fn check_service_exists(coastfile: &SsgCoastfile, service: &str) -> Result<()> {
    if coastfile.services.iter().any(|s| s.name == service) {
        Ok(())
    } else {
        Err(service_not_found_error(service, coastfile))
    }
}

fn service_not_found_error(service: &str, coastfile: &SsgCoastfile) -> CoastError {
    let available: Vec<&str> = coastfile.services.iter().map(|s| s.name.as_str()).collect();
    let list = if available.is_empty() {
        "(the SSG Coastfile declares no services)".to_string()
    } else {
        format!("[{}]", available.join(", "))
    };
    CoastError::coastfile(format!(
        "coast ssg import-host-volume: service '{service}' is not declared in the SSG Coastfile. \
         Available services: {list}. Add `[shared_services.{service}]` first, or pass \
         --service <existing-name>."
    ))
}

fn check_for_duplicate(coastfile: &SsgCoastfile, service: &str, mount: &Path) -> Result<()> {
    let Some(svc) = coastfile.services.iter().find(|s| s.name == service) else {
        return Ok(()); // `check_service_exists` already ran.
    };
    for existing in &svc.volumes {
        let existing_target = match existing {
            SsgVolumeEntry::HostBindMount { container_path, .. } => container_path,
            SsgVolumeEntry::InnerNamedVolume { container_path, .. } => container_path,
        };
        if existing_target == mount {
            return Err(CoastError::coastfile(format!(
                "coast ssg import-host-volume: service '{service}' already declares a volume \
                 at '{mount}'. Remove the existing entry first or pick a different --mount \
                 path.",
                mount = mount.display(),
            )));
        }
    }
    Ok(())
}

// --- output: snippet ---

fn render_snippet(updated: &SsgCoastfile, summary: ImportSummary<'_>) -> String {
    let Some(svc) = updated.services.iter().find(|s| s.name == summary.service) else {
        unreachable!("service presence is checked in run_import");
    };
    let mut out = String::new();
    out.push_str(&format!(
        "# Add the following to Coastfile.shared_service_groups ({volume} -> {mount}):\n\n",
        volume = summary.volume,
        mount = summary.mount.display(),
    ));
    out.push_str(&render_service_block(svc));
    out.push_str(&format!(
        "\n# Bind line: {}:{}\n",
        summary.mountpoint.display(),
        summary.mount.display(),
    ));
    out
}

fn render_service_block(svc: &SsgSharedServiceConfig) -> String {
    // Hand-render only the subset of fields we need. Mirrors
    // `SsgCoastfile::to_standalone_toml`'s per-service shape for
    // just this one service. Keeps the snippet focused.
    let mut out = String::new();
    out.push_str(&format!("[shared_services.{}]\n", svc.name));
    out.push_str(&format!("image = \"{}\"\n", svc.image));
    if !svc.ports.is_empty() {
        let ports = svc
            .ports
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("ports = [{ports}]\n"));
    }
    if !svc.volumes.is_empty() {
        out.push_str("volumes = [\n");
        for entry in &svc.volumes {
            let rendered = match entry {
                SsgVolumeEntry::HostBindMount {
                    host_path,
                    container_path,
                } => format!("{}:{}", host_path.display(), container_path.display()),
                SsgVolumeEntry::InnerNamedVolume {
                    name,
                    container_path,
                } => format!("{}:{}", name, container_path.display()),
            };
            out.push_str(&format!("    \"{rendered}\",\n"));
        }
        out.push_str("]\n");
    }
    if !svc.env.is_empty() {
        let mut pairs: Vec<_> = svc.env.iter().collect();
        pairs.sort_by_key(|(k, _)| k.as_str());
        let rendered = pairs
            .iter()
            .map(|(k, v)| format!("{k} = \"{v}\""))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("env = {{ {rendered} }}\n"));
    }
    if svc.auto_create_db {
        out.push_str("auto_create_db = true\n");
    }
    out
}

// --- output: apply ---

fn apply_to_disk(
    coastfile: &SsgCoastfile,
    path: &Path,
    coastfile_raw: &str,
    summary: ImportSummary<'_>,
) -> Result<SsgResponse> {
    let backup = backup_path_for(path);

    // Write backup first (original bytes, not the re-serialized
    // form — preserves comments, formatting, etc.).
    std::fs::write(&backup, coastfile_raw).map_err(|e| CoastError::Io {
        message: format!(
            "coast ssg import-host-volume: failed to write backup at '{}': {e}",
            backup.display()
        ),
        path: backup.clone(),
        source: Some(e),
    })?;

    // Write the re-serialized Coastfile with the new volume.
    let rendered = coastfile.to_standalone_toml();
    std::fs::write(path, &rendered).map_err(|e| CoastError::Io {
        message: format!(
            "coast ssg import-host-volume: failed to write updated Coastfile at '{}': {e}",
            path.display()
        ),
        path: path.to_path_buf(),
        source: Some(e),
    })?;

    Ok(SsgResponse {
        message: format!(
            "coast ssg import-host-volume: applied.\n  \
             volume: {volume}\n  \
             service: {service}\n  \
             mount: {mountpoint}:{mount}\n  \
             coastfile: {path}\n  \
             backup: {backup}",
            volume = summary.volume,
            service = summary.service,
            mountpoint = summary.mountpoint.display(),
            mount = summary.mount.display(),
            path = path.display(),
            backup = backup.display(),
        ),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    })
}

fn backup_path_for(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_owned();
    backup.push(".bak");
    PathBuf::from(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_COASTFILE: &str = r#"
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "coast" }
"#;

    fn snippet_inputs(
        volume: &str,
        service: &str,
        mount: &str,
        mountpoint: &str,
    ) -> HostVolumeImportInputs {
        HostVolumeImportInputs {
            volume: volume.to_string(),
            service: service.to_string(),
            mount: PathBuf::from(mount),
            mountpoint: PathBuf::from(mountpoint),
            coastfile_path: None,
            coastfile_raw: MINIMAL_COASTFILE.to_string(),
            apply: false,
        }
    }

    #[test]
    fn run_import_snippet_happy_path_includes_bind_line() {
        let inputs = snippet_inputs(
            "infra_pg",
            "postgres",
            "/var/lib/postgresql/data-new",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        let resp = run_import(inputs).expect("snippet mode succeeds");
        assert!(
            resp.message
                .contains("/var/lib/docker/volumes/infra_pg/_data:/var/lib/postgresql/data-new"),
            "snippet missing the bind line: {}",
            resp.message
        );
        assert!(
            resp.message.contains("[shared_services.postgres]"),
            "snippet missing service header"
        );
    }

    #[test]
    fn run_import_rejects_relative_mount() {
        let mut inputs = snippet_inputs(
            "infra_pg",
            "postgres",
            "relative/path",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        inputs.mount = PathBuf::from("relative/path");
        let err = run_import(inputs).unwrap_err();
        assert!(err.to_string().contains("absolute container path"));
    }

    #[test]
    fn run_import_errors_on_missing_service_with_available_list() {
        let inputs = snippet_inputs(
            "infra_pg",
            "mongo",
            "/data",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        let err = run_import(inputs).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("service 'mongo' is not declared"), "{msg}");
        assert!(
            msg.contains("[postgres]"),
            "should list available services: {msg}"
        );
    }

    #[test]
    fn run_import_rejects_duplicate_mount_path() {
        // The fixture already declares `pg_data:/var/lib/postgresql/data`.
        let inputs = snippet_inputs(
            "infra_pg",
            "postgres",
            "/var/lib/postgresql/data",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        let err = run_import(inputs).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("already declares a volume"), "{msg}");
        assert!(msg.contains("/var/lib/postgresql/data"), "{msg}");
    }

    #[test]
    fn run_import_rejects_apply_without_coastfile_path() {
        let mut inputs = snippet_inputs(
            "infra_pg",
            "postgres",
            "/data-x",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        inputs.apply = true;
        inputs.coastfile_path = None;
        let err = run_import(inputs).unwrap_err();
        assert!(err.to_string().contains("requires an on-disk Coastfile"));
    }

    #[test]
    fn run_import_rejects_empty_volume() {
        let inputs = snippet_inputs(
            "",
            "postgres",
            "/data-y",
            "/var/lib/docker/volumes/infra_pg/_data",
        );
        let err = run_import(inputs).unwrap_err();
        assert!(err.to_string().contains("VOLUME must not be empty"));
    }

    #[test]
    fn run_import_apply_writes_both_bak_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Coastfile.shared_service_groups");
        std::fs::write(&path, MINIMAL_COASTFILE).unwrap();

        let inputs = HostVolumeImportInputs {
            volume: "infra_pg".to_string(),
            service: "postgres".to_string(),
            mount: PathBuf::from("/srv/data"),
            mountpoint: PathBuf::from("/var/lib/docker/volumes/infra_pg/_data"),
            coastfile_path: Some(path.clone()),
            coastfile_raw: MINIMAL_COASTFILE.to_string(),
            apply: true,
        };
        let resp = run_import(inputs).unwrap();
        assert!(resp.message.contains("applied"));

        // Backup appends ".bak" to the full filename (not "replace the
        // extension") — so for `Coastfile.shared_service_groups` we
        // expect `Coastfile.shared_service_groups.bak`.
        let expected_backup = {
            let mut b = path.as_os_str().to_owned();
            b.push(".bak");
            PathBuf::from(b)
        };
        assert!(
            expected_backup.exists(),
            "backup file should exist at {}",
            expected_backup.display()
        );

        let backup_bytes = std::fs::read_to_string(&expected_backup).unwrap();
        assert_eq!(
            backup_bytes.trim_end(),
            MINIMAL_COASTFILE.trim_end(),
            "backup should contain original bytes"
        );

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("/var/lib/docker/volumes/infra_pg/_data:/srv/data"),
            "updated Coastfile should contain the new bind line, got:\n{written}"
        );
        // Existing volume entry must still be there.
        assert!(
            written.contains("pg_data:/var/lib/postgresql/data"),
            "existing volume entry must be preserved, got:\n{written}"
        );
    }

    #[test]
    fn run_import_apply_round_trips_through_parser() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Coastfile.shared_service_groups");
        std::fs::write(&path, MINIMAL_COASTFILE).unwrap();

        let inputs = HostVolumeImportInputs {
            volume: "infra_pg".to_string(),
            service: "postgres".to_string(),
            mount: PathBuf::from("/srv/data-rt"),
            mountpoint: PathBuf::from("/var/lib/docker/volumes/infra_pg/_data"),
            coastfile_path: Some(path.clone()),
            coastfile_raw: MINIMAL_COASTFILE.to_string(),
            apply: true,
        };
        run_import(inputs).unwrap();

        let reparsed = SsgCoastfile::from_file(&path).expect("rewritten file parses");
        let pg = reparsed
            .services
            .iter()
            .find(|s| s.name == "postgres")
            .expect("postgres preserved");
        let has_new = pg.volumes.iter().any(|v| {
            matches!(
                v,
                SsgVolumeEntry::HostBindMount { host_path, container_path }
                    if host_path == Path::new("/var/lib/docker/volumes/infra_pg/_data")
                        && container_path == Path::new("/srv/data-rt")
            )
        });
        assert!(
            has_new,
            "re-parsed coastfile should contain the new host bind mount"
        );
    }

    #[test]
    fn backup_path_for_appends_bak_to_full_filename() {
        assert_eq!(
            backup_path_for(Path::new("/x/Coastfile.shared_service_groups")),
            PathBuf::from("/x/Coastfile.shared_service_groups.bak")
        );
        // Also works when there is no extension.
        assert_eq!(
            backup_path_for(Path::new("/x/Coastfile")),
            PathBuf::from("/x/Coastfile.bak")
        );
    }
}
