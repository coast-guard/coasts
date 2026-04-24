//! SSG state: record types + extension trait for `StateDb`.
//!
//! Phase: ssg-phase-20 (per-project correction; see `DESIGN.md §23`).
//!
//! Four tables land in the daemon's SQLite state DB via migration in
//! `coast-daemon/src/state/mod.rs`:
//!
//! - `ssg` — keyed by `project TEXT PRIMARY KEY`. One row per project
//!   that has run `coast ssg build`/`run`. Tracks the outer DinD
//!   container and its backing build.
//! - `ssg_services` — per-service rows keyed by `(project, service_name)`,
//!   written on `coast ssg run` once dynamic host ports are allocated.
//! - `ssg_port_checkouts` — Phase 6. Rows written when the user maps a
//!   canonical host port to an SSG service via `coast ssg checkout`.
//!   Keyed by `(project, canonical_port)`.
//! - `ssg_consumer_pins` — Phase 16 consumer-side build pin. Keyed by
//!   consumer project name (unchanged).
//!
//! This module exposes an [`SsgStateExt`] trait that
//! `coast_daemon::state::StateDb` implements (in
//! `coast-daemon/src/state/ssg.rs`). Keeps SSG DB logic colocated with
//! the feature crate while using the existing daemon handle.

use coast_core::error::Result;

/// `ssg` row for a single project's Shared Service Group.
///
/// Primary key is the consumer project name (Phase 20). Multiple
/// rows coexist when multiple projects each have their own SSG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgRecord {
    /// The consumer project name. Matches the `[coast] name` in the
    /// project's main Coastfile.
    pub project: String,
    pub container_id: Option<String>,
    /// One of: `created`, `running`, `stopped`.
    pub status: String,
    pub build_id: Option<String>,
    /// RFC 3339 timestamp (chrono `to_rfc3339()`).
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgServiceRecord {
    /// The consumer project name that owns this SSG service.
    pub project: String,
    pub service_name: String,
    pub container_port: u16,
    pub dynamic_host_port: u16,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgPortCheckoutRecord {
    /// The consumer project name that owns the SSG service whose
    /// canonical port is checked out to the host.
    pub project: String,
    pub canonical_port: u16,
    pub service_name: String,
    pub socat_pid: Option<i32>,
    /// RFC 3339 timestamp.
    pub created_at: String,
}

/// Consumer-local pin to a specific SSG build (Phase 16, §17-9
/// SETTLED). Drift check and auto-start evaluate against the
/// pinned build instead of the current `latest` symlink.
///
/// Primary key is the consumer project name. Multiple worktrees of
/// the same project share one pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgConsumerPinRecord {
    pub project: String,
    pub build_id: String,
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
    // --- ssg (per-project) ---

    /// Upsert the row for `project` (replaces by project name).
    fn upsert_ssg(
        &self,
        project: &str,
        status: &str,
        container_id: Option<&str>,
        build_id: Option<&str>,
    ) -> Result<()>;

    /// Read the row for `project`, or `None` if never populated.
    fn get_ssg(&self, project: &str) -> Result<Option<SsgRecord>>;

    /// Delete the row for `project`. Idempotent.
    fn clear_ssg(&self, project: &str) -> Result<()>;

    /// List every SSG row, ordered alphabetically by project.
    /// Used by `coast ssg ls` and for cross-project audits.
    fn list_ssgs(&self) -> Result<Vec<SsgRecord>>;

    // --- ssg_services ---

    /// Insert (or replace by `(project, service_name)`) a service row.
    fn upsert_ssg_service(&self, rec: &SsgServiceRecord) -> Result<()>;

    /// List every service row for `project`, ordered alphabetically by name.
    fn list_ssg_services(&self, project: &str) -> Result<Vec<SsgServiceRecord>>;

    /// Update the status column for one service under `project`.
    fn update_ssg_service_status(
        &self,
        project: &str,
        name: &str,
        status: &str,
    ) -> Result<()>;

    /// Remove every `ssg_services` row for `project`. Used when the
    /// project's SSG is removed or rebuilt from scratch.
    fn clear_ssg_services(&self, project: &str) -> Result<()>;

    // --- ssg_port_checkouts ---

    /// Insert (or replace by `(project, canonical_port)`) a checkout row.
    fn upsert_ssg_port_checkout(&self, rec: &SsgPortCheckoutRecord) -> Result<()>;

    /// List every checkout row for `project`, ordered by `canonical_port` ascending.
    fn list_ssg_port_checkouts(
        &self,
        project: &str,
    ) -> Result<Vec<SsgPortCheckoutRecord>>;

    /// Delete the checkout row for `(project, canonical_port)`, if any. Idempotent.
    fn delete_ssg_port_checkout(
        &self,
        project: &str,
        canonical_port: u16,
    ) -> Result<()>;

    /// Update just the `socat_pid` column for a checkout row (Phase
    /// 6). Used by `coast ssg stop` to null the PID after killing the
    /// socat while preserving the row, and by `run / start` to record
    /// the fresh PID after re-spawning against a new dynamic port.
    fn update_ssg_port_checkout_socat_pid(
        &self,
        project: &str,
        canonical_port: u16,
        socat_pid: Option<i32>,
    ) -> Result<()>;

    /// Delete every checkout row for `project`. Phase 6 uses this
    /// from `coast ssg rm` (destructive — user explicitly removed
    /// the SSG, so stale checkouts must go).
    fn clear_ssg_port_checkouts(&self, project: &str) -> Result<()>;

    // --- ssg_consumer_pins (Phase 16) ---

    /// Insert (or replace by `project`) a pin row.
    fn upsert_ssg_consumer_pin(&self, rec: &SsgConsumerPinRecord) -> Result<()>;

    /// Read the pin row for `project`, or `None` if not pinned.
    fn get_ssg_consumer_pin(&self, project: &str) -> Result<Option<SsgConsumerPinRecord>>;

    /// Delete the pin row for `project`, if any. Returns `true` when
    /// a row existed, `false` when the call was a no-op. Idempotent.
    fn delete_ssg_consumer_pin(&self, project: &str) -> Result<bool>;

    /// List every pin row, ordered alphabetically by `project`. Used
    /// by `auto_prune_preserving` to enumerate build_ids that must
    /// survive a prune pass.
    fn list_ssg_consumer_pins(&self) -> Result<Vec<SsgConsumerPinRecord>>;
}
