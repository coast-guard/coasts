//! Reverse SSH tunnel pair helpers for remote coasts consuming the SSG.
//!
//! Phase: ssg-phase-4.5 initial, rewritten in Phase 18 for symmetric
//! remote routing, refined in Phase 30 to terminate at the host
//! socat's stable virtual port. See `DESIGN.md §20` and `§24`.
//!
//! The remote `coast-service` binary is SSG-agnostic: it receives
//! `SharedServicePortForward { name, port, remote_port }` in the
//! `RunRequest` and runs alias-IP + socat routing inside the remote
//! DinD that forwards canonical `{name}:{port}` to
//! `host.docker.internal:{remote_port}`. What sits on the local end of
//! the reverse tunnel (inline container vs Phase 28 host socat) is a
//! local-only distinction.
//!
//! This module builds the `(remote_port, local_port)` pairs that
//! `coast-daemon` hands to `ssh -R`:
//!
//! - **Inline shared services**: pair is `(fwd.remote_port, fwd.port)`.
//!   Local side is the canonical port (where the inline shared service
//!   container publishes on `localhost`); remote side is the daemon-
//!   allocated dyn port for the reverse-tunnel bind.
//! - **SSG-backed shared services** (Phase 30): pair is
//!   `(fwd.remote_port, fwd.remote_port)`. Both sides carry the
//!   project's stable virtual port — the remote bind matches the
//!   local port the host socat is listening on, so `ssh -R` connects
//!   straight to the host socat without any further rewriting.
//!   `fwd.remote_port` is set to the virtual port in
//!   `setup_shared_service_tunnels` for SSG forwards.

use coast_core::protocol::SharedServicePortForward;

use crate::state::SsgServiceRecord;

/// Build the `(remote_port, local_port)` pairs for `ssh -R`.
///
/// For each forward:
///
/// - **SSG-backed** (the forward's `(name, port)` matches an
///   `ssg_services` row): pair is `(fwd.remote_port, fwd.remote_port)`.
///   Phase 30 collapses the two sides to the project's stable virtual
///   port, which the caller has already stamped onto `fwd.remote_port`.
/// - **Inline**: pair is `(fwd.remote_port, fwd.port)`. Local side is
///   the canonical port (the inline shared-service container publishes
///   there on `localhost`); remote side is the daemon-allocated dyn
///   port — same as Phase 18.
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
            let is_ssg_backed = ssg_services
                .iter()
                .any(|svc| svc.service_name == fwd.name && svc.container_port == fwd.port);
            let local_port = if is_ssg_backed {
                // Phase 30: symmetric — both sides of `ssh -R` are the
                // project's virtual port. The caller stamped it onto
                // `fwd.remote_port` for SSG forwards.
                fwd.remote_port
            } else {
                // Inline: local side is the canonical port (the inline
                // shared service container's bind on localhost).
                fwd.port
            };
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
    fn ssg_pairs_are_symmetric_on_the_virtual_port() {
        // Phase 30: setup_shared_service_tunnels stamps the project's
        // stable virtual port onto `fwd.remote_port` for SSG-backed
        // forwards. The rewriter then emits (vport, vport) — both
        // sides bind/connect to the same number.
        let forwards = vec![
            fwd("postgres", 5432, 42001), // virtual_port for postgres:5432
            fwd("redis", 6379, 42002),    // virtual_port for redis:6379
        ];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(42001, 42001), (42002, 42002)]);
    }

    #[test]
    fn inline_only_uses_canonical_as_local_and_remote_port_as_remote() {
        let forwards = vec![fwd("postgres", 5432, 61001), fwd("redis", 6379, 61002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &[]);
        // Inline: local side = canonical (fwd.port), remote side = fwd.remote_port.
        assert_eq!(pairs, vec![(61001, 5432), (61002, 6379)]);
    }

    #[test]
    fn mixed_keeps_inline_canonical_and_makes_ssg_symmetric() {
        // Phase 30: inline rows use (dyn, canonical), SSG rows use
        // (vport, vport). The same vector can carry both shapes.
        let forwards = vec![
            fwd("postgres", 5432, 42001),  // SSG-backed: vport
            fwd("my_inline", 8080, 61002), // inline: dyn
            fwd("redis", 6379, 42003),     // SSG-backed: vport
        ];
        let services = vec![svc("postgres", 5432, 60001), svc("redis", 6379, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(42001, 42001), (61002, 8080), (42003, 42003)]);
    }

    #[test]
    fn multi_port_service_emits_symmetric_pair_per_forward() {
        // Phase 30: each SSG container_port has its own virtual_port,
        // so multi-port services produce one symmetric pair per port.
        let forwards = vec![
            fwd("kafka", 9092, 42010),
            fwd("kafka", 9093, 42011),
            fwd("kafka", 9094, 42012),
        ];
        let services = vec![
            svc("kafka", 9092, 60010),
            svc("kafka", 9093, 60011),
            svc("kafka", 9094, 60012),
        ];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(42010, 42010), (42011, 42011), (42012, 42012)]);
    }

    #[test]
    fn unknown_name_falls_back_to_inline_shape() {
        // Forward whose `(name, port)` doesn't match any
        // `ssg_services` row is treated as inline: local = canonical,
        // remote = fwd.remote_port.
        let forwards = vec![fwd("mongo", 27017, 61020)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61020, 27017)]);
    }

    #[test]
    fn matching_name_but_wrong_port_falls_back_to_inline_shape() {
        // A `postgres` SSG service published on 5432 doesn't apply to
        // a `postgres` forward on 9999 — the rewriter keeps inline
        // semantics (local = canonical 9999).
        let forwards = vec![fwd("postgres", 9999, 61030)];
        let services = vec![svc("postgres", 5432, 60001)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(61030, 9999)]);
    }

    #[test]
    fn preserves_input_order_with_mixed_ssg_matches() {
        let forwards = vec![
            fwd("z_last", 10000, 42040),  // SSG: vport
            fwd("a_first", 5432, 42041),  // SSG: vport
            fwd("m_middle", 7777, 61042), // inline: dyn
        ];
        let services = vec![svc("a_first", 5432, 60001), svc("z_last", 10000, 60002)];
        let pairs = rewrite_reverse_tunnel_pairs(&forwards, &services);
        assert_eq!(pairs, vec![(42040, 42040), (42041, 42041), (61042, 7777)]);
    }

    /// Phase 24 invariant (caller-scoping) survives Phase 30: the
    /// caller still passes in `ssg_services` only for the consumer's
    /// own project, so the rewriter can never accidentally route to a
    /// different project's services. After Phase 30 the rewriter just
    /// uses the boolean "is this an SSG service?" decision; what
    /// makes per-project isolation work is that the caller stamps
    /// the right project's `virtual_port` onto `fwd.remote_port`
    /// upstream of this helper.
    #[test]
    fn caller_scoping_still_drives_per_project_isolation() {
        // Two different projects with the same (name, port) but
        // different virtual ports stamped onto fwd.remote_port.
        let services = vec![svc("postgres", 5432, 60001)];

        let pairs_a = rewrite_reverse_tunnel_pairs(&[fwd("postgres", 5432, 42001)], &services);
        assert_eq!(
            pairs_a,
            vec![(42001, 42001)],
            "project A's virtual port shows up on both sides"
        );

        let pairs_b = rewrite_reverse_tunnel_pairs(&[fwd("postgres", 5432, 42100)], &services);
        assert_eq!(
            pairs_b,
            vec![(42100, 42100)],
            "project B's virtual port shows up on both sides"
        );
    }
}
