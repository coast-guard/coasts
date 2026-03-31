use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::{InstanceStatus, PortMapping, RuntimeType};

// --- Archive ---

/// Request to archive a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArchiveProjectRequest {
    /// Project name.
    pub project: String,
}

/// Response after archiving a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArchiveProjectResponse {
    /// Project name.
    pub project: String,
    /// Number of instances that were stopped.
    pub instances_stopped: usize,
    /// Number of shared services that were stopped.
    pub shared_services_stopped: usize,
}

/// Request to unarchive a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnarchiveProjectRequest {
    /// Project name.
    pub project: String,
}

/// Response after unarchiving a project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnarchiveProjectResponse {
    /// Project name.
    pub project: String,
}

// --- Checkout ---

/// Request to check out an instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckoutRequest {
    /// Instance name, or None to unbind all ports.
    pub name: Option<String>,
    /// Project name.
    pub project: String,
}

/// Response after a successful checkout.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CheckoutResponse {
    /// Instance that is now checked out (None if --none).
    pub checked_out: Option<String>,
    /// Canonical port mappings now active.
    pub ports: Vec<PortMapping>,
}

// --- Ports ---

/// Request for port-related operations on an instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum PortsRequest {
    /// List port allocations for an instance.
    List {
        /// Instance name.
        name: String,
        /// Project name.
        project: String,
    },
    /// Mark a service as the primary port for this instance.
    SetPrimary {
        /// Instance name.
        name: String,
        /// Project name.
        project: String,
        /// Logical service name to mark as primary.
        service: String,
    },
    /// Remove the primary port designation from this instance.
    UnsetPrimary {
        /// Instance name.
        name: String,
        /// Project name.
        project: String,
    },
}

/// Response with port allocations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortsResponse {
    /// Instance name.
    pub name: String,
    /// Port allocations.
    pub ports: Vec<PortMapping>,
    /// Human-readable message (populated for set/unset operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Subdomain host for dynamic ports (e.g., "dev-1.localhost"), if subdomain routing is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdomain_host: Option<String>,
}

// --- Exec ---

/// Request to exec into a coast container.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExecRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
    /// Optional compose service name to target instead of the coast container.
    #[serde(default)]
    pub service: Option<String>,
    /// When true, bypass host UID:GID mapping and run as container root/default.
    #[serde(default)]
    pub root: bool,
    /// Command to run (default: ["sh"]).
    pub command: Vec<String>,
}

/// Response from exec (for non-streaming).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExecResponse {
    /// Exit code.
    pub exit_code: i32,
    /// Stdout output.
    pub stdout: String,
    /// Stderr output.
    pub stderr: String,
}

// --- Logs ---

/// Request to stream logs from a coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogsRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
    /// Optional service name to filter.
    #[serde(default)]
    pub service: Option<String>,
    /// Optional number of lines to tail.
    #[serde(default)]
    pub tail: Option<u32>,
    /// Whether to tail all available lines (equivalent to `--tail all`).
    #[serde(default)]
    pub tail_all: bool,
    /// Whether to follow (tail -f).
    #[serde(default)]
    pub follow: bool,
}

/// Response with log output.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogsResponse {
    /// Log output.
    pub output: String,
}

// --- Ps ---

/// Request to get inner compose service status.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PsRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// A single inner service status.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServiceStatus {
    /// Service name.
    pub name: String,
    /// Container status.
    pub status: String,
    /// Exposed ports.
    pub ports: String,
    /// Docker image.
    pub image: String,
    /// Service kind: "compose" for Docker Compose services, "bare" for bare process services.
    #[serde(default)]
    pub kind: Option<String>,
}

/// Response with inner service statuses.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PsResponse {
    /// Instance name.
    pub name: String,
    /// Inner service statuses.
    pub services: Vec<ServiceStatus>,
}

// --- Ls ---

/// Request to list all instances.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LsRequest {
    /// Optional project filter.
    pub project: Option<String>,
}

