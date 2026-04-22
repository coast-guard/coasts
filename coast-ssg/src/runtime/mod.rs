//! Singleton DinD runtime for the Shared Service Group.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.3`, `§9.4`.
//!
//! One DinD container per host machine. Inside it, `docker compose up`
//! runs the services declared in the active SSG build's inner compose
//! file. Published ports are all dynamic; the allocated values are
//! recorded in `ssg_services` for consumers and reverse-tunnel builders.
//!
//! Layout:
//! - [`lifecycle`] — run / start / stop / restart / rm.
//! - [`compose_synth`] — generate the inner `compose.yml` from `SsgCoastfile`.
//! - [`bind_mounts`] — symmetric-path plumbing (see `DESIGN.md §10`).
//! - [`auto_create_db`] — nested docker-exec DB creation (see `DESIGN.md §13`).
//! - [`ports`] — dynamic port allocation wrapper.

// ssg-phase-3: SsgRuntime + lifecycle verbs.
pub mod lifecycle;

// ssg-phase-2: synthesize inner compose.yml from SsgCoastfile.
pub mod compose_synth;

// ssg-phase-3: symmetric-path bind mount translation (see DESIGN.md §10.2).
pub mod bind_mounts;

// ssg-phase-5: nested docker-exec for per-instance DB creation (DESIGN.md §13).
pub mod auto_create_db;

// ssg-phase-3: dynamic port allocation.
pub mod ports;

// ssg-phase-6: pure planner for `coast ssg checkout` / `uncheckout`.
pub mod port_checkout;

// ssg-phase-15: zero-copy host-volume import orchestrator
// (DESIGN.md §10.7). The daemon resolves the volume's Mountpoint via
// bollard and delegates here to rewrite the SSG Coastfile.
pub mod host_volume_import;

// ssg-phase-16: consumer pinning (DESIGN.md §17-9 SETTLED).
// `coast ssg checkout-build` writes the pin; drift check + auto-start
// read it and prefer the pinned build over `latest`.
pub mod pinning;

pub use port_checkout::{plan_checkouts, SsgCheckoutPlan, SsgCheckoutTarget};
pub use ports::{allocate_service_ports, SsgServicePortPlan};
