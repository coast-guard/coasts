//! `coast ssg doctor` — read-only permission check on host bind
//! mounts of the active SSG's known-image services.
//!
//! Phase: ssg-phase-8. See `coast-ssg/DESIGN.md §10.5` for motivation
//! and `coast-ssg/src/doctor.rs` for the pure evaluator.
//!
//! The pure evaluator lives in `coast-ssg` and returns findings
//! given a manifest and a stat closure. This module is the I/O
//! adapter: it reads the active manifest via
//! `coast_ssg::daemon_integration::load_latest_ssg_manifest_with_id`
//! and supplies a real-fs stat closure built on
//! `std::fs::metadata` + `std::os::unix::fs::MetadataExt`. No writes,
//! no Docker calls, no auto-fix.
//!
//! Absent the Unix `MetadataExt`, we still build and compile on
//! non-Unix targets but every path stats as `Missing` there — the
//! daemon only runs on macOS / Linux anyway.

use std::path::Path;
use std::sync::Arc;

use coast_core::error::Result;
use coast_core::protocol::SsgResponse;
use coast_ssg::build::artifact::SsgManifest;
use coast_ssg::daemon_integration::load_latest_ssg_manifest_with_id;
use coast_ssg::doctor::{evaluate_doctor, StatResult};

use crate::server::AppState;

/// Dispatch target for `SsgRequest::Doctor`. Loads the active SSG
/// manifest, stats every host bind mount, and returns findings.
pub async fn handle_doctor(_state: &Arc<AppState>) -> Result<SsgResponse> {
    let manifest = load_latest_ssg_manifest_with_id()?;
    Ok(build_doctor_response(manifest, real_stat))
}

/// Pure response shaper for `coast ssg doctor`. Decoupled from
/// Docker and filesystem so the summary logic is fully unit-testable.
///
/// `stat_fn` is the injected stat closure that
/// [`evaluate_doctor`](coast_ssg::doctor::evaluate_doctor) calls for
/// each host bind-mount source.
fn build_doctor_response<F>(manifest: Option<(String, SsgManifest)>, stat_fn: F) -> SsgResponse
where
    F: FnMut(&Path) -> StatResult,
{
    let Some((build_id, manifest)) = manifest else {
        return SsgResponse {
            message: "No SSG build found. Run `coast ssg build` first.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
            findings: Vec::new(),
        };
    };

    let findings = evaluate_doctor(&manifest, stat_fn);

    let (ok, warn, info) = summarize(&findings);
    let message = if warn == 0 && info == 0 && ok == 0 {
        format!(
            "SSG '{build_id}' has no services matching the doctor's known-image table; \
             nothing to check."
        )
    } else if warn == 0 {
        format!("SSG '{build_id}' looks healthy: {ok} ok, {info} info.")
    } else {
        format!(
            "SSG '{build_id}': {warn} warning(s), {ok} ok, {info} info. \
             Fix the warnings before `coast ssg run`."
        )
    };

    SsgResponse {
        message,
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings,
    }
}

fn summarize(findings: &[coast_core::protocol::SsgDoctorFinding]) -> (usize, usize, usize) {
    let mut ok = 0;
    let mut warn = 0;
    let mut info = 0;
    for f in findings {
        match f.severity.as_str() {
            "ok" => ok += 1,
            "warn" => warn += 1,
            "info" => info += 1,
            _ => {}
        }
    }
    (ok, warn, info)
}

