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
use std::path::PathBuf;

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

/// Read and parse the PID from `~/.coast/coastd.pid`.
/// Returns `None` if the file doesn't exist or contains invalid content.
pub(crate) fn read_pid(path: &PathBuf) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// Check whether a process with the given PID is alive using signal 0.
pub(crate) fn is_running(pid: u32) -> bool {
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

fn daemon_status() -> Result<DaemonStatus> {
    let pid_file = pid_path()?;
    let sock_file = socket_path()?;

    let pid = read_pid(&pid_file);
    let running = pid.is_some_and(is_running);
    let socket_exists = sock_file.exists();

    Ok(DaemonStatus {
        pid,
        running,
        socket_exists,
    })
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

pub(crate) async fn execute_kill(force: bool) -> Result<()> {
    let status = daemon_status()?;

    if !status.running {
        if status.pid.is_some() {
            cleanup_stale_files()?;
            println!("coastd is not running (cleaned up stale PID file)");
        } else {
            println!("coastd is not running");
        }
        return Ok(());
    }

    let pid = status.pid.unwrap();
    let nix_pid = Pid::from_raw(pid as i32);

    if force {
        eprint!("force-killing coastd (pid: {pid})...");
        signal::kill(nix_pid, Signal::SIGKILL).context("Failed to send SIGKILL to coastd")?;
    } else {
        eprint!("stopping coastd (pid: {pid})...");
        signal::kill(nix_pid, Signal::SIGTERM).context("Failed to send SIGTERM to coastd")?;
    }

    let graceful_timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(100);

    loop {
        if !is_running(pid) {
            eprintln!();
            println!("{}", "coastd stopped".green());
            cleanup_stale_files()?;
            return Ok(());
        }
        if start.elapsed() > graceful_timeout {
            if force {
                eprintln!();
                bail!("coastd (pid: {pid}) did not exit after SIGKILL. This is unexpected.");
            }
            // Auto-escalate to SIGKILL
            eprintln!(" escalating to SIGKILL");
            signal::kill(nix_pid, Signal::SIGKILL).context("Failed to send SIGKILL to coastd")?;

            let kill_timeout = std::time::Duration::from_secs(5);
            let kill_start = std::time::Instant::now();
            loop {
                if !is_running(pid) {
                    println!("{}", "coastd killed".green());
                    cleanup_stale_files()?;
                    return Ok(());
                }
                if kill_start.elapsed() > kill_timeout {
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
        println!("coastd is already running (pid: {pid})");
        return Ok(());
    }

    if status.pid.is_some() {
        cleanup_stale_files()?;
    }

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
            if is_running(pid) {
                println!("{} (pid: {pid})", "coastd started".green());
                return Ok(());
            }
        }
        if start.elapsed() > timeout {
            let log = log_path().unwrap_or_default();
            bail!(
                "coastd did not start within 5 seconds. \
                 Check {} for errors. \
                 Spawned process PID was {child_pid}.",
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
    let coastd = resolve_coastd_path();
    let coastd_str = coastd.to_string_lossy();
    let coast_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".coast");
    std::fs::create_dir_all(&coast_dir)?;
    let log_dir = coast_dir.to_string_lossy();

    if cfg!(target_os = "macos") {
        let plist_path = launchd_plist_path()?;
        if plist_path.exists() {
            println!(
                "{} {}",
                "note".cyan().bold(),
                t!(
                    "cli.info.daemon_already_installed",
                    path = plist_path.display().to_string()
                ),
            );
            return Ok(());
        }
        if let Some(parent) = plist_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = generate_launchd_plist(&coastd_str, &log_dir);
        std::fs::write(&plist_path, &content)
            .with_context(|| format!("Failed to write {}", plist_path.display()))?;

        let status = std::process::Command::new("launchctl")
            .args(["load", &plist_path.to_string_lossy()])
            .status()
            .context("Failed to run launchctl load")?;

        if !status.success() {
            bail!("launchctl load failed (exit code {:?})", status.code());
        }

        println!(
            "{} {}",
            "ok".green().bold(),
            t!(
                "cli.ok.daemon_installed",
                path = plist_path.display().to_string()
            ),
        );
    } else if cfg!(target_os = "linux") {
        let unit_path = systemd_unit_path()?;
        if unit_path.exists() {
            println!(
                "{} {}",
                "note".cyan().bold(),
                t!(
                    "cli.info.daemon_already_installed",
                    path = unit_path.display().to_string()
                ),
            );
            return Ok(());
        }
        if let Some(parent) = unit_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = generate_systemd_unit(&coastd_str);
        std::fs::write(&unit_path, &content)
            .with_context(|| format!("Failed to write {}", unit_path.display()))?;

        let status = std::process::Command::new("systemctl")
            .args(["--user", "enable", "--now", "coastd"])
            .status()
            .context("Failed to run systemctl --user enable --now coastd")?;

        if !status.success() {
            bail!(
                "systemctl --user enable --now coastd failed (exit code {:?})",
                status.code()
            );
        }

        println!(
            "{} {}",
            "ok".green().bold(),
            t!(
                "cli.ok.daemon_installed",
                path = unit_path.display().to_string()
            ),
        );
    } else {
        bail!("Automatic daemon installation is only supported on macOS and Linux.");
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
    fn test_is_running_current_process() {
        let pid = std::process::id();
        assert!(is_running(pid));
    }

    #[test]
    fn test_is_running_dead_pid() {
        // PID 99999999 is almost certainly not running
        assert!(!is_running(99_999_999));
    }

    #[test]
    fn test_stale_pid_detected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coastd.pid");
        std::fs::write(&path, "99999999").unwrap();

        let pid = read_pid(&path.to_path_buf());
        assert_eq!(pid, Some(99_999_999));
        assert!(!is_running(pid.unwrap()));
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
        assert!(plist.contains("<string>/usr/local/bin/coastd</string>"));
        assert!(plist.contains("<string>--foreground</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains(&format!("<string>{LAUNCHD_LABEL}</string>")));
        assert!(plist.contains("/Users/test/.coast/coastd.stdout.log"));
        assert!(plist.contains("/Users/test/.coast/coastd.stderr.log"));
        assert!(plist.starts_with("<?xml"));
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
}
