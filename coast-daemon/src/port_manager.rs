/// Port allocation and socat process management for coast daemon.
///
/// This module handles:
/// - Dynamic port allocation in the ephemeral range (49152-65535)
/// - Socat command construction for port forwarding
/// - Socat process lifecycle management (spawn, kill, track PIDs)
///
/// Port forwarding is the mechanism behind `coast checkout` (instant port swap)
/// and always-on dynamic ports. The daemon spawns socat processes that forward
/// traffic from host ports to coast container ports.
use std::collections::HashMap;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};

/// The lower bound of the ephemeral/dynamic port range (inclusive).
const PORT_RANGE_START: u16 = 49152;

/// The upper bound of the ephemeral/dynamic port range (inclusive).
const PORT_RANGE_END: u16 = 65535;

/// Maximum number of attempts to find a free port before giving up.
const MAX_ALLOCATION_ATTEMPTS: u32 = 1000;

/// A pair of socat process PIDs for a single port forwarding entry.
///
/// Each port mapping gets two socat processes:
/// - `canonical_pid`: Forwards the canonical (well-known) port. Only active
///   when this instance is checked out. Killed and respawned on checkout swap.
/// - `dynamic_pid`: Forwards the dynamically-allocated port. Always active
///   from `coast run` until `coast stop` or `coast rm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForwardingPair {
    /// PID of the socat process forwarding the canonical port.
    pub canonical_pid: u32,
    /// PID of the socat process forwarding the dynamic port.
    pub dynamic_pid: u32,
}

/// Manages active socat port-forwarding processes.
///
/// Tracks all running socat processes keyed by a logical identifier so they
/// can be cleanly stopped on checkout swap, instance stop, or instance removal.
pub struct PortForwarder {
    /// Active forwarding pairs, keyed by a logical identifier
    /// (typically "{project}--{instance}--{logical_name}").
    active: HashMap<String, ForwardingPair>,
}

impl PortForwarder {
    /// Create a new `PortForwarder` with no active forwarding processes.
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    /// Start both canonical and dynamic socat forwarders for a port mapping.
    ///
    /// Spawns two socat processes:
    /// 1. Canonical: listens on `canonical_port`, forwards to `coast_ip:target_port`
    /// 2. Dynamic: listens on `dynamic_port`, forwards to `coast_ip:target_port`
    ///
    /// Returns a `ForwardingPair` with both PIDs.
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Port` if either socat process fails to spawn.
    pub fn start_forwarding(
        &mut self,
        key: &str,
        canonical_port: u16,
        dynamic_port: u16,
        coast_ip: &str,
        target_port: u16,
    ) -> Result<ForwardingPair> {
        let canonical_cmd = socat_command_canonical(canonical_port, coast_ip, target_port);
        let dynamic_cmd = socat_command_dynamic(dynamic_port, coast_ip, target_port);

        let canonical_pid = spawn_socat(&canonical_cmd).map_err(|e| {
            CoastError::port(format!(
                "Failed to start canonical port forwarding on port {canonical_port}: {e}"
            ))
        })?;

        let dynamic_pid = match spawn_socat(&dynamic_cmd) {
            Ok(pid) => pid,
            Err(e) => {
                // Clean up the canonical socat we already spawned.
                let _ = kill_socat(canonical_pid);
                return Err(CoastError::port(format!(
                    "Failed to start dynamic port forwarding on port {dynamic_port}: {e}. \
                     Canonical forwarder on port {canonical_port} has been cleaned up."
                )));
            }
        };

        let pair = ForwardingPair {
            canonical_pid,
            dynamic_pid,
        };

        info!(
            key = key,
            canonical_port = canonical_port,
            dynamic_port = dynamic_port,
            canonical_pid = canonical_pid,
            dynamic_pid = dynamic_pid,
            "Started port forwarding"
        );

        self.active.insert(key.to_string(), pair);
        Ok(pair)
    }

    /// Stop both canonical and dynamic socat forwarders for a key.
    ///
    /// Kills both socat processes and removes the entry from tracking.
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Port` if killing either process fails.
    /// Both processes are attempted even if the first kill fails.
    pub fn stop_forwarding(&mut self, key: &str) -> Result<()> {
        if let Some(pair) = self.active.remove(key) {
            let mut errors = Vec::new();

            if let Err(e) = kill_socat(pair.canonical_pid) {
                errors.push(format!(
                    "Failed to kill canonical socat (PID {}): {}",
                    pair.canonical_pid, e
                ));
            }

            if let Err(e) = kill_socat(pair.dynamic_pid) {
                errors.push(format!(
                    "Failed to kill dynamic socat (PID {}): {}",
                    pair.dynamic_pid, e
                ));
            }

            if !errors.is_empty() {
                return Err(CoastError::port(errors.join("; ")));
            }

            info!(key = key, "Stopped port forwarding");
        } else {
            debug!(
                key = key,
                "No active forwarding found for key, nothing to stop"
            );
        }

        Ok(())
    }

