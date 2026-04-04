/// `coast daemon` — manage the coastd daemon process.
///
/// Provides subcommands to check status, start, stop (kill), restart,
/// install (auto-start at login), and uninstall the background daemon.
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use rust_i18n::t;
use std::io::{self, Read as _, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Arguments for the `coast daemon` command.
#[derive(Debug, Args)]
pub struct DaemonArgs {
    /// The daemon management action to perform.
    #[command(subcommand)]
    pub action: DaemonAction,
}

/// Available daemon management actions.
#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Check if the daemon is running.
    Status,
    /// Stop the daemon process.
    Kill {
        /// Send SIGKILL immediately instead of graceful SIGTERM.
        #[arg(long, short)]
        force: bool,
    },
    /// Start the daemon process.
    Start,
    /// Restart the daemon process (kill + start).
    Restart {
        /// Force-kill the daemon if it doesn't stop gracefully.
        #[arg(long, short)]
        force: bool,
    },
    /// Show daemon logs.
    Logs {
        /// Follow log output in real time (like tail -f).
        #[arg(long)]
        tail: bool,
    },
    /// Register coastd to start automatically at login.
    Install,
    /// Remove the automatic startup registration.
    Uninstall,
}

/// Resolved daemon status information.
struct DaemonStatus {
    pid: Option<u32>,
    running: bool,
    socket_exists: bool,
}

pub(crate) fn pid_path() -> Result<PathBuf> {
    Ok(coast_core::artifact::coast_home()?.join("coastd.pid"))
}

pub(crate) fn socket_path() -> Result<PathBuf> {
    Ok(coast_core::artifact::coast_home()?.join("coastd.sock"))
}

fn log_path() -> Result<PathBuf> {
    Ok(coast_core::artifact::coast_home()?.join("coastd.log"))
}

/// Find the matching `coastd` binary. When the current executable is
/// `coast-dev`, looks for `coastd-dev`; when it's `coast`, looks for `coastd`.
/// Checks next to the current executable first, then falls back to PATH.
fn resolve_coastd_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let exe_name = exe.file_name().unwrap_or_default().to_string_lossy();
        let daemon_name = if let Some(suffix) = exe_name.strip_prefix("coast") {
            format!("coastd{suffix}")
        } else {
            "coastd".to_string()
        };
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(&daemon_name);
            if sibling.exists() {
                return sibling;
            }
        }
        return PathBuf::from(daemon_name);
    }
    PathBuf::from("coastd")
}

#[cfg(target_os = "macos")]
fn launchd_path_value() -> Option<String> {
    let current = std::env::var_os("PATH");
    let existing_entries: Vec<PathBuf> = current
        .as_ref()
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();
    let updated_entries = extend_path_entries(
        existing_entries,
        [
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]
        .into_iter()
        .filter(|path| path.is_dir()),
    );

    std::env::join_paths(updated_entries)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[cfg(not(target_os = "macos"))]
fn launchd_path_value() -> Option<String> {
    None
}

#[cfg(any(target_os = "macos", test))]
fn extend_path_entries<I>(mut existing_entries: Vec<PathBuf>, candidates: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    for candidate in candidates {
        if !existing_entries.iter().any(|entry| entry == &candidate) {
            existing_entries.push(candidate);
        }
    }

    existing_entries
}

/// Read and parse the PID from `~/.coast/coastd.pid`.
/// Returns `None` if the file doesn't exist or contains invalid content.
pub(crate) fn read_pid(path: &PathBuf) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// Check whether a process with the given PID is alive using signal 0.
pub(crate) fn is_process_alive(pid: u32) -> bool {
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

pub(crate) fn is_running(pid: u32) -> bool {
    is_process_alive(pid)
}

fn has_command_boundary(rest: &str) -> bool {
    match rest.chars().next() {
        None => true,
        Some('\0') => true,
        Some(ch) => ch.is_whitespace(),
    }
}

fn command_starts_with_executable(command: &str, executable: &str) -> bool {
    command
        .strip_prefix(executable)
        .is_some_and(has_command_boundary)
}

fn command_matches_daemon(command: &str, daemon_path: &Path) -> bool {
    let command = command.trim_start();
    let daemon_path_str = daemon_path.to_string_lossy();
    let daemon_name = daemon_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let executable = command.split('\0').next().unwrap_or(command).trim();

    if command_starts_with_executable(command, daemon_path_str.as_ref())
        || command_starts_with_executable(executable, daemon_path_str.as_ref())
        || command_starts_with_executable(command, daemon_name.as_ref())
        || command_starts_with_executable(executable, daemon_name.as_ref())
    {
        return true;
    }

    let first_token = executable.split_whitespace().next().unwrap_or(executable);

    Path::new(first_token)
        .file_name()
        .map(|name| name == std::ffi::OsStr::new(daemon_name.as_ref()))
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn process_command(pid: u32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if cmdline.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&cmdline).into_owned())
}

#[cfg(not(target_os = "linux"))]
fn process_command(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!command.is_empty()).then_some(command)
}

