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

// TODO(ssg-phase-3): SsgRuntime + lifecycle verbs.
mod lifecycle;

// ssg-phase-2: synthesize inner compose.yml from SsgCoastfile.
pub mod compose_synth;

// TODO(ssg-phase-3): symmetric-path bind mount translation (see DESIGN.md §10.2).
mod bind_mounts;

// TODO(ssg-phase-5): nested docker-exec for per-instance DB creation.
mod auto_create_db;

// TODO(ssg-phase-3): dynamic port allocation wrapper over coast-daemon's port_manager.
mod ports;
