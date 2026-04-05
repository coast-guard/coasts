//! SSH tunnel manager for remote daemon connections.
//!
//! Manages SSH tunnels that forward local ports to remote coastd instances.
//! Tunnels are established on-demand and monitored for health.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::state::remotes::{Remote, Tunnel, TunnelStatus};
use coast_core::error::{CoastError, Result};

/// Default local port for the first tunnel (subsequent tunnels increment from here).
/// For dev mode (port 31416), tunnels start at 31417 to avoid collision.
/// For production (port 31415), tunnels start at 31416.
const DEFAULT_TUNNEL_BASE_PORT: u16 = 31417;

/// Remote daemon Unix socket path (relative to user's home directory).
/// For dev mode: ~/.coast-dev/coastd.sock
/// For production: ~/.coast/coastd.sock
const REMOTE_DAEMON_SOCKET_DEV: &str = ".coast-dev/coastd.sock";
const REMOTE_DAEMON_SOCKET_PROD: &str = ".coast/coastd.sock";

/// Configuration for establishing a tunnel.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TunnelConfig {
    /// Remote configuration
    pub remote: Remote,
    /// Local port to bind
    pub local_port: u16,
    /// Remote port to forward to
    pub remote_port: u16,
}

/// Manages SSH tunnels to remote daemons.
pub struct TunnelManager {
    /// Active SSH tunnel processes, keyed by remote name.
    tunnels: Arc<RwLock<HashMap<String, TunnelProcess>>>,
    /// Next available local port for new tunnels.
    next_port: Arc<RwLock<u16>>,
}

/// An active SSH tunnel process.
struct TunnelProcess {
    /// The SSH child process.
    child: Child,
    /// Local port this tunnel is bound to.
    local_port: u16,
}

/// Result of a connect operation - tunnel state to persist.
#[derive(Debug, Clone)]
pub struct ConnectResult {
    pub local_port: u16,
    pub tunnel_state: Tunnel,
}

/// Result of a disconnect operation - tunnel state to persist.
#[derive(Debug, Clone)]
pub struct DisconnectResult {
    pub disconnected: bool,
    pub tunnel_state: Option<Tunnel>,
}

impl TunnelManager {
    /// Create a new tunnel manager.
    pub fn new() -> Self {
        Self {
            tunnels: Arc::new(RwLock::new(HashMap::new())),
            next_port: Arc::new(RwLock::new(DEFAULT_TUNNEL_BASE_PORT)),
        }
    }

    /// Get the local port for an existing tunnel, or None if not connected.
    pub async fn get_tunnel_port(&self, remote_name: &str) -> Option<u16> {
        let tunnels = self.tunnels.read().await;
        tunnels.get(remote_name).map(|t| t.local_port)
    }

    /// Check if a tunnel is active for the given remote.
    pub async fn is_connected(&self, remote_name: &str) -> bool {
        let tunnels = self.tunnels.read().await;
        tunnels.contains_key(remote_name)
    }

