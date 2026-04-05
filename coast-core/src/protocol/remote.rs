//! Remote VM management protocol types.
//!
//! Request/response types for managing remote VMs, SSH tunnels, and sync sessions.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Remote VM Management
// ---------------------------------------------------------------------------

/// Request to add a new remote VM configuration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteAddRequest {
    /// Unique name for this remote (e.g., "staging", "dev-vm")
    pub name: String,
    /// Hostname or IP address
    pub host: String,
    /// SSH username
    pub user: String,
    /// SSH port (default: 22)
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Root directory for coast workspaces on the remote
    pub workspace_root: String,
    /// Path to SSH private key (optional)
    pub ssh_key_path: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// Response after adding a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteAddResponse {
    /// The name of the added remote
    pub name: String,
    /// Success message
    pub message: String,
}

/// Request to remove a remote VM configuration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteRemoveRequest {
    /// Name of the remote to remove
    pub name: String,
}

/// Response after removing a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteRemoveResponse {
    /// Whether the remote was found and removed
    pub removed: bool,
    /// Message describing the result
    pub message: String,
}

/// Request to list all configured remotes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteListRequest {}

/// Information about a configured remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteInfo {
    /// Unique name for this remote
    pub name: String,
    /// Hostname or IP address
    pub host: String,
    /// SSH username
    pub user: String,
    /// SSH port
    pub port: u16,
    /// Root directory for coast workspaces on the remote
    pub workspace_root: String,
    /// Path to SSH private key (if specified)
    pub ssh_key_path: Option<String>,
    /// Current tunnel status (if any)
    pub tunnel_status: Option<String>,
    /// Number of projects using this remote
    pub project_count: u32,
}

/// Response listing all configured remotes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteListResponse {
    /// List of configured remotes
    pub remotes: Vec<RemoteInfo>,
}

/// Request to setup coastd on a remote VM.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteSetupRequest {
    /// Name of the remote to setup
    pub name: String,
    /// Force reinstall even if coastd is already present
    #[serde(default)]
    pub force: bool,
}

/// Response after setting up a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteSetupResponse {
    /// Whether setup was successful
    pub success: bool,
    /// coastd version installed on remote
    pub version: Option<String>,
    /// Message describing the result
    pub message: String,
}

/// Request to ping/test connection to a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemotePingRequest {
    /// Name of the remote to ping
    pub name: String,
}

/// Response from pinging a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemotePingResponse {
    /// Whether the remote is reachable
    pub reachable: bool,
    /// Whether SSH connection succeeded
    pub ssh_ok: bool,
    /// Whether coastd is running on the remote
    pub daemon_ok: bool,
    /// coastd version on remote (if daemon_ok)
    pub daemon_version: Option<String>,
    /// Round-trip latency in milliseconds
    pub latency_ms: Option<u64>,
    /// Error message if not reachable
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Tunnel Management
// ---------------------------------------------------------------------------

/// Request to connect (establish tunnel) to a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteConnectRequest {
    /// Name of the remote to connect to
    pub name: String,
}

/// Response after connecting to a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteConnectResponse {
    /// Whether connection was established
    pub connected: bool,
    /// Local port the tunnel is bound to
    pub local_port: Option<u16>,
    /// Message describing the result
    pub message: String,
}

/// Request to disconnect from a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteDisconnectRequest {
    /// Name of the remote to disconnect from
    pub name: String,
}

/// Response after disconnecting from a remote.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteDisconnectResponse {
    /// Whether disconnection was successful
    pub disconnected: bool,
    /// Message describing the result
    pub message: String,
}

// ---------------------------------------------------------------------------
// Sync Management
// ---------------------------------------------------------------------------

/// Request to get sync status.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncStatusRequest {
    /// Project name (optional, if not specified returns all)
    pub project: Option<String>,
}

/// Information about a sync session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncSessionInfo {
    /// Project name
    pub project: String,
    /// Remote this session syncs to
    pub remote_name: String,
    /// Local path being synced
    pub local_path: String,
    /// Remote path being synced to
    pub remote_path: String,
    /// Current sync status
    pub status: String,
    /// Last successful sync time (ISO 8601)
    pub last_sync_at: Option<String>,
}

