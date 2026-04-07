use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info};

use coast_core::error::{CoastError, Result};
use coast_core::types::RemoteConnection;

struct CachedTunnel {
    local_port: u16,
    ssh_pid: Option<u32>,
}

static TUNNEL_CACHE: LazyLock<TokioMutex<HashMap<String, CachedTunnel>>> =
    LazyLock::new(|| TokioMutex::new(HashMap::new()));

/// Kill all orphaned SSH tunnel processes from a previous daemon session.
///
/// On daemon startup, there may be leftover `ssh -N` processes from a
/// previous daemon session. This kills them to avoid port conflicts and
/// zombie tunnels. They will be re-established by `restore_remote_tunnels`.
pub fn cleanup_orphaned_ssh_tunnels() {
    match Command::new("pkill").args(["-f", "ssh -N"]).output() {
        Ok(output) => {
            if output.status.success() {
                info!("Cleaned up orphaned SSH tunnel processes from previous session");
            } else {
                debug!("No orphaned SSH tunnel processes found");
            }
        }
        Err(_) => {
            debug!("pkill not available, skipping orphaned SSH tunnel cleanup");
        }
    }

    let _ = Command::new("sh")
        .args(["-c", "rm -f /tmp/coast-ssh-*"])
        .output();

    if let Ok(mut cache) = TUNNEL_CACHE.try_lock() {
        cache.clear();
    }
}

/// An active SSH tunnel to a remote coast-service.
pub struct SshTunnel {
    /// Local port that forwards to the remote coast-service.
    pub local_port: u16,
    /// PID of the SSH process. None for cached (non-owned) tunnels.
    pub ssh_pid: Option<u32>,
}

fn tunnel_cache_key(config: &RemoteConnection) -> String {
    format!("{}@{}:{}", config.user, config.host, config.port)
}

impl SshTunnel {
    /// Get or create a cached SSH tunnel. Reuses an existing tunnel if
    /// it is still alive (port responds), otherwise creates a new one.
    pub async fn establish_cached(config: &RemoteConnection) -> Result<Self> {
        let key = tunnel_cache_key(config);
        let mut cache = TUNNEL_CACHE.lock().await;

        if let Some(cached) = cache.get(&key) {
            if wait_for_tunnel_ready(cached.local_port, 500).await {
                debug!(
                    local_port = cached.local_port,
                    host = %config.host,
                    "reusing cached SSH tunnel"
                );
                return Ok(Self {
                    local_port: cached.local_port,
                    ssh_pid: None,
                });
            }
            if let Some(pid) = cached.ssh_pid {
                debug!(pid, "cached tunnel dead, killing stale process");
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            cache.remove(&key);
        }
        drop(cache);

        let tunnel = Self::establish(config).await?;
        let mut cache = TUNNEL_CACHE.lock().await;
        cache.insert(
            key,
            CachedTunnel {
                local_port: tunnel.local_port,
                ssh_pid: tunnel.ssh_pid,
            },
        );

        Ok(Self {
            local_port: tunnel.local_port,
            ssh_pid: None,
        })
    }

    /// Establish a fresh SSH tunnel (not cached).
    ///
    /// Opens a local port that forwards to the coast-service port on the remote
    /// host (default: 31420).
    pub async fn establish(config: &RemoteConnection) -> Result<Self> {
        let local_port = find_available_port().await?;
        let remote_service_port = 31420u16;

        let ssh_key_str = config.ssh_key.display().to_string();
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args([
            "-N",
            "-L",
            &format!("{local_port}:localhost:{remote_service_port}"),
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/coast-ssh-%r@%h:%p",
            "-o",
            "ControlPersist=300",
            &format!("{}@{}", config.user, config.host),
        ]);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| CoastError::state(format!("failed to start SSH tunnel: {e}")))?;

        let ssh_pid = child.id();

        // Wait for the tunnel to actually accept connections (up to 5s).
        let ready = wait_for_tunnel_ready(local_port, 5000).await;
        if ready {
            info!(local_port, ?ssh_pid, host = %config.host, "SSH tunnel established");
        } else {
            tracing::warn!(
                local_port, ?ssh_pid, host = %config.host,
                "SSH tunnel may not be ready (timed out waiting for port)"
            );
        }

        Ok(Self {
            local_port,
            ssh_pid,
        })
    }

