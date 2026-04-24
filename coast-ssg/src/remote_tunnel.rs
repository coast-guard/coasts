//! Reverse SSH tunnel pair helpers for remote coasts consuming the SSG.
//!
//! Phase: ssg-phase-4.5 initial, rewritten in Phase 18 for symmetric
//! remote routing. See `DESIGN.md §20`.
//!
//! The remote `coast-service` binary is SSG-agnostic: it receives
//! `SharedServicePortForward { name, port, remote_port }` in the
//! `RunRequest` and runs alias-IP + socat routing inside the remote
//! DinD that forwards canonical `{name}:{port}` to
//! `host.docker.internal:{remote_port}`. What sits on the local end of
//! the reverse tunnel (inline container vs SSG DinD publish) is a
//! local-only distinction.
//!
//! This module builds the `(remote_port, local_port)` pairs that
//! `coast-daemon` hands to `ssh -R`. The **local** side of each pair is
//! rewritten for SSG-backed forwards to target the SSG's dynamic host
//! port; the **remote** side is always the `remote_port` the daemon
//! allocated in `setup_shared_service_tunnels`.

use coast_core::protocol::SharedServicePortForward;

use crate::state::SsgServiceRecord;

/// Build the `(remote_port, local_port)` pairs for `ssh -R`.
///
/// For each forward:
/// - `remote_port` comes straight from `fwd.remote_port` — the dynamic
///   port the daemon allocated on the remote VM.
/// - `local_port` is the SSG's dynamic host port when the forward
///   matches an `ssg_services` entry by `(service_name, container_port)`,
///   otherwise falls back to `fwd.port` (the canonical port, which is
///   what inline shared services want the tunnel to hit on localhost).
///
/// The returned vector has exactly one entry per input forward, in the
/// same order as `forwards`.
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
            (fwd.remote_port, local_port)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fwd(name: &str, port: u16, remote_port: u16) -> SharedServicePortForward {
        SharedServicePortForward {
            name: name.to_string(),
            port,
            remote_port,
        }
    }

    fn svc(name: &str, container: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            project: "test-proj".to_string(),
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
    fn ssg_only_rewrites_local_side_keeps_remote_port() {
        let forwards = vec![fwd("postgres", 5432, 61001), fwd("redis", 6379, 61002)];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        // remote side = fwd.remote_port (61xxx), local side = SSG dynamic (60xxx).
        assert_eq!(pairs, vec![(61001, 60001), (61002, 60002)]);
    }

    #[test]
    fn inline_only_uses_canonical_as_local_and_remote_port_as_remote() {
        let forwards = vec![fwd("postgres", 5432, 61001), fwd("redis", 6379, 61002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &[]);
        // Inline: local side = canonical (fwd.port), remote side = fwd.remote_port.
        assert_eq!(pairs, vec![(61001, 5432), (61002, 6379)]);
    }

    #[test]
    fn mixed_rewrites_ssg_entries_only() {
        let forwards = vec![
            fwd("postgres", 5432, 61001),  // SSG-backed
            fwd("my_inline", 8080, 61002), // inline
            fwd("redis", 6379, 61003),     // SSG-backed
        ];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61001, 60001), (61002, 8080), (61003, 60002)]);
    }

    #[test]
    fn multi_port_service_emits_one_pair_per_forward() {
        let forwards = vec![
            fwd("kafka", 9092, 61010),
            fwd("kafka", 9093, 61011),
            fwd("kafka", 9094, 61012),
        ];
        let services = vec![
            svc("kafka", 9092, 60010),
            svc("kafka", 9093, 60011),
            svc("kafka", 9094, 60012),
        ];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61010, 60010), (61011, 60011), (61012, 60012)]);
    }

    #[test]
    fn unknown_name_falls_back_to_canonical_local_port() {
        let forwards = vec![fwd("mongo", 27017, 61020)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61020, 27017)]);
    }

    #[test]
    fn matching_name_but_wrong_port_falls_back_to_canonical_local_port() {
        // If an SSG service is named `postgres` on 5432 but the forward
        // targets port 9999, we should NOT use the 5432 record's
        // dynamic port. The tunnel pair keeps the canonical local side.
        let forwards = vec![fwd("postgres", 9999, 61030)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61030, 9999)]);
    }

    #[test]
    fn preserves_input_order_even_with_mixed_ssg_matches() {
        let forwards = vec![
            fwd("z_last", 10000, 61040),
            fwd("a_first", 5432, 61041),
            fwd("m_middle", 7777, 61042),
        ];
        let services = vec![svc("a_first", 5432, 60001), svc("z_last", 10000, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61040, 60002), (61041, 60001), (61042, 7777)]);
    }

    /// Phase 24 invariant: the caller (`synthesize_ssg_forwards`)
    /// only passes in `ssg_services` for the consumer's own project,
    /// so even when a *different* project owns the same `(name,
    /// port)` tuple at a different dynamic host port, the rewriter
    /// cannot see or route to it. Each project's forward resolves
    /// against its own slice.
    #[test]
    fn caller_scoping_isolates_same_name_port_across_projects() {
        let forwards = vec![fwd("postgres", 5432, 61050)];

        // Pretend we are project A: caller hands us A's services.
        let services_a = vec![svc("postgres", 5432, 60001)];
        let pairs_a = rewrite_reverse_tunnel_pairs(&forwards, &services_a);
        assert_eq!(pairs_a, vec![(61050, 60001)]);

        // Pretend we are project B: caller hands us B's services.
        // Same canonical (name, port), different dynamic host port.
        let services_b = vec![svc("postgres", 5432, 60002)];
        let pairs_b = rewrite_reverse_tunnel_pairs(&forwards, &services_b);
        assert_eq!(pairs_b, vec![(61050, 60002)]);

        // If the caller accidentally merged both projects' services
        // into a single slice, the rewriter would route to the
        // first match (a source of latent cross-project bugs).
        // Keeping this assertion ensures the *caller contract* is
        // the enforcement mechanism: `synthesize_ssg_forwards` must
        // filter by `cf.name` before reaching this helper.
        let merged = vec![svc("postgres", 5432, 60001), svc("postgres", 5432, 60002)];
        let pairs_merged = rewrite_reverse_tunnel_pairs(&forwards, &merged);
        assert_eq!(
            pairs_merged,
            vec![(61050, 60001)],
            "documented behavior: first match wins; callers MUST pre-filter by project"
        );
    }
}
