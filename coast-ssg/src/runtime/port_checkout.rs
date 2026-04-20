//! Pure planner for `coast ssg checkout` / `uncheckout`.
//!
//! Phase: ssg-phase-6. See `DESIGN.md §12`.
//!
//! Resolves a `SsgCheckoutTarget` (one service by name, or `--all`)
//! against the current `ssg_services` rows into a list of concrete
//! `(canonical_port, dynamic_host_port, service_name)` triples that
//! the daemon turns into socat processes.
//!
//! This module is intentionally socat-free and DB-free — the daemon
//! owns the port_manager + state DB side. Keeping the planner pure
//! means it can be tested without Docker and reused from the
//! daemon-restart recovery path.

use coast_core::error::{CoastError, Result};

use crate::state::SsgServiceRecord;

/// What the user asked to check out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsgCheckoutTarget {
    /// One named SSG service (`coast ssg checkout <service>`).
    Service(String),
    /// Every SSG service with a published port
    /// (`coast ssg checkout --all`).
    All,
}

/// One socat to spawn on behalf of a checkout request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgCheckoutPlan {
    pub service_name: String,
    pub canonical_port: u16,
    pub dynamic_host_port: u16,
}

/// Resolve a checkout target into one or more plans.
///
/// Behavior:
/// - `Service(name)`: returns exactly one plan for the matching row,
///   or a DESIGN-shaped "service not in active SSG" error.
/// - `All`: returns a plan per row in `services`, sorted by
///   `canonical_port` for deterministic CLI output.
///
/// Errors: unknown service name, or an empty services list when the
/// caller asked for `All`. Both suggest the SSG isn't running or the
/// user mistyped a name.
pub fn plan_checkouts(
    services: &[SsgServiceRecord],
    target: &SsgCheckoutTarget,
) -> Result<Vec<SsgCheckoutPlan>> {
    match target {
        SsgCheckoutTarget::Service(name) => {
            let svc = services
                .iter()
                .find(|s| s.service_name == *name)
                .ok_or_else(|| unknown_service_error(name, services))?;
            Ok(vec![SsgCheckoutPlan {
                service_name: svc.service_name.clone(),
                canonical_port: svc.container_port,
                dynamic_host_port: svc.dynamic_host_port,
            }])
        }
        SsgCheckoutTarget::All => {
            if services.is_empty() {
                return Err(CoastError::state(
                    "No SSG services to check out. Run `coast ssg run` first.",
                ));
            }
            let mut plans: Vec<SsgCheckoutPlan> = services
                .iter()
                .map(|s| SsgCheckoutPlan {
                    service_name: s.service_name.clone(),
                    canonical_port: s.container_port,
                    dynamic_host_port: s.dynamic_host_port,
                })
                .collect();
            plans.sort_by_key(|p| p.canonical_port);
            Ok(plans)
        }
    }
}

fn unknown_service_error(referenced: &str, services: &[SsgServiceRecord]) -> CoastError {
    let mut available: Vec<&str> = services.iter().map(|s| s.service_name.as_str()).collect();
    available.sort();
    let available_list = if available.is_empty() {
        "(the active SSG has no services — is it running?)".to_string()
    } else {
        format!("[{}]", available.join(", "))
    };
    CoastError::state(format!(
        "SSG service '{referenced}' not found. Available services: {available_list}."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, container: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            service_name: name.to_string(),
            container_port: container,
            dynamic_host_port: dynamic,
            status: "running".to_string(),
        }
    }

    #[test]
    fn plan_checkouts_single_service_matches_by_name() {
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let plans =
            plan_checkouts(&services, &SsgCheckoutTarget::Service("postgres".into())).unwrap();
        assert_eq!(
            plans,
            vec![SsgCheckoutPlan {
                service_name: "postgres".into(),
                canonical_port: 5432,
                dynamic_host_port: 60001,
            }]
        );
    }

    #[test]
    fn plan_checkouts_unknown_service_errors_with_available_list() {
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let err =
            plan_checkouts(&services, &SsgCheckoutTarget::Service("mongo".into())).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'mongo'"));
        assert!(msg.contains("[postgres, redis]"));
    }

    #[test]
    fn plan_checkouts_unknown_service_when_empty_ssg_hints_at_run() {
        let err = plan_checkouts(&[], &SsgCheckoutTarget::Service("postgres".into())).unwrap_err();
        assert!(err.to_string().contains("is it running"));
    }

    #[test]
    fn plan_checkouts_all_sorts_by_canonical_port() {
        // Input order is redis-first; output must be postgres-first
        // because 5432 < 6379.
        let services = vec![svc("redis", 6379, 60002), svc("postgres", 5432, 60001)];
        let plans = plan_checkouts(&services, &SsgCheckoutTarget::All).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].service_name, "postgres");
        assert_eq!(plans[0].canonical_port, 5432);
        assert_eq!(plans[1].service_name, "redis");
        assert_eq!(plans[1].canonical_port, 6379);
    }

    #[test]
    fn plan_checkouts_all_on_empty_ssg_errors() {
        let err = plan_checkouts(&[], &SsgCheckoutTarget::All).unwrap_err();
        assert!(err.to_string().contains("No SSG services"));
    }

    #[test]
    fn plan_checkouts_preserves_service_multi_port_rows() {
        // A service with multiple rows (one per container port, e.g. kafka
        // 9092/9093/9094) produces one plan per row.
        let services = vec![
            svc("kafka", 9092, 60010),
            svc("kafka", 9093, 60011),
            svc("kafka", 9094, 60012),
        ];
        let plans = plan_checkouts(&services, &SsgCheckoutTarget::All).unwrap();
        assert_eq!(plans.len(), 3);
        let ports: Vec<u16> = plans.iter().map(|p| p.canonical_port).collect();
        assert_eq!(ports, vec![9092, 9093, 9094]);

        // Service(name) picks the FIRST row matching the name (which
        // is stable because ssg_services is alphabetized, but for
        // multi-port services the caller would typically use --all).
        let single =
            plan_checkouts(&services, &SsgCheckoutTarget::Service("kafka".into())).unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].canonical_port, 9092);
    }
}