    /// Establish an SSH tunnel to a remote daemon.
    ///
    /// If a tunnel already exists, returns the existing local port.
    /// Otherwise, establishes a new tunnel and returns the local port.
    ///
    /// Returns a `ConnectResult` containing the tunnel state to persist.
    pub async fn connect(&self, remote: &Remote) -> Result<ConnectResult> {
        // Check if already connected
        if let Some(port) = self.get_tunnel_port(&remote.name).await {
            info!(remote = %remote.name, port, "tunnel already connected");
            return Ok(ConnectResult {
                local_port: port,
                tunnel_state: Tunnel {
                    remote_name: remote.name.clone(),
                    local_port: port,
                    ssh_pid: None, // Already connected, pid unknown
                    status: TunnelStatus::Connected,
                    connected_at: Some(Utc::now()),
                },
            });
        }

        // Allocate a local port
        let local_port = {
            let mut next = self.next_port.write().await;
            let port = *next;
            *next += 1;
            port
        };

        // Build SSH command
        let mut cmd = Command::new("ssh");

        // Add SSH key if specified
        if let Some(ref key_path) = remote.ssh_key_path {
            cmd.arg("-i").arg(key_path);
        }

        // SSH options for non-interactive, stable tunneling
        // Forward local TCP port to remote Unix socket
        // Format: local_port:/path/to/remote.sock (OpenSSH 6.7+)
        let remote_socket_path = format!("/home/{}/{}", remote.user, REMOTE_DAEMON_SOCKET_DEV);
        cmd.args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N", // Don't execute remote command
            "-L",
            &format!("{}:{}", local_port, remote_socket_path),
            "-p",
            &remote.port.to_string(),
            &format!("{}@{}", remote.user, remote.host),
        ]);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        info!(
            remote = %remote.name,
            host = %remote.host,
            local_port,
            "establishing SSH tunnel"
        );

        // Spawn the SSH process
        let mut child = cmd.spawn().map_err(|e| CoastError::Remote {
            message: format!("failed to spawn SSH tunnel to '{}': {e}", remote.name),
        })?;

        // Get the PID
        let pid = child.id();

        // Spawn a task to monitor stderr for errors
        let stderr = child.stderr.take();
        let remote_name_clone = remote.name.clone();
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.is_empty() {
                        warn!(remote = %remote_name_clone, "ssh: {}", line);
                    }
                }
            });
        }

        // Give SSH a moment to establish the connection
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check if the process is still running
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(CoastError::Remote {
                    message: format!(
                        "SSH tunnel to '{}' exited immediately with status: {}",
                        remote.name, status
                    ),
                });
            }
            Ok(None) => {
                // Still running, good
            }
            Err(e) => {
                return Err(CoastError::Remote {
                    message: format!("failed to check SSH tunnel status: {e}"),
                });
            }
        }

        // Store the tunnel process
        {
            let mut tunnels = self.tunnels.write().await;
            tunnels.insert(remote.name.clone(), TunnelProcess { child, local_port });
        }

        // Build tunnel state for persistence
        let tunnel = Tunnel {
            remote_name: remote.name.clone(),
            local_port,
            ssh_pid: pid,
            status: TunnelStatus::Connected,
            connected_at: Some(Utc::now()),
        };

        info!(
            remote = %remote.name,
            local_port,
            pid = ?pid,
            "SSH tunnel established"
        );

        Ok(ConnectResult {
            local_port,
            tunnel_state: tunnel,
        })
    }

    /// Disconnect an SSH tunnel.
    ///
    /// Returns a `DisconnectResult` containing the tunnel state to persist.
    pub async fn disconnect(&self, remote_name: &str) -> Result<DisconnectResult> {
        let mut tunnels = self.tunnels.write().await;

        if let Some(mut tunnel) = tunnels.remove(remote_name) {
            info!(remote = %remote_name, "disconnecting SSH tunnel");

            // Kill the SSH process
            if let Err(e) = tunnel.child.kill().await {
                warn!(remote = %remote_name, "failed to kill SSH process: {e}");
            }

            // Build tunnel state for persistence
            let tunnel_state = Tunnel {
                remote_name: remote_name.to_string(),
                local_port: tunnel.local_port,
                ssh_pid: None,
                status: TunnelStatus::Disconnected,
                connected_at: None,
            };

            Ok(DisconnectResult {
                disconnected: true,
                tunnel_state: Some(tunnel_state),
            })
        } else {
            debug!(remote = %remote_name, "no tunnel to disconnect");
            Ok(DisconnectResult {
                disconnected: false,
                tunnel_state: None,
            })
        }
    }

    /// Disconnect all tunnels.
    ///
    /// Returns a list of tunnel states to persist.
    pub async fn disconnect_all(&self) -> Vec<Tunnel> {
        let remote_names: Vec<String> = {
            let tunnels = self.tunnels.read().await;
            tunnels.keys().cloned().collect()
        };

        let mut states = Vec::new();
        for name in remote_names {
            match self.disconnect(&name).await {
                Ok(result) => {
                    if let Some(state) = result.tunnel_state {
                        states.push(state);
                    }
                }
                Err(e) => {
                    error!(remote = %name, "failed to disconnect tunnel: {e}");
                }
            }
        }
        states
    }

    /// Check health of all tunnels.
    ///
    /// Returns a list of (remote_name, healthy, optional_tunnel_state_to_persist).
    pub async fn health_check(&self) -> Vec<(String, bool, Option<Tunnel>)> {
        let mut results = Vec::new();
        let remote_names: Vec<String> = {
            let tunnels = self.tunnels.read().await;
            tunnels.keys().cloned().collect()
        };

        for name in remote_names {
            let healthy = self.check_tunnel_health(&name).await;
            let tunnel_state = if !healthy {
                Some(Tunnel {
                    remote_name: name.clone(),
                    local_port: 0, // Will be updated on reconnect
                    ssh_pid: None,
                    status: TunnelStatus::Disconnected,
                    connected_at: None,
                })
            } else {
                None
            };
            results.push((name, healthy, tunnel_state));
        }

        results
    }

    /// Check if a specific tunnel is healthy.
    async fn check_tunnel_health(&self, remote_name: &str) -> bool {
        let tunnels = self.tunnels.read().await;

        if let Some(tunnel) = tunnels.get(remote_name) {
            // Try to connect to the local port
            tokio::net::TcpStream::connect(format!("127.0.0.1:{}", tunnel.local_port))
                .await
                .is_ok()
        } else {
            false
        }
    }

    /// Get tunnel status for all remotes.
    pub async fn get_tunnel_statuses(&self) -> HashMap<String, TunnelStatus> {
        let tunnels = self.tunnels.read().await;
        tunnels
            .keys()
            .map(|name| (name.clone(), TunnelStatus::Connected))
            .collect()
    }

    /// Get status of a specific tunnel.
    pub async fn get_tunnel_status(&self, remote_name: &str) -> TunnelStatus {
        let tunnels = self.tunnels.read().await;
        if tunnels.contains_key(remote_name) {
            TunnelStatus::Connected
        } else {
            TunnelStatus::Disconnected
        }
    }
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tunnel_manager_new() {
        let manager = TunnelManager::new();
        assert!(!manager.is_connected("test").await);
        assert!(manager.get_tunnel_port("test").await.is_none());
    }

    #[tokio::test]
    async fn test_tunnel_statuses_empty() {
        let manager = TunnelManager::new();
        let statuses = manager.get_tunnel_statuses().await;
        assert!(statuses.is_empty());
    }
}