/// A summary of an instance for listing.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstanceSummary {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
    /// Current status.
    pub status: InstanceStatus,
    /// Branch name.
    pub branch: Option<String>,
    /// Runtime type.
    pub runtime: RuntimeType,
    /// Whether this instance is currently checked out.
    pub checked_out: bool,
    /// Project root directory (from manifest.json).
    pub project_root: Option<String>,
    /// Git worktree name if assigned (None = on project root).
    pub worktree: Option<String>,
    /// Build ID this instance was created from.
    #[serde(default)]
    pub build_id: Option<String>,
    /// Coastfile type (None = "default", Some("light") = Coastfile.light).
    #[serde(default)]
    pub coastfile_type: Option<String>,
    /// Number of port allocations for this instance.
    #[serde(default)]
    pub port_count: u32,
    /// Primary port service name (from build settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_port_service: Option<String>,
    /// Canonical port of the primary service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_port_canonical: Option<u16>,
    /// Dynamic port of the primary service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_port_dynamic: Option<u16>,
    /// Fully resolved URL for the primary service (with subdomain routing applied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_port_url: Option<String>,
    /// Number of inner services that are not in "running" state.
    #[serde(default)]
    pub down_service_count: u32,
    /// Name of the remote where this instance is running (None = local).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_name: Option<String>,
}

/// A project that has been built (has an image artifact in ~/.coast/images/).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KnownProject {
    /// Project name (directory name under ~/.coast/images/).
    pub name: String,
    /// Project root directory from manifest.json.
    pub project_root: Option<String>,
    /// Whether this project is archived.
    #[serde(default)]
    pub archived: bool,
}

/// Response with instance list.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LsResponse {
    /// All instances.
    pub instances: Vec<InstanceSummary>,
    /// Built projects discovered from ~/.coast/images/.
    #[serde(default)]
    pub known_projects: Vec<KnownProject>,
}

// --- Docs ---

/// Request to browse docs or read a specific markdown file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocsRequest {
    /// Optional docs path. If omitted, returns only the docs tree.
    ///
    /// Paths are relative to the locale root. Examples:
    /// - `GETTING_STARTED`
    /// - `coastfiles`
    /// - `coastfiles/COASTFILE.md`
    #[serde(default)]
    pub path: Option<String>,
    /// Optional language override. Falls back to daemon language when omitted.
    #[serde(default)]
    pub language: Option<String>,
}

/// Kind of node in the docs tree.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum DocsNodeKind {
    File,
    Dir,
}

/// A node in the docs tree.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocsNode {
    /// Display name (file or directory name).
    pub name: String,
    /// Relative path from locale root.
    pub path: String,
    /// File or directory.
    pub kind: DocsNodeKind,
    /// Nested children for directories.
    #[serde(default)]
    pub children: Vec<DocsNode>,
}

/// Response for docs tree/content requests.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DocsResponse {
    /// Resolved locale used for this response.
    pub locale: String,
    /// Full docs tree for the locale.
    pub tree: Vec<DocsNode>,
    /// Resolved markdown path, when content was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Markdown file content, when a path was requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// --- Search Docs ---

/// Request to search docs using hybrid semantic + keyword scoring.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchDocsRequest {
    /// Search query.
    pub query: String,
    /// Optional max number of results. Defaults to 10 when omitted.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Optional language override. Falls back to daemon language when omitted.
    #[serde(default)]
    pub language: Option<String>,
}

/// A single docs search result.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchDocsResult {
    /// Relative markdown path for the matched section.
    pub path: String,
    /// Resolved route path used by UI/links.
    pub route: String,
    /// Section heading.
    pub heading: String,
    /// Matched snippet.
    pub snippet: String,
    /// Combined ranking score.
    pub score: f64,
}

/// Response for docs search.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SearchDocsResponse {
    /// Original query.
    pub query: String,
    /// Resolved locale used for this response.
    pub locale: String,
    /// Search strategy label (e.g. "hybrid_keyword_semantic").
    pub strategy: String,
    /// Ranked results.
    pub results: Vec<SearchDocsResult>,
}

// --- Lookup ---

/// Request to look up coast instances for the caller's current worktree.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LookupRequest {
    /// Project name.
    pub project: String,
    /// Worktree the caller is inside, or None if on the project root.
    pub worktree: Option<String>,
}

/// A single instance returned by a lookup query.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LookupInstance {
    /// Instance name.
    pub name: String,
    /// Current status.
    pub status: InstanceStatus,
    /// Whether this instance is currently checked out.
    pub checked_out: bool,
    /// Branch name.
    pub branch: Option<String>,
    /// Fully resolved primary service URL, if a primary port is configured.
    pub primary_url: Option<String>,
    /// All port mappings for this instance.
    pub ports: Vec<PortMapping>,
}

/// Response for a lookup query.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LookupResponse {
    /// Project name.
    pub project: String,
    /// Worktree that was looked up, or None for project root.
    pub worktree: Option<String>,
    /// Project root directory on the host.
    pub project_root: Option<String>,
    /// Instances assigned to the requested worktree (or project root).
    pub instances: Vec<LookupInstance>,
}