/// Real-fs stat closure. Returns `Missing` when the path does not
/// exist or we cannot access it; otherwise returns the UID/GID via
/// Unix `MetadataExt`.
fn real_stat(path: &Path) -> StatResult {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata(path) {
            Ok(m) => StatResult::Ok {
                uid: m.uid(),
                gid: m.gid(),
            },
            Err(_) => StatResult::Missing,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        StatResult::Missing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_counts_each_severity() {
        use coast_core::protocol::SsgDoctorFinding;
        let f = |s: &str| SsgDoctorFinding {
            service: "pg".into(),
            path: "/tmp/x".into(),
            severity: s.into(),
            message: "".into(),
        };
        let findings = vec![f("ok"), f("ok"), f("warn"), f("info"), f("info"), f("info")];
        assert_eq!(summarize(&findings), (2, 1, 3));
    }

    #[test]
    fn summarize_ignores_unknown_severity() {
        use coast_core::protocol::SsgDoctorFinding;
        let findings = vec![coast_core::protocol::SsgDoctorFinding {
            service: "pg".into(),
            path: "/tmp/x".into(),
            severity: "???".into(),
            message: "".into(),
        }];
        assert_eq!(summarize(&findings), (0, 0, 0));
    }

    // --- Phase 9 coverage: build_doctor_response unit tests ---
    //
    // The pure-helper extraction lets us test the response-shaping
    // logic without loading real filesystem manifests or stat-ing
    // real paths. `build_doctor_response` now owns every message
    // branch of `handle_doctor`.

    use chrono::Utc;
    use coast_ssg::build::artifact::{SsgManifest, SsgManifestService};

    fn manifest_with(services: Vec<(&str, &str, Vec<&str>)>) -> SsgManifest {
        SsgManifest {
            build_id: "b9_20260420120000".to_string(),
            built_at: Utc::now(),
            coastfile_hash: "b9".to_string(),
            services: services
                .into_iter()
                .map(|(name, image, vols)| SsgManifestService {
                    name: name.to_string(),
                    image: image.to_string(),
                    ports: vec![5432],
                    env_keys: vec![],
                    volumes: vols.into_iter().map(str::to_string).collect(),
                    auto_create_db: false,
                })
                .collect(),
        }
    }

    #[test]
    fn build_doctor_response_no_manifest_returns_build_hint() {
        let resp = build_doctor_response(None, |_| StatResult::Missing);
        assert_eq!(
            resp.message,
            "No SSG build found. Run `coast ssg build` first.",
        );
        assert!(resp.findings.is_empty());
        assert!(resp.services.is_empty());
    }

    #[test]
    fn build_doctor_response_manifest_only_unknown_images_says_nothing_to_check() {
        let m = manifest_with(vec![
            ("web", "nginx:1", vec!["/var/web:/var/www"]),
            ("cache", "memcached:1", vec!["/var/cache:/var/cache"]),
        ]);
        let resp = build_doctor_response(Some(("b9".to_string(), m)), |_| StatResult::Ok {
            uid: 0,
            gid: 0,
        });
        assert!(
            resp.message.contains("nothing to check"),
            "got: {}",
            resp.message
        );
        assert!(
            resp.findings.is_empty(),
            "unknown images produce no findings"
        );
    }

    #[test]
    fn build_doctor_response_all_ok_says_looks_healthy() {
        let m = manifest_with(vec![(
            "postgres",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let resp = build_doctor_response(Some(("b9_20260420120000".to_string(), m)), |_| {
            StatResult::Ok { uid: 999, gid: 999 }
        });
        assert!(
            resp.message.contains("looks healthy"),
            "got: {}",
            resp.message
        );
        assert!(resp.message.contains("1 ok"));
        assert_eq!(resp.findings.len(), 1);
        assert_eq!(resp.findings[0].severity, "ok");
    }

    #[test]
    fn build_doctor_response_with_warnings_says_fix_warnings() {
        let m = manifest_with(vec![(
            "postgres",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let resp = build_doctor_response(Some(("b9_20260420120000".to_string(), m)), |_| {
            StatResult::Ok { uid: 0, gid: 0 }
        });
        assert!(
            resp.message.contains("1 warning(s)"),
            "got: {}",
            resp.message
        );
        assert!(
            resp.message.contains("Fix the warnings"),
            "got: {}",
            resp.message
        );
        assert_eq!(resp.findings.len(), 1);
        assert_eq!(resp.findings[0].severity, "warn");
    }

    #[test]
    fn build_doctor_response_build_id_appears_in_message() {
        // Regression: message must cite the exact build_id so operators
        // can tell which SSG artifact was just audited.
        let m = manifest_with(vec![(
            "postgres",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let resp = build_doctor_response(Some(("deadbeef_20260101".to_string(), m)), |_| {
            StatResult::Ok { uid: 999, gid: 999 }
        });
        assert!(
            resp.message.contains("deadbeef_20260101"),
            "build_id must appear in message; got: {}",
            resp.message
        );
    }
}
