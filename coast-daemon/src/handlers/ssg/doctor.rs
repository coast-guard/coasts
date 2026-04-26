//! `coast ssg doctor` — read-only permission check on host bind
//! mounts of the active SSG's known-image services.
//!
//! Phase: ssg-phase-8. See `coast-ssg/DESIGN.md §10.5` for motivation
//! and `coast-ssg/src/doctor.rs` for the pure evaluator.
//!
//! The pure evaluator lives in `coast-ssg` and returns findings
//! given a manifest and a stat closure. This module is the I/O
//! adapter: it reads the project's SSG manifest from state
//! (Phase 23: `ssg_consumer_pins` > `ssg.latest_build_id`, no global
//! `~/.coast/ssg/latest` fallback) and supplies a real-fs stat
//! closure built on `std::fs::metadata` + `std::os::unix::fs::MetadataExt`.
//! No writes, no Docker calls, no auto-fix.
//!
//! Absent the Unix `MetadataExt`, we still build and compile on
//! non-Unix targets but every path stats as `Missing` there — the
//! daemon only runs on macOS / Linux anyway.

use std::path::Path;
use std::sync::Arc;

use coast_core::error::Result;
use coast_core::protocol::SsgResponse;
use coast_ssg::build::artifact::SsgManifest;
use coast_ssg::doctor::{evaluate_doctor, evaluate_secrets_doctor, StatResult};

use crate::server::AppState;

/// Dispatch target for `SsgAction::Doctor`. Loads the project's SSG
/// manifest, stats every host bind mount, and returns findings.
pub async fn handle_doctor(project: &str, state: &Arc<AppState>) -> Result<SsgResponse> {
    let manifest = load_project_ssg_manifest(project, state).await?;
    let stored = load_stored_secret_names(project);
    Ok(build_doctor_response(manifest, real_stat, &stored))
}

/// Read the set of secret names currently in the keystore for
/// `coast_image = "ssg:<project>"`.
///
/// Phase 33: powers `evaluate_secrets_doctor` so the doctor can
/// report when a `[secrets.<name>]` block was declared at build
/// time but the keystore row has since been wiped (e.g. via
/// `coast ssg secrets clear`). Returns an empty set on any error
/// so a missing or corrupt keystore degrades to "no secrets
/// found" rather than failing the entire doctor run.
fn load_stored_secret_names(project: &str) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let Ok(home) = coast_core::artifact::coast_home() else {
        return out;
    };
    let db_path = home.join("keystore.db");
    if !db_path.exists() {
        return out;
    }
    let key_path = home.join("keystore.key");
    let Ok(keystore) = coast_secrets::keystore::Keystore::open(&db_path, &key_path) else {
        return out;
    };
    let image_key = coast_ssg::build::keystore_image_key(project);
    if let Ok(rows) = keystore.get_all_secrets(&image_key) {
        for row in rows {
            out.insert(row.secret_name);
        }
    }
    out
}

async fn load_project_ssg_manifest(
    project: &str,
    state: &Arc<AppState>,
) -> Result<Option<(String, SsgManifest)>> {
    use coast_ssg::state::SsgStateExt;

    let build_id = {
        let db = state.db.lock().await;
        let pin = db.get_ssg_consumer_pin(project)?;
        let latest = db.get_ssg(project)?.and_then(|r| r.latest_build_id);
        pin.map(|p| p.build_id).or(latest)
    };
    let Some(build_id) = build_id else {
        return Ok(None);
    };

    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id)?;
    let manifest_path = build_dir.join("manifest.json");
    let content =
        std::fs::read_to_string(&manifest_path).map_err(|e| coast_core::error::CoastError::Io {
            message: format!(
                "failed to read SSG manifest '{}': {e}",
                manifest_path.display()
            ),
            path: manifest_path.clone(),
            source: Some(e),
        })?;
    let manifest: SsgManifest = serde_json::from_str(&content).map_err(|e| {
        coast_core::error::CoastError::artifact(format!(
            "failed to parse SSG manifest '{}': {e}",
            manifest_path.display()
        ))
    })?;
    Ok(Some((build_id, manifest)))
}

