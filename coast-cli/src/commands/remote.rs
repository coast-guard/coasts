/// `coast remote` command — manage registered remote machines.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{RemoteRequest, RemoteResponse, Request, Response};
use coast_core::types::RemoteEntry;

/// Arguments for `coast remote`.
#[derive(Debug, Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub action: RemoteAction,
}

/// Subcommands for `coast remote`.
#[derive(Debug, Subcommand)]
pub enum RemoteAction {
    /// Register a new remote machine.
    Add {
        /// Name for this remote (e.g. "my-vm").
        name: String,
        /// SSH destination (user@host or user@host:port or just host).
        destination: String,
        /// SSH port (overrides port in destination).
        #[arg(long, short)]
        port: Option<u16>,
        /// Path to SSH private key (default: use SSH agent).
        #[arg(long, short)]
        key: Option<String>,
        /// Workspace sync strategy: mutagen or rsync.
        #[arg(long, default_value = "mutagen")]
        sync: String,
    },
    /// List all registered remotes.
    Ls,
    /// Remove a registered remote.
    Rm {
        /// Name of the remote to remove.
        name: String,
    },
    /// Test SSH connectivity to a registered remote.
    Test {
        /// Name of the remote to test.
        name: String,
    },
    /// Install and start coast-service on a registered remote.
    Setup {
        /// Name of the remote to set up.
        name: String,
        /// Deploy using Docker instead of copying the binary directly.
        #[arg(long)]
        docker: bool,
    },
    /// Clean up orphaned Docker volumes and workspaces on a remote.
    Prune {
        /// Name of the remote to prune.
        name: String,
        /// Show what would be removed without actually removing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Parse a destination string into (user, host, port).
fn parse_destination(dest: &str) -> (Option<String>, String, Option<u16>) {
    let (user, host_port) = if let Some((u, rest)) = dest.split_once('@') {
        (Some(u.to_string()), rest)
    } else {
        (None, dest)
    };

    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        match p.parse::<u16>() {
            Ok(port) if !h.is_empty() => (h.to_string(), Some(port)),
            _ => (host_port.to_string(), None),
        }
    } else {
        (host_port.to_string(), None)
    };

    (user, host, port)
}

/// Execute the `coast remote` command.
pub async fn execute(args: &RemoteArgs) -> Result<()> {
    let request = match &args.action {
        RemoteAction::Add {
            name,
            destination,
            port,
            key,
            sync,
        } => {
            let (parsed_user, parsed_host, parsed_port) = parse_destination(destination);
            let user = parsed_user
                .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "root".to_string()));
            let final_port = port.or(parsed_port).unwrap_or(22);

            Request::Remote(RemoteRequest::Add {
                name: name.clone(),
                host: parsed_host,
                user,
                port: final_port,
                ssh_key: key.clone(),
                sync_strategy: sync.clone(),
            })
        }
        RemoteAction::Ls => Request::Remote(RemoteRequest::Ls),
        RemoteAction::Rm { name } => Request::Remote(RemoteRequest::Rm { name: name.clone() }),
        RemoteAction::Test { name } => Request::Remote(RemoteRequest::Test { name: name.clone() }),
        RemoteAction::Setup { name, docker } => Request::Remote(RemoteRequest::Setup {
            name: name.clone(),
            docker: *docker,
        }),
        RemoteAction::Prune { name, dry_run } => Request::Remote(RemoteRequest::Prune {
            name: name.clone(),
            dry_run: *dry_run,
        }),
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Remote(resp) => {
            print_remote_response(&resp, &args.action);
            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("Unexpected response from daemon");
        }
    }
}

fn print_remote_response(resp: &RemoteResponse, action: &RemoteAction) {
    match action {
        RemoteAction::Ls => {
            if resp.remotes.is_empty() {
                println!("  No remotes registered. Use `coast remote add` to add one.");
            } else {
                println!("{}", format_remote_table(&resp.remotes));
            }
        }
        RemoteAction::Test { .. } => {
            println!("{} {}", "ok".green().bold(), resp.message);
        }
        _ => {
            println!("{} {}", "ok".green().bold(), resp.message);
        }
    }
}

