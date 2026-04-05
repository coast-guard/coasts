/// `coast remote` command — manage remote VMs for remote development.
///
/// Provides subcommands to add, remove, list, setup, and ping remote VMs.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{
    RemoteAddRequest, RemoteConnectRequest, RemoteDisconnectRequest, RemoteInfo, RemoteListRequest,
    RemotePingRequest, RemoteRemoveRequest, RemoteRequest, RemoteResponse, RemoteSetupRequest,
    Request, Response,
};

/// Arguments for `coast remote`.
#[derive(Debug, Args)]
pub struct RemoteArgs {
    /// Remote subcommand.
    #[command(subcommand)]
    pub action: RemoteAction,
}

/// Subcommands for `coast remote`.
#[derive(Debug, Subcommand)]
pub enum RemoteAction {
    /// Add a new remote VM configuration.
    Add {
        /// Unique name for this remote (e.g., "staging", "dev-vm").
        name: String,
        /// Connection string in format user@host[:port].
        connection: String,
        /// Root directory for coast workspaces on the remote.
        #[arg(long, default_value = "~/.coast/workspaces")]
        workspace_root: String,
        /// Path to SSH private key (optional, uses default SSH key if not specified).
        #[arg(long, short = 'i')]
        identity: Option<String>,
    },
    /// Remove a remote VM configuration.
    Remove {
        /// Name of the remote to remove.
        name: String,
    },
    /// List all configured remotes.
    #[command(name = "ls")]
    List,
    /// Setup coastd on a remote VM.
    Setup {
        /// Name of the remote to setup.
        name: String,
        /// Force reinstall even if coastd is already present.
        #[arg(long)]
        force: bool,
    },
    /// Ping a remote to test connectivity.
    Ping {
        /// Name of the remote to ping.
        name: String,
    },
    /// Connect to a remote (establish SSH tunnel).
    Connect {
        /// Name of the remote to connect to.
        name: String,
    },
    /// Disconnect from a remote (close SSH tunnel).
    Disconnect {
        /// Name of the remote to disconnect from.
        name: String,
    },
}

/// Execute the `coast remote` command.
pub async fn execute(args: &RemoteArgs) -> Result<()> {
    let request = match &args.action {
        RemoteAction::Add {
            name,
            connection,
            workspace_root,
            identity,
        } => {
            // Parse connection string: user@host[:port]
            let (user, host, port) = parse_connection(connection)?;

            Request::Remote(RemoteRequest::Add(RemoteAddRequest {
                name: name.clone(),
                host,
                user,
                port,
                workspace_root: workspace_root.clone(),
                ssh_key_path: identity.clone(),
            }))
        }
        RemoteAction::Remove { name } => {
            Request::Remote(RemoteRequest::Remove(RemoteRemoveRequest {
                name: name.clone(),
            }))
        }
        RemoteAction::List => Request::Remote(RemoteRequest::List(RemoteListRequest {})),
        RemoteAction::Setup { name, force } => {
            Request::Remote(RemoteRequest::Setup(RemoteSetupRequest {
                name: name.clone(),
                force: *force,
            }))
        }
        RemoteAction::Ping { name } => Request::Remote(RemoteRequest::Ping(RemotePingRequest {
            name: name.clone(),
        })),
        RemoteAction::Connect { name } => {
            Request::Remote(RemoteRequest::Connect(RemoteConnectRequest {
                name: name.clone(),
            }))
        }
        RemoteAction::Disconnect { name } => {
            Request::Remote(RemoteRequest::Disconnect(RemoteDisconnectRequest {
                name: name.clone(),
            }))
        }
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Remote(resp) => handle_remote_response(resp),
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("Unexpected response from daemon");
        }
    }
}