fn is_daemon_process_running(pid: u32) -> bool {
    let daemon_path = resolve_coastd_path();
    is_process_alive(pid)
        && process_command(pid)
            .map(|command| command_matches_daemon(&command, &daemon_path))
            .unwrap_or(false)
}

fn daemon_status() -> Result<DaemonStatus> {
    let pid_file = pid_path()?;
    let sock_file = socket_path()?;

    let pid = read_pid(&pid_file);
    let running = pid.is_some_and(is_daemon_process_running);
    let socket_exists = sock_file.exists();

    Ok(DaemonStatus {
        pid,
        running,
        socket_exists,
    })
}

pub(crate) fn is_daemon_running() -> Result<bool> {
    Ok(daemon_status()?.running)
}

/// Execute the `coast daemon` command.
pub async fn execute(args: &DaemonArgs) -> Result<()> {
    match &args.action {
        DaemonAction::Status => execute_status().await,
        DaemonAction::Kill { force } => execute_kill(*force).await,
        DaemonAction::Start => execute_start().await,
        DaemonAction::Restart { force } => execute_restart(*force).await,
        DaemonAction::Logs { tail } => execute_logs(*tail).await,
        DaemonAction::Install => execute_install().await,
        DaemonAction::Uninstall => execute_uninstall().await,
    }
}

