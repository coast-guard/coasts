//! Dynamic port allocation for SSG services.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.3`, `§11`.
//!
//! Allocates one dynamic host port per SSG service. The result is a
//! deterministic `Vec<SsgServicePortPlan>` ordered by service name that
//! callers use to (a) publish ports on the outer DinD container and
//! (b) populate the `ssg_services` state table. Reverse-tunnel
//! construction (Phase 4.5) and consumer-side routing (Phase 4) also
//! read from this plan via the state DB.
//!
//! Port allocation delegates to [`coast_core::port::allocate_dynamic_port_excluding`]
//! so `coast-ssg` stays free of a `coast-daemon` dependency (which would
//! be a cycle — see §17.11 in `coast-ssg/DESIGN.md`).

use std::collections::HashSet;

use coast_core::error::{CoastError, Result};
use coast_core::port as core_port;

use crate::build::artifact::{SsgManifest, SsgManifestService};

/// One service's port assignment.
///
/// `container_port` is the service's declared inner port (e.g. 5432 for
/// postgres). `dynamic_host_port` is the ephemeral host port the outer
/// DinD will publish to. Consumer coasts forward canonical ports to
/// `host.docker.internal:{dynamic_host_port}` via existing socat plumbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgServicePortPlan {
    pub service: String,
    pub container_port: u16,
    pub dynamic_host_port: u16,
}

/// Allocate one dynamic host port per declared service port in the manifest.
///
/// Services are processed in the order they appear in the manifest (the
/// manifest itself is built in a deterministic sort — see
/// `SsgCoastfile::validate_and_build`). Services with no declared
/// container ports are skipped (sidecars).
///
/// Returns `CoastError::Port` when the allocator cannot find enough
/// free ports in the ephemeral range after the standard retry budget.
pub fn allocate_service_ports(manifest: &SsgManifest) -> Result<Vec<SsgServicePortPlan>> {
    let mut excluded: HashSet<u16> = HashSet::new();
    let mut plans = Vec::new();

    for svc in &manifest.services {
        for container_port in service_container_ports(svc) {
            let dynamic = core_port::allocate_dynamic_port_excluding(&excluded).map_err(|err| {
                CoastError::port(format!(
                    "failed to allocate dynamic host port for SSG service '{}' (inner port {}): {}",
                    svc.name, container_port, err
                ))
            })?;
            excluded.insert(dynamic);
            plans.push(SsgServicePortPlan {
                service: svc.name.clone(),
                container_port,
                dynamic_host_port: dynamic,
            });
        }
    }

    Ok(plans)
}

/// Return a service's declared container ports in declaration order.
///
/// Extracted for testability; the manifest is the canonical input.
fn service_container_ports(svc: &SsgManifestService) -> Vec<u16> {
    svc.ports.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_manifest(services: Vec<(&str, Vec<u16>)>) -> SsgManifest {
        SsgManifest {
            build_id: "test_19700101000000".to_string(),
            built_at: Utc::now(),
            coastfile_hash: "deadbeef".to_string(),
            services: services
                .into_iter()
                .map(|(name, ports)| SsgManifestService {
                    name: name.to_string(),
                    image: format!("{name}:latest"),
                    ports,
                    env_keys: Vec::new(),
                    volumes: Vec::new(),
                    auto_create_db: false,
                })
                .collect(),
        }
    }

    #[test]
    fn test_single_service_single_port() {
        let manifest = make_manifest(vec![("postgres", vec![5432])]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].service, "postgres");
        assert_eq!(plans[0].container_port, 5432);
        assert!(plans[0].dynamic_host_port >= 49152);
    }

    #[test]
    fn test_multiple_services_preserve_order() {
        let manifest = make_manifest(vec![
            ("postgres", vec![5432]),
            ("redis", vec![6379]),
            ("mongodb", vec![27017]),
        ]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        let services: Vec<&str> = plans.iter().map(|p| p.service.as_str()).collect();
        assert_eq!(services, vec!["postgres", "redis", "mongodb"]);
    }

    #[test]
    fn test_all_dynamic_ports_unique() {
        let manifest = make_manifest(vec![
            ("postgres", vec![5432]),
            ("redis", vec![6379]),
            ("mongodb", vec![27017]),
            ("mysql", vec![3306]),
            ("rabbitmq", vec![5672]),
        ]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        let unique: HashSet<u16> = plans.iter().map(|p| p.dynamic_host_port).collect();
        assert_eq!(unique.len(), plans.len(), "dynamic ports must be unique");
    }

    #[test]
    fn test_service_with_multiple_ports_gets_one_plan_per_port() {
        let manifest = make_manifest(vec![("postgres", vec![5432, 5433])]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].service, "postgres");
        assert_eq!(plans[0].container_port, 5432);
        assert_eq!(plans[1].service, "postgres");
        assert_eq!(plans[1].container_port, 5433);
        assert_ne!(plans[0].dynamic_host_port, plans[1].dynamic_host_port);
    }

    #[test]
    fn test_service_with_no_ports_is_skipped() {
        let manifest = make_manifest(vec![("sidecar", vec![]), ("postgres", vec![5432])]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].service, "postgres");
    }

    #[test]
    fn test_empty_manifest_returns_empty_plan() {
        let manifest = make_manifest(vec![]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        assert!(plans.is_empty());
    }

    #[test]
    fn test_all_ports_in_ephemeral_range() {
        let manifest = make_manifest(vec![
            ("a", vec![5432]),
            ("b", vec![6379]),
            ("c", vec![27017]),
        ]);
        let plans = allocate_service_ports(&manifest).expect("allocate");
        for plan in &plans {
            assert!(
                (core_port::PORT_RANGE_START..=core_port::PORT_RANGE_END)
                    .contains(&plan.dynamic_host_port),
                "plan {plan:?} has a port outside the ephemeral range"
            );
        }
    }
}
