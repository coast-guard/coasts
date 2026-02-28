use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Git repository info for a project.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectGitResponse {
    pub is_git_repo: bool,
    pub current_branch: Option<String>,
    pub local_branches: Vec<String>,
    #[serde(default)]
    pub worktrees: Vec<String>,
}

/// Response for GET/POST /settings.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SettingResponse {
    pub key: String,
    pub value: String,
}

/// Summary of a Docker image inside a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImageSummary {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub created: String,
    pub size: String,
}

/// Summary of a Docker volume inside a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VolumeSummaryResponse {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub scope: String,
    pub labels: String,
}

/// Summary of shared services grouped by project.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProjectSharedSummary {
    pub project: String,
    pub total: usize,
    pub running: usize,
}

/// A file/directory entry from the coast container filesystem.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FileEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub size: u64,
}

/// A grep match result from searching inside a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GrepMatch {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Git file status entry from inside a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,
}

/// Live container resource stats (CPU, memory, disk, network).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ContainerStats {
    pub timestamp: String,
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub pids: u64,
}

/// Host terminal session info.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SessionInfo {
    pub id: String,
    pub project: String,
    pub title: Option<String>,
}

/// Interactive exec session info for a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ExecSessionInfo {
    pub id: String,
    pub project: String,
    pub name: String,
    pub title: Option<String>,
    pub agent_shell_id: Option<i64>,
    pub is_active_agent: Option<bool>,
}

/// Interactive exec session info for an inner compose service.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ServiceExecSessionInfo {
    pub id: String,
    pub project: String,
    pub name: String,
    pub service: String,
    pub title: Option<String>,
}

/// Interactive exec session info for a host-side shared service.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HostServiceSessionInfo {
    pub id: String,
    pub project: String,
    pub service: String,
    pub title: Option<String>,
}

/// Whether an agent shell is available for a coast instance.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentShellAvailableResponse {
    pub available: bool,
}

/// Response after spawning a new agent shell.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SpawnAgentShellResponse {
    pub session_id: String,
    pub agent_shell_id: i64,
    pub is_active_agent: bool,
    pub title: Option<String>,
}

/// Response after activating an agent shell.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ActivateAgentShellResponse {
    pub shell_id: i64,
    pub is_active_agent: bool,
}

/// Response after closing an agent shell.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CloseAgentShellResponse {
    pub shell_id: i64,
    pub closed: bool,
}

/// Response for revealing a secret value.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct RevealSecretResponse {
    pub name: String,
    pub value: String,
}

/// Response for Docker image inspect on the host daemon.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ImageInspectResponse {
    pub inspect: Value,
    pub containers: Vec<Value>,
}

/// Response for Docker volume inspect on the host daemon.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct VolumeInspectResponse {
    pub inspect: Value,
    pub containers: Vec<Value>,
    pub coastfile: Option<Value>,
}

/// Response for shared services grouped by project.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SharedAllResponse {
    pub projects: Vec<ProjectSharedSummary>,
}

/// Response for reading a file inside a coast container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FileReadResponse {
    pub content: String,
    pub path: String,
    pub mime: String,
}

/// Simple success response.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SuccessResponse {
    pub success: bool,
}

/// Response for uploading a file.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UploadResponse {
    pub path: String,
}

/// Response for getting a setting value.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GetSettingResponse {
    pub key: String,
    pub value: Option<String>,
}

/// Request to set the display language.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetLanguageRequest {
    pub language: String,
}

/// Response confirming the language was set.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetLanguageResponse {
    pub language: String,
}

/// Response returning the current language.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GetLanguageResponse {
    pub language: String,
}

/// The action to perform on the analytics setting.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum AnalyticsAction {
    Enable,
    Disable,
    Status,
}

/// Request to change or query the analytics setting.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetAnalyticsRequest {
    pub action: AnalyticsAction,
}

/// Response confirming the analytics setting was changed (or queried).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetAnalyticsResponse {
    pub enabled: bool,
}

/// Response returning the current analytics setting (HTTP GET).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct GetAnalyticsResponse {
    pub enabled: bool,
}

/// Response for listing available Coastfile types.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CoastfileTypesResponse {
    pub project: String,
    pub types: Vec<String>,
}

/// Response for clearing logs.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ClearLogsResponse {
    pub cleared: bool,
}

/// Response for inspecting a service container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ServiceInspectResponse {
    pub inspect: Value,
}

/// Response for inspecting a host-side service container.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HostServiceInspectResponse {
    pub inspect: Value,
}

/// Response for inspecting a host-side Docker image.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct HostImageInspectResponse {
    pub inspect: Value,
}

/// Sent server-to-client on PTY WebSocket connect.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TerminalSessionInit {
    pub session_id: String,
}

/// Docker system info (total memory, CPUs, version).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DockerInfoResponse {
    pub mem_total_bytes: u64,
    pub cpus: u64,
    pub os: String,
    pub server_version: String,
    pub can_adjust: bool,
}

/// Response after requesting Docker Desktop settings to be opened.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OpenDockerSettingsResponse {
    pub success: bool,
}

/// Client-to-server resize command (sent after 0x01 prefix byte).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TerminalResize {
    pub cols: u16,
    pub rows: u16,
}
