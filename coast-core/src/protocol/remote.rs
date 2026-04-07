/// Protocol types for remote coast operations.
///
/// Includes both:
/// - CLI <-> daemon types for managing registered remotes (add/ls/rm/test)
/// - daemon <-> coast-service types for workspace sync and tunnel setup
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::RemoteEntry;

// ---------------------------------------------------------------------------
// CLI <-> daemon: remote machine management
// ---------------------------------------------------------------------------

/// Request to manage registered remote machines.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum RemoteRequest {
    /// Register a new remote machine.
    Add {
        name: String,
        host: String,
        user: String,
        port: u16,
        ssh_key: Option<String>,
        sync_strategy: String,
    },
    /// List all registered remotes.
    Ls,
    /// Remove a registered remote by name.
    Rm { name: String },
    /// Test connectivity to a registered remote.
    Test { name: String },
    /// Install coast-service on a registered remote.
    Setup { name: String, docker: bool },
    /// Prune orphaned resources on a remote.
    Prune { name: String, dry_run: bool },
}

/// Response for remote machine management operations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteResponse {
    /// Human-readable status message.
    pub message: String,
    /// List of remotes (populated for Ls, single-entry for Add, empty for Rm).
    pub remotes: Vec<RemoteEntry>,
}

// ---------------------------------------------------------------------------
// daemon <-> coast-service: workspace sync and tunnels
// ---------------------------------------------------------------------------

/// Request to sync workspace files to the remote coast-service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncWorkspaceRequest {
    /// Project name.
    pub project: String,
    /// Instance name on the remote side.
    pub instance_name: String,
    /// Absolute path to the workspace directory on the local machine.
    pub local_workspace_path: String,
    /// Target path on the remote machine where /workspace is stored.
    pub remote_workspace_path: String,
}

/// Response after workspace sync completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncWorkspaceResponse {
    /// Number of files transferred.
    pub files_transferred: u64,
    /// Total bytes transferred.
    pub bytes_transferred: u64,
}

/// Request to set up port forwarding tunnels for a remote coast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelSetupRequest {
    /// Pairs of (local_dynamic_port, remote_container_port).
    pub port_mappings: Vec<(u16, u16)>,
}

/// Response after tunnel setup completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelSetupResponse {
    /// PIDs of the SSH forwarding processes (for cleanup on stop/rm).
    pub tunnel_pids: Vec<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_request_add_round_trip() {
        let req = RemoteRequest::Add {
            name: "my-vm".into(),
            host: "10.0.0.1".into(),
            user: "ubuntu".into(),
            port: 22,
            ssh_key: Some("~/.ssh/id_rsa".into()),
            sync_strategy: "rsync".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            RemoteRequest::Add {
                name, host, port, ..
            } => {
                assert_eq!(name, "my-vm");
                assert_eq!(host, "10.0.0.1");
                assert_eq!(port, 22);
            }
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn test_remote_request_ls_round_trip() {
        let req = RemoteRequest::Ls;
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, RemoteRequest::Ls));
    }

    #[test]
    fn test_remote_request_rm_round_trip() {
        let req = RemoteRequest::Rm {
            name: "old-vm".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            RemoteRequest::Rm { name } => assert_eq!(name, "old-vm"),
            _ => panic!("expected Rm"),
        }
    }

    #[test]
    fn test_remote_request_test_round_trip() {
        let req = RemoteRequest::Test {
            name: "test-vm".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            RemoteRequest::Test { name } => assert_eq!(name, "test-vm"),
            _ => panic!("expected Test"),
        }
    }

    #[test]
    fn test_remote_request_setup_round_trip() {
        let req = RemoteRequest::Setup {
            name: "my-vm".into(),
            docker: false,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            RemoteRequest::Setup { name, docker } => {
                assert_eq!(name, "my-vm");
                assert!(!docker);
            }
            _ => panic!("expected Setup"),
        }
    }

    #[test]
    fn test_remote_request_setup_docker_round_trip() {
        let req = RemoteRequest::Setup {
            name: "prod-box".into(),
            docker: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: RemoteRequest = serde_json::from_str(&json).unwrap();
        match decoded {
            RemoteRequest::Setup { name, docker } => {
                assert_eq!(name, "prod-box");
                assert!(docker);
            }
            _ => panic!("expected Setup"),
        }
    }

    #[test]
    fn test_remote_response_round_trip() {
        let resp = RemoteResponse {
            message: "1 remote(s)".into(),
            remotes: vec![RemoteEntry {
                name: "my-vm".into(),
                host: "10.0.0.1".into(),
                user: "ubuntu".into(),
                port: 22,
                ssh_key: None,
                sync_strategy: "rsync".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: RemoteResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.remotes.len(), 1);
        assert_eq!(decoded.remotes[0].name, "my-vm");
        assert_eq!(decoded.message, "1 remote(s)");
    }

    #[test]
    fn test_remote_response_empty_remotes() {
        let resp = RemoteResponse {
            message: "removed".into(),
            remotes: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: RemoteResponse = serde_json::from_str(&json).unwrap();
        assert!(decoded.remotes.is_empty());
    }

    #[test]
    fn test_sync_workspace_request_round_trip() {
        let req = SyncWorkspaceRequest {
            project: "my-app".into(),
            instance_name: "dev-1".into(),
            local_workspace_path: "/home/user/project".into(),
            remote_workspace_path: "~/coast-workspaces/my-app/dev-1".into(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: SyncWorkspaceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.project, "my-app");
        assert_eq!(decoded.instance_name, "dev-1");
    }

    #[test]
    fn test_tunnel_setup_request_round_trip() {
        let req = TunnelSetupRequest {
            port_mappings: vec![(59000, 3000), (59001, 8080)],
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: TunnelSetupRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.port_mappings.len(), 2);
        assert_eq!(decoded.port_mappings[0], (59000, 3000));
    }

    #[test]
    fn test_remote_request_as_full_request_round_trip() {
        use crate::protocol::{Request, Response};

        let req = Request::Remote(RemoteRequest::Ls);
        let json = serde_json::to_string(&req).unwrap();
        let decoded: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, Request::Remote(RemoteRequest::Ls)));

        let resp = Response::Remote(RemoteResponse {
            message: "ok".into(),
            remotes: vec![],
        });
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: Response = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, Response::Remote(_)));
    }
}
