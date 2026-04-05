use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::RuntimeType;

/// A running or stopped coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CoastInstance {
    pub name: String,
    pub status: InstanceStatus,
    pub project: String,
    pub branch: Option<String>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    pub container_id: Option<String>,
    pub runtime: RuntimeType,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub worktree_name: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub coastfile_type: Option<String>,
    /// Name of the remote where this instance is running (None = local).
    #[serde(default)]
    pub remote_name: Option<String>,
}

/// Status of a coast instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    Enqueued,
    Provisioning,
    Assigning,
    Unassigning,
    Starting,
    Stopping,
    Running,
    Stopped,
    CheckedOut,
    Idle,
}

impl InstanceStatus {
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "enqueued" => Some(Self::Enqueued),
            "provisioning" => Some(Self::Provisioning),
            "assigning" => Some(Self::Assigning),
            "unassigning" => Some(Self::Unassigning),
            "starting" => Some(Self::Starting),
            "stopping" => Some(Self::Stopping),
            "running" => Some(Self::Running),
            "stopped" => Some(Self::Stopped),
            "checked_out" => Some(Self::CheckedOut),
            "idle" => Some(Self::Idle),
            _ => None,
        }
    }

    pub fn as_db_str(&self) -> &'static str {
        match self {
            Self::Enqueued => "enqueued",
            Self::Provisioning => "provisioning",
            Self::Assigning => "assigning",
            Self::Unassigning => "unassigning",
            Self::Starting => "starting",
            Self::Stopping => "stopping",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::CheckedOut => "checked_out",
            Self::Idle => "idle",
        }
    }

    pub fn can_assign(&self) -> bool {
        matches!(
            self,
            Self::Running | Self::Idle | Self::CheckedOut | Self::Assigning | Self::Unassigning
        )
    }
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

/// A port mapping from logical name to port numbers.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortMapping {
    pub logical_name: String,
    pub canonical_port: u16,
    pub dynamic_port: u16,
    #[serde(default)]
    pub is_primary: bool,
}

/// Health status of a port (result of TCP probe).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortHealthStatus {
    pub logical_name: String,
    pub canonical_port: u16,
    pub dynamic_port: u16,
    pub is_primary: bool,
    pub healthy: bool,
}
