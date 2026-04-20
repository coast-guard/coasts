//! SSG state: record types + extension trait for `StateDb`.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §8`.
//!
//! Three tables land in the daemon's SQLite state DB via migration in
//! `coast-daemon/src/state/mod.rs`:
//!
//! - `ssg` — singleton row keyed by `id = 1` (enforced by `CHECK`).
//!   Tracks the outer DinD container and its backing build.
//! - `ssg_services` — per-service rows keyed by `service_name`, written
//!   on `coast ssg run` once dynamic host ports are allocated.
//! - `ssg_port_checkouts` — Phase 6. Rows written when the user maps a
//!   canonical host port to an SSG service via `coast ssg checkout`.
//!
//! This module exposes an [`SsgStateExt`] trait that
//! `coast_daemon::state::StateDb` implements (in
//! `coast-daemon/src/state/ssg.rs`). Keeps SSG DB logic colocated with
//! the feature crate while using the existing daemon handle.

use coast_core::error::Result;

/// Singleton `ssg` row. `CHECK (id = 1)` guarantees one-per-host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgRecord {
    pub container_id: Option<String>,
    /// One of: `created`, `running`, `stopped`.
    pub status: String,
    pub build_id: Option<String>,
    /// RFC 3339 timestamp (chrono `to_rfc3339()`).
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgServiceRecord {
    pub service_name: String,
    pub container_port: u16,
    pub dynamic_host_port: u16,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgPortCheckoutRecord {
    pub canonical_port: u16,
    pub service_name: String,
    pub socat_pid: Option<i32>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

/// Typed CRUD for the SSG state tables.
///
/// Implemented on `coast_daemon::state::StateDb` in
/// `coast-daemon/src/state/ssg.rs`. The trait lives here in
/// `coast-ssg` so feature code can refer to it without round-tripping
/// through daemon internals.
///
/// Trait methods are synchronous. Lifecycle orchestrators never hold a
/// `&dyn SsgStateExt` across an `await` boundary — `StateDb` wraps
/// `rusqlite::Connection` which is `!Sync`, so doing so would reject
/// the `Send` bound on streamed futures. Callers read the current
/// state at the start of an operation, perform all Docker work, then
/// apply writes at the end (see `coast-daemon/src/handlers/ssg.rs`).
pub trait SsgStateExt {
    // --- ssg singleton ---

    /// Upsert the singleton row (replaces by `id = 1`).
    fn upsert_ssg(
        &self,
        status: &str,
        container_id: Option<&str>,
        build_id: Option<&str>,
    ) -> Result<()>;

    /// Read the singleton row, or `None` if never populated.
    fn get_ssg(&self) -> Result<Option<SsgRecord>>;

    /// Delete the singleton row. Idempotent.
    fn clear_ssg(&self) -> Result<()>;

    // --- ssg_services ---

    /// Insert (or replace by `service_name`) a service row.
    fn upsert_ssg_service(&self, rec: &SsgServiceRecord) -> Result<()>;

    /// List every service row, ordered alphabetically by name.
    fn list_ssg_services(&self) -> Result<Vec<SsgServiceRecord>>;

    /// Update the status column for one service.
    fn update_ssg_service_status(&self, name: &str, status: &str) -> Result<()>;

    /// Remove every `ssg_services` row. Used when the SSG is removed or
    /// rebuilt from scratch.
    fn clear_ssg_services(&self) -> Result<()>;

    // --- ssg_port_checkouts ---

    /// Insert (or replace by `canonical_port`) a checkout row.
    fn upsert_ssg_port_checkout(&self, rec: &SsgPortCheckoutRecord) -> Result<()>;

    /// List every checkout row, ordered by `canonical_port` ascending.
    fn list_ssg_port_checkouts(&self) -> Result<Vec<SsgPortCheckoutRecord>>;

    /// Delete the checkout row for `canonical_port`, if any. Idempotent.
    fn delete_ssg_port_checkout(&self, canonical_port: u16) -> Result<()>;

    /// Update just the `socat_pid` column for a checkout row (Phase
    /// 6). Used by `coast ssg stop` to null the PID after killing the
    /// socat while preserving the row, and by `run / start` to record
    /// the fresh PID after re-spawning against a new dynamic port.
    fn update_ssg_port_checkout_socat_pid(
        &self,
        canonical_port: u16,
        socat_pid: Option<i32>,
    ) -> Result<()>;

    /// Delete every checkout row. Phase 6 uses this from
    /// `coast ssg rm` (destructive — user explicitly removed the
    /// SSG, so stale checkouts must go).
    fn clear_ssg_port_checkouts(&self) -> Result<()>;
}
