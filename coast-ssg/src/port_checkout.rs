//! Host canonical-port checkout for SSG services.
//!
//! Phase: ssg-phase-6. See `DESIGN.md §12`.
//!
//! `coast ssg checkout <service>` spawns a host-level socat that binds
//! the canonical port (5432 for postgres, 6379 for redis, ...) and
//! forwards to the SSG's dynamic host port. This lets host tools (MCPs,
//! ad-hoc `psql`, agents running on the host) reach SSG services at
//! their canonical names without knowing the dynamic port.
//!
//! Coasts themselves never use this path — they always reach SSG
//! services via the docker0 alias-IP socat forwarders inside the DinD,
//! pointed at the dynamic host port. Checkout is purely a host-side
//! convenience, mechanically equivalent to
//! `coast_daemon::port_manager::PortForwarder` but targeting the SSG
//! instead of a coast instance.
//!
//! If another owner (a coast instance or a different SSG service)
//! currently binds the canonical port, checkout displaces that owner
//! and records the takeover in `ssg_port_checkouts`.

// TODO(ssg-phase-6): checkout_service, uncheckout_service, displacement logic,
// ssg_port_checkouts CRUD shims, re-binding on daemon restart.
