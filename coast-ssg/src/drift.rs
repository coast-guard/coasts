//! SSG drift detection for consumer `coast build` / `coast run`.
//!
//! Phase: ssg-phase-7. See `DESIGN.md §6.1`.
//!
//! A consumer coast's `coast build` records the active SSG's build id
//! plus the image refs for every `from_group = true` service it
//! references, in the coast build's `manifest.json`:
//!
//! ```json
//! {
//!   "ssg": {
//!     "build_id": "...",
//!     "services": ["postgres", "redis"],
//!     "images": {"postgres": "postgres:16", "redis": "redis:7"}
//!   }
//! }
//! ```
//!
//! At `coast run` time the daemon reads that snapshot and calls
//! [`evaluate_drift`] against the SSG's current `latest` manifest.
//! Three outcomes:
//!
//! - [`DriftOutcome::Match`] — build ids align, proceed silently.
//! - [`DriftOutcome::SameImageWarn`] — build ids differ but every
//!   referenced service still carries the same image ref. The daemon
//!   emits a warning and proceeds.
//! - [`DriftOutcome::HardError`] — an image ref changed for a
//!   referenced service or a referenced service is missing. The
//!   daemon fails `coast run` with the DESIGN.md §6.1 verbatim error
//!   so the user rebuilds the coast.
//!
//! This module is intentionally pure (no I/O, no Docker). The daemon
//! owns the read side (loading the two manifests) and the wording
//! side (turning `DriftOutcome` into progress events and error text).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::build::artifact::SsgManifest;

/// What `coast build` recorded about the active SSG at build time.
///
/// Serialized under the top-level `ssg` key of a consumer coast's
/// `manifest.json`. Only the fields needed for drift detection are
/// included — the full SSG manifest stays where it lives in the SSG
/// artifact directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedSsgRef {
    pub build_id: String,
    /// Only the services the consumer actually references (via
    /// `from_group = true`). Keeps the block minimal and makes
    /// "service missing" a well-defined check.
    pub services: Vec<String>,
    /// `service_name -> image_ref` for every entry in `services`.
    /// `BTreeMap` for deterministic JSON output.
    pub images: BTreeMap<String, String>,
}

/// Outcome of evaluating a consumer's recorded SSG snapshot against
/// the currently-active SSG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftOutcome {
    /// `recorded.build_id == active.build_id`. Proceed silently.
    Match,
    /// Build ids differ, but every referenced service still resolves
    /// to the same image ref in the active SSG. Daemon should warn
    /// and proceed.
    SameImageWarn {
        old_build_id: String,
        new_build_id: String,
    },
    /// A hard error: the user must `coast build` again (or pin the
    /// old SSG build) before `coast run` can proceed.
    HardError { reason: DriftHardErrorReason },
}

/// The specific reason a drift evaluation escalated to a hard error.
///
/// Carries enough detail for the daemon to render an actionable
/// suffix after the DESIGN §6.1 verbatim sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftHardErrorReason {
    /// A referenced service's image changed between the recorded
    /// snapshot and the active SSG.
    ImageChanged {
        service: String,
        old_image: String,
        new_image: String,
    },
    /// A service referenced by the consumer no longer exists in the
    /// active SSG build. `available` is the current service list so
    /// the error can suggest the correction.
    ServiceMissing {
        service: String,
        available: Vec<String>,
    },
}

