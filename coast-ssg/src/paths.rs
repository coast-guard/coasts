//! Filesystem path helpers for the SSG.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! Layout:
//!
//! ```text
//! ~/.coast/ssg/
//!   builds/
//!     {build_id}/
//!       manifest.json
//!       ssg-coastfile.toml
//!       compose.yml
//!   latest -> builds/{build_id}
//! ```
//!
//! Image tarballs are cached in the shared pool at
//! `~/.coast/image-cache/` (unchanged from the regular coast build
//! pipeline) — not under `ssg/`.

use std::path::PathBuf;

use coast_core::artifact::coast_home;
use coast_core::error::Result;

/// `~/.coast/ssg/` (respects `$COAST_HOME`).
pub fn ssg_home() -> Result<PathBuf> {
    Ok(coast_home()?.join("ssg"))
}

/// `~/.coast/ssg/builds/`.
pub fn ssg_builds_dir() -> Result<PathBuf> {
    Ok(ssg_home()?.join("builds"))
}

/// `~/.coast/ssg/latest` (symlink to a build directory).
pub fn ssg_latest_link() -> Result<PathBuf> {
    Ok(ssg_home()?.join("latest"))
}

/// `~/.coast/ssg/builds/{build_id}/`.
pub fn ssg_build_dir(build_id: &str) -> Result<PathBuf> {
    Ok(ssg_builds_dir()?.join(build_id))
}

/// Read the `latest` symlink to get the active build_id, if any.
///
/// Returns `None` when the symlink is missing, broken, or the target
/// path has no final filename component.
pub fn resolve_latest_build_id() -> Option<String> {
    let link = ssg_latest_link().ok()?;
    std::fs::read_link(&link)
        .ok()
        .and_then(|target| target.file_name().map(|f| f.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `COAST_HOME` is process-global. Serialize tests that mutate it
    // so the overrides don't stomp on each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_coast_home<F: FnOnce(&std::path::Path)>(f: F) {
        let guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", tmp.path());
        }

        f(tmp.path());

        unsafe {
            match prev {
                Some(v) => std::env::set_var("COAST_HOME", v),
                None => std::env::remove_var("COAST_HOME"),
            }
        }
        drop(guard);
    }

    #[test]
    fn ssg_home_respects_coast_home() {
        with_coast_home(|root| {
            assert_eq!(ssg_home().unwrap(), root.join("ssg"));
        });
    }

    #[test]
    fn ssg_builds_dir_under_ssg_home() {
        with_coast_home(|root| {
            assert_eq!(ssg_builds_dir().unwrap(), root.join("ssg").join("builds"));
        });
    }

    #[test]
    fn ssg_latest_link_under_ssg_home() {
        with_coast_home(|root| {
            assert_eq!(ssg_latest_link().unwrap(), root.join("ssg").join("latest"));
        });
    }

    #[test]
    fn ssg_build_dir_includes_build_id() {
        with_coast_home(|root| {
            assert_eq!(
                ssg_build_dir("b1_20260101").unwrap(),
                root.join("ssg").join("builds").join("b1_20260101")
            );
        });
    }

    #[test]
    fn resolve_latest_build_id_returns_none_when_missing() {
        with_coast_home(|_root| {
            assert!(resolve_latest_build_id().is_none());
        });
    }

    #[test]
    fn resolve_latest_build_id_reads_symlink_target() {
        with_coast_home(|root| {
            let builds = root.join("ssg").join("builds");
            std::fs::create_dir_all(&builds).unwrap();
            let target = builds.join("abc_20260101000000");
            std::fs::create_dir_all(&target).unwrap();

            let link = root.join("ssg").join("latest");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &link).unwrap();

            assert_eq!(
                resolve_latest_build_id(),
                Some("abc_20260101000000".to_string())
            );
        });
    }
}