    /// Tear down the SSH tunnel.
    pub fn kill(&self) {
        if let Some(pid) = self.ssh_pid {
            debug!(pid, "killing SSH tunnel");
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Kill SSH tunnel processes for specific dynamic ports (per-instance, not global).
pub fn kill_tunnels_for_ports(dynamic_ports: &[u16]) {
    for port in dynamic_ports {
        let pattern = format!("ssh -N -L {}:", port);
        let _ = Command::new("pkill").args(["-f", &pattern]).output();
    }
}

/// Invalidate the tunnel cache for a specific remote host.
pub async fn invalidate_cache_for_host(config: &RemoteConnection) {
    let key = tunnel_cache_key(config);
    let mut cache = TUNNEL_CACHE.lock().await;
    if let Some(entry) = cache.remove(&key) {
        if let Some(pid) = entry.ssh_pid {
            debug!(pid, host = %config.host, "killing cached tunnel for host");
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
}

/// Set up SSH port forwards for individual service ports.
///
/// For each `(local_port, remote_port)` pair, creates an SSH tunnel that
/// forwards `localhost:{local_port}` to the remote's `localhost:{remote_port}`.
/// Returns the PIDs of the spawned SSH processes.
///
/// Each tunnel is verified after spawn: if the SSH process exits immediately
/// (e.g. due to SSH rate limiting or MaxStartups), it is retried once after
/// a brief delay.
pub async fn forward_ports(
    config: &RemoteConnection,
    port_mappings: &[(u16, u16)],
) -> Result<Vec<u32>> {
    let mut pids = Vec::new();

    for &(local_port, remote_port) in port_mappings {
        match spawn_forward_tunnel(config, local_port, remote_port).await {
            Ok(pid) => pids.push(pid),
            Err(e) => {
                tracing::warn!(
                    local_port,
                    remote_port,
                    error = %e,
                    "tunnel spawn failed on first attempt, retrying after 1s"
                );
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                match spawn_forward_tunnel(config, local_port, remote_port).await {
                    Ok(pid) => pids.push(pid),
                    Err(e2) => {
                        tracing::warn!(
                            local_port,
                            remote_port,
                            error = %e2,
                            "tunnel spawn failed on retry"
                        );
                    }
                }
            }
        }
    }

    Ok(pids)
}

async fn spawn_forward_tunnel(
    config: &RemoteConnection,
    local_port: u16,
    remote_port: u16,
) -> std::result::Result<u32, String> {
    let ssh_key_str = config.ssh_key.display().to_string();
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-N",
        "-L",
        &format!("{local_port}:localhost:{remote_port}"),
        "-p",
        &config.port.to_string(),
        "-i",
        &ssh_key_str,
        "-o",
        "StrictHostKeyChecking=no",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
        &format!("{}@{}", config.user, config.host),
    ]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start SSH: {e}"))?;

    let pid = child.id().ok_or("no PID for SSH process")?;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    match child.try_wait() {
        Ok(Some(status)) => {
            let stderr_msg = if let Some(mut stderr) = child.stderr.take() {
                let mut buf = String::new();
                let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut buf).await;
                buf
            } else {
                String::new()
            };
            Err(format!(
                "SSH exited immediately with {status}: {stderr_msg}"
            ))
        }
        Ok(None) => {
            debug!(local_port, remote_port, pid, "port forward established");
            Ok(pid)
        }
        Err(e) => Err(format!("failed to check SSH status: {e}")),
    }
}

/// Set up SSH **reverse** port forwards for shared service ports.
///
/// For each `(remote_port, local_port)` pair, creates an SSH tunnel that
/// makes `remote_host:remote_port` forward back to `localhost:local_port`
/// on the local machine. This allows the remote DinD container to reach
/// locally-running shared services (postgres, redis, etc.) via
/// `host.docker.internal`.
///
/// Returns the PIDs of the spawned SSH processes.
pub async fn reverse_forward_ports(
    config: &RemoteConnection,
    port_mappings: &[(u16, u16)],
) -> Result<Vec<u32>> {
    let mut pids = Vec::new();

    if !port_mappings.is_empty() {
        release_stale_remote_ports(config, port_mappings).await;
    }

    for &(remote_port, local_port) in port_mappings {
        let ssh_key_str = config.ssh_key.display().to_string();
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args([
            "-N",
            "-R",
            &format!("0.0.0.0:{remote_port}:localhost:{local_port}"),
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ServerAliveCountMax=3",
            &format!("{}@{}", config.user, config.host),
        ]);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let child = cmd.spawn().map_err(|e| {
            CoastError::state(format!(
                "failed to start reverse port forward {remote_port}<-{local_port}: {e}"
            ))
        })?;

        if let Some(pid) = child.id() {
            pids.push(pid);
            info!(
                remote_port,
                local_port, pid, "reverse port forward established (ssh -R)"
            );
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    Ok(pids)
}

/// Kill any stale sshd processes on the remote that are holding our
/// reverse tunnel ports. This prevents the race where a daemon restart
/// tries to bind ports still held by the previous session's sshd.
async fn release_stale_remote_ports(config: &RemoteConnection, port_mappings: &[(u16, u16)]) {
    let kill_cmds: Vec<String> = port_mappings
        .iter()
        .map(|(remote_port, _)| {
            format!(
                "sudo fuser -k {remote_port}/tcp 2>/dev/null || \
                 fuser -k {remote_port}/tcp 2>/dev/null || true"
            )
        })
        .collect();
    let combined = kill_cmds.join("; ");

    let ssh_key_str = config.ssh_key.display().to_string();
    let result = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/coast-ssh-%r@%h:%p",
            "-o",
            "ControlPersist=300",
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
        ])
        .arg(format!("{}@{}", config.user, config.host))
        .arg(&combined)
        .output()
        .await;

    match result {
        Ok(o) if o.status.success() => {
            debug!(
                host = %config.host,
                "released stale remote ports"
            );
        }
        _ => {
            debug!(
                host = %config.host,
                "fuser not available or no stale ports to release"
            );
        }
    }
}

async fn wait_for_tunnel_ready(port: u16, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

async fn find_available_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| CoastError::state(format!("failed to find available port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| CoastError::state(format!("failed to get local address: {e}")))?
        .port();
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dummy_config() -> RemoteConnection {
        RemoteConnection {
            host: "192.0.2.1".to_string(),
            port: 22,
            user: "testuser".to_string(),
            ssh_key: PathBuf::from("/nonexistent/key"),
            workspace_sync: coast_core::types::SyncStrategy::Mutagen,
        }
    }

    #[tokio::test]
    async fn test_forward_ports_empty_list() {
        let config = dummy_config();
        let result = forward_ports(&config, &[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_reverse_forward_ports_empty_list() {
        let config = dummy_config();
        let result = reverse_forward_ports(&config, &[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_find_available_port_returns_nonzero() {
        let port = find_available_port().await.unwrap();
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_find_available_port_returns_unique_ports() {
        let p1 = find_available_port().await.unwrap();
        let p2 = find_available_port().await.unwrap();
        assert_ne!(p1, p2);
    }
}
