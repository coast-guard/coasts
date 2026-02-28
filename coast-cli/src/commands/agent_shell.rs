/// `coast agent-shell` command — manage per-instance agent shells.
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use coast_core::protocol::{
    self, AgentShellInputResponse, AgentShellRequest, AgentShellResponse, Request, Response,
};

/// Arguments for `coast agent-shell`.
#[derive(Debug, Args)]
pub struct AgentShellArgs {
    /// Name of the coast instance.
    pub name: String,

    /// Agent shell subcommand.
    #[command(subcommand)]
    pub action: AgentShellAction,
}

/// Subcommands for `coast agent-shell`.
#[derive(Debug, Subcommand)]
pub enum AgentShellAction {
    /// List shells for an instance and show active/live state.
    Ls,
    /// Activate a shell ID.
    Activate {
        /// Agent shell ID.
        shell_id: i64,
    },
    /// Spawn a new shell.
    Spawn {
        /// Activate the newly spawned shell immediately.
        #[arg(long)]
        activate: bool,
    },
    /// Attach an interactive tty session to a shell.
    Tty {
        /// Optional shell ID. Defaults to the active shell.
        #[arg(long)]
        shell: Option<i64>,
    },
    /// Read the last N lines of shell output.
    #[command(name = "read-last-lines")]
    ReadLastLines {
        /// Number of lines to read from the end of output.
        number: usize,
        /// Optional shell ID. Defaults to the active shell.
        #[arg(long)]
        shell: Option<i64>,
    },
    /// Read full currently buffered shell output.
    #[command(name = "read-output")]
    ReadOutput {
        /// Optional shell ID. Defaults to the active shell.
        #[arg(long)]
        shell: Option<i64>,
    },
    /// Send input text to shell.
    Input {
        /// Input text to write.
        input: String,
        /// Optional shell ID. Defaults to the active shell.
        #[arg(long)]
        shell: Option<i64>,
        /// Do not append submit key (`\\r`, Enter) after input text.
        #[arg(long = "no-send")]
        no_send: bool,
        /// Print exact bytes that will be sent to the shell.
        #[arg(long = "show-bytes")]
        show_bytes: bool,
    },
    /// Show status for active shell or a specified shell.
    #[command(name = "session-status")]
    SessionStatus {
        /// Optional shell ID. Defaults to the active shell.
        #[arg(long)]
        shell: Option<i64>,
    },
}

fn daemon_socket_path() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".coast")
        .join("coastd.sock")
}

#[cfg(unix)]
struct RawModeGuard {
    original: Option<nix::sys::termios::Termios>,
}

#[cfg(unix)]
impl RawModeGuard {
    fn new() -> Result<Self> {
        if !std::io::stdin().is_terminal() {
            return Ok(Self { original: None });
        }
        let stdin = std::io::stdin();
        let original =
            nix::sys::termios::tcgetattr(&stdin).context("failed to read terminal attributes")?;
        let mut raw = original.clone();
        raw.local_flags
            .remove(nix::sys::termios::LocalFlags::ICANON | nix::sys::termios::LocalFlags::ECHO);
        nix::sys::termios::tcsetattr(&stdin, nix::sys::termios::SetArg::TCSANOW, &raw)
            .context("failed to enable raw terminal mode")?;
        Ok(Self {
            original: Some(original),
        })
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if let Some(ref original) = self.original {
            let stdin = std::io::stdin();
            let _ =
                nix::sys::termios::tcsetattr(&stdin, nix::sys::termios::SetArg::TCSANOW, original);
        }
    }
}

#[cfg(not(unix))]
struct RawModeGuard;

#[cfg(not(unix))]
impl RawModeGuard {
    fn new() -> Result<Self> {
        Ok(Self)
    }
}

