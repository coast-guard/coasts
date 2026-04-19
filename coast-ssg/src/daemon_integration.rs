//! Public hooks the `coast-daemon` crate calls into.
//!
//! Phase: ssg-phase-3.5 and ssg-phase-4. See `DESIGN.md §4.1`, `§11.1`, `§15`.
//!
//! This module is the *only* public surface `coast-daemon` consumes for
//! runtime integration. Handlers under
//! `coast-daemon/src/handlers/run/ssg_integration.rs` call into these
//! functions and never reach across into other modules of this crate.
//! Keeping the contract narrow here is what lets an agent follow
//! `provision.rs` -> one adapter call -> one crate entrypoint.
//!
//! Expected entrypoints (Phase 3.5 / 4):
//!
//! - `ensure_ready_for_instance` — called before a consumer coast
//!   provisions; auto-starts the SSG if any `from_group = true` service
//!   is referenced.
//! - `synthesize_shared_service_configs` — builds the list of
//!   `SharedServiceConfig`s that the existing
//!   `shared_service_routing` and `compose_rewrite` paths consume, with
//!   `host_port` set to the SSG's dynamic host port.
//! - `active_ssg_service_ports` — map of `service_name -> dynamic_host_port`
//!   for remote-coast reverse-tunnel construction.

// TODO(ssg-phase-3.5 / 4): ensure_ready_for_instance,
// synthesize_shared_service_configs, active_ssg_service_ports.
