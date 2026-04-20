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
    Stop,
    /// Stop + start.
    Restart,
    /// Remove the SSG container. `with_data = true` also removes inner named volumes.
    Rm { with_data: bool },
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
}

/// Response for SSG operations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SsgResponse {
    pub message: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub services: Vec<SsgServiceInfo>,
    #[serde(default)]
    pub ports: Vec<SsgPortInfo>,
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