/// Pure response shaper for `coast ssg doctor`. Decoupled from
/// Docker and filesystem so the summary logic is fully unit-testable.
///
/// `stat_fn` is the injected stat closure that
/// [`evaluate_doctor`](coast_ssg::doctor::evaluate_doctor) calls for
/// each host bind-mount source.
///
/// `stored_secret_names` is the set of secret names currently
/// present in the keystore for `ssg:<project>`. Phase 33: appended
/// onto the bind-mount findings via
/// [`evaluate_secrets_doctor`](coast_ssg::doctor::evaluate_secrets_doctor).
fn build_doctor_response<F>(
    manifest: Option<(String, SsgManifest)>,
    stat_fn: F,
    stored_secret_names: &std::collections::HashSet<String>,
) -> SsgResponse
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
            listings: Vec::new(),
            builds: Vec::new(),
        };
    };

    let mut findings = evaluate_doctor(&manifest, stat_fn);
    // Phase 33: secret-extraction findings appended onto the
    // bind-mount findings list. Same `severity` taxonomy so the
    // SPA renderer doesn't need to know the difference.
    findings.extend(evaluate_secrets_doctor(&manifest, stored_secret_names));

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
        listings: Vec::new(),
        builds: Vec::new(),
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
            secret_injects: vec![],
        }
    }

    fn empty_secret_set() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    #[test]
    fn build_doctor_response_no_manifest_returns_build_hint() {
        let resp = build_doctor_response(None, |_| StatResult::Missing, &empty_secret_set());
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
        let resp = build_doctor_response(
            Some(("b9".to_string(), m)),
            |_| StatResult::Ok { uid: 0, gid: 0 },
            &empty_secret_set(),
        );
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
        let resp = build_doctor_response(
            Some(("b9_20260420120000".to_string(), m)),
            |_| StatResult::Ok { uid: 999, gid: 999 },
            &empty_secret_set(),
        );
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
        let resp = build_doctor_response(
            Some(("b9_20260420120000".to_string(), m)),
            |_| StatResult::Ok { uid: 0, gid: 0 },
            &empty_secret_set(),
        );
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
    fn build_doctor_response_emits_info_for_unextracted_secret() {
        // Phase 33: a manifest declaring a secret whose keystore
        // entry is missing produces an `info`-level finding so the
        // user knows to rebuild before running.
        use coast_ssg::build::artifact::{SsgManifestSecretInject, SsgManifestService};
        let m = SsgManifest {
            build_id: "b9_20260420120000".to_string(),
            built_at: chrono::Utc::now(),
            coastfile_hash: "b9".to_string(),
            services: vec![SsgManifestService {
                name: "postgres".to_string(),
                image: "postgres:16".to_string(),
                ports: vec![5432],
                env_keys: vec!["POSTGRES_PASSWORD".to_string()],
                volumes: vec!["/var/coast-data/pg:/var/lib/postgresql/data".to_string()],
                auto_create_db: false,
            }],
            secret_injects: vec![SsgManifestSecretInject {
                secret_name: "pg_password".to_string(),
                inject_type: "env".to_string(),
                inject_target: "POSTGRES_PASSWORD".to_string(),
                services: vec!["postgres".to_string()],
            }],
        };
        // Empty stored set ⇒ secret declared but missing.
        let resp = build_doctor_response(
            Some(("b9_20260420120000".to_string(), m)),
            |_| StatResult::Ok { uid: 999, gid: 999 },
            &empty_secret_set(),
        );
        let info_findings: Vec<_> = resp
            .findings
            .iter()
            .filter(|f| f.severity == "info" && f.service == "pg_password")
            .collect();
        assert_eq!(
            info_findings.len(),
            1,
            "expected one info finding for the missing secret; got: {:?}",
            resp.findings
        );
        assert!(
            info_findings[0]
                .message
                .contains("missing from the keystore"),
            "got: {}",
            info_findings[0].message
        );
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
        let resp = build_doctor_response(
            Some(("deadbeef_20260101".to_string(), m)),
            |_| StatResult::Ok { uid: 999, gid: 999 },
            &empty_secret_set(),
        );
        assert!(
            resp.message.contains("deadbeef_20260101"),
            "build_id must appear in message; got: {}",
            resp.message
        );
    }
}
