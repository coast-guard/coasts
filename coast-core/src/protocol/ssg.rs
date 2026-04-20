//! Protocol types for the Shared Service Group singleton.
//!
//! Phase: ssg-phase-1. See `coast-ssg/DESIGN.md §7` for the full CLI
//! surface these map to and `§9` for lifecycle semantics.
//!
//! Mirrors the shape of [`super::secret_shared::SharedRequest`] /
//! [`super::secret_shared::SharedResponse`] — `#[serde(tag = "action")]`
//! tagged unions with `#[ts(export)]` for TypeScript client generation.
//!
//! No runtime handler is wired in this commit (phase 1 part 3/3). The
//! daemon dispatcher returns a structured "not yet implemented" error
//! until phase 2 lands the real handler.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to manage the singleton Shared Service Group.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SsgRequest {
    /// Build the SSG from `Coastfile.shared_service_groups`.
    Build {
        file: Option<PathBuf>,
        working_dir: Option<PathBuf>,
        config: Option<String>,
    },
    /// Create and start the singleton DinD container for the first time.
    Run,
    /// Start an existing but stopped SSG.
    Start,
    /// Stop the SSG (its inner services stop with it).
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
    /// Remove the SSG container. `with_data = true` also removes inner
    /// named volumes. `force = true` proceeds even if remote shadow
    /// coasts are consuming the SSG (same semantics as `Stop.force`).
    Rm {
        with_data: bool,
        #[serde(default)]
        force: bool,
    },
    /// Show SSG container status and per-service status.
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