    /// Stop only the canonical socat forwarder, leaving the dynamic one running.
    ///
    /// This is used during checkout swap: the canonical port is reassigned
    /// to a different instance, but the dynamic port continues forwarding.
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Port` if killing the process fails.
    pub fn stop_canonical(&mut self, key: &str) -> Result<()> {
        if let Some(pair) = self.active.get(key) {
            kill_socat(pair.canonical_pid)?;
            info!(
                key = key,
                pid = pair.canonical_pid,
                "Stopped canonical port forwarding"
            );
        } else {
            debug!(
                key = key,
                "No active forwarding found for key, nothing to stop"
            );
        }

        Ok(())
    }

    /// Stop forwarding by raw PIDs, without requiring a tracking key.
    ///
    /// Useful when restoring state from the database where we have PIDs
    /// but may not have loaded the full tracking map.
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Port` if killing either process fails.
    pub fn stop_forwarding_by_pids(&self, canonical_pid: u32, dynamic_pid: u32) -> Result<()> {
        let mut errors = Vec::new();

        if let Err(e) = kill_socat(canonical_pid) {
            errors.push(format!(
                "Failed to kill canonical socat (PID {}): {}",
                canonical_pid, e
            ));
        }

        if let Err(e) = kill_socat(dynamic_pid) {
            errors.push(format!(
                "Failed to kill dynamic socat (PID {}): {}",
                dynamic_pid, e
            ));
        }

        if !errors.is_empty() {
            return Err(CoastError::port(errors.join("; ")));
        }

        Ok(())
    }