async fn execute_status() -> Result<()> {
    let status = daemon_status()?;

    if status.running {
        let pid = status.pid.unwrap();
        println!(
            "{} {} (pid: {})",
            "coastd".bold(),
            "is running".green().bold(),
            pid
        );
        if status.socket_exists {
            let sock = socket_path()?;
            println!("  socket: {}", sock.display());
        }
        let api_port: u16 = std::env::var("COAST_API_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(31415);
        println!("  api:    http://localhost:{api_port}");
    } else {
        println!("{} {}", "coastd".bold(), "is not running".red().bold());
        if status.pid.is_some() {
            println!(
                "  {} stale PID file exists (process is dead)",
                "warning:".yellow().bold()
            );
        }
    }

    Ok(())
}

/// Kill any remaining coastd processes by binary path. This catches
/// launchd-respawned or orphaned daemons that aren't tracked by the PID file.
/// Uses the full resolved path for precise matching (won't hit unrelated processes).
fn kill_all_coastd_processes() {
    let coastd_path = resolve_coastd_path();
    let path_str = coastd_path.to_string_lossy().to_string();
    if path_str.is_empty() {
        return;
    }
    // -f matches against the full command line (including path).
    // Using the resolved path ensures we only kill our own coastd, not
    // other users' or other installations.
    let _ = std::process::Command::new("pkill")
        .args(["-9", "-f", &path_str])
        .status();
}

pub(crate) async fn execute_kill(force: bool) -> Result<()> {
    let status = daemon_status()?;

    if !status.running {
        if status.pid.is_some() {
            cleanup_stale_files()?;
            println!("coastd is not running (cleaned up stale PID file)");
        } else {
            println!("coastd is not running");
        }
        kill_all_coastd_processes();
        return Ok(());
    }

    let pid = status.pid.unwrap();
    let nix_pid = Pid::from_raw(pid as i32);

    // Tell the service manager to stop managing coastd BEFORE killing it.
    // Without this, launchd (macOS) or systemd (Linux) will immediately
    // respawn the daemon after we kill the process.
    unload_service_manager();

    if force {
        eprint!("force-killing coastd (pid: {pid})...");
        match signal::kill(nix_pid, Signal::SIGKILL) {
            Ok(()) => {}
            Err(nix::errno::Errno::ESRCH) => {
                eprintln!();
                cleanup_stale_files()?;
                kill_all_coastd_processes();
                println!("coastd is not running (cleaned up stale PID file)");
                return Ok(());
            }
            Err(e) => return Err(e).context("Failed to send SIGKILL to coastd"),
        }
    } else {
        eprint!("stopping coastd (pid: {pid})...");
        match signal::kill(nix_pid, Signal::SIGTERM) {
            Ok(()) => {}
            Err(nix::errno::Errno::ESRCH) => {
                eprintln!();
                cleanup_stale_files()?;
                kill_all_coastd_processes();
                println!("coastd is not running (cleaned up stale PID file)");
                return Ok(());
            }
            Err(e) => return Err(e).context("Failed to send SIGTERM to coastd"),
        }
    }

    let graceful_timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        if !is_daemon_process_running(pid) {
            eprintln!();
            println!("{}", "coastd stopped".green());
            cleanup_stale_files()?;
            kill_all_coastd_processes();
            return Ok(());
        }
        if start.elapsed() > graceful_timeout {
            if force {
                eprintln!();
                kill_all_coastd_processes();
                cleanup_stale_files()?;
                bail!("coastd (pid: {pid}) did not exit after SIGKILL. This is unexpected.");
            }
            // Auto-escalate to SIGKILL
            eprintln!(" escalating to SIGKILL");
            signal::kill(nix_pid, Signal::SIGKILL).context("Failed to send SIGKILL to coastd")?;

            let kill_timeout = std::time::Duration::from_secs(5);
            let kill_start = std::time::Instant::now();
            loop {
                if !is_daemon_process_running(pid) {
                    println!("{}", "coastd killed".green());
                    cleanup_stale_files()?;
                    kill_all_coastd_processes();
                    return Ok(());
                }
                if kill_start.elapsed() > kill_timeout {
                    kill_all_coastd_processes();
                    cleanup_stale_files()?;
                    bail!("coastd (pid: {pid}) did not exit after SIGKILL. This is unexpected.");
                }
                tokio::time::sleep(poll_interval).await;
            }
        }
        tokio::time::sleep(poll_interval).await;
    }
}

