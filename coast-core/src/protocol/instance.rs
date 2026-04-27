use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::{AssignAction, PortMapping};

/// A shared service port that should be forwarded to the remote DinD container.
///
/// Phase 18 symmetric-routing invariant: every forward carries both the
/// canonical port the consumer app dials (`port`) AND a `remote_port` the
/// daemon has chosen for the remote-side bind. sshd binds the reverse
/// tunnel on `remote_port`; socat inside the remote DinD forwards
/// canonical `port` traffic (bound to a docker0 alias IP) to
/// `host.docker.internal:remote_port`. See
/// [`coast-ssg/DESIGN.md §20`](../../coast-ssg/DESIGN.md) for the full flow.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedServicePortForward {
    /// Shared service name (e.g. "postgres", "redis").
    pub name: String,
    /// Canonical container port the consumer app dials (e.g. 5432, 6379).
    pub port: u16,
    /// Port on the remote VM where sshd binds the reverse tunnel.
    ///
    /// Phase 18 (inline shared services): a dynamic port allocated by the
    /// daemon via `port_manager::allocate_dynamic_port` once per forward,
    /// independent per coast so concurrent consumer coasts on one remote
    /// cannot collide.
    ///
    /// Phase 30 (SSG-backed shared services, `from_group = true`): the
    /// project's stable virtual port from `ssg_virtual_ports` —
    /// **shared** across every consumer instance of the project on the
    /// same remote VM. The daemon coalesces tunnels per
    /// `(project, remote_host, service, container_port)` so only the
    /// first instance spawns the `ssh -R`; subsequent instances reuse
    /// it (see `ssg_shared_tunnels`). Both sides of `ssh -R` use the
    /// same value, and the local endpoint terminates at the Phase 28
    /// host socat for that virtual port. See
    /// [`coast-ssg/DESIGN.md §24`](../../coast-ssg/DESIGN.md).
    pub remote_port: u16,
}

/// Request to create and start a new coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
    /// Branch to use (if any).
    pub branch: Option<String>,
    /// Git commit SHA at the time of creation.
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// Optional worktree to assign after provisioning completes.
    #[serde(default)]
    pub worktree: Option<String>,
    /// Optional build ID to use directly (overrides latest-per-type resolution).
    #[serde(default)]
    pub build_id: Option<String>,
    /// Coastfile type to use for build resolution (None = "default").
    #[serde(default)]
    pub coastfile_type: Option<String>,
    /// Which registered remote to use (for remote types).
    #[serde(default)]
    pub remote: Option<String>,
    /// Force-remove any dangling Docker container with the same name before creating.
    #[serde(default)]
    pub force_remove_dangling: bool,
    /// Shared service ports forwarded via SSH reverse tunnel to the remote.
    #[serde(default)]
    pub shared_service_ports: Vec<SharedServicePortForward>,
}

/// Response after a successful run.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunResponse {
    /// Instance name.
    pub name: String,
    /// Container ID on host daemon.
    pub container_id: String,
    /// Dynamic port allocations.
    pub ports: Vec<PortMapping>,
}

/// Request to assign (or reassign) a worktree to an existing coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssignRequest {
    /// Instance name (the slot to assign to).
    pub name: String,
    /// Project name.
    pub project: String,
    /// Worktree (branch) to assign to this instance.
    pub worktree: String,
    /// Git commit SHA for the worktree being assigned.
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// When true, analyze and report the assign plan without executing it.
    #[serde(default)]
    pub explain: bool,
    /// When true, refresh the cached ignored-file bootstrap before assigning.
    #[serde(default)]
    pub force_sync: bool,
    /// Per-service assign actions computed by the daemon. When empty,
    /// coast-service falls back to restarting all services.
    #[serde(default)]
    #[ts(skip)]
    pub service_actions: HashMap<String, AssignAction>,
}

/// Response after a successful worktree assignment.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssignResponse {
    /// Instance name.
    pub name: String,
    /// The worktree that was assigned.
    pub worktree: String,
    /// The previous worktree (if any).
    pub previous_worktree: Option<String>,
    /// Time elapsed in milliseconds.
    #[serde(default)]
    pub time_elapsed_ms: u64,
}

/// Per-service action in an explain response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssignExplainService {
    /// Compose service name.
    pub name: String,
    /// Action that would be taken (none, hot, restart, rebuild).
    pub action: String,
}

/// Response for `coast assign --explain`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssignExplainResponse {
    /// Instance name.
    pub name: String,
    /// Target worktree.
    pub worktree: String,
    /// Current branch of the instance.
    pub current_branch: Option<String>,
    /// Per-service action plan.
    pub services: Vec<AssignExplainService>,
    /// Paths excluded from the file diff and gitignored sync.
    pub exclude_paths: Vec<String>,
    /// Number of tracked files that would be diffed.
    pub tracked_file_count: usize,
    /// Number of gitignored files that would be synced (first assign only).
    pub gitignored_file_count: usize,
    /// Whether the worktree already exists on disk.
    pub worktree_exists: bool,
    /// Whether ignored-file bootstrap can be skipped for this assign.
    pub worktree_synced: bool,
    /// Whether bare services have an install step.
    pub has_bare_install: bool,
    /// Files changed between current and target branch.
    pub changed_files_count: usize,
}

