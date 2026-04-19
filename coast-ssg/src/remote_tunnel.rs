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

// TODO(ssg-phase-4.5): rewrite_reverse_tunnel_pairs(forwards, ssg_services) -> Vec<(remote, local)>.
