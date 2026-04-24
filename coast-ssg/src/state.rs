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
    /// One of: `built`, `created`, `running`, `stopped`.
    ///
    /// Phase 23 introduces `built`: set by `ssg build` when creating
    /// the row before a container has ever been started. Gets
    /// overwritten on the first `ssg run`.
    pub status: String,
    /// The build the running container was started on. `None` until
    /// the first `ssg run`; set by `ssg run`/`start`. Distinct from
    /// [`latest_build_id`] which tracks the most recent `ssg build`.
    pub build_id: Option<String>,
    /// Phase 23: most recent `ssg build` output for this project.
    ///
    /// Written by `coast ssg build`; read by the consumer resolver
    /// (`ensure_ready_for_consumer`) in place of the global
    /// `~/.coast/ssg/latest` symlink. `None` before the first
    /// `ssg build`. See `coast-ssg/DESIGN.md §23`.
    pub latest_build_id: Option<String>,
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

/// Phase 26 / 28 (§24.5): host-owned virtual port for an SSG service
/// port.
///
/// Stable per `(project, service_name, container_port)` — allocated
/// once (by `virtual_port_allocator::allocate_or_reuse`) and
/// preserved across `ssg build`/`rm`/`run` cycles. Dropped only on
/// `ssg rm --with-data` (data + identity both gone).
///
/// Phase 28: per-port keying replaces Phase 26's per-service keying
/// so multi-port services (e.g. minio 9000+9001) get one stable
/// virtual port per `ssg_services` row.
///
/// Lives in its own `ssg_virtual_ports` table — NOT on
/// `ssg_services`, because the latter is wiped-and-reinserted on
/// every `ssg run` by `lifecycle.rs::apply_to_state_and_response`.
/// Same identity-vs-lifecycle-scope split as `ssg_consumer_pins`
/// vs. `ssg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgVirtualPortRecord {
    pub project: String,
    pub service_name: String,
    pub container_port: u16,
    pub port: u16,
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

    /// Phase 23: set the project's `latest_build_id` to the given
    /// value. Used by `coast ssg build` to record the just-produced
    /// artifact. Creates the row with `status = "built"` when absent;
    /// when the row already exists (e.g. SSG is running), only the
    /// `latest_build_id` column is updated — `container_id`,
    /// `build_id`, `status`, and `created_at` are preserved so a
    /// running SSG stays running even after a rebuild.
    fn set_latest_build_id(&self, project: &str, build_id: &str) -> Result<()>;

    // --- ssg_services ---

    /// Insert (or replace by `(project, service_name)`) a service row.
    fn upsert_ssg_service(&self, rec: &SsgServiceRecord) -> Result<()>;

    /// List every service row for `project`, ordered alphabetically by name.
    fn list_ssg_services(&self, project: &str) -> Result<Vec<SsgServiceRecord>>;

    /// Update the status column for one service under `project`.
    fn update_ssg_service_status(&self, project: &str, name: &str, status: &str) -> Result<()>;

    /// Remove every `ssg_services` row for `project`. Used when the
    /// project's SSG is removed or rebuilt from scratch.
    fn clear_ssg_services(&self, project: &str) -> Result<()>;

    // --- ssg_port_checkouts ---

    /// Insert (or replace by `(project, canonical_port)`) a checkout row.
    fn upsert_ssg_port_checkout(&self, rec: &SsgPortCheckoutRecord) -> Result<()>;

    /// List every checkout row for `project`, ordered by `canonical_port` ascending.
    fn list_ssg_port_checkouts(&self, project: &str) -> Result<Vec<SsgPortCheckoutRecord>>;

    /// Delete the checkout row for `(project, canonical_port)`, if any. Idempotent.
    fn delete_ssg_port_checkout(&self, project: &str, canonical_port: u16) -> Result<()>;

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

    // --- ssg_virtual_ports (Phase 26 / §24.5; per-port keying — Phase 28) ---

    /// Read the persisted virtual port for
    /// `(project, service_name, container_port)`, or `None` if never
    /// allocated. Used by the allocator's reuse path and by Phase 28
    /// consumer provisioning to look up the forwarding target.
    fn get_ssg_virtual_port(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
    ) -> Result<Option<u16>>;

    /// Insert (or replace by `(project, service_name, container_port)`)
    /// a virtual-port row. Replace semantics are appropriate because
    /// the allocator may re-bind to a new port after a
    /// collision-recovery pass.
    fn upsert_ssg_virtual_port(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
        port: u16,
    ) -> Result<()>;

    /// List every virtual-port row for `project`, ordered by
    /// `(service_name, container_port)`. Also used by the allocator
    /// to avoid reusing a port that is already held by another
    /// service on the same host.
    fn list_ssg_virtual_ports(&self, project: &str) -> Result<Vec<SsgVirtualPortRecord>>;

    /// Delete every virtual-port row for `project`. Called by
    /// `ssg rm --with-data` — identity is gone, let the ports be
    /// re-used. Idempotent.
    fn clear_ssg_virtual_ports(&self, project: &str) -> Result<()>;

    /// Delete the single virtual-port row for
    /// `(project, service_name, container_port)`. Used by the
    /// collision-rebind path when a persisted virtual port has been
    /// claimed outside Coast and the allocator must pick a fresh one.
    /// Idempotent.
    fn clear_ssg_virtual_port_one(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
    ) -> Result<()>;

    /// Return the set of virtual ports already assigned to ANY
    /// `(project, service_name, container_port)` triple. Used by the
    /// allocator to avoid handing the same virtual port to two
    /// services across different projects. Unordered.
    fn list_all_ssg_virtual_port_numbers(&self) -> Result<Vec<u16>>;
}
