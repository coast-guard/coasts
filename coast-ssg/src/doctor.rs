//! Host bind-mount permission sanity checks for the active SSG.
//!
//! Phase: ssg-phase-8. See `DESIGN.md §10.5` for the user-facing
//! motivation.
//!
//! Several images that users run inside the SSG need specific
//! ownership on their data directories. Postgres refuses to start
//! when `/var/lib/postgresql/data` is owned by root instead of the
//! `postgres` user (UID 999 in the official image). MySQL, MariaDB,
//! MongoDB, and Bitnami Redis have the same pattern.
//!
//! This module is a **pure planner**: given an
//! [`SsgManifest`](crate::build::artifact::SsgManifest) and a stat
//! closure that returns `(uid, gid)` for a host path, it produces a
//! list of [`SsgDoctorFinding`] diagnostics that the daemon wraps
//! into an `SsgResponse`. Nothing here touches the filesystem
//! directly; that lives in
//! [`coast-daemon/src/handlers/ssg/doctor.rs`](../../coast-daemon/src/handlers/ssg/doctor.rs).
//!
//! No auto-fix: permissions on user-owned bytes are not something
//! Coast silently mutates. See `DESIGN.md §17` SETTLED #27.

use std::path::{Path, PathBuf};

use coast_core::protocol::SsgDoctorFinding;

use crate::build::artifact::SsgManifest;

/// An image prefix that ships with a well-known data-directory UID/GID.
///
/// Prefix matching is deliberate — `postgres:16` and `postgres` both
/// match `postgres`. Users who pull a fork under a different name
/// (e.g. `ghcr.io/baosystems/postgis`) don't get false positives.
///
/// `alpine_uid` / `alpine_gid` cover the alpine-tagged variant
/// separately because the upstream Dockerfile creates the data-dir
/// user with a different UID there (postgres uses UID 70 on alpine
/// versus 999 on the debian-based tag). The doctor classifies each
/// image's tag and picks the correct pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownImage {
    pub prefix: &'static str,
    pub expected_uid: u32,
    pub expected_gid: u32,
    pub alpine_uid: Option<u32>,
    pub alpine_gid: Option<u32>,
    pub note: &'static str,
}

impl KnownImage {
    /// Return the `(uid, gid)` pair expected for `image_tag`. Tags
    /// containing `alpine` fall back to the alpine pair when one is
    /// configured; otherwise the default pair is used.
    pub fn expected_for_tag(&self, image_tag: &str) -> (u32, u32) {
        if image_tag.contains("alpine") {
            if let (Some(u), Some(g)) = (self.alpine_uid, self.alpine_gid) {
                return (u, g);
            }
        }
        (self.expected_uid, self.expected_gid)
    }
}

/// Table of known images with data-directory ownership expectations.
///
/// These are the upstream-official images only. Forks (Bitnami,
/// timescaledb, postgis variants, etc.) may use different UIDs and
/// are intentionally skipped — we'd rather say nothing than emit a
/// wrong warning.
pub const KNOWN_IMAGES: &[KnownImage] = &[
    KnownImage {
        prefix: "postgres",
        expected_uid: 999,
        expected_gid: 999,
        alpine_uid: Some(70),
        alpine_gid: Some(70),
        note: "postgres runs as UID 999 (debian) or 70 (alpine) in the official image",
    },
    KnownImage {
        prefix: "mysql",
        expected_uid: 999,
        expected_gid: 999,
        alpine_uid: None,
        alpine_gid: None,
        note: "mysql runs as UID 999 in the official image",
    },
    KnownImage {
        prefix: "mariadb",
        expected_uid: 999,
        expected_gid: 999,
        alpine_uid: None,
        alpine_gid: None,
        note: "mariadb runs as UID 999 in the official image",
    },
    KnownImage {
        prefix: "mongo",
        expected_uid: 999,
        expected_gid: 999,
        alpine_uid: None,
        alpine_gid: None,
        note: "mongo runs as UID 999 in the official image",
    },
];