    /// Stop only a canonical socat forwarder by raw PID.
    ///
    /// Used during checkout swap when we know the PID from the state DB.
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Port` if killing the process fails.
    pub fn stop_canonical_by_pid(&self, pid: u32) -> Result<()> {
        kill_socat(pid)
    }

    /// Get all active forwarding pairs.
    pub fn active_pairs(&self) -> &HashMap<String, ForwardingPair> {
        &self.active
    }

    /// Check if a key has active forwarding.
    pub fn is_active(&self, key: &str) -> bool {
        self.active.contains_key(key)
    }
}

impl Default for PortForwarder {
    fn default() -> Self {
        Self::new()
    }
}

/// Allocate a dynamic port by finding an unused port in the ephemeral range.
///
/// Tries to bind a TCP listener on successive ports starting from a random
/// offset within the ephemeral range. Returns the first port that successfully
/// binds.
///
/// # Errors
///
/// Returns `CoastError::Port` if no free port can be found after
/// `MAX_ALLOCATION_ATTEMPTS` attempts.
pub fn allocate_dynamic_port() -> Result<u16> {
    // Start from a pseudo-random offset to reduce collisions when multiple
    // allocations happen in quick succession.
    let range_size = (PORT_RANGE_END - PORT_RANGE_START + 1) as u32;
    let start_offset = (std::process::id() ^ (timestamp_nanos() as u32)) % range_size;

    for i in 0..MAX_ALLOCATION_ATTEMPTS {
        let offset = (start_offset + i) % range_size;
        let port = PORT_RANGE_START + offset as u16;

        if is_port_available(port) {
            debug!(port = port, "Allocated dynamic port");
            return Ok(port);
        }
    }

    Err(CoastError::port(format!(
        "Could not find an available port after {MAX_ALLOCATION_ATTEMPTS} attempts \
         in range {PORT_RANGE_START}-{PORT_RANGE_END}. Too many ports may be in use. \
         Try stopping some coast instances with `coast stop <name>` to free ports."
    )))
}

/// Check whether a port is available by attempting to bind a TCP listener on it.
///
/// Returns `true` if the port is free (bind succeeds), `false` otherwise.
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Build the socat command arguments for canonical port forwarding.
///
/// Canonical ports are the well-known ports declared in the Coastfile (e.g., 3000, 5432).
/// They are only forwarded for the currently checked-out instance.
///
/// Produces: `["socat", "TCP-LISTEN:{canonical_port},fork,reuseaddr", "TCP:{coast_ip}:{target_port}"]`
pub fn socat_command_canonical(
    canonical_port: u16,
    coast_ip: &str,
    target_port: u16,
) -> Vec<String> {
    build_socat_command(canonical_port, coast_ip, target_port)
}

/// Build the socat command arguments for dynamic port forwarding.
///
/// Dynamic ports are always-on ports allocated at `coast run` time. They allow
/// access to an instance's services regardless of checkout status.
///
/// Produces: `["socat", "TCP-LISTEN:{dynamic_port},fork,reuseaddr", "TCP:{coast_ip}:{target_port}"]`
pub fn socat_command_dynamic(dynamic_port: u16, coast_ip: &str, target_port: u16) -> Vec<String> {
    build_socat_command(dynamic_port, coast_ip, target_port)
}

/// Build a socat command that listens on `listen_port` and forwards to `coast_ip:target_port`.
///
/// The `fork` option allows multiple simultaneous connections.
/// The `reuseaddr` option allows rapid rebinding after the process exits.
fn build_socat_command(listen_port: u16, coast_ip: &str, target_port: u16) -> Vec<String> {
    vec![
        "socat".to_string(),
        format!("TCP-LISTEN:{listen_port},fork,reuseaddr"),
        format!("TCP:{coast_ip}:{target_port}"),
    ]
}

/// Spawn a socat process with the given command arguments.
///
/// The process is spawned in the background (not waited on). The returned PID
/// can be used later with `kill_socat` to terminate it.
///
/// # Errors
///
/// Returns `CoastError::Port` if the socat binary cannot be found or the
/// process fails to spawn.
pub fn spawn_socat(cmd: &[String]) -> Result<u32> {
    if cmd.is_empty() {
        return Err(CoastError::port("Empty socat command"));
    }

    let mut command = Command::new(&cmd[0]);
    if cmd.len() > 1 {
        command.args(&cmd[1..]);
    }

    // Detach stdin/stdout/stderr so the socat process runs independently.
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    // Make the socat process a new process group leader so that killpg() in
    // kill_socat() reaches both the parent listener and any fork()ed children
    // handling active connections.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = command.spawn().map_err(|e| {
        CoastError::port(format!(
            "Failed to spawn socat process `{}`: {}. \
             Ensure socat is installed (e.g., `brew install socat` on macOS, \
             `apt-get install socat` on Ubuntu).",
            cmd.join(" "),
            e
        ))
    })?;

    let pid = child.id();
    info!(pid = pid, cmd = cmd.join(" "), "Spawned socat process");
    Ok(pid)
}

/// Kill a socat process and all its forked children by process group.
///
/// Socat runs with `fork` mode, creating a child process per connection.
/// We use `killpg` (kill process group) instead of `kill` so that both the
/// listening parent and any forked children handling active connections are
/// terminated together.
///
/// Uses SIGKILL for instant termination -- socat is a stateless forwarder
/// with nothing to clean up, and checkout/unbind must free ports immediately.
///
/// Each socat process is spawned as its own process group leader (via
/// `setpgid(0, 0)` in `spawn_socat`), so `killpg` only affects that socat
/// tree, not unrelated processes.
///
/// # Errors
///
/// Returns `CoastError::Port` if the signal cannot be sent for reasons other
/// than the process not existing (e.g., permission denied).
pub fn kill_socat(pid: u32) -> Result<()> {
    let nix_pid = Pid::from_raw(pid as i32);

    match signal::killpg(nix_pid, Signal::SIGKILL) {
        Ok(()) => {
            info!(pid = pid, "Sent SIGKILL to socat process group");
            Ok(())
        }
        Err(nix::errno::Errno::ESRCH) => {
            warn!(
                pid = pid,
                "Socat process group already exited (not found), treating as success"
            );
            Ok(())
        }
        Err(e) => Err(CoastError::port(format!(
            "Failed to kill socat process group (PGID {pid}): {e}. \
             You may need to manually kill the process with `kill -9 -- -{pid}`."
        ))),
    }
}

/// Kill all orphaned socat processes that were spawned by coast.
///
/// On daemon startup, there may be leftover socat processes from a previous
/// daemon session (e.g., if the daemon was killed without a clean shutdown).
/// This function finds and kills them to avoid port conflicts.
///
/// Detection: uses `pkill` to kill socat processes matching our command pattern
/// (`fork,reuseaddr` in the arguments — unique to coast's socat usage).
pub fn cleanup_orphaned_socat() {
    // Use pkill to kill all matching socat processes in one shot.
    // The pattern "fork,reuseaddr" is specific to coast's socat forwarding commands.
    match Command::new("pkill")
        .args(["-f", "socat TCP-LISTEN.*fork,reuseaddr"])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                info!("Cleaned up orphaned socat processes from previous session");
            } else {
                debug!("No orphaned socat processes found");
            }
        }
        Err(_) => {
            debug!("pkill not available, skipping orphaned socat cleanup");
        }
    }
}

/// A socat command to restore during daemon startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreSocatCmd {
    /// The socat command arguments to spawn.
    pub cmd: Vec<String>,
    /// Whether this is a canonical (checkout) or dynamic (always-on) forwarder.
    pub is_canonical: bool,
    /// Logical port name for DB updates (e.g., "web").
    pub logical_name: String,
}

/// A port allocation to restore, decoupled from the DB record type.
pub struct PortToRestore<'a> {
    pub logical_name: &'a str,
    pub canonical_port: u16,
    pub dynamic_port: u16,
}

/// Compute the socat commands needed to restore port forwarding for an instance.
///
/// Returns a list of socat commands: dynamic forwarders for all ports, plus
/// canonical forwarders if the instance is checked out. Dynamic commands come
/// first so they are spawned before canonical ones that depend on them.
pub fn restoration_commands(
    ports: &[PortToRestore<'_>],
    coast_ip: &str,
    is_checked_out: bool,
) -> Vec<RestoreSocatCmd> {
    let mut cmds = Vec::new();

    for p in ports {
        cmds.push(RestoreSocatCmd {
            cmd: socat_command_dynamic(p.dynamic_port, coast_ip, p.canonical_port),
            is_canonical: false,
            logical_name: p.logical_name.to_string(),
        });
    }

    if is_checked_out {
        for p in ports {
            cmds.push(RestoreSocatCmd {
                cmd: socat_command_canonical(p.canonical_port, "127.0.0.1", p.dynamic_port),
                is_canonical: true,
                logical_name: p.logical_name.to_string(),
            });
        }
    }

    cmds
}

/// Get a nanosecond timestamp for pseudo-random seed mixing.
/// Falls back to 0 if the system clock is unavailable.
fn timestamp_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_dynamic_port() {
        let port = allocate_dynamic_port().unwrap();

        // Verify it's in the expected range.
        assert!(
            (PORT_RANGE_START..=PORT_RANGE_END).contains(&port),
            "Allocated port {port} is outside range {PORT_RANGE_START}-{PORT_RANGE_END}"
        );

        // Verify the port is actually available (we should be able to bind it).
        let listener = TcpListener::bind(("127.0.0.1", port));
        assert!(
            listener.is_ok(),
            "Allocated port {port} should be bindable but was not"
        );
    }

    #[test]
    fn test_allocate_multiple_ports() {
        let mut ports = Vec::new();

        for _ in 0..10 {
            let port = allocate_dynamic_port().unwrap();
            assert!(
                (PORT_RANGE_START..=PORT_RANGE_END).contains(&port),
                "Port {port} is outside valid range"
            );
            ports.push(port);
        }

        // While rapid allocation may occasionally produce duplicates due to
        // race conditions in testing, all ports should be valid. Check that
        // we got at least some unique ports (not all the same).
        let unique_count = {
            let mut sorted = ports.clone();
            sorted.sort();
            sorted.dedup();
            sorted.len()
        };

        // We should get at least 2 unique ports out of 10 allocations,
        // though in practice we expect all 10 to be unique.
        assert!(
            unique_count >= 2,
            "Expected at least 2 unique ports from 10 allocations, got {unique_count}: {ports:?}"
        );
    }

    #[test]
    fn test_is_port_available_on_free_port() {
        // Binding to port 0 asks the OS to assign any free port. This always
        // succeeds, making `is_port_available(0)` deterministically true
        // without any race window.
        assert!(
            is_port_available(0),
            "Port 0 (OS-assigned) should always be bindable"
        );
    }

    #[test]
    fn test_is_port_available_on_occupied_port() {
        // Bind a port and keep it held.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        // Port should NOT be available while the listener is held.
        assert!(
            !is_port_available(port),
            "Port {port} should NOT be available while occupied"
        );

        drop(listener);
    }

    #[test]
    fn test_socat_command_canonical() {
        let cmd = socat_command_canonical(3000, "172.17.0.2", 3000);

        assert_eq!(cmd.len(), 3);
        assert_eq!(cmd[0], "socat");
        assert_eq!(cmd[1], "TCP-LISTEN:3000,fork,reuseaddr");
        assert_eq!(cmd[2], "TCP:172.17.0.2:3000");
    }

    #[test]
    fn test_socat_command_canonical_different_ports() {
        let cmd = socat_command_canonical(8080, "10.0.0.5", 3000);

        assert_eq!(cmd[0], "socat");
        assert_eq!(cmd[1], "TCP-LISTEN:8080,fork,reuseaddr");
        assert_eq!(cmd[2], "TCP:10.0.0.5:3000");
    }

    #[test]
    fn test_socat_command_dynamic() {
        let cmd = socat_command_dynamic(52345, "172.17.0.2", 3000);

        assert_eq!(cmd.len(), 3);
        assert_eq!(cmd[0], "socat");
        assert_eq!(cmd[1], "TCP-LISTEN:52345,fork,reuseaddr");
        assert_eq!(cmd[2], "TCP:172.17.0.2:3000");
    }

    #[test]
    fn test_socat_command_dynamic_different_ports() {
        let cmd = socat_command_dynamic(60000, "192.168.1.100", 5432);

        assert_eq!(cmd[0], "socat");
        assert_eq!(cmd[1], "TCP-LISTEN:60000,fork,reuseaddr");
        assert_eq!(cmd[2], "TCP:192.168.1.100:5432");
    }

    #[test]
    fn test_socat_commands_have_fork_and_reuseaddr() {
        let cmd = socat_command_canonical(3000, "172.17.0.2", 3000);
        assert!(
            cmd[1].contains("fork"),
            "Canonical socat command should include 'fork' for concurrent connections"
        );
        assert!(
            cmd[1].contains("reuseaddr"),
            "Canonical socat command should include 'reuseaddr' for rapid rebinding"
        );

        let cmd = socat_command_dynamic(50000, "172.17.0.2", 3000);
        assert!(cmd[1].contains("fork"));
        assert!(cmd[1].contains("reuseaddr"));
    }

    #[test]
    fn test_forwarding_pair_struct() {
        let pair = ForwardingPair {
            canonical_pid: 12345,
            dynamic_pid: 67890,
        };

        assert_eq!(pair.canonical_pid, 12345);
        assert_eq!(pair.dynamic_pid, 67890);

        // Test Clone.
        let cloned = pair;
        assert_eq!(cloned, pair);

        // Test Debug.
        let debug_str = format!("{:?}", pair);
        assert!(debug_str.contains("12345"));
        assert!(debug_str.contains("67890"));
    }

    #[test]
    fn test_forwarding_pair_equality() {
        let pair1 = ForwardingPair {
            canonical_pid: 100,
            dynamic_pid: 200,
        };
        let pair2 = ForwardingPair {
            canonical_pid: 100,
            dynamic_pid: 200,
        };
        let pair3 = ForwardingPair {
            canonical_pid: 100,
            dynamic_pid: 300,
        };

        assert_eq!(pair1, pair2);
        assert_ne!(pair1, pair3);
    }

    #[test]
    fn test_kill_socat_nonexistent_pid() {
        // Use a PID that is extremely unlikely to exist.
        // PID 4194304 is beyond the typical max PID on most systems.
        let result = kill_socat(4_194_304);

        // Should succeed because ESRCH (no such process) is treated as success.
        assert!(
            result.is_ok(),
            "Killing a nonexistent PID should succeed (ESRCH is treated as success)"
        );
    }

    #[test]
    fn test_port_forwarder_new() {
        let forwarder = PortForwarder::new();
        assert!(forwarder.active_pairs().is_empty());
    }

    #[test]
    fn test_port_forwarder_default() {
        let forwarder = PortForwarder::default();
        assert!(forwarder.active_pairs().is_empty());
    }

    #[test]
    fn test_port_forwarder_is_active() {
        let forwarder = PortForwarder::new();
        assert!(!forwarder.is_active("test-key"));
    }

    #[test]
    fn test_port_forwarder_stop_forwarding_nonexistent_key() {
        let mut forwarder = PortForwarder::new();

        // Stopping a key that doesn't exist should succeed silently.
        let result = forwarder.stop_forwarding("nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_port_forwarder_stop_canonical_nonexistent_key() {
        let mut forwarder = PortForwarder::new();

        // Stopping canonical for a key that doesn't exist should succeed silently.
        let result = forwarder.stop_canonical("nonexistent");
        assert!(result.is_ok());
    }

    #[test]
    fn test_port_forwarder_stop_forwarding_by_pids_nonexistent() {
        let forwarder = PortForwarder::new();

        // Both PIDs don't exist, but ESRCH is treated as success.
        let result = forwarder.stop_forwarding_by_pids(4_194_304, 4_194_305);
        assert!(result.is_ok());
    }

    #[test]
    fn test_port_forwarder_stop_canonical_by_pid_nonexistent() {
        let forwarder = PortForwarder::new();

        let result = forwarder.stop_canonical_by_pid(4_194_304);
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_socat_command_format() {
        let cmd = build_socat_command(8080, "10.0.0.1", 80);

        assert_eq!(
            cmd,
            vec![
                "socat".to_string(),
                "TCP-LISTEN:8080,fork,reuseaddr".to_string(),
                "TCP:10.0.0.1:80".to_string(),
            ]
        );
    }

    #[test]
    fn test_spawn_socat_empty_command() {
        let result = spawn_socat(&[]);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Empty socat command"),
            "Error message should mention empty command, got: {err_msg}"
        );
    }

    #[test]
    fn test_spawn_socat_nonexistent_binary() {
        let cmd = vec![
            "coast-nonexistent-binary-that-does-not-exist-12345".to_string(),
            "arg1".to_string(),
        ];

        let result = spawn_socat(&cmd);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Failed to spawn socat process"),
            "Error should mention spawn failure, got: {err_msg}"
        );
    }

    #[test]
    fn test_port_range_constants() {
        assert_eq!(PORT_RANGE_START, 49152);
        assert_eq!(PORT_RANGE_END, 65535);
        assert!(
            PORT_RANGE_START < PORT_RANGE_END,
            "range start must be less than end"
        );

        // The ephemeral range should have plenty of ports.
        let range_size = (PORT_RANGE_END - PORT_RANGE_START + 1) as u32;
        assert!(
            range_size > 16000,
            "Ephemeral range should have >16000 ports, got {range_size}"
        );
    }

    #[test]
    fn test_socat_command_with_ipv4_addresses() {
        // Standard Docker bridge.
        let cmd = build_socat_command(3000, "172.17.0.2", 3000);
        assert_eq!(cmd[2], "TCP:172.17.0.2:3000");

        // Custom network.
        let cmd = build_socat_command(5432, "172.18.0.5", 5432);
        assert_eq!(cmd[2], "TCP:172.18.0.5:5432");

        // Localhost.
        let cmd = build_socat_command(8080, "127.0.0.1", 80);
        assert_eq!(cmd[2], "TCP:127.0.0.1:80");
    }

    #[test]
    fn test_allocate_port_is_in_range() {
        for _ in 0..5 {
            let port = allocate_dynamic_port().unwrap();
            assert!(
                (PORT_RANGE_START..=PORT_RANGE_END).contains(&port),
                "Port {port} is outside valid range"
            );
        }
    }

    #[test]
    fn test_socat_command_canonical_with_localhost() {
        // This is how checkout works on macOS: canonical port forwards to
        // localhost:dynamic_port (since container IPs aren't routable on macOS).
        let cmd = socat_command_canonical(33000, "127.0.0.1", 59000);
        assert_eq!(cmd[0], "socat");
        assert_eq!(cmd[1], "TCP-LISTEN:33000,fork,reuseaddr");
        assert_eq!(cmd[2], "TCP:127.0.0.1:59000");
    }

    #[test]
    fn test_socat_canonical_checkout_pattern() {
        // Verify the exact pattern used by the checkout handler:
        // canonical_port listens, forwards to localhost:dynamic_port.
        let canonical = 3000u16;
        let dynamic = 52345u16;
        let cmd = socat_command_canonical(canonical, "127.0.0.1", dynamic);
        assert_eq!(
            cmd,
            vec![
                "socat".to_string(),
                format!("TCP-LISTEN:{canonical},fork,reuseaddr"),
                format!("TCP:127.0.0.1:{dynamic}"),
            ]
        );
    }

    #[test]
    fn test_cleanup_orphaned_socat_no_crash() {
        // cleanup_orphaned_socat should not panic even when there are no
        // socat processes running. It's a best-effort cleanup.
        cleanup_orphaned_socat();
    }

    #[test]
    fn test_cleanup_orphaned_socat_kills_matching_process() {
        // Spawn a real socat process that matches our pattern, then
        // verify cleanup_orphaned_socat kills it.

        // Only run this test if socat is installed.
        if Command::new("socat").arg("-V").output().is_err() {
            return;
        }

        let port = allocate_dynamic_port().unwrap();
        let cmd = build_socat_command(port, "127.0.0.1", port);

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("socat should spawn");

        let pid = child.id();

        // Verify the process is running.
        assert!(
            signal::kill(Pid::from_raw(pid as i32), None).is_ok(),
            "socat process should be running"
        );

        // Run cleanup — should kill our socat process.
        cleanup_orphaned_socat();

        // Reap the child to clear the zombie. Since we spawned it, we must
        // call wait(). In production, orphaned socat processes are parented
        // by init/launchd which handles reaping automatically.
        let status = child.wait().expect("should be able to wait for child");
        assert!(
            !status.success(),
            "socat should have exited due to signal, not exited cleanly"
        );
    }

    #[test]
    fn test_restoration_commands_running_instance() {
        let ports = vec![
            PortToRestore {
                logical_name: "web",
                canonical_port: 3000,
                dynamic_port: 50000,
            },
            PortToRestore {
                logical_name: "backend",
                canonical_port: 8080,
                dynamic_port: 50001,
            },
        ];

        let cmds = restoration_commands(&ports, "172.17.0.2", false);

        assert_eq!(
            cmds.len(),
            2,
            "running instance should only get dynamic socat"
        );
        assert!(cmds.iter().all(|c| !c.is_canonical));
        assert_eq!(cmds[0].logical_name, "web");
        assert_eq!(
            cmds[0].cmd,
            vec![
                "socat",
                "TCP-LISTEN:50000,fork,reuseaddr",
                "TCP:172.17.0.2:3000"
            ]
        );
        assert_eq!(
            cmds[1].cmd,
            vec![
                "socat",
                "TCP-LISTEN:50001,fork,reuseaddr",
                "TCP:172.17.0.2:8080"
            ]
        );
    }

    #[test]
    fn test_restoration_commands_checked_out_instance() {
        let ports = vec![
            PortToRestore {
                logical_name: "web",
                canonical_port: 3000,
                dynamic_port: 50000,
            },
            PortToRestore {
                logical_name: "backend",
                canonical_port: 8080,
                dynamic_port: 50001,
            },
        ];

        let cmds = restoration_commands(&ports, "172.17.0.2", true);

        assert_eq!(
            cmds.len(),
            4,
            "checked-out instance gets dynamic + canonical"
        );

        let dynamic: Vec<_> = cmds.iter().filter(|c| !c.is_canonical).collect();
        let canonical: Vec<_> = cmds.iter().filter(|c| c.is_canonical).collect();

        assert_eq!(dynamic.len(), 2);
        assert_eq!(canonical.len(), 2);

        assert_eq!(
            canonical[0].cmd,
            vec![
                "socat",
                "TCP-LISTEN:3000,fork,reuseaddr",
                "TCP:127.0.0.1:50000"
            ]
        );
        assert_eq!(
            canonical[1].cmd,
            vec![
                "socat",
                "TCP-LISTEN:8080,fork,reuseaddr",
                "TCP:127.0.0.1:50001"
            ]
        );
    }

    #[test]
    fn test_restoration_commands_empty_ports() {
        let cmds = restoration_commands(&[], "172.17.0.2", true);
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_restoration_commands_dynamic_before_canonical() {
        let ports = vec![PortToRestore {
            logical_name: "web",
            canonical_port: 3000,
            dynamic_port: 50000,
        }];
        let cmds = restoration_commands(&ports, "172.17.0.2", true);

        assert_eq!(cmds.len(), 2);
        assert!(!cmds[0].is_canonical, "dynamic should come first");
        assert!(cmds[1].is_canonical, "canonical should come second");
    }

    #[test]
    fn test_restoration_commands_preserves_logical_name() {
        let ports = vec![
            PortToRestore {
                logical_name: "web",
                canonical_port: 3000,
                dynamic_port: 50000,
            },
            PortToRestore {
                logical_name: "postgres",
                canonical_port: 5432,
                dynamic_port: 50002,
            },
        ];
        let cmds = restoration_commands(&ports, "10.0.0.1", false);

        assert_eq!(cmds[0].logical_name, "web");
        assert_eq!(cmds[1].logical_name, "postgres");
    }
}