pub(crate) async fn execute_start() -> Result<()> {
    let status = daemon_status()?;

    if status.running {
        let pid = status.pid.unwrap();
        // Verify the daemon is healthy by checking its socket. A
        // launchd-respawned daemon from a previous version may have a PID file
        // but a missing or unresponsive socket.
        let sock = socket_path()?;
        let socket_ok = sock.exists() && tokio::net::UnixStream::connect(&sock).await.is_ok();
        if socket_ok {
            println!("coastd is already running (pid: {pid})");
            return Ok(());
        }
        eprintln!("killing stale coastd (pid: {pid}, socket not responding)");
        let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        kill_all_coastd_processes();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        cleanup_stale_files()?;
    } else if status.pid.is_some() {
        cleanup_stale_files()?;
    }

    // Kill any stale coastd processes not tracked by the PID file (e.g.
    // launchd-respawned from a previous version holding the flock).
    kill_all_coastd_processes();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let coastd = resolve_coastd_path();
    let child = std::process::Command::new(&coastd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "Failed to start coastd at '{}'. Is it installed?\n  \
                 Install with: cargo install --path coast-daemon",
                coastd.display()
            )
        })?;

    let child_pid = child.id();

    // The daemon daemonizes itself (fork+setsid), so the spawned process exits
    // quickly and the real daemon PID ends up in the PID file. Give it a moment
    // to write the PID file, then read the actual daemon PID from it.
    let timeout = std::time::Duration::from_secs(5);
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(100);
    let pid_file = pid_path()?;

    loop {
        if let Some(pid) = read_pid(&pid_file) {
            if is_daemon_process_running(pid) {
                reload_service_manager();
                println!("{} (pid: {pid})", "coastd started".green());
                return Ok(());
            }
        }
        if start.elapsed() > timeout {
            let log = log_path().unwrap_or_default();
            let coastd_name = resolve_coastd_path()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            bail!(
                "coastd did not start within 5 seconds.\n  \
                 Check {} for errors.\n  \
                 Spawned process PID was {child_pid}.\n  \
                 If the log says \"another coastd is already running\", run:\n    \
                 killall {coastd_name} && coast daemon start",
                log.display()
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn execute_restart(force: bool) -> Result<()> {
    let status = daemon_status()?;
    if status.running {
        execute_kill(force).await?;
    }
    execute_start().await
}

/// Restart the daemon if it's currently running. Used after updates
/// to pick up the new binary.
pub async fn restart_daemon_if_running() -> Result<()> {
    let status = daemon_status()?;

    if status.running {
        execute_kill(false).await?;
        execute_start().await?;
    }

    Ok(())
}

const TAIL_LINES: usize = 20;

async fn execute_logs(tail: bool) -> Result<()> {
    let path = log_path()?;

    if !path.exists() {
        bail!(
            "No log file found at {}. \
             The daemon may not have been started yet, or is running in foreground mode.",
            path.display()
        );
    }

    if tail {
        let mut file = std::fs::File::open(&path)
            .with_context(|| format!("Failed to open {}", path.display()))?;

        // Print the last N lines first, like `tail -f` does.
        let last_lines = read_last_n_lines(&mut file, TAIL_LINES)?;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for line in &last_lines {
            out.write_all(line.as_bytes())?;
            out.write_all(b"\n")?;
        }
        out.flush()?;
        drop(out);

        // Now follow from the current position (end of file).
        let poll_interval = std::time::Duration::from_millis(200);
        let mut buf = vec![0u8; 8192];

        loop {
            match file.read(&mut buf) {
                Ok(0) => {
                    tokio::time::sleep(poll_interval).await;
                }
                Ok(n) => {
                    let mut out = io::stdout().lock();
                    out.write_all(&buf[..n])?;
                    out.flush()?;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => bail!("Error reading log file: {e}"),
            }
        }
    } else {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        print!("{contents}");
        Ok(())
    }
}

/// Read the last `n` lines from a file, then leave the file positioned at EOF.
fn read_last_n_lines(file: &mut std::fs::File, n: usize) -> Result<Vec<String>> {
    let metadata = file.metadata()?;
    let file_len = metadata.len();

    if file_len == 0 {
        return Ok(vec![]);
    }

    // Read up to 64KB from the end — more than enough for the last N lines.
    let read_size = std::cmp::min(file_len, 64 * 1024) as usize;
    let start = file_len - read_size as u64;
    file.seek(SeekFrom::Start(start))?;

    let mut buf = vec![0u8; read_size];
    file.read_exact(&mut buf)?;

    let text = String::from_utf8_lossy(&buf);
    let all_lines: Vec<&str> = text.lines().collect();

    // If we started mid-file, skip the first (likely partial) line.
    let skip = if start > 0 { 1 } else { 0 };
    let lines: Vec<String> = all_lines
        .iter()
        .skip(skip)
        .rev()
        .take(n)
        .rev()
        .map(ToString::to_string)
        .collect();

    file.seek(SeekFrom::End(0))?;
    Ok(lines)
}

// ---------------------------------------------------------------------------
// Install / Uninstall — auto-start coastd at login
// ---------------------------------------------------------------------------

const LAUNCHD_LABEL: &str = "com.coast.coastd";
const SYSTEMD_SERVICE: &str = "coastd.service";

/// Tell the system service manager to stop managing coastd. On macOS this
/// runs `launchctl unload` on the plist; on Linux it runs `systemctl --user
/// stop`. Prevents the service manager from respawning the daemon after we
/// kill it (the root cause of the stale-daemon-after-update bug).
fn unload_service_manager() {
    if cfg!(target_os = "macos") {
        if let Ok(plist) = launchd_plist_path() {
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["unload", &plist.to_string_lossy()])
                    .status();
            }
        }
    } else if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "stop", SYSTEMD_SERVICE])
            .status();
    }
}