/// Request to unassign a worktree, returning the instance to the repo's default branch.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnassignRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful unassign.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnassignResponse {
    /// Instance name.
    pub name: String,
    /// The default branch the instance was returned to.
    pub worktree: String,
    /// The previous worktree.
    pub previous_worktree: Option<String>,
    /// Time elapsed in milliseconds.
    #[serde(default)]
    pub time_elapsed_ms: u64,
}

/// Request to rebuild images inside a DinD container from the bind-mounted workspace.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RebuildRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful rebuild.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RebuildResponse {
    /// Instance name.
    pub name: String,
    /// Services that were rebuilt.
    pub services_rebuilt: Vec<String>,
}

/// Request to restart all services inside a running coast instance.
///
/// For compose projects: `docker compose down` + `docker compose up -d`.
/// For bare services: `stop-all.sh` + `start-all.sh`.
/// Respects `autostart = false` (tears down but skips restart).
/// Does NOT affect shared services.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestartServicesRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful services restart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestartServicesResponse {
    /// Instance name.
    pub name: String,
    /// Services that were restarted (empty if autostart=false).
    pub services_restarted: Vec<String>,
}

/// Request to stop a running instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StopRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful stop.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StopResponse {
    /// Instance name.
    pub name: String,
}

/// Request to start a stopped instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StartRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful start.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StartResponse {
    /// Instance name.
    pub name: String,
    /// Dynamic port allocations.
    pub ports: Vec<PortMapping>,
}

/// Request to remove an instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RmRequest {
    /// Instance name.
    pub name: String,
    /// Project name.
    pub project: String,
}

/// Response after a successful removal.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RmResponse {
    /// Instance name.
    pub name: String,
}

/// Request to remove a project build artifact and associated Docker resources.
///
/// When `build_ids` is non-empty, only those specific builds are removed
/// (just their artifact directories and Docker image tags). When empty,
/// the entire project build is removed (existing behavior).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RmBuildRequest {
    /// Project name.
    pub project: String,
    /// Specific build IDs to remove. When empty, removes the entire project build.
    #[serde(default)]
    pub build_ids: Vec<String>,
}

/// Response after removing a project build.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RmBuildResponse {
    /// Project name.
    pub project: String,
    /// Number of containers removed.
    pub containers_removed: usize,
    /// Number of volumes removed.
    pub volumes_removed: usize,
    /// Number of images removed.
    pub images_removed: usize,
    /// Whether the artifact directory was removed.
    pub artifact_removed: bool,
    /// Number of individual builds removed (when build_ids was provided).
    #[serde(default)]
    pub builds_removed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_request_force_remove_dangling_defaults_false() {
        let json = r#"{"name":"dev-1","project":"my-app","branch":null}"#;
        let req: RunRequest = serde_json::from_str(json).unwrap();
        assert!(!req.force_remove_dangling);
    }

    #[test]
    fn test_run_request_force_remove_dangling_round_trip() {
        let req = RunRequest {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: true,
            remote: None,
            shared_service_ports: Vec::new(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: RunRequest = serde_json::from_str(&json).unwrap();
        assert!(deserialized.force_remove_dangling);
        assert_eq!(deserialized.name, "dev-1");
    }

    #[test]
    fn test_run_request_force_remove_dangling_explicit_false() {
        let json = r#"{"name":"x","project":"p","branch":null,"force_remove_dangling":false}"#;
        let req: RunRequest = serde_json::from_str(json).unwrap();
        assert!(!req.force_remove_dangling);
    }

    #[test]
    fn test_shared_service_port_forward_round_trip_preserves_remote_port() {
        // Phase 18 invariant: the wire protocol carries all three
        // fields (name, port, remote_port). Ensure serde doesn't drop
        // `remote_port` on round-trip so coast-service receives the
        // exact port coast-daemon allocated.
        let fwd = SharedServicePortForward {
            name: "postgres".to_string(),
            port: 5432,
            remote_port: 61034,
        };
        let json = serde_json::to_string(&fwd).unwrap();
        assert!(json.contains("\"remote_port\":61034"));
        let deserialized: SharedServicePortForward = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "postgres");
        assert_eq!(deserialized.port, 5432);
        assert_eq!(deserialized.remote_port, 61034);
    }

    #[test]
    fn test_shared_service_port_forward_missing_remote_port_is_rejected() {
        // Hard cutover: no serde default. A payload that omits
        // `remote_port` fails deserialization so stale clients/servers
        // from before Phase 18 are rejected loudly.
        let json = r#"{"name":"postgres","port":5432}"#;
        let result: serde_json::Result<SharedServicePortForward> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing remote_port should fail deserialization, got: {result:?}"
        );
    }
}