/// Classify an image reference into its [`KnownImage`] entry, if any.
///
/// Matches the leading `name` component of an image reference against
/// [`KNOWN_IMAGES`] prefixes. `registry/name:tag` is split on the
/// last `/` so `docker.io/postgres:16` still resolves to `postgres`.
pub fn classify_image(image: &str) -> Option<&'static KnownImage> {
    let after_slash = image.rsplit('/').next().unwrap_or(image);
    let name = after_slash.split(':').next().unwrap_or(after_slash);
    KNOWN_IMAGES.iter().find(|k| k.prefix == name)
}

/// Parse a manifest volume entry (`"source:target[:mode]"`) and
/// return the host bind source if it is an absolute path.
///
/// Named-volume entries (`"pg_wal:/var/lib/postgresql/wal"`) return
/// `None`. Mode suffixes on bind mounts (`:ro`) are tolerated.
pub fn host_bind_source(volume_entry: &str) -> Option<PathBuf> {
    let mut parts = volume_entry.splitn(2, ':');
    let source = parts.next()?;
    if !source.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(source))
}

/// Stat result for a host path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatResult {
    Ok { uid: u32, gid: u32 },
    Missing,
}

/// Evaluate every `(service, host-bind-source)` pair against the
/// known-image table and return one [`SsgDoctorFinding`] per match.
///
/// `stat_fn` is injected so tests can stub filesystem I/O; daemon
/// callers pass a real-fs closure built on `std::fs::metadata`.
///
/// Rules:
///
/// - Service with a non-matching image (not in [`KNOWN_IMAGES`]) —
///   skipped silently. Too many forks, too many false positives.
/// - Service with a matching image and a host bind source whose
///   directory is missing — `info` finding ("will be created on
///   run with default ownership").
/// - Service with a matching image and a host bind source whose
///   owner UID/GID matches the image's expectation — `ok` finding.
/// - Mismatch — `warn` finding citing both observed and expected
///   UIDs and the recommended `chown` command.
/// - Inner named volumes are ignored (we have no host path to stat).
pub fn evaluate_doctor<F>(manifest: &SsgManifest, mut stat_fn: F) -> Vec<SsgDoctorFinding>
where
    F: FnMut(&Path) -> StatResult,
{
    let mut findings: Vec<SsgDoctorFinding> = Vec::new();

    for svc in &manifest.services {
        let Some(known) = classify_image(&svc.image) else {
            continue;
        };

        let (exp_uid, exp_gid) = known.expected_for_tag(&svc.image);

        let mut matched_any_bind = false;
        for volume in &svc.volumes {
            let Some(host_path) = host_bind_source(volume) else {
                continue;
            };
            matched_any_bind = true;

            match stat_fn(&host_path) {
                StatResult::Missing => {
                    findings.push(SsgDoctorFinding {
                        service: svc.name.clone(),
                        path: host_path.display().to_string(),
                        severity: "info".to_string(),
                        message: format!(
                            "Directory does not exist yet. `coast ssg run` will create it; {}.",
                            known.note
                        ),
                    });
                }
                StatResult::Ok { uid, gid } => {
                    if uid == exp_uid && gid == exp_gid {
                        findings.push(SsgDoctorFinding {
                            service: svc.name.clone(),
                            path: host_path.display().to_string(),
                            severity: "ok".to_string(),
                            message: format!("Owner matches {exp_uid}:{exp_gid}."),
                        });
                    } else {
                        findings.push(SsgDoctorFinding {
                            service: svc.name.clone(),
                            path: host_path.display().to_string(),
                            severity: "warn".to_string(),
                            message: format!(
                                "Owner {uid}:{gid} but {name} expects {exp_uid}:{exp_gid}. \
                                 Run `sudo chown -R {exp_uid}:{exp_gid} {path}` before \
                                 `coast ssg run`.",
                                name = known.prefix,
                                path = host_path.display(),
                            ),
                        });
                    }
                }
            }
        }

        // A matching image with *only* inner named volumes (no host
        // binds) still deserves a tiny "ok — nothing to check" ping so
        // the user sees we looked at this service.
        if !matched_any_bind {
            findings.push(SsgDoctorFinding {
                service: svc.name.clone(),
                path: String::new(),
                severity: "info".to_string(),
                message: format!(
                    "No host bind mounts declared; inner named volumes are opaque to the host. \
                     {}.",
                    known.note
                ),
            });
        }
    }

    findings
}