/// Handle a remote response and print appropriate output.
fn handle_remote_response(resp: RemoteResponse) -> Result<()> {
    match resp {
        RemoteResponse::Add(r) => {
            println!("{} {}", "✓".green().bold(), r.message);
        }
        RemoteResponse::Remove(r) => {
            if r.removed {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                println!("{} {}", "!".yellow().bold(), r.message);
            }
        }
        RemoteResponse::List(r) => {
            if r.remotes.is_empty() {
                println!("No remotes configured.");
                println!();
                println!(
                    "Add a remote with: {} remote add <name> <user@host>",
                    "coast".cyan()
                );
            } else {
                println!("{}", format_remotes_table(&r.remotes));
            }
        }
        RemoteResponse::Setup(r) => {
            if r.success {
                let version_info = r.version.map(|v| format!(" (v{})", v)).unwrap_or_default();
                println!(
                    "{} {}{}",
                    "✓".green().bold(),
                    r.message,
                    version_info.dimmed()
                );
            } else {
                bail!("{}", r.message);
            }
        }
        RemoteResponse::Ping(r) => {
            print_ping_result(&r)?;
        }
        RemoteResponse::Connect(r) => {
            if r.connected {
                let port_info = r
                    .local_port
                    .map(|p| format!(" (local port: {})", p))
                    .unwrap_or_default();
                println!("{} {}{}", "✓".green().bold(), r.message, port_info.dimmed());
            } else {
                bail!("{}", r.message);
            }
        }
        RemoteResponse::Disconnect(r) => {
            if r.disconnected {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                println!("{} {}", "!".yellow().bold(), r.message);
            }
        }
    }
    Ok(())
}

/// Print ping result with status indicators.
fn print_ping_result(r: &coast_core::protocol::RemotePingResponse) -> Result<()> {
    if r.reachable {
        let ssh_status = if r.ssh_ok {
            "✓ SSH".green().to_string()
        } else {
            "✗ SSH".red().to_string()
        };

        let daemon_status = if r.daemon_ok {
            let version = r
                .daemon_version
                .as_ref()
                .map(|v| format!(" (v{})", v))
                .unwrap_or_default();
            format!("{}{}", "✓ coastd".green(), version.dimmed())
        } else {
            "✗ coastd".red().to_string()
        };

        let latency = r
            .latency_ms
            .map(|ms| format!("{}ms", ms))
            .unwrap_or_else(|| "?".to_string());

        println!(
            "{} {} | {} | latency: {}",
            "✓".green().bold(),
            ssh_status,
            daemon_status,
            latency
        );
        Ok(())
    } else {
        let error = r.error.as_deref().unwrap_or("Unknown error");
        bail!("Remote not reachable: {}", error);
    }
}

/// Parse a connection string in format user@host[:port].
fn parse_connection(connection: &str) -> Result<(String, String, u16)> {
    // Split on @ to get user and host:port
    let parts: Vec<&str> = connection.splitn(2, '@').collect();
    if parts.len() != 2 {
        bail!(
            "Invalid connection format. Expected user@host[:port], got: {}",
            connection
        );
    }

    let user = parts[0].to_string();
    let host_port = parts[1];

    // Check for port
    if let Some(colon_idx) = host_port.rfind(':') {
        // Could be IPv6 address or host:port
        // Try parsing as port first
        let potential_port = &host_port[colon_idx + 1..];
        if let Ok(port) = potential_port.parse::<u16>() {
            let host = host_port[..colon_idx].to_string();
            return Ok((user, host, port));
        }
    }

    // No port specified, use default
    Ok((user, host_port.to_string(), 22))
}

