use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Volume isolation strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeStrategy {
    Isolated,
    Shared,
}

impl VolumeStrategy {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "isolated" => Some(Self::Isolated),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// Configuration for a volume declared in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeConfig {
    pub name: String,
    pub strategy: VolumeStrategy,
    pub service: String,
    pub mount: PathBuf,
    pub snapshot_source: Option<String>,
}

/// One published port on a shared service.
///
/// Phase 28 renamed `host_port` to `forwarding_port` to reflect what
/// the value actually means: it's the port a consumer's in-DinD
/// socat connects to via `host.docker.internal:<forwarding_port>`,
/// not literally a "Docker host publish port" anymore. After
/// Phase 28's host-socat supervisor lands, `forwarding_port` is the
/// stable virtual port that fronts the SSG's ephemeral dyn port; the
/// SSG itself can rebuild without consumers ever observing a change
/// in this value.
///
/// The serialized name stays `host_port` (`#[serde(rename)]`) so
/// existing artifact manifests, snapshots, and protocol payloads
/// continue to round-trip without a breaking wire change. TOML
/// Coastfile syntax is unaffected — `host_port`/`container_port`
/// never appeared as struct field names there; the parser
/// constructs a `SharedServicePort` from a `"host:container"` string
/// (see `coast-core/src/coastfile/field_parsers.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedServicePort {
    #[serde(rename = "host_port")]
    pub forwarding_port: u16,
    pub container_port: u16,
}

impl SharedServicePort {
    pub const fn same(port: u16) -> Self {
        Self {
            forwarding_port: port,
            container_port: port,
        }
    }

    pub const fn new(forwarding_port: u16, container_port: u16) -> Self {
        Self {
            forwarding_port,
            container_port,
        }
    }

    pub const fn is_identity_mapping(&self) -> bool {
        self.forwarding_port == self.container_port
    }
}

impl fmt::Display for SharedServicePort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_identity_mapping() {
            write!(f, "{}", self.forwarding_port)
        } else {
            write!(f, "{}:{}", self.forwarding_port, self.container_port)
        }
    }
}

/// Configuration for a shared service in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedServiceConfig {
    pub name: String,
    pub image: String,
    pub ports: Vec<SharedServicePort>,
    pub volumes: Vec<String>,
    pub env: HashMap<String, String>,
    pub auto_create_db: bool,
    pub inject: Option<InjectType>,
}

/// Reference to a service defined in the Shared Service Group.
///
/// A consumer Coastfile opts into an SSG-owned service with
/// `[shared_services.<name>] from_group = true`. The image, ports,
/// env, and volumes come from the active SSG build; only per-project
/// overrides live here. See `coast-ssg/DESIGN.md §6`.
///
/// This sits alongside `SharedServiceConfig` in the parsed Coastfile
/// (as `Coastfile.shared_service_group_refs`) rather than replacing it,
/// so existing call sites that iterate `cf.shared_services` continue to
/// see only inline host-daemon-spawned services — which is the only
/// correct interpretation for those callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedServiceGroupRef {
    /// Service name. Must match a service in the active SSG build
    /// (validated at consumer `coast run` time, not at parse time).
    pub name: String,
    /// Override for the SSG service's `auto_create_db`. `None` means
    /// inherit whatever the SSG service declares; `Some(true)`
    /// enables and `Some(false)` explicitly disables (even when the
    /// SSG service enables it). Three-valued semantics per
    /// DESIGN.md §6.
    pub auto_create_db: Option<bool>,
    /// Per-project inject target. The SSG Coastfile itself does not
    /// set `inject` (it is project-local by definition), so each
    /// consumer decides its own env var / file path.
    pub inject: Option<InjectType>,
}

/// Configuration for a secret in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretConfig {
    pub name: String,
    pub extractor: String,
    pub params: HashMap<String, String>,
    pub inject: InjectType,
    pub ttl: Option<String>,
}

/// How a secret or connection detail is injected into a coast container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectType {
    Env(String),
    File(PathBuf),
}

impl InjectType {
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(var) = s.strip_prefix("env:") {
            if var.is_empty() {
                return Err(
                    "inject env target cannot be empty. Use format: env:VAR_NAME".to_string(),
                );
            }
            Ok(Self::Env(var.to_string()))
        } else if let Some(path) = s.strip_prefix("file:") {
            if path.is_empty() {
                return Err(
                    "inject file target cannot be empty. Use format: file:/path/in/container"
                        .to_string(),
                );
            }
            Ok(Self::File(PathBuf::from(path)))
        } else {
            Err(format!(
                "invalid inject format '{}'. Expected 'env:VAR_NAME' or 'file:/path/in/container'",
                s
            ))
        }
    }

    pub fn to_inject_string(&self) -> String {
        match self {
            Self::Env(var) => format!("env:{var}"),
            Self::File(path) => format!("file:{}", path.display()),
        }
    }
}