/// Pure drift evaluator. Safe to call from any async context since
/// it does no I/O.
///
/// `referenced` is the list of service names the consumer actually
/// cares about (from `coastfile.shared_service_group_refs`). Services
/// in `recorded.services` but not in `referenced` are ignored — they
/// would only appear if the recorded block is a superset, which v1
/// doesn't produce but future versions might.
pub fn evaluate_drift(
    recorded: &RecordedSsgRef,
    active: &SsgManifest,
    referenced: &[String],
) -> DriftOutcome {
    if recorded.build_id == active.build_id {
        return DriftOutcome::Match;
    }

    // Build ids differ. Scan every referenced service: if any image
    // drifted or disappeared, it's a hard error; otherwise warn.
    let mut active_available: Vec<String> =
        active.services.iter().map(|s| s.name.clone()).collect();
    active_available.sort();

    for service_name in referenced {
        let recorded_image = recorded.images.get(service_name);
        let active_service = active.services.iter().find(|s| s.name == *service_name);

        match (recorded_image, active_service) {
            (_, None) => {
                return DriftOutcome::HardError {
                    reason: DriftHardErrorReason::ServiceMissing {
                        service: service_name.clone(),
                        available: active_available.clone(),
                    },
                };
            }
            (Some(old_image), Some(active_svc)) if *old_image != active_svc.image => {
                return DriftOutcome::HardError {
                    reason: DriftHardErrorReason::ImageChanged {
                        service: service_name.clone(),
                        old_image: old_image.clone(),
                        new_image: active_svc.image.clone(),
                    },
                };
            }
            (None, Some(_)) => {
                // A referenced service with no recorded image entry
                // is treated as a benign gap (older manifest shape).
                // Skip — don't error, don't warn; v1 consumer
                // records always populate images alongside services.
            }
            (Some(old_image), Some(active_svc)) => {
                debug_assert_eq!(old_image, &active_svc.image);
            }
        }
    }

    DriftOutcome::SameImageWarn {
        old_build_id: recorded.build_id.clone(),
        new_build_id: active.build_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::artifact::SsgManifestService;
    use chrono::TimeZone;

    fn active_manifest(build_id: &str, services: &[(&str, &str)]) -> SsgManifest {
        SsgManifest {
            build_id: build_id.to_string(),
            built_at: chrono::Utc.with_ymd_and_hms(2026, 4, 19, 0, 0, 0).unwrap(),
            coastfile_hash: "deadbeef".to_string(),
            services: services
                .iter()
                .map(|(name, image)| SsgManifestService {
                    name: (*name).to_string(),
                    image: (*image).to_string(),
                    ports: vec![5432],
                    env_keys: Vec::new(),
                    volumes: Vec::new(),
                    auto_create_db: false,
                })
                .collect(),
        }
    }

    fn recorded(build_id: &str, services: &[(&str, &str)]) -> RecordedSsgRef {
        RecordedSsgRef {
            build_id: build_id.to_string(),
            services: services.iter().map(|(n, _)| (*n).to_string()).collect(),
            images: services
                .iter()
                .map(|(n, i)| ((*n).to_string(), (*i).to_string()))
                .collect(),
        }
    }

    #[test]
    fn match_when_build_ids_align() {
        let rec = recorded("build-A", &[("postgres", "postgres:16")]);
        let act = active_manifest("build-A", &[("postgres", "postgres:16")]);
        assert_eq!(
            evaluate_drift(&rec, &act, &["postgres".to_string()]),
            DriftOutcome::Match
        );
    }

    #[test]
    fn warn_when_build_id_differs_but_images_match() {
        let rec = recorded(
            "build-A",
            &[("postgres", "postgres:16"), ("redis", "redis:7")],
        );
        let act = active_manifest(
            "build-B",
            &[("postgres", "postgres:16"), ("redis", "redis:7")],
        );
        let outcome = evaluate_drift(&rec, &act, &["postgres".to_string(), "redis".to_string()]);
        match outcome {
            DriftOutcome::SameImageWarn {
                old_build_id,
                new_build_id,
            } => {
                assert_eq!(old_build_id, "build-A");
                assert_eq!(new_build_id, "build-B");
            }
            other => panic!("expected SameImageWarn, got {other:?}"),
        }
    }

    #[test]
    fn hard_error_when_image_changed_for_referenced_service() {
        let rec = recorded("build-A", &[("postgres", "postgres:16")]);
        let act = active_manifest("build-B", &[("postgres", "postgres:17")]);
        let outcome = evaluate_drift(&rec, &act, &["postgres".to_string()]);
        match outcome {
            DriftOutcome::HardError {
                reason:
                    DriftHardErrorReason::ImageChanged {
                        service,
                        old_image,
                        new_image,
                    },
            } => {
                assert_eq!(service, "postgres");
                assert_eq!(old_image, "postgres:16");
                assert_eq!(new_image, "postgres:17");
            }
            other => panic!("expected ImageChanged, got {other:?}"),
        }
    }

    #[test]
    fn hard_error_when_referenced_service_missing_from_active() {
        let rec = recorded(
            "build-A",
            &[("postgres", "postgres:16"), ("redis", "redis:7")],
        );
        // Active SSG no longer has redis.
        let act = active_manifest("build-B", &[("postgres", "postgres:16")]);
        let outcome = evaluate_drift(&rec, &act, &["postgres".to_string(), "redis".to_string()]);
        match outcome {
            DriftOutcome::HardError {
                reason:
                    DriftHardErrorReason::ServiceMissing {
                        service, available, ..
                    },
            } => {
                assert_eq!(service, "redis");
                assert_eq!(available, vec!["postgres".to_string()]);
            }
            other => panic!("expected ServiceMissing, got {other:?}"),
        }
    }

    #[test]
    fn non_referenced_service_change_is_ignored() {
        // Consumer references only postgres; redis image changed in
        // the active SSG. Since the consumer doesn't care about
        // redis, drift should resolve as same-image warn (build ids
        // differ but everything the consumer references matches).
        let rec = recorded("build-A", &[("postgres", "postgres:16")]);
        let act = active_manifest(
            "build-B",
            &[("postgres", "postgres:16"), ("redis", "redis:8")],
        );
        let outcome = evaluate_drift(&rec, &act, &["postgres".to_string()]);
        assert!(matches!(outcome, DriftOutcome::SameImageWarn { .. }));
    }

    #[test]
    fn empty_referenced_list_matches_even_with_drifted_build_ids() {
        // A consumer with no references shouldn't even be calling
        // evaluate_drift, but if the daemon does anyway we stay safe
        // and report SameImageWarn (build ids differ; nothing to
        // check).
        let rec = recorded("build-A", &[]);
        let act = active_manifest("build-B", &[("postgres", "postgres:16")]);
        let outcome = evaluate_drift(&rec, &act, &[]);
        assert!(matches!(outcome, DriftOutcome::SameImageWarn { .. }));
    }

    #[test]
    fn partial_overlap_missing_service_wins_over_same_image() {
        // Consumer references postgres + redis. Postgres image
        // matches; redis is missing. Missing-service beats the
        // would-be-warn — users must rebuild or pin.
        let rec = recorded(
            "build-A",
            &[("postgres", "postgres:16"), ("redis", "redis:7")],
        );
        let act = active_manifest("build-B", &[("postgres", "postgres:16")]);
        let outcome = evaluate_drift(&rec, &act, &["postgres".to_string(), "redis".to_string()]);
        match outcome {
            DriftOutcome::HardError {
                reason: DriftHardErrorReason::ServiceMissing { service, .. },
            } => assert_eq!(service, "redis"),
            other => panic!("expected ServiceMissing, got {other:?}"),
        }
    }
}