/// Re-register coastd with the system service manager after starting.
/// On macOS this runs `launchctl load`; on Linux `systemctl --user start`.
fn reload_service_manager() {
    if cfg!(target_os = "macos") {
        if let Ok(plist) = launchd_plist_path() {
            if plist.exists() {
                let _ = std::process::Command::new("launchctl")
                    .args(["load", &plist.to_string_lossy()])
                    .status();
            }
        }
    } else if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", SYSTEMD_SERVICE])
            .status();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallPlatform {
    MacOs,
    Linux,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceManagerCommand {
    program: &'static str,
    args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstallPlan {
    print_already_installed_note: bool,
    write_registration: bool,
    ensure_running: bool,
}

fn current_install_platform() -> Result<InstallPlatform> {
    if cfg!(target_os = "macos") {
        Ok(InstallPlatform::MacOs)
    } else if cfg!(target_os = "linux") {
        Ok(InstallPlatform::Linux)
    } else {
        bail!("Automatic daemon installation is only supported on macOS and Linux.");
    }
}

fn build_install_plan(registration_exists: bool, daemon_running: bool) -> InstallPlan {
    InstallPlan {
        print_already_installed_note: registration_exists,
        write_registration: !registration_exists,
        ensure_running: !registration_exists || !daemon_running,
    }
}

fn install_registration_path(platform: InstallPlatform) -> Result<PathBuf> {
    match platform {
        InstallPlatform::MacOs => launchd_plist_path(),
        InstallPlatform::Linux => systemd_unit_path(),
    }
}

fn already_registered_message(path: &Path) -> String {
    format!("coastd is already registered at {}.", path.display())
}

fn write_install_registration(
    platform: InstallPlatform,
    registration_path: &Path,
    coastd_path: &str,
    log_dir: &str,
) -> Result<()> {
    if let Some(parent) = registration_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = match platform {
        InstallPlatform::MacOs => generate_launchd_plist(coastd_path, log_dir),
        InstallPlatform::Linux => generate_systemd_unit(coastd_path),
    };

    std::fs::write(registration_path, &content)
        .with_context(|| format!("Failed to write {}", registration_path.display()))?;

    Ok(())
}

fn ensure_registered_daemon_command(
    platform: InstallPlatform,
    registration_path: &Path,
) -> ServiceManagerCommand {
    match platform {
        InstallPlatform::MacOs => ServiceManagerCommand {
            program: "launchctl",
            args: vec![
                "load".to_string(),
                registration_path.to_string_lossy().into_owned(),
            ],
        },
        InstallPlatform::Linux => ServiceManagerCommand {
            program: "systemctl",
            args: vec![
                "--user".to_string(),
                "enable".to_string(),
                "--now".to_string(),
                "coastd".to_string(),
            ],
        },
    }
}

fn running_in_wsl() -> bool {
    if std::env::var_os("WSL_DISTRO_NAME").is_some() || std::env::var_os("WSL_INTEROP").is_some() {
        return true;
    }
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn systemctl_available() -> bool {
    std::process::Command::new("systemctl")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn run_ensure_registered_daemon(platform: InstallPlatform, registration_path: &Path) -> Result<()> {
    if platform == InstallPlatform::Linux && !systemctl_available() {
        let wsl_hint = if running_in_wsl() {
            "\n\n  To enable systemd in WSL, add to /etc/wsl.conf:\n    \
             [boot]\n    \
             systemd=true\n  \
             Then restart WSL: wsl --shutdown (from PowerShell)\n"
        } else {
            ""
        };

        eprintln!(
            "{} systemctl is not available — the systemd unit was written to\n  \
             {} but could not be enabled.{}\n\n  \
             Starting the daemon directly instead...",
            "warning:".yellow().bold(),
            registration_path.display(),
            wsl_hint,
        );

        return Ok(());
    }

    let command = ensure_registered_daemon_command(platform, registration_path);
    let status = std::process::Command::new(command.program)
        .args(&command.args)
        .status()
        .with_context(|| match platform {
            InstallPlatform::MacOs => "Failed to run launchctl load".to_string(),
            InstallPlatform::Linux => {
                "Failed to run systemctl --user enable --now coastd".to_string()
            }
        })?;

    if !status.success() {
        match platform {
            InstallPlatform::MacOs => {
                bail!("launchctl load failed (exit code {:?})", status.code())
            }
            InstallPlatform::Linux => bail!(
                "systemctl --user enable --now coastd failed (exit code {:?})",
                status.code()
            ),
        }
    }

    Ok(())
}

/// Path to the macOS Launch Agent plist.
fn launchd_plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist")))
}

/// Path to the systemd user service unit.
fn systemd_unit_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join(SYSTEMD_SERVICE))
}

/// Generate a macOS launchd plist that starts `coastd --foreground` at login.
pub fn generate_launchd_plist(coastd_path: &str, log_dir: &str) -> String {
    let env_block = launchd_path_value()
        .map(|path| {
            format!(
                "    <key>EnvironmentVariables</key>\n    <dict>\n        <key>PATH</key>\n        <string>{path}</string>\n    </dict>\n"
            )
        })
        .unwrap_or_default();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{coastd_path}</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
{env_block}
    <key>StandardOutPath</key>
    <string>{log_dir}/coastd.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/coastd.stderr.log</string>
</dict>
</plist>
"#
    )
}

/// Generate a systemd user service unit that starts `coastd --foreground`.
pub fn generate_systemd_unit(coastd_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Coast Daemon\n\
         \n\
         [Service]\n\
         ExecStart={coastd_path} --foreground\n\
         Restart=on-failure\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

async fn execute_install() -> Result<()> {
    let platform = current_install_platform()?;
    let registration_path = install_registration_path(platform)?;
    let status = daemon_status()?;
    let plan = build_install_plan(registration_path.exists(), status.running);
    let coastd = resolve_coastd_path();
    let coastd_str = coastd.to_string_lossy();
    let coast_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".coast");
    std::fs::create_dir_all(&coast_dir)?;
    let log_dir = coast_dir.to_string_lossy();

    if plan.print_already_installed_note {
        println!(
            "{} {}",
            "note".cyan().bold(),
            already_registered_message(&registration_path)
        );
    }

    if plan.write_registration {
        write_install_registration(platform, &registration_path, &coastd_str, &log_dir)?;
    }

    let needs_direct_start = if plan.ensure_running {
        let no_systemctl = platform == InstallPlatform::Linux && !systemctl_available();
        run_ensure_registered_daemon(platform, &registration_path)?;
        no_systemctl
    } else {
        false
    };

    if needs_direct_start && !daemon_status()?.running {
        execute_start().await?;
    }

    if plan.write_registration {
        println!(
            "{} {}",
            "ok".green().bold(),
            t!(
                "cli.ok.daemon_installed",
                path = registration_path.display().to_string()
            ),
        );
    }

    Ok(())
}

async fn execute_uninstall() -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist_path = launchd_plist_path()?;
        if !plist_path.exists() {
            println!(
                "{} {}",
                "note".cyan().bold(),
                t!("cli.info.daemon_not_installed"),
            );
            return Ok(());
        }

        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();

        std::fs::remove_file(&plist_path)
            .with_context(|| format!("Failed to remove {}", plist_path.display()))?;

        println!(
            "{} {}",
            "ok".green().bold(),
            t!("cli.ok.daemon_uninstalled"),
        );
    } else if cfg!(target_os = "linux") {
        let unit_path = systemd_unit_path()?;
        if !unit_path.exists() {
            println!(
                "{} {}",
                "note".cyan().bold(),
                t!("cli.info.daemon_not_installed"),
            );
            return Ok(());
        }

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "coastd"])
            .status();

        std::fs::remove_file(&unit_path)
            .with_context(|| format!("Failed to remove {}", unit_path.display()))?;

        println!(
            "{} {}",
            "ok".green().bold(),
            t!("cli.ok.daemon_uninstalled"),
        );
    } else {
        bail!("Automatic daemon installation is only supported on macOS and Linux.");
    }

    Ok(())
}