/// Phase 33: emit findings about declared `[secrets.<name>]` blocks
/// vs. encrypted entries actually present in the keystore.
///
/// Pure planner: takes `manifest.secret_injects` (the snapshot of
/// declared injects captured at build time) and a closure that
/// reports the set of secret names currently in the keystore for
/// `coast_image = "ssg:<project>"`. The closure shape lets the
/// daemon-side doctor handler do the actual `coast_secrets`
/// lookup without dragging the keystore dependency into this pure
/// module.
///
/// Findings:
/// - `info`: declared in manifest but missing from keystore. The
///   user probably ran `coast ssg secrets clear` since the last
///   `ssg build`. Suggests a fix.
/// - `ok`: declared and present.
/// - No finding emitted for keystore rows that aren't in the
///   manifest — those are stale entries from a previous build but
///   harmless (run-time matches by manifest, not keystore).
///
/// Returns an empty vec when the manifest declares no secrets.
pub fn evaluate_secrets_doctor(
    manifest: &SsgManifest,
    stored_secret_names: &std::collections::HashSet<String>,
) -> Vec<SsgDoctorFinding> {
    let mut findings = Vec::with_capacity(manifest.secret_injects.len());
    for inject in &manifest.secret_injects {
        if stored_secret_names.contains(&inject.secret_name) {
            findings.push(SsgDoctorFinding {
                service: inject.secret_name.clone(),
                path: format!("{}:{}", inject.inject_type, inject.inject_target),
                severity: "ok".to_string(),
                message: "Secret extracted and present in the keystore.".to_string(),
            });
        } else {
            findings.push(SsgDoctorFinding {
                service: inject.secret_name.clone(),
                path: format!("{}:{}", inject.inject_type, inject.inject_target),
                severity: "info".to_string(),
                message: "Declared in Coastfile but missing from the keystore. \
                     Run `coast ssg build` to re-extract (services that \
                     reference this secret will fail at compose-up time \
                     until then)."
                    .to_string(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::build::artifact::SsgManifestService;

    fn manifest_with(services: Vec<SsgManifestService>) -> SsgManifest {
        SsgManifest {
            build_id: "test".to_string(),
            built_at: Utc::now(),
            coastfile_hash: "deadbeef".to_string(),
            services,
            secret_injects: vec![],
        }
    }

    fn svc(name: &str, image: &str, volumes: Vec<&str>) -> SsgManifestService {
        SsgManifestService {
            name: name.to_string(),
            image: image.to_string(),
            ports: vec![5432],
            env_keys: vec![],
            volumes: volumes.into_iter().map(str::to_string).collect(),
            auto_create_db: false,
        }
    }

    #[test]
    fn classify_matches_postgres_variants() {
        assert_eq!(classify_image("postgres").unwrap().prefix, "postgres");
        assert_eq!(classify_image("postgres:16").unwrap().prefix, "postgres");
        assert_eq!(
            classify_image("postgres:16-alpine").unwrap().prefix,
            "postgres"
        );
        assert_eq!(
            classify_image("docker.io/postgres:16").unwrap().prefix,
            "postgres"
        );
    }

    #[test]
    fn classify_ignores_unknown_images() {
        assert!(classify_image("nginx:latest").is_none());
        assert!(classify_image("ghcr.io/baosystems/postgis:12-3.3").is_none());
        assert!(classify_image("memcached").is_none());
    }

    #[test]
    fn host_bind_source_accepts_absolute_and_rejects_named() {
        assert_eq!(
            host_bind_source("/var/coast-data/pg:/var/lib/postgresql/data"),
            Some(PathBuf::from("/var/coast-data/pg"))
        );
        assert_eq!(
            host_bind_source("pg_wal:/var/lib/postgresql/wal"),
            None,
            "inner named volumes have no host source"
        );
        assert_eq!(
            host_bind_source("/var/data:/data:ro"),
            Some("/var/data".into())
        );
    }

    #[test]
    fn unknown_image_emits_no_findings() {
        let m = manifest_with(vec![svc(
            "cache",
            "memcached:1",
            vec!["/tmp/cache:/var/cache"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 0, gid: 0 });
        assert!(
            findings.is_empty(),
            "unknown image must not trigger findings"
        );
    }

    #[test]
    fn ok_finding_when_owner_matches() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 999, gid: 999 });
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "ok");
        assert_eq!(findings[0].service, "pg");
        assert_eq!(findings[0].path, "/var/coast-data/pg");
    }

    #[test]
    fn warn_finding_when_owner_mismatches() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 0, gid: 0 });
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "warn");
        assert!(
            findings[0].message.contains("0:0"),
            "observed owner must be in message, got: {}",
            findings[0].message
        );
        assert!(findings[0].message.contains("999:999"));
        assert!(findings[0].message.contains("sudo chown"));
        assert!(findings[0].message.contains("/var/coast-data/pg"));
    }

    #[test]
    fn info_finding_when_host_dir_missing() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Missing);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "info");
        assert!(findings[0].message.contains("does not exist"));
    }

    #[test]
    fn info_finding_when_matching_image_has_only_named_volumes() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16",
            vec!["pg_data:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 999, gid: 999 });
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "info");
        assert!(findings[0].message.contains("No host bind mounts"));
    }

    #[test]
    fn alpine_postgres_accepts_uid_70() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16-alpine",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 70, gid: 70 });
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "ok", "alpine postgres expects UID 70");
        assert!(findings[0].message.contains("70:70"));
    }

    #[test]
    fn alpine_postgres_warns_on_uid_999() {
        let m = manifest_with(vec![svc(
            "pg",
            "postgres:16-alpine",
            vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
        )]);
        let findings = evaluate_doctor(&m, |_| StatResult::Ok { uid: 999, gid: 999 });
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "warn");
        assert!(
            findings[0].message.contains("expects 70:70"),
            "alpine variant expects 70:70, not 999:999"
        );
    }

    #[test]
    fn multiple_services_produce_combined_findings() {
        let m = manifest_with(vec![
            svc(
                "pg",
                "postgres:16",
                vec!["/var/coast-data/pg:/var/lib/postgresql/data"],
            ),
            svc(
                "nginx",
                "nginx:latest",
                vec!["/var/coast-data/web:/usr/share/nginx/html"],
            ),
            svc(
                "db2",
                "mysql:8",
                vec!["/var/coast-data/mysql:/var/lib/mysql"],
            ),
        ]);
        let findings = evaluate_doctor(&m, |p| {
            if p == Path::new("/var/coast-data/pg") {
                StatResult::Ok { uid: 999, gid: 999 }
            } else {
                StatResult::Ok { uid: 0, gid: 0 }
            }
        });
        assert_eq!(
            findings.len(),
            2,
            "nginx is unknown, skipped; pg ok; mysql warn"
        );
        let pg = findings
            .iter()
            .find(|f| f.service == "pg")
            .expect("pg finding");
        assert_eq!(pg.severity, "ok");
        let mysql = findings
            .iter()
            .find(|f| f.service == "db2")
            .expect("mysql finding");
        assert_eq!(mysql.severity, "warn");
        assert!(mysql.message.contains("mysql expects 999:999"));
    }
}