fn format_remote_table(remotes: &[RemoteEntry]) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "  {:<15} {:<30} {:<6} {:<8}",
        "NAME".bold(),
        "HOST".bold(),
        "PORT".bold(),
        "SYNC".bold(),
    ));

    for r in remotes {
        let host_display = format!("{}@{}", r.user, r.host);
        lines.push(format!(
            "  {:<15} {:<30} {:<6} {:<8}",
            r.name, host_display, r.port, r.sync_strategy,
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
    fn test_remote_add_basic() {
        let cli =
            TestCli::try_parse_from(["test", "add", "my-vm", "ubuntu@192.168.1.100"]).unwrap();
        match &cli.args.action {
            RemoteAction::Add {
                name, destination, ..
            } => {
                assert_eq!(name, "my-vm");
                assert_eq!(destination, "ubuntu@192.168.1.100");
            }
            _ => panic!("Expected Add action"),
        }
    }

    #[test]
    fn test_remote_add_with_options() {
        let cli = TestCli::try_parse_from([
            "test",
            "add",
            "my-vm",
            "root@10.0.0.1",
            "--port",
            "2222",
            "--key",
            "~/.ssh/my_key",
            "--sync",
            "mutagen",
        ])
        .unwrap();
        match &cli.args.action {
            RemoteAction::Add {
                name,
                destination,
                port,
                key,
                sync,
            } => {
                assert_eq!(name, "my-vm");
                assert_eq!(destination, "root@10.0.0.1");
                assert_eq!(*port, Some(2222));
                assert_eq!(key.as_deref(), Some("~/.ssh/my_key"));
                assert_eq!(sync, "mutagen");
            }
            _ => panic!("Expected Add action"),
        }
    }

    #[test]
    fn test_remote_ls() {
        let cli = TestCli::try_parse_from(["test", "ls"]).unwrap();
        assert!(matches!(cli.args.action, RemoteAction::Ls));
    }

    #[test]
    fn test_remote_rm() {
        let cli = TestCli::try_parse_from(["test", "rm", "old-vm"]).unwrap();
        match &cli.args.action {
            RemoteAction::Rm { name } => assert_eq!(name, "old-vm"),
            _ => panic!("Expected Rm action"),
        }
    }

    #[test]
    fn test_remote_test() {
        let cli = TestCli::try_parse_from(["test", "test", "my-vm"]).unwrap();
        match &cli.args.action {
            RemoteAction::Test { name } => assert_eq!(name, "my-vm"),
            _ => panic!("Expected Test action"),
        }
    }

    #[test]
    fn test_remote_setup_basic() {
        let cli = TestCli::try_parse_from(["test", "setup", "my-vm"]).unwrap();
        match &cli.args.action {
            RemoteAction::Setup { name, docker } => {
                assert_eq!(name, "my-vm");
                assert!(!docker);
            }
            _ => panic!("Expected Setup action"),
        }
    }

    #[test]
    fn test_remote_setup_docker() {
        let cli = TestCli::try_parse_from(["test", "setup", "my-vm", "--docker"]).unwrap();
        match &cli.args.action {
            RemoteAction::Setup { name, docker } => {
                assert_eq!(name, "my-vm");
                assert!(docker);
            }
            _ => panic!("Expected Setup action"),
        }
    }

    #[test]
    fn test_remote_add_missing_args() {
        assert!(TestCli::try_parse_from(["test", "add"]).is_err());
        assert!(TestCli::try_parse_from(["test", "add", "name-only"]).is_err());
    }

    #[test]
    fn test_parse_destination_user_at_host() {
        let (user, host, port) = parse_destination("ubuntu@192.168.1.100");
        assert_eq!(user.as_deref(), Some("ubuntu"));
        assert_eq!(host, "192.168.1.100");
        assert_eq!(port, None);
    }

    #[test]
    fn test_parse_destination_user_at_host_port() {
        let (user, host, port) = parse_destination("root@myserver.com:2222");
        assert_eq!(user.as_deref(), Some("root"));
        assert_eq!(host, "myserver.com");
        assert_eq!(port, Some(2222));
    }

    #[test]
    fn test_parse_destination_bare_host() {
        let (user, host, port) = parse_destination("10.0.0.5");
        assert_eq!(user, None);
        assert_eq!(host, "10.0.0.5");
        assert_eq!(port, None);
    }

    #[test]
    fn test_format_remote_table_has_header() {
        let remotes = vec![RemoteEntry {
            name: "test-vm".into(),
            host: "10.0.0.1".into(),
            user: "ubuntu".into(),
            port: 22,
            ssh_key: None,
            sync_strategy: "mutagen".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        }];
        let output = format_remote_table(&remotes);
        assert!(output.contains("NAME"));
        assert!(output.contains("HOST"));
        assert!(output.contains("test-vm"));
        assert!(output.contains("ubuntu@10.0.0.1"));
    }
}