pub async fn execute(args: &AgentShellArgs, project: &str) -> Result<()> {
    match &args.action {
        AgentShellAction::Tty { shell } => execute_tty(&args.name, project, *shell).await,
        AgentShellAction::Ls => {
            execute_one_shot(
                AgentShellRequest::Ls {
                    project: project.to_string(),
                    name: args.name.clone(),
                },
                &args.name,
            )
            .await
        }
        AgentShellAction::Activate { shell_id } => {
            execute_one_shot(
                AgentShellRequest::Activate {
                    project: project.to_string(),
                    name: args.name.clone(),
                    shell_id: *shell_id,
                },
                &args.name,
            )
            .await
        }
        AgentShellAction::Spawn { activate } => {
            execute_one_shot(
                AgentShellRequest::Spawn {
                    project: project.to_string(),
                    name: args.name.clone(),
                    activate: *activate,
                },
                &args.name,
            )
            .await
        }
        AgentShellAction::ReadLastLines { number, shell } => {
            execute_one_shot(
                AgentShellRequest::ReadLastLines {
                    project: project.to_string(),
                    name: args.name.clone(),
                    lines: *number,
                    shell_id: *shell,
                },
                &args.name,
            )
            .await
        }
        AgentShellAction::ReadOutput { shell } => {
            execute_one_shot(
                AgentShellRequest::ReadOutput {
                    project: project.to_string(),
                    name: args.name.clone(),
                    shell_id: *shell,
                },
                &args.name,
            )
            .await
        }
        AgentShellAction::Input {
            input,
            shell,
            no_send,
            show_bytes,
        } => execute_input(&args.name, project, input, *shell, *no_send, *show_bytes).await,
        AgentShellAction::SessionStatus { shell } => {
            execute_one_shot(
                AgentShellRequest::SessionStatus {
                    project: project.to_string(),
                    name: args.name.clone(),
                    shell_id: *shell,
                },
                &args.name,
            )
            .await
        }
    }
}

fn prepare_input_payload(input: &str, no_send: bool) -> String {
    if no_send {
        input.to_string()
    } else {
        // Legacy helper used by tests and --no-send path.
        format!("{input}\r")
    }
}

fn submit_key_payload() -> &'static str {
    "\r"
}

fn format_input_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("0x{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

async fn send_input_request(
    instance_name: &str,
    project: &str,
    shell: Option<i64>,
    input: String,
) -> Result<AgentShellInputResponse> {
    let response = super::send_request(Request::AgentShell(AgentShellRequest::Input {
        project: project.to_string(),
        name: instance_name.to_string(),
        input,
        shell_id: shell,
    }))
    .await?;

    match response {
        Response::AgentShell(AgentShellResponse::Input(resp)) => Ok(resp),
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon for input request"),
    }
}

async fn execute_input(
    instance_name: &str,
    project: &str,
    input: &str,
    shell: Option<i64>,
    no_send: bool,
    show_bytes: bool,
) -> Result<()> {
    if no_send {
        let payload = prepare_input_payload(input, true);
        if show_bytes {
            eprintln!(
                "{} sending {} byte(s): {}",
                "debug".cyan().bold(),
                payload.len(),
                format_input_bytes(payload.as_bytes())
            );
        }
        let resp = send_input_request(instance_name, project, shell, payload).await?;
        println!(
            "{} Wrote {} byte(s) to shell {}.",
            "ok".green().bold(),
            resp.bytes_written,
            resp.shell_id
        );
        return Ok(());
    }

    // Send text and submit as separate PTY writes.
    if show_bytes {
        eprintln!(
            "{} sending {} byte(s): {}",
            "debug".cyan().bold(),
            input.len(),
            format_input_bytes(input.as_bytes())
        );
        eprintln!(
            "{} sending {} byte(s): {}",
            "debug".cyan().bold(),
            submit_key_payload().len(),
            format_input_bytes(submit_key_payload().as_bytes())
        );
    }

    let text_resp = send_input_request(instance_name, project, shell, input.to_string()).await?;
    // Small separation to avoid TUI paste-style newline behavior.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let submit_resp = send_input_request(
        instance_name,
        project,
        shell,
        submit_key_payload().to_string(),
    )
    .await?;

    println!(
        "{} Wrote {} byte(s) to shell {} ({} + submit key).",
        "ok".green().bold(),
        text_resp.bytes_written + submit_resp.bytes_written,
        submit_resp.shell_id,
        text_resp.bytes_written
    );
    Ok(())
}

