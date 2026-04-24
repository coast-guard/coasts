//! Consumer pinning orchestrator for `coast ssg checkout-build`.
//!
//! Phase: ssg-phase-16. See `DESIGN.md §17-9` (SETTLED — Phase 16).
//!
//! A consumer coast pins its project to a specific SSG `build_id`.
//! Drift evaluation in [`coast-daemon/src/handlers/run/ssg_integration.rs::validate_ssg_drift`]
//! and auto-start in `ensure_ready_for_consumer` both read the pin
//! and prefer the pinned manifest over the `latest` symlink.
//!
//! This module owns two pure helpers:
//!
//! - [`validate_pinnable_build`] confirms a `build_id` resolves to a
//!   directory under `~/.coast/ssg/builds/` with a valid
//!   `manifest.json`. Called at pin time by the daemon handler so
//!   `coast ssg checkout-build <typo>` fails fast.
//! - [`resolve_effective_manifest`] returns `(build_id, manifest)`
//!   for the pin if one exists and its build dir is still on disk,
//!   otherwise falls back to the `latest` symlink. Returns `None`
//!   when no build exists at all.
//!
//! Neither helper touches the daemon state DB — the caller passes a
//! loaded [`PinRecord`] (or `None` for "no pin") so this module
//! stays test-friendly.

use std::path::Path;

use coast_core::error::{CoastError, Result};

use crate::build::artifact::SsgManifest;
use crate::paths;

/// Handoff shape for pin state loaded from `ssg_consumer_pins` in the
/// daemon. Carries just the fields this module cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRecord {
    pub project: String,
    pub build_id: String,
}

/// Confirm `build_id` points at an existing SSG build directory with
/// a readable `manifest.json`. Returns the parsed manifest on success
/// so the caller can display the pin summary without re-reading the
/// file.
pub fn validate_pinnable_build(build_id: &str) -> Result<SsgManifest> {
    if build_id.trim().is_empty() {
        return Err(CoastError::coastfile(
            "coast ssg checkout-build: BUILD_ID must not be empty.",
        ));
    }
    let build_dir = paths::ssg_build_dir(build_id)?;
    if !build_dir.is_dir() {
        return Err(CoastError::coastfile(format!(
            "coast ssg checkout-build: SSG build '{build_id}' not found on disk (expected \
             '{}'). Run `ls ~/.coast/ssg/builds/` to see available builds.",
            build_dir.display()
        )));
    }
    read_manifest_from(&build_dir, build_id)
}

/// Resolve the "effective" SSG manifest for drift check + auto-start:
/// prefer the pinned build if a pin is provided AND its directory is
/// still on disk; otherwise fall back to `latest_build_id` (the
/// caller-supplied default build for this project).
///
/// Phase 23: the second argument is the project's `latest_build_id`
/// resolved from daemon state (`ssg.latest_build_id`), NOT the
/// global `~/.coast/ssg/latest` symlink. Passing `None` means the
/// caller has decided there is no default build for this project —
/// the function returns `Ok(None)` in that case.
///
/// Returns `(build_id, manifest)` when a build is available, `None`
/// when neither a pin nor a project-latest is in play, and
/// propagates an error when the pin is provided but its build has
/// been pruned — that's a hard error so consumers notice before
/// `coast run` silently falls through to latest.
pub fn resolve_effective_manifest(
    pin: Option<&PinRecord>,
    project_latest_build_id: Option<&str>,
) -> Result<Option<(String, SsgManifest)>> {
    if let Some(pin) = pin {
        let build_dir = paths::ssg_build_dir(&pin.build_id)?;
        if !build_dir.is_dir() {
            return Err(pinned_build_missing_error(&pin.build_id));
        }
        let manifest = read_manifest_from(&build_dir, &pin.build_id)?;
        return Ok(Some((pin.build_id.clone(), manifest)));
    }

    let Some(latest_id) = project_latest_build_id else {
        return Ok(None);
    };
    let build_dir = paths::ssg_build_dir(latest_id)?;
    if !build_dir.is_dir() {
        return Ok(None);
    }
    let manifest = read_manifest_from(&build_dir, latest_id)?;
    Ok(Some((latest_id.to_string(), manifest)))
}

/// Hard-error message when a pinned build has been pruned out from
/// under a consumer. Promoted to a free function so the daemon drift
/// handler can reuse the exact wording.
pub fn pinned_build_missing_error(build_id: &str) -> CoastError {
    CoastError::coastfile(format!(
        "SSG build '{build_id}' is pinned for this coast but no longer exists on disk. \
         Run `coast ssg uncheckout-build` to drop the pin and fall back to the latest \
         build, or re-run `coast ssg build` against the Coastfile that produced \
         '{build_id}'."
    ))
}