/// Response with sync status.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncStatusResponse {
    /// List of sync sessions
    pub sessions: Vec<SyncSessionInfo>,
}

/// Request to pause sync for a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncPauseRequest {
    /// Project name
    pub project: String,
}

/// Response after pausing sync.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncPauseResponse {
    /// Whether pause was successful
    pub paused: bool,
    /// Message describing the result
    pub message: String,
}

/// Request to resume sync for a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncResumeRequest {
    /// Project name
    pub project: String,
}

/// Response after resuming sync.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncResumeResponse {
    /// Whether resume was successful
    pub resumed: bool,
    /// Message describing the result
    pub message: String,
}

/// Request to flush pending sync changes for a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncFlushRequest {
    /// Project name
    pub project: String,
}

/// Response after flushing sync.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncFlushResponse {
    /// Whether flush was successful
    pub flushed: bool,
    /// Message describing the result
    pub message: String,
}

/// Request to create a new sync session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncCreateRequest {
    /// Project name
    pub project: String,
    /// Branch/worktree name
    pub branch: String,
    /// Remote name to sync to
    pub remote_name: String,
    /// Local path to sync (optional, defaults to project worktree)
    pub local_path: Option<String>,
}

/// Response after creating sync session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncCreateResponse {
    /// Whether creation was successful
    pub created: bool,
    /// Session name
    pub session_name: Option<String>,
    /// Message describing the result
    pub message: String,
}

/// Request to terminate a sync session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncTerminateRequest {
    /// Project name
    pub project: String,
}

/// Response after terminating sync session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SyncTerminateResponse {
    /// Whether termination was successful
    pub terminated: bool,
    /// Message describing the result
    pub message: String,
}

// ---------------------------------------------------------------------------
// Unified Remote Request/Response
// ---------------------------------------------------------------------------

/// All remote-related requests.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum RemoteRequest {
    /// Add a new remote configuration
    Add(RemoteAddRequest),
    /// Remove a remote configuration
    Remove(RemoteRemoveRequest),
    /// List all remotes
    List(RemoteListRequest),
    /// Setup coastd on a remote
    Setup(RemoteSetupRequest),
    /// Ping/test a remote connection
    Ping(RemotePingRequest),
    /// Connect to a remote (establish tunnel)
    Connect(RemoteConnectRequest),
    /// Disconnect from a remote
    Disconnect(RemoteDisconnectRequest),
}

/// All remote-related responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum RemoteResponse {
    /// Response to Add request
    Add(RemoteAddResponse),
    /// Response to Remove request
    Remove(RemoteRemoveResponse),
    /// Response to List request
    List(RemoteListResponse),
    /// Response to Setup request
    Setup(RemoteSetupResponse),
    /// Response to Ping request
    Ping(RemotePingResponse),
    /// Response to Connect request
    Connect(RemoteConnectResponse),
    /// Response to Disconnect request
    Disconnect(RemoteDisconnectResponse),
}

/// All sync-related requests.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SyncRequest {
    /// Get sync status
    Status(SyncStatusRequest),
    /// Pause sync for a project
    Pause(SyncPauseRequest),
    /// Resume sync for a project
    Resume(SyncResumeRequest),
    /// Flush pending sync changes
    Flush(SyncFlushRequest),
    /// Create a new sync session
    Create(SyncCreateRequest),
    /// Terminate a sync session
    Terminate(SyncTerminateRequest),
}

/// All sync-related responses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SyncResponse {
    /// Response to Status request
    Status(SyncStatusResponse),
    /// Response to Pause request
    Pause(SyncPauseResponse),
    /// Response to Resume request
    Resume(SyncResumeResponse),
    /// Response to Flush request
    Flush(SyncFlushResponse),
    /// Response to Create request
    Create(SyncCreateResponse),
    /// Response to Terminate request
    Terminate(SyncTerminateResponse),
}
