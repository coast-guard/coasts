/// CLI <-> daemon IPC protocol.
///
/// Defines request and response enums for every command,
/// serialized as JSON over the Unix domain socket.
///
/// Domain types are split across submodules:
/// - [`build`]: Build and extractor types + `BuildProgressEvent`
/// - [`instance`]: Run, Assign, Unassign, Rebuild, Stop, Start, Rm, RmBuild
/// - [`query`]: Archive, Checkout, Ports, Exec, Logs, Ps, Ls, Lookup
/// - [`secret_shared`]: Secret and Shared service types
/// - [`builds`]: Build inspection types
/// - [`mcp`]: MCP server/tool/location types
/// - [`agent_shell`]: Agent shell session types
/// - [`events`]: ErrorResponse and CoastEvent
/// - [`remote`]: Remote VM and sync management types
pub mod agent_shell;
pub mod api_types;
pub mod build;
pub mod builds;
pub mod events;
pub mod instance;
pub mod mcp;
pub mod query;
pub mod remote;
pub mod secret_shared;

#[cfg(test)]
mod tests;

pub use agent_shell::*;
pub use api_types::*;
pub use build::*;
pub use builds::*;
pub use events::*;
pub use instance::*;
pub use mcp::*;
pub use query::*;
pub use remote::*;
pub use secret_shared::*;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A request from the CLI to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum Request {
    /// Build a coast image from a Coastfile.
    Build(BuildRequest),
    /// Re-run secret extractors using the cached build Coastfile.
    RerunExtractors(RerunExtractorsRequest),
    /// Create and start a new coast instance.
    Run(RunRequest),
    /// Stop a running instance.
    Stop(StopRequest),
    /// Start a stopped instance.
    Start(StartRequest),
    /// Remove an instance.
    Rm(RmRequest),
    /// Check out an instance (bind canonical ports).
    Checkout(CheckoutRequest),
    /// Get port allocations for an instance.
    Ports(PortsRequest),
    /// Execute a command inside a coast container.
    Exec(ExecRequest),
    /// Stream logs from inside a coast container.
    Logs(LogsRequest),
    /// Get inner compose service status.
    Ps(PsRequest),
    /// List all instances.
    Ls(LsRequest),
    /// List docs tree or read a docs markdown path.
    Docs(DocsRequest),
    /// Search docs using hybrid semantic + keyword scoring.
    SearchDocs(SearchDocsRequest),
    /// Manage per-instance secrets.
    Secret(SecretRequest),
    /// Manage shared services.
    Shared(SharedRequest),
    /// Assign (or reassign) a branch to an existing coast instance.
    Assign(AssignRequest),
    /// Unassign a worktree, returning the instance to the repo's default branch.
    Unassign(UnassignRequest),
    /// Rebuild images inside DinD from the bind-mounted workspace.
    Rebuild(RebuildRequest),
    /// Restart all services inside a running coast instance.
    RestartServices(RestartServicesRequest),
    /// Remove a project build artifact and associated Docker resources.
    RmBuild(RmBuildRequest),
    /// Archive a project (stop instances/services, hide from main list).
    ArchiveProject(ArchiveProjectRequest),
    /// Unarchive a project (restore to main list).
    UnarchiveProject(UnarchiveProjectRequest),
    /// Inspect build artifacts.
    Builds(BuildsRequest),
    /// List MCP servers for an instance.
    McpLs(McpLsRequest),
    /// List tools for an MCP server.
    McpTools(McpToolsRequest),
    /// List MCP client connector locations.
    McpLocations(McpLocationsRequest),
    /// Manage agent shell sessions for a coast instance.
    AgentShell(AgentShellRequest),
    /// Set the display language for the daemon and CLI.
    SetLanguage(SetLanguageRequest),
    /// Change or query the analytics setting.
    SetAnalytics(SetAnalyticsRequest),
    /// Look up coast instances for the caller's current worktree.
    Lookup(LookupRequest),
    /// Query whether the daemon is in a safe state for self-update.
    IsSafeToUpdate(UpdateSafetyRequest),
    /// Drain mutating work and prepare the daemon for self-update.
    PrepareForUpdate(PrepareForUpdateRequest),
    /// Manage remote VM configurations.
    Remote(RemoteRequest),
    /// Manage file sync sessions.
    Sync(SyncRequest),
}