/// Format a table of remotes for display.
fn format_remotes_table(remotes: &[RemoteInfo]) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "  {:<15} {:<25} {:<10} {:<12} {}",
        "NAME".bold(),
        "HOST".bold(),
        "PORT".bold(),
        "STATUS".bold(),
        "PROJECTS".bold(),
    ));

    for remote in remotes {
        let status = remote.tunnel_status.as_deref().unwrap_or("disconnected");

        let status_colored = match status {
            "connected" => "connected".green().to_string(),
            "connecting" => "connecting".yellow().to_string(),
            _ => "disconnected".dimmed().to_string(),
        };

        lines.push(format!(
            "  {:<15} {:<25} {:<10} {:<12} {}",
            remote.name,
            format!("{}@{}", remote.user, remote.host),
            remote.port,
            status_colored,
            remote.project_count,
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: RemoteArgs,
    }

    #[test]
    fn test_remote_add_args() {
        let cli = TestCli::try_parse_from(["test", "add", "staging", "user@example.com"]).unwrap();
        match cli.args.action {
            RemoteAction::Add {
                name,
                connection,
                workspace_root,
                identity,
            } => {
                assert_eq!(name, "staging");
                assert_eq!(connection, "user@example.com");
                assert_eq!(workspace_root, "~/.coast/workspaces");
                assert!(identity.is_none());
            }
            _ => panic!("Expected Add action"),
        }
    }

    #[test]
    fn test_remote_add_with_port_and_key() {
        let cli = TestCli::try_parse_from([
            "test",
            "add",
            "dev-vm",
            "admin@192.168.1.100:2222",
            "--workspace-root",
            "/data/coast",
            "-i",
            "/home/user/.ssh/dev_key",
        ])
        .unwrap();
        match cli.args.action {
            RemoteAction::Add {
                name,
                connection,
                workspace_root,
                identity,
            } => {
                assert_eq!(name, "dev-vm");
                assert_eq!(connection, "admin@192.168.1.100:2222");
                assert_eq!(workspace_root, "/data/coast");
                assert_eq!(identity, Some("/home/user/.ssh/dev_key".to_string()));
            }
            _ => panic!("Expected Add action"),
        }
    }

    #[test]
    fn test_remote_remove_args() {
        let cli = TestCli::try_parse_from(["test", "remove", "staging"]).unwrap();
        match cli.args.action {
            RemoteAction::Remove { name } => {
                assert_eq!(name, "staging");
            }
            _ => panic!("Expected Remove action"),
        }
    }

    #[test]
    fn test_remote_list_args() {
        let cli = TestCli::try_parse_from(["test", "ls"]).unwrap();
        assert!(matches!(cli.args.action, RemoteAction::List));
    }

    #[test]
    fn test_remote_setup_args() {
        let cli = TestCli::try_parse_from(["test", "setup", "staging"]).unwrap();
        match cli.args.action {
            RemoteAction::Setup { name, force } => {
                assert_eq!(name, "staging");
                assert!(!force);
            }
            _ => panic!("Expected Setup action"),
        }
    }

    #[test]
    fn test_remote_setup_force() {
        let cli = TestCli::try_parse_from(["test", "setup", "staging", "--force"]).unwrap();
        match cli.args.action {
            RemoteAction::Setup { name, force } => {
                assert_eq!(name, "staging");
                assert!(force);
            }
            _ => panic!("Expected Setup action"),
        }
    }

    #[test]
    fn test_remote_ping_args() {
        let cli = TestCli::try_parse_from(["test", "ping", "staging"]).unwrap();
        match cli.args.action {
            RemoteAction::Ping { name } => {
                assert_eq!(name, "staging");
            }
            _ => panic!("Expected Ping action"),
        }
    }

    #[test]
    fn test_remote_connect_args() {
        let cli = TestCli::try_parse_from(["test", "connect", "staging"]).unwrap();
        match cli.args.action {
            RemoteAction::Connect { name } => {
                assert_eq!(name, "staging");
            }
            _ => panic!("Expected Connect action"),
        }
    }

    #[test]
    fn test_remote_disconnect_args() {
        let cli = TestCli::try_parse_from(["test", "disconnect", "staging"]).unwrap();
        match cli.args.action {
            RemoteAction::Disconnect { name } => {
                assert_eq!(name, "staging");
            }
            _ => panic!("Expected Disconnect action"),
        }
    }

    #[test]
    fn test_parse_connection_basic() {
        let (user, host, port) = parse_connection("user@example.com").unwrap();
        assert_eq!(user, "user");
        assert_eq!(host, "example.com");
        assert_eq!(port, 22);
    }

    #[test]
    fn test_parse_connection_with_port() {
        let (user, host, port) = parse_connection("admin@192.168.1.100:2222").unwrap();
        assert_eq!(user, "admin");
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, 2222);
    }

    #[test]
    fn test_parse_connection_invalid() {
        let result = parse_connection("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_remotes_table_empty() {
        let output = format_remotes_table(&[]);
        // Should only have header
        assert!(output.contains("NAME"));
        assert!(output.contains("HOST"));
        assert!(!output.contains("staging"));
    }

    #[test]
    fn test_format_remotes_table_with_remotes() {
        let remotes = vec![
            RemoteInfo {
                name: "staging".to_string(),
                host: "example.com".to_string(),
                user: "deploy".to_string(),
                port: 22,
                workspace_root: "/home/deploy/.coast".to_string(),
                ssh_key_path: None,
                tunnel_status: Some("connected".to_string()),
                project_count: 3,
            },
            RemoteInfo {
                name: "dev-vm".to_string(),
                host: "192.168.1.100".to_string(),
                user: "admin".to_string(),
                port: 2222,
                workspace_root: "/data/coast".to_string(),
                ssh_key_path: Some("/home/user/.ssh/dev_key".to_string()),
                tunnel_status: None,
                project_count: 0,
            },
        ];

        let output = format_remotes_table(&remotes);
        assert!(output.contains("staging"));
        assert!(output.contains("dev-vm"));
        assert!(output.contains("deploy@example.com"));
        assert!(output.contains("admin@192.168.1.100"));
        assert!(output.contains("22"));
        assert!(output.contains("2222"));
    }
}