fn read_manifest_from(build_dir: &Path, build_id: &str) -> Result<SsgManifest> {
    let manifest_path = build_dir.join("manifest.json");
    let raw = std::fs::read_to_string(&manifest_path).map_err(|e| CoastError::Io {
        message: format!(
            "failed to read SSG manifest for build '{build_id}' at '{}': {e}",
            manifest_path.display()
        ),
        path: manifest_path.clone(),
        source: Some(e),
    })?;
    serde_json::from_str(&raw).map_err(|e| {
        CoastError::artifact(format!(
            "failed to parse SSG manifest '{}': {e}",
            manifest_path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_coast_home;

    fn write_build(root: &Path, build_id: &str, manifest_body: &str) {
        let dir = root.join("ssg").join("builds").join(build_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), manifest_body).unwrap();
    }

    fn minimal_manifest(build_id: &str) -> String {
        format!(
            r#"{{
  "build_id": "{build_id}",
  "built_at": "2026-04-22T00:00:00Z",
  "coastfile_hash": "hash-{build_id}",
  "services": []
}}"#
        )
    }

    fn flip_latest(root: &Path, build_id: &str) {
        let latest = root.join("ssg").join("latest");
        let _ = std::fs::remove_file(&latest);
        std::os::unix::fs::symlink(std::path::Path::new("builds").join(build_id), &latest).unwrap();
    }

    // --- validate_pinnable_build ---

    #[test]
    fn validate_pinnable_build_rejects_empty_id() {
        let err = validate_pinnable_build("").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn validate_pinnable_build_rejects_missing_dir() {
        with_coast_home(|_root| {
            let err = validate_pinnable_build("not-a-real-build-id").unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not found on disk"), "got: {msg}");
            assert!(msg.contains("not-a-real-build-id"));
        });
    }

    #[test]
    fn validate_pinnable_build_returns_manifest_on_hit() {
        with_coast_home(|root| {
            write_build(root, "b1_xyz", &minimal_manifest("b1_xyz"));
            let m = validate_pinnable_build("b1_xyz").unwrap();
            assert_eq!(m.build_id, "b1_xyz");
        });
    }

    #[test]
    fn validate_pinnable_build_errors_on_missing_manifest() {
        with_coast_home(|root| {
            let dir = root.join("ssg").join("builds").join("b2");
            std::fs::create_dir_all(&dir).unwrap();
            // No manifest.json written.
            let err = validate_pinnable_build("b2").unwrap_err();
            assert!(err.to_string().contains("failed to read SSG manifest"));
        });
    }

    // --- resolve_effective_manifest ---

    #[test]
    fn resolve_effective_manifest_returns_none_when_no_builds() {
        with_coast_home(|_root| {
            let out = resolve_effective_manifest(None, None).unwrap();
            assert!(out.is_none());
        });
    }

    #[test]
    fn resolve_effective_manifest_returns_project_latest_when_no_pin() {
        with_coast_home(|root| {
            write_build(root, "b_latest", &minimal_manifest("b_latest"));
            // Phase 23: no more global `latest` symlink fallback —
            // the caller supplies the project's own latest directly.
            let (id, _) = resolve_effective_manifest(None, Some("b_latest"))
                .unwrap()
                .unwrap();
            assert_eq!(id, "b_latest");
        });
    }

    #[test]
    fn resolve_effective_manifest_returns_none_when_project_has_no_latest() {
        with_coast_home(|root| {
            // Build exists on disk but the caller says this project
            // has no latest build (no row in `ssg.latest_build_id`).
            write_build(root, "b_other_project", &minimal_manifest("b_other_project"));
            // A global `latest` symlink is meaningless here and must
            // NOT be used — that leak is the Phase 23 regression.
            flip_latest(root, "b_other_project");
            let out = resolve_effective_manifest(None, None).unwrap();
            assert!(
                out.is_none(),
                "no project-latest must not fall through to global symlink: {out:?}"
            );
        });
    }

    #[test]
    fn resolve_effective_manifest_prefers_pin_over_project_latest() {
        with_coast_home(|root| {
            write_build(root, "b_old", &minimal_manifest("b_old"));
            write_build(root, "b_latest", &minimal_manifest("b_latest"));
            let pin = PinRecord {
                project: "proj".to_string(),
                build_id: "b_old".to_string(),
            };
            let (id, m) = resolve_effective_manifest(Some(&pin), Some("b_latest"))
                .unwrap()
                .unwrap();
            assert_eq!(id, "b_old");
            assert_eq!(m.build_id, "b_old");
        });
    }

    #[test]
    fn resolve_effective_manifest_hard_errors_when_pinned_build_pruned() {
        with_coast_home(|root| {
            write_build(root, "b_latest", &minimal_manifest("b_latest"));
            let pin = PinRecord {
                project: "proj".to_string(),
                build_id: "b_pruned".to_string(),
            };
            let err = resolve_effective_manifest(Some(&pin), Some("b_latest")).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("no longer exists"), "got: {msg}");
            assert!(msg.contains("b_pruned"));
            assert!(msg.contains("uncheckout-build"));
        });
    }

    #[test]
    fn pinned_build_missing_error_mentions_remedy_commands() {
        let err = pinned_build_missing_error("b_x");
        let msg = err.to_string();
        assert!(msg.contains("uncheckout-build"));
        assert!(msg.contains("coast ssg build"));
        assert!(msg.contains("b_x"));
    }
}