async fn execute_one_shot(req: AgentShellRequest, instance_name: &str) -> Result<()> {
    let response = super::send_request(Request::AgentShell(req)).await?;
    match response {
        Response::AgentShell(AgentShellResponse::Ls(resp)) => {
            if resp.shells.is_empty() {
                println!("No agent shells found for instance '{}'.", resp.name);
                return Ok(());
            }
            println!("Agent shells for '{}':", resp.name);
            println!(
                "  {:<8} {:<8} {:<8} {}",
                "ID".bold(),
                "ACTIVE".bold(),
                "LIVE".bold(),
                "STATUS".bold()
            );
            for shell in &resp.shells {
                let active = if shell.is_active {
                    "yes".green().to_string()
                } else {
                    "no".dimmed().to_string()
                };
                let live = if shell.is_live {
                    "live".green().to_string()
                } else {
                    "dead".red().to_string()
                };
                println!(
                    "  {:<8} {:<8} {:<8} {}",
                    shell.shell_id, active, live, shell.status
                );
            }
            Ok(())
        }
        Response::AgentShell(AgentShellResponse::Activate(resp)) => {
            let icon = if resp.changed {
                "ok".green().bold().to_string()
            } else {
                "info".yellow().bold().to_string()
            };
            println!("{} {}", icon, resp.message);
            Ok(())
        }
        Response::AgentShell(AgentShellResponse::Spawn(resp)) => {
            println!(
                "{} Spawned agent shell {} (session {}).",
                "ok".green().bold(),
                resp.shell_id,
                resp.session_id
            );
            Ok(())
        }
        Response::AgentShell(AgentShellResponse::ReadLastLines(resp))
        | Response::AgentShell(AgentShellResponse::ReadOutput(resp)) => {
            if !resp.output.is_empty() {
                print!("{}", resp.output);
                std::io::stdout().flush().ok();
            }
            Ok(())
        }
        Response::AgentShell(AgentShellResponse::Input(resp)) => {
            println!(
                "{} Wrote {} byte(s) to shell {}.",
                "ok".green().bold(),
                resp.bytes_written,
                resp.shell_id
            );
            Ok(())
        }
        Response::AgentShell(AgentShellResponse::SessionStatus(resp)) => {
            println!("Status: {}", resp.status);
            if let Some(shell_id) = resp.shell_id {
                println!("Shell: {}", shell_id);
            }
            println!("{}", resp.message);
            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        Response::AgentShell(_) => {
            bail!("Unexpected agent-shell response for one-shot request");
        }
        _ => {
            bail!(
                "Unexpected response from daemon for instance '{}'",
                instance_name
            );
        }
    }
}

async fn execute_tty(name: &str, project: &str, shell: Option<i64>) -> Result<()> {
    let socket = daemon_socket_path();
    let stream = UnixStream::connect(&socket).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("coastd is not running. Start it with: coast daemon start")
        } else {
            anyhow::anyhow!("Failed to connect to coastd at {}: {}", socket.display(), e)
        }
    })?;
    let (read_half, mut write_half) = stream.into_split();

    let init = Request::AgentShell(AgentShellRequest::Tty {
        project: project.to_string(),
        name: name.to_string(),
        shell_id: shell,
    });
    let encoded = protocol::encode_request(&init).context("failed to encode tty request")?;
    write_half
        .write_all(&encoded)
        .await
        .context("failed to send tty request")?;

    let _raw_mode = RawModeGuard::new()?;
    let mut reader = BufReader::new(read_half);
    let mut stdin = tokio::io::stdin();
    let mut stdin_buf = [0u8; 1024];
    let mut line = String::new();

    loop {
        line.clear();
        tokio::select! {
            read = reader.read_line(&mut line) => {
                let bytes = read.context("failed reading tty stream from daemon")?;
                if bytes == 0 {
                    break;
                }
                let resp = protocol::decode_response(line.trim_end().as_bytes())
                    .context("failed to decode tty stream response")?;
                match resp {
                    Response::AgentShell(AgentShellResponse::TtyAttached(_)) => {}
                    Response::AgentShell(AgentShellResponse::TtyOutput(out)) => {
                        print!("{}", out.data);
                        std::io::stdout().flush().ok();
                    }
                    Response::AgentShell(AgentShellResponse::TtyClosed(_)) => break,
                    Response::Error(e) => bail!("{}", e.error),
                    _ => {}
                }
            }
            input = stdin.read(&mut stdin_buf) => {
                let read_n = input.context("failed to read local tty input")?;
                if read_n == 0 {
                    let detach = Request::AgentShell(AgentShellRequest::TtyDetach);
                    let encoded = protocol::encode_request(&detach)
                        .context("failed to encode tty detach")?;
                    write_half.write_all(&encoded).await
                        .context("failed to send tty detach")?;
                    break;
                }
                let input_req = Request::AgentShell(AgentShellRequest::TtyInput {
                    data: String::from_utf8_lossy(&stdin_buf[..read_n]).into_owned(),
                });
                let encoded = protocol::encode_request(&input_req)
                    .context("failed to encode tty input")?;
                write_half.write_all(&encoded).await
                    .context("failed to send tty input")?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: AgentShellArgs,
    }

    #[test]
    fn test_agent_shell_ls_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "ls"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
        assert!(matches!(cli.args.action, AgentShellAction::Ls));
    }

    #[test]
    fn test_agent_shell_activate_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "activate", "2"]).unwrap();
        match cli.args.action {
            AgentShellAction::Activate { shell_id } => assert_eq!(shell_id, 2),
            _ => panic!("expected activate"),
        }
    }

    #[test]
    fn test_agent_shell_tty_args_with_shell() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "tty", "--shell", "3"]).unwrap();
        match cli.args.action {
            AgentShellAction::Tty { shell } => assert_eq!(shell, Some(3)),
            _ => panic!("expected tty"),
        }
    }

    #[test]
    fn test_agent_shell_read_last_lines_args() {
        let cli =
            TestCli::try_parse_from(["test", "dev-1", "read-last-lines", "50", "--shell", "2"])
                .unwrap();
        match cli.args.action {
            AgentShellAction::ReadLastLines { number, shell } => {
                assert_eq!(number, 50);
                assert_eq!(shell, Some(2));
            }
            _ => panic!("expected read-last-lines"),
        }
    }

    #[test]
    fn test_agent_shell_read_output_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "read-output"]).unwrap();
        match cli.args.action {
            AgentShellAction::ReadOutput { shell } => assert_eq!(shell, None),
            _ => panic!("expected read-output"),
        }
    }

    #[test]
    fn test_agent_shell_input_args() {
        let cli =
            TestCli::try_parse_from(["test", "dev-1", "input", "hello", "--shell", "4"]).unwrap();
        match cli.args.action {
            AgentShellAction::Input {
                input,
                shell,
                no_send,
                show_bytes,
            } => {
                assert_eq!(input, "hello");
                assert_eq!(shell, Some(4));
                assert!(!no_send);
                assert!(!show_bytes);
            }
            _ => panic!("expected input"),
        }
    }

    #[test]
    fn test_agent_shell_input_args_no_send() {
        let cli = TestCli::try_parse_from([
            "test",
            "dev-1",
            "input",
            "hello",
            "--no-send",
            "--shell",
            "4",
        ])
        .unwrap();
        match cli.args.action {
            AgentShellAction::Input {
                input,
                shell,
                no_send,
                show_bytes,
            } => {
                assert_eq!(input, "hello");
                assert_eq!(shell, Some(4));
                assert!(no_send);
                assert!(!show_bytes);
            }
            _ => panic!("expected input"),
        }
    }

    #[test]
    fn test_agent_shell_input_args_show_bytes() {
        let cli = TestCli::try_parse_from([
            "test",
            "dev-1",
            "input",
            "hello",
            "--show-bytes",
            "--shell",
            "4",
        ])
        .unwrap();
        match cli.args.action {
            AgentShellAction::Input {
                input,
                shell,
                no_send,
                show_bytes,
            } => {
                assert_eq!(input, "hello");
                assert_eq!(shell, Some(4));
                assert!(!no_send);
                assert!(show_bytes);
            }
            _ => panic!("expected input"),
        }
    }

    #[test]
    fn test_prepare_input_payload_default_appends_enter() {
        assert_eq!(prepare_input_payload("hello", false), "hello\r");
    }

    #[test]
    fn test_prepare_input_payload_no_send_keeps_raw_input() {
        assert_eq!(prepare_input_payload("hello", true), "hello");
    }

    #[test]
    fn test_format_input_bytes_includes_cr() {
        assert_eq!(
            format_input_bytes(b"hello\r"),
            "0x68 0x65 0x6C 0x6C 0x6F 0x0D"
        );
    }

    #[test]
    fn test_submit_key_payload_is_cr() {
        assert_eq!(submit_key_payload().as_bytes(), b"\r");
    }

    #[test]
    fn test_agent_shell_session_status_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "session-status"]).unwrap();
        match cli.args.action {
            AgentShellAction::SessionStatus { shell } => assert!(shell.is_none()),
            _ => panic!("expected session-status"),
        }
    }
}
