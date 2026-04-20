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
use coast_ssg::daemon_integration::load_latest_ssg_manifest_with_id;
use coast_ssg::doctor::{evaluate_doctor, StatResult};

use crate::server::AppState;

/// Dispatch target for `SsgRequest::Doctor`.
pub async fn handle_doctor(_state: &Arc<AppState>) -> Result<SsgResponse> {
    let Some((build_id, manifest)) = load_latest_ssg_manifest_with_id()? else {
        return Ok(SsgResponse {
            message: "No SSG build found. Run `coast ssg build` first.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
            findings: Vec::new(),
        });
    };

    let findings = evaluate_doctor(&manifest, real_stat);

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

    Ok(SsgResponse {
        message,
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings,
    })
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
}