/// Remove stale PID and socket files left behind by a dead daemon.
pub(crate) fn cleanup_stale_files() -> Result<()> {
    let pid_file = pid_path()?;
    if pid_file.exists() {
        let _ = std::fs::remove_file(&pid_file);
    }
    let sock_file = socket_path()?;
    if sock_file.exists() {
        let _ = std::fs::remove_file(&sock_file);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_read_pid_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "12345\n").unwrap();
        assert_eq!(read_pid(&path.to_path_buf()), Some(12345));
    }

    #[test]
    fn test_read_pid_no_newline() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "67890").unwrap();
        assert_eq!(read_pid(&path.to_path_buf()), Some(67890));
    }

    #[test]
    fn test_read_pid_invalid_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "not-a-pid").unwrap();
        assert_eq!(read_pid(&path.to_path_buf()), None);
    }

    #[test]
    fn test_read_pid_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "").unwrap();
        assert_eq!(read_pid(&path.to_path_buf()), None);
    }

    #[test]
    fn test_read_pid_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.pid");
        assert_eq!(read_pid(&path.to_path_buf()), None);
    }

    #[test]
    fn test_is_process_alive_current_process() {
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn test_is_process_alive_dead_pid() {
        // PID 99999999 is almost certainly not running
        assert!(!is_process_alive(99_999_999));
    }

    #[test]
    fn test_stale_pid_detected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "99999999").unwrap();

        let pid = read_pid(&path.to_path_buf());
        assert_eq!(pid, Some(99_999_999));
        assert!(!is_process_alive(pid.unwrap()));
    }

    #[test]
    fn test_command_matches_daemon_for_absolute_path() {
        assert!(command_matches_daemon(
            "/usr/local/bin/coastd --flag",
            Path::new("coastd")
        ));
    }

    #[test]
    fn test_command_matches_daemon_for_linux_cmdline() {
        assert!(command_matches_daemon(
            "/usr/local/bin/coastd\0--foreground\0",
            Path::new("coastd")
        ));
    }

    #[test]
    fn test_command_matches_daemon_for_path_with_spaces() {
        assert!(command_matches_daemon(
            "/Volumes/Coast Tools/coastd --foreground",
            Path::new("/Volumes/Coast Tools/coastd")
        ));
    }

    #[test]
    fn test_command_matches_daemon_rejects_other_processes() {
        assert!(!command_matches_daemon(
            "/usr/bin/bash -lc coastd",
            Path::new("coastd")
        ));
    }

    #[test]
    fn test_read_last_n_lines_empty_file() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 5).unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn test_read_last_n_lines_fewer_than_n() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(tmp.as_file_mut(), b"line1\nline2\nline3\n").unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 5).unwrap();
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn test_read_last_n_lines_more_than_n() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(tmp.as_file_mut(), b"line1\nline2\nline3\nline4\nline5\n")
            .unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 3).unwrap();
        assert_eq!(lines, vec!["line3", "line4", "line5"]);
    }

    #[test]
    fn test_read_last_n_lines_single_line() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(tmp.as_file_mut(), b"only\n").unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 5).unwrap();
        assert_eq!(lines, vec!["only"]);
    }

    #[test]
    fn test_read_last_n_lines_no_trailing_newline() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(tmp.as_file_mut(), b"alpha\nbeta\ngamma").unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 5).unwrap();
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_generate_launchd_plist_content() {
        let plist = generate_launchd_plist("/usr/local/bin/coastd", "/Users/test/.coast");
        let lines: Vec<&str> = plist.lines().collect();
        let stdout_path_index = lines
            .iter()
            .position(|line| *line == "    <key>StandardOutPath</key>")
            .unwrap();

        assert!(plist.contains("<string>/usr/local/bin/coastd</string>"));
        assert!(plist.contains("<string>--foreground</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains(&format!("<string>{LAUNCHD_LABEL}</string>")));
        assert!(plist.contains("/Users/test/.coast/coastd.stdout.log"));
        assert!(plist.contains("/Users/test/.coast/coastd.stderr.log"));
        assert!(plist.starts_with("<?xml"));
        assert!(plist.contains("<key>StandardOutPath</key>"));
        assert!(!plist.lines().any(|line| line == "\\"));
        assert_ne!(lines[stdout_path_index - 1], "\\");
        #[cfg(target_os = "macos")]
        {
            assert!(plist.contains("<key>EnvironmentVariables</key>"));
            assert_eq!(lines[stdout_path_index - 2], "    </dict>");
        }
    }

    #[test]
    fn test_extend_path_entries_appends_only_missing_candidates() {
        let updated = extend_path_entries(
            vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")],
            [
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/bin"),
            ],
        );

        assert_eq!(
            updated,
            vec![
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
                PathBuf::from("/opt/homebrew/bin"),
            ]
        );
    }

    #[test]
    fn test_generate_systemd_unit_content() {
        let unit = generate_systemd_unit("/opt/coast/coastd");
        assert!(unit.contains("ExecStart=/opt/coast/coastd --foreground"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
        assert!(unit.contains("Description=Coast Daemon"));
    }

    #[test]
    fn test_build_install_plan() {
        let cases = [
            (
                "fresh install",
                false,
                false,
                InstallPlan {
                    print_already_installed_note: false,
                    write_registration: true,
                    ensure_running: true,
                },
            ),
            (
                "fresh install while daemon already running",
                false,
                true,
                InstallPlan {
                    print_already_installed_note: false,
                    write_registration: true,
                    ensure_running: true,
                },
            ),
            (
                "already installed and running",
                true,
                true,
                InstallPlan {
                    print_already_installed_note: true,
                    write_registration: false,
                    ensure_running: false,
                },
            ),
            (
                "already installed but stopped",
                true,
                false,
                InstallPlan {
                    print_already_installed_note: true,
                    write_registration: false,
                    ensure_running: true,
                },
            ),
        ];

        for (name, registration_exists, daemon_running, expected) in cases {
            assert_eq!(
                build_install_plan(registration_exists, daemon_running),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn test_ensure_registered_daemon_command_selection() {
        let plist_path = PathBuf::from("/Users/test/Library/LaunchAgents/com.coast.coastd.plist");
        assert_eq!(
            ensure_registered_daemon_command(InstallPlatform::MacOs, &plist_path),
            ServiceManagerCommand {
                program: "launchctl",
                args: vec![
                    "load".to_string(),
                    "/Users/test/Library/LaunchAgents/com.coast.coastd.plist".to_string(),
                ],
            }
        );

        let unit_path = PathBuf::from("/Users/test/.config/systemd/user/coastd.service");
        assert_eq!(
            ensure_registered_daemon_command(InstallPlatform::Linux, &unit_path),
            ServiceManagerCommand {
                program: "systemctl",
                args: vec![
                    "--user".to_string(),
                    "enable".to_string(),
                    "--now".to_string(),
                    "coastd".to_string(),
                ],
            }
        );
    }

    #[test]
    fn test_daemon_install_parse() {
        use clap::Parser;
        #[derive(Debug, Parser)]
        struct Cli {
            #[command(flatten)]
            args: DaemonArgs,
        }
        let cli = Cli::try_parse_from(["test", "install"]).unwrap();
        assert!(matches!(cli.args.action, DaemonAction::Install));
    }

    #[test]
    fn test_daemon_uninstall_parse() {
        use clap::Parser;
        #[derive(Debug, Parser)]
        struct Cli {
            #[command(flatten)]
            args: DaemonArgs,
        }
        let cli = Cli::try_parse_from(["test", "uninstall"]).unwrap();
        assert!(matches!(cli.args.action, DaemonAction::Uninstall));
    }

    #[test]
    fn test_read_last_n_lines_exactly_n() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(tmp.as_file_mut(), b"one\ntwo\nthree\nfour\n").unwrap();
        let file = tmp.as_file_mut();
        let lines = read_last_n_lines(file, 4).unwrap();
        assert_eq!(lines, vec!["one", "two", "three", "four"]);
    }

    #[test]
    fn test_unload_service_manager_does_not_panic() {
        unload_service_manager();
    }

    #[test]
    fn test_reload_service_manager_does_not_panic() {
        reload_service_manager();
    }

    #[test]
    fn test_kill_all_coastd_processes_does_not_panic() {
        kill_all_coastd_processes();
    }

    #[test]
    fn test_resolve_coastd_path_is_not_empty() {
        let path = resolve_coastd_path();
        assert!(
            !path.to_string_lossy().is_empty(),
            "resolved coastd path should not be empty"
        );
    }
}
