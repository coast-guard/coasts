//! Protocol types for per-project Shared Service Groups.
//!
//! Phase: ssg-phase-20 (per-project correction). See
//! `coast-ssg/DESIGN.md §23`.
//!
//! Every SSG request carries the consumer `project` name (from the
//! sibling `Coastfile`'s `[coast] name`) at the top level so the
//! daemon can route to the correct per-project SSG. The per-verb
//! payload lives in [`SsgAction`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to manage a project's Shared Service Group.
///
/// `project` is the consumer project name (from `[coast].name` in the
/// sibling `Coastfile`). The daemon looks up state in the `ssg` /
/// `ssg_services` / `ssg_port_checkouts` tables filtered by this
/// project (`coast-ssg/DESIGN.md §23`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgRequest {
    pub project: String,
    pub action: SsgAction,
}

/// Per-verb payload for [`SsgRequest`].
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SsgAction {
    /// Build the SSG from `Coastfile.shared_service_groups`.
    Build {
        file: Option<PathBuf>,
        working_dir: Option<PathBuf>,
        config: Option<String>,
    },
    /// Create and start the project's SSG DinD container for the first time.
    Run,
    /// Start an existing but stopped SSG for this project.
    Start,
    /// Stop the project's SSG (its inner services stop with it).
    ///
    /// When `force = true`, proceed even if remote shadow coasts are
    /// currently consuming the SSG. The daemon tears down the reverse
    /// SSH tunnels it spawned on behalf of those shadows. Without
    /// `force`, the daemon refuses and lists the blocking shadows.
    /// See `coast-ssg/DESIGN.md §20.6`.
    Stop {
        #[serde(default)]
        force: bool,
    },
    /// Stop + start.
    Restart,
    /// Remove the project's SSG container. `with_data = true` also
    /// removes inner named volumes. `force = true` proceeds even if
    /// remote shadow coasts are consuming the SSG (same semantics as
    /// `Stop.force`).
    Rm {
        with_data: bool,
        #[serde(default)]
        force: bool,
    },
    /// Show SSG container status and per-service status for this project.
    Ps,
    /// Logs from the outer DinD container or a specific inner service.
    Logs {
        service: Option<String>,
        tail: Option<u32>,
        follow: bool,
    },
    /// Exec into the SSG container or a specific inner service.
    Exec {
        service: Option<String>,
        command: Vec<String>,
    },
    /// List per-service dynamic host ports.
    Ports,
    /// Bind canonical host ports via socat forwarders.
    ///
    /// `service = Some(name), all = false` binds one service. `service
    /// = None, all = true` binds every service. Other combinations are
    /// rejected by the handler (phase 6).
    Checkout { service: Option<String>, all: bool },
    /// Tear down a canonical-port checkout.
    Uncheckout { service: Option<String>, all: bool },
    /// Read-only permission check on host bind mounts for known images.
    ///
    /// Reads the active SSG's `manifest.json`, matches each service's
    /// image against a built-in known-image table, and reports host
    /// bind-mount directories whose owner UID/GID diverges from the
    /// image's expected value (e.g. postgres expects 999:999).
    /// Does not modify anything. See `coast-ssg/DESIGN.md §10.5`.
    Doctor,
    /// Pin this project's consumer coast to a specific SSG `build_id`.
    /// Drift checks and auto-start honor the pin. See `DESIGN.md §17-9`
    /// (SETTLED — Phase 16). The project comes from `SsgRequest.project`.
    CheckoutBuild {
        /// SSG build id to pin to. Must resolve to an on-disk build
        /// dir with a `manifest.json`; validated at pin time.
        build_id: String,
    },
    /// Clear the SSG build pin for this project. Idempotent.
    UncheckoutBuild,
    /// Show the current SSG build pin for this project (if any).
    ShowPin,
    /// List every per-project SSG known to the daemon. Cross-project
    /// verb: the `project` field on the enclosing [`SsgRequest`] is
    /// ignored (CLI sends an empty string). See `coast-ssg/DESIGN.md
    /// §23` — Phase 22.
    Ls,
    /// Zero-copy migration helper: resolve a host Docker named
    /// volume's mountpoint and emit (or apply) the equivalent SSG
    /// Coastfile bind-mount entry. See `DESIGN.md §10.7`.
    ImportHostVolume {
        /// Host Docker named volume name (must already exist).
        volume: String,
        /// Target `[shared_services.<name>]` section.
        service: String,
        /// Absolute container path to bind the volume mountpoint at.
        mount: PathBuf,
        /// SSG Coastfile discovery (same triplet as `Build`).
        #[serde(default)]
        file: Option<PathBuf>,
        #[serde(default)]
        working_dir: Option<PathBuf>,
        #[serde(default)]
        config: Option<String>,
        /// When `true`, rewrite the SSG Coastfile in place with a
        /// `.bak` backup. When `false`, print a TOML snippet to
        /// stdout. Rejected when combined with inline `config`.
        #[serde(default)]
        apply: bool,
    },
}

/// Response for SSG operations.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgResponse {
    pub message: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub services: Vec<SsgServiceInfo>,
    #[serde(default)]
    pub ports: Vec<SsgPortInfo>,
    /// Findings produced by `coast ssg doctor`. Empty for every other
    /// verb. `#[serde(default)]` keeps older clients forward-compatible.
    #[serde(default)]
    pub findings: Vec<SsgDoctorFinding>,
    /// Rows produced by `coast ssg ls`. Empty for every other verb.
    /// See `coast-ssg/DESIGN.md §23` — Phase 22.
    #[serde(default)]
    pub listings: Vec<SsgListing>,
}

/// One finding emitted by `coast ssg doctor`.
///
/// Severity is a lowercase string so the wire format stays stable even
/// if more severities are added. Known values: `ok`, `warn`, `info`.
/// See `coast-ssg/src/doctor.rs` for the evaluator.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgDoctorFinding {
    pub service: String,
    pub path: String,
    pub severity: String,
    pub message: String,
}

/// Info about one SSG-managed inner service.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgServiceInfo {
    pub name: String,
    pub image: String,
    pub inner_port: u16,
    pub dynamic_host_port: u16,
    #[serde(default)]
    pub container_id: Option<String>,
    pub status: String,
}

/// Per-service port info shown by `coast ssg ports`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgPortInfo {
    pub service: String,
    pub canonical_port: u16,
    pub dynamic_host_port: u16,
    pub checked_out: bool,
}

/// One row in the `coast ssg ls` output — metadata for a single
/// project's SSG. See `coast-ssg/DESIGN.md §23` (per-project SSG)
/// and Phase 22.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgListing {
    /// Consumer project name (`[coast].name` from the project's Coastfile).
    pub project: String,
    /// One of: `created`, `running`, `stopped`. Matches the `status`
    /// column of the `ssg` row for this project.
    pub status: String,
    /// The build id the SSG is currently wired to, if any.
    #[serde(default)]
    pub build_id: Option<String>,
    /// The outer DinD container id, if the SSG has been run at least once.
    #[serde(default)]
    pub container_id: Option<String>,
    /// Number of inner services registered for this project in
    /// `ssg_services`. Zero when the SSG has never been run.
    pub service_count: u32,
    /// RFC 3339 timestamp (chrono `to_rfc3339()`) of the `ssg` row.
    pub created_at: String,
}

/// Streaming chunk of log output for `coast ssg logs --follow`.
///
/// Plain struct wrapper around a single text payload. Exists because
/// `serde(tag = "type")` on the `Response` enum cannot serialize tuple
/// newtype variants holding primitives.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgLogChunk {
    pub chunk: String,
}
