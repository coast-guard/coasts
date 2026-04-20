//! Coast Shared Service Group (SSG) runtime.
//!
//! Singleton Docker-in-Docker host process that runs infrastructure
//! services (postgres, redis, mongodb, ...) shared across multiple Coast
//! projects. A consumer Coastfile opts into an SSG-owned service with
//! `[shared_services.<name>] from_group = true` instead of inlining the
//! service on the host Docker daemon.
//!
//! Start here:
//! - [`README.md`](../../coast-ssg/README.md) — agent bootstrap doc and
//!   external-touchpoint map.
//! - [`DESIGN.md`](../../coast-ssg/DESIGN.md) — full design, phased plan,
//!   and implementation progress tracker.
//!
//! # Discoverability contract
//!
//! This crate follows a strict LLM-discoverability convention documented
//! in `DESIGN.md §4.2`:
//!
//! - Every SSG-related file anywhere in the repository has `ssg` in its
//!   path. `Glob **/*ssg*` returns the complete feature map.
//! - Every public type is prefixed `Ssg` (`SsgCoastfile`, `SsgRuntime`,
//!   `SsgBuildArtifact`, ...). `rg '\bSsg'` returns every feature type.
//! - `coast-service` never imports this crate — remote coasts use the
//!   pre-existing `SharedServicePortForward` protocol (see `DESIGN.md §20`).
//!
//! # Phase 0 status
//!
//! Module skeleton only. No public API is defined yet. Phase 1 introduces
//! the `SsgCoastfile` parser and consumer-side `from_group` field (see
//! `DESIGN.md §16`).

// ssg-phase-1: SSG Coastfile parser (see DESIGN.md §5).
pub mod coastfile;
pub use coastfile::{SsgCoastfile, SsgSection, SsgSharedServiceConfig, SsgVolumeEntry};

// TODO(ssg-phase-2): Build pipeline and artifact layout (see DESIGN.md §9.1).
pub mod build;

// TODO(ssg-phase-3): Singleton DinD runtime (see DESIGN.md §9).
pub mod runtime;

// ssg-phase-2: StateDb extension trait for SSG rows (see DESIGN.md §8).
pub mod state;
pub use state::{SsgPortCheckoutRecord, SsgRecord, SsgServiceRecord, SsgStateExt};

// TODO(ssg-phase-2): Filesystem path helpers (`~/.coast/ssg/...`).
pub mod paths;

// TODO(ssg-phase-6): Host canonical-port checkout for SSG services (see DESIGN.md §12).
pub mod port_checkout;

// TODO(ssg-phase-3.5 / 4): Public hooks `coast-daemon` calls into (see DESIGN.md §4.1).
pub mod daemon_integration;

// TODO(ssg-phase-4.5): Reverse SSH tunnel pair helpers for remote coasts (see DESIGN.md §20.2).
pub mod remote_tunnel;

#[cfg(test)]
pub(crate) mod test_support;