/// A response from the daemon to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "type")]
pub enum Response {
    /// Streaming build progress event (sent before the final Build response).
    BuildProgress(BuildProgressEvent),
    /// Build completed.
    Build(BuildResponse),
    /// Streaming re-run extractors progress event (sent before the final response).
    RerunExtractorsProgress(BuildProgressEvent),
    /// Re-run extractors completed.
    RerunExtractors(RerunExtractorsResponse),
    /// Streaming run progress event (sent before the final Run response).
    RunProgress(BuildProgressEvent),
    /// Streaming assign progress event (sent before the final Assign response).
    AssignProgress(BuildProgressEvent),
    /// Instance created and started.
    Run(RunResponse),
    /// Streaming stop progress event (sent before the final Stop response).
    StopProgress(BuildProgressEvent),
    /// Instance stopped.
    Stop(StopResponse),
    /// Streaming start progress event (sent before the final Start response).
    StartProgress(BuildProgressEvent),
    /// Instance started.
    Start(StartResponse),
    /// Instance removed.
    Rm(RmResponse),
    /// Checkout completed.
    Checkout(CheckoutResponse),
    /// Port allocations.
    Ports(PortsResponse),
    /// Exec output.
    Exec(ExecResponse),
    /// Streaming log output chunks (sent before final Logs response).
    LogsProgress(LogsResponse),
    /// Log output.
    Logs(LogsResponse),
    /// Service status.
    Ps(PsResponse),
    /// Instance list.
    Ls(LsResponse),
    /// Docs tree/content response.
    Docs(DocsResponse),
    /// Docs search results.
    SearchDocs(SearchDocsResponse),
    /// Secret operation result.
    Secret(SecretResponse),
    /// Shared service operation result.
    Shared(SharedResponse),
    /// Worktree assignment completed.
    Assign(AssignResponse),
    /// Explain-only analysis of what an assign would do.
    AssignExplain(AssignExplainResponse),
    /// Streaming unassign progress event (sent before the final Unassign response).
    UnassignProgress(BuildProgressEvent),
    /// Unassign completed (instance returned to default branch).
    Unassign(UnassignResponse),
    /// Rebuild completed.
    Rebuild(RebuildResponse),
    /// Services restart completed.
    RestartServices(RestartServicesResponse),
    /// Streaming rm-build progress event (sent before the final RmBuild response).
    RmBuildProgress(BuildProgressEvent),
    /// Build artifact removed.
    RmBuild(RmBuildResponse),
    /// Project archived.
    ArchiveProject(ArchiveProjectResponse),
    /// Project unarchived.
    UnarchiveProject(UnarchiveProjectResponse),
    /// Build inspection result.
    Builds(Box<BuildsResponse>),
    /// MCP server list.
    McpLs(McpLsResponse),
    /// MCP tools list.
    McpTools(McpToolsResponse),
    /// MCP client locations.
    McpLocations(McpLocationsResponse),
    /// Agent shell operation result.
    AgentShell(AgentShellResponse),
    /// Language set confirmation.
    SetLanguage(SetLanguageResponse),
    /// Analytics setting confirmation.
    SetAnalytics(SetAnalyticsResponse),
    /// Lookup result for the caller's current worktree.
    Lookup(LookupResponse),
    /// Structured self-update safety report.
    UpdateSafety(UpdateSafetyResponse),
    /// Result of preparing the daemon for self-update.
    PrepareForUpdate(PrepareForUpdateResponse),
    /// Remote VM management result.
    Remote(RemoteResponse),
    /// Sync management result.
    Sync(SyncResponse),
    /// Error response.
    Error(ErrorResponse),
}

/// Encode a request as a newline-terminated JSON string for sending over the socket.
pub fn encode_request(req: &Request) -> crate::error::Result<Vec<u8>> {
    let mut data = serde_json::to_vec(req)?;
    data.push(b'\n');
    Ok(data)
}

/// Decode a request from a JSON byte slice.
pub fn decode_request(data: &[u8]) -> crate::error::Result<Request> {
    let req = serde_json::from_slice(data)?;
    Ok(req)
}

/// Encode a response as a newline-terminated JSON string.
pub fn encode_response(resp: &Response) -> crate::error::Result<Vec<u8>> {
    let mut data = serde_json::to_vec(resp)?;
    data.push(b'\n');
    Ok(data)
}

/// Decode a response from a JSON byte slice.
pub fn decode_response(data: &[u8]) -> crate::error::Result<Response> {
    let resp = serde_json::from_slice(data)?;
    Ok(resp)
}
