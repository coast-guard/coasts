//! Dynamic port allocation for SSG services.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.3`, `§11`.
//!
//! Thin wrapper around `coast_daemon::port_manager::allocate_dynamic_port`
//! (or the equivalent shared helper) so the SSG allocates one dynamic
//! host port per service. Allocations are written to `ssg_services` and
//! used when publishing ports on the outer DinD (`-p dyn:canonical`)
//! and when building reverse SSH tunnel pairs for remote consumers
//! (see [`crate::remote_tunnel`]).

// TODO(ssg-phase-3): allocate_ssg_service_ports(services) -> HashMap<name, dynamic>.
