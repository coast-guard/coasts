use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A registered remote machine that can run coast-service.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteEntry {
    /// User-chosen name for this remote (e.g. "my-vm").
    pub name: String,
    /// Remote host address (IP or hostname).
    pub host: String,
    /// SSH user on the remote host.
    pub user: String,
    /// SSH port on the remote host.
    pub port: u16,
    /// Path to the SSH private key (None = use SSH agent default).
    pub ssh_key: Option<String>,
    /// File sync strategy for /workspace.
    pub sync_strategy: String,
    /// When this remote was registered.
    pub created_at: String,
}

/// System stats snapshot from a remote machine.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteStats {
    pub total_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub cpu_count: u32,
    pub cpu_usage_percent: f32,
    pub total_disk_bytes: u64,
    pub used_disk_bytes: u64,
    pub service_version: Option<String>,
}

/// Response for remote stats endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteStatsResponse {
    pub stats: std::collections::HashMap<String, RemoteStats>,
}

/// Configuration for remote coast execution from the Coastfile.
///
/// Connection details (host, user, port, SSH key) are NOT stored here --
/// they come from registered remotes (`coast remote add`) at runtime.
/// The Coastfile only declares preferences like sync strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// File sync strategy for /workspace.
    pub workspace_sync: SyncStrategy,
}

/// Resolved remote connection details for SSH operations.
///
/// Constructed at runtime from a registered `RemoteEntry` plus
/// the Coastfile's `RemoteConfig` preferences.
#[derive(Debug, Clone)]
pub struct RemoteConnection {
    pub host: String,
    pub user: String,
    pub ssh_key: PathBuf,
    pub port: u16,
    pub workspace_sync: SyncStrategy,
}

impl RemoteConnection {
    pub fn from_entry(entry: &RemoteEntry, config: &RemoteConfig) -> Self {
        let ssh_key = entry
            .ssh_key
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/"))
                    .join(".ssh/id_rsa")
            });
        Self {
            host: entry.host.clone(),
            user: entry.user.clone(),
            ssh_key,
            port: entry.port,
            workspace_sync: config.workspace_sync.clone(),
        }
    }
}

/// Strategy for syncing workspace files to the remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SyncStrategy {
    #[default]
    Rsync,
    Mutagen,
}

impl SyncStrategy {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "rsync" => Some(Self::Rsync),
            "mutagen" => Some(Self::Mutagen),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rsync => "rsync",
            Self::Mutagen => "mutagen",
        }
    }
}

impl std::fmt::Display for SyncStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
