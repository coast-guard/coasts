//! Reverse SSH tunnel pair helpers for remote coasts consuming the SSG.
//!
//! Phase: ssg-phase-4.5. See `DESIGN.md §20`.
//!
//! The remote `coast-service` binary is completely SSG-agnostic: it
//! continues to receive `SharedServicePortForward { name, port }` in the
//! `RunRequest` and knows only that "some process" is reachable at
//! `host.docker.internal:{port}` on its side of the reverse tunnel.
//!
//! All SSG-awareness lives on the local side. `coast-daemon` calls into
//! this module to rewrite the local end of each reverse-tunnel pair:
//!
//! ```text
//! old: (canonical_port /* remote */, canonical_port /* local */)
//! new: (canonical_port /* remote */, ssg_dynamic_port /* local */)
//! ```
//!
//! No change to the remote's compose rewriter, no new RPC, no new
//! protocol type. `coast-service` does not import this module and
//! never will.

use coast_core::protocol::SharedServicePortForward;

use crate::state::SsgServiceRecord;

/// Rewrite the local side of each reverse-tunnel pair so that
/// SSG-backed services point at the SSG's dynamic host port while
/// inline services keep their existing identity mapping.
///
/// A `forward.name` that matches an entry in `ssg_services` with the
/// same `container_port` produces `(forward.port, svc.dynamic_host_port)`.
/// Any other forward (inline shared service, or an SSG ref that
/// doesn't appear in `ssg_services`) falls back to
/// `(forward.port, forward.port)` — the pre-Phase-4.5 behavior.
///
/// The returned vector has exactly one entry per input forward, in
/// the same order as `forwards`.
pub fn rewrite_reverse_tunnel_pairs(
    forwards: &[SharedServicePortForward],
    ssg_services: &[SsgServiceRecord],
) -> Vec<(u16, u16)> {
    forwards
        .iter()
        .map(|fwd| {
            let local_port = ssg_services
                .iter()
                .find(|svc| svc.service_name == fwd.name && svc.container_port == fwd.port)
                .map(|svc| svc.dynamic_host_port)
                .unwrap_or(fwd.port);
            (fwd.port, local_port)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fwd(name: &str, port: u16) -> SharedServicePortForward {
        SharedServicePortForward {
            name: name.to_string(),
            port,
        }
    }

    fn svc(name: &str, container: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            service_name: name.to_string(),
            container_port: container,
            dynamic_host_port: dynamic,
            status: "running".to_string(),
        }
    }

    #[test]
    fn empty_forwards_returns_empty_pairs() {
        let pairs = rewrite_reverse_tunnel_pairs(&[], &[]);
        assert!(pairs.is_empty());
    }

    #[test]
    fn ssg_only_rewrites_local_side() {
        let forwards = vec![fwd("postgres", 5432), fwd("redis", 6379)];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(5432, 60001), (6379, 60002)]);
    }

    #[test]
    fn inline_only_keeps_identity_mapping() {
        let forwards = vec![fwd("postgres", 5432), fwd("redis", 6379)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &[]);
        assert_eq!(pairs, vec![(5432, 5432), (6379, 6379)]);
    }

    #[test]
    fn mixed_rewrites_ssg_entries_only() {
        let forwards = vec![
            fwd("postgres", 5432),  // SSG-backed
            fwd("my_inline", 8080), // inline (not in SSG)
            fwd("redis", 6379),     // SSG-backed
        ];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(5432, 60001), (8080, 8080), (6379, 60002)]);
    }

    #[test]
    fn multi_port_service_emits_one_pair_per_forward() {
        // kafka declares 3 ports; forwards has 3 entries, SSG services
        // has 3 matching rows (one per container_port).
        let forwards = vec![fwd("kafka", 9092), fwd("kafka", 9093), fwd("kafka", 9094)];
        let services = vec![
            svc("kafka", 9092, 60010),
            svc("kafka", 9093, 60011),
            svc("kafka", 9094, 60012),
        ];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(9092, 60010), (9093, 60011), (9094, 60012)]);
    }

    #[test]
    fn unknown_name_falls_back_to_identity() {
        let forwards = vec![fwd("mongo", 27017)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(27017, 27017)]);
    }

    #[test]
    fn matching_name_but_wrong_port_falls_back_to_identity() {
        // If an SSG service is named `postgres` on 5432 but the forward
        // targets port 9999, we should NOT use the 5432 record's
        // dynamic port. The tunnel pair keeps the inline identity.
        let forwards = vec![fwd("postgres", 9999)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(9999, 9999)]);
    }

    #[test]
    fn preserves_input_order_even_with_mixed_ssg_matches() {
        let forwards = vec![
            fwd("z_last", 10000),
            fwd("a_first", 5432),
            fwd("m_middle", 7777),
        ];
        let services = vec![svc("a_first", 5432, 60001), svc("z_last", 10000, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(10000, 60002), (5432, 60001), (7777, 7777)]);
    }
}
