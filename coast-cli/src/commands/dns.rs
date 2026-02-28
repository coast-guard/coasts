/// `coast dns` command — manage the localcoast DNS resolver.
///
/// Subcommands configure macOS to route `*.localcoast` DNS queries to the
/// coastd embedded DNS server on port 5354. This is a one-time sudo
/// operation that persists across reboots.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

const RESOLVER_DIR: &str = "/etc/resolver";
const RESOLVER_PATH: &str = "/etc/resolver/localcoast";
const DNS_PORT: u16 = 5354;

/// Arguments for `coast dns`.
#[derive(Debug, Args)]
pub struct DnsArgs {
    #[command(subcommand)]
    pub action: DnsAction,
}

/// Subcommands for `coast dns`.
#[derive(Debug, Subcommand)]
pub enum DnsAction {
    /// Configure macOS to resolve *.localcoast via coastd (one-time sudo).
    Setup,
    /// Check if the localcoast DNS resolver is configured and working.
    Status,
    /// Remove the localcoast DNS resolver configuration (sudo).
    Remove,
}

/// Execute the `coast dns` command.
pub async fn execute(args: &DnsArgs) -> Result<()> {
    match &args.action {
        DnsAction::Setup => setup().await,
        DnsAction::Status => status(),
        DnsAction::Remove => remove().await,
    }
}

async fn setup() -> Result<()> {
    if cfg!(not(target_os = "macos")) {
        bail!(
            "coast dns setup is currently macOS-only. On Linux, manually add \
             'nameserver 127.0.0.1' with 'port {DNS_PORT}' to your DNS configuration."
        );
    }

    if std::path::Path::new(RESOLVER_PATH).exists() {
        println!(
            "{} localcoast DNS resolver is already configured at {}",
            "ok".green().bold(),
            RESOLVER_PATH.bold(),
        );
        return Ok(());
    }

    println!(
        "{}",
        "Setting up localcoast DNS resolver (requires sudo)...".bold()
    );

    let mkdir_status = std::process::Command::new("sudo")
        .args(["mkdir", "-p", RESOLVER_DIR])
        .status()?;

    if !mkdir_status.success() {
        bail!("failed to create {RESOLVER_DIR}");
    }

    let resolver_content = format!("nameserver 127.0.0.1\nport {DNS_PORT}\n");
    let tee_status = std::process::Command::new("sudo")
        .args(["tee", RESOLVER_PATH])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(resolver_content.as_bytes())?;
            }
            child.wait()
        })?;

    if !tee_status.success() {
        bail!("failed to write {RESOLVER_PATH}");
    }

    println!(
        "{} localcoast DNS resolver configured at {}",
        "ok".green().bold(),
        RESOLVER_PATH.bold(),
    );
    println!(
        "  macOS will now resolve {domain} to 127.0.0.1 via coastd DNS on port {DNS_PORT}.",
        domain = "localcoast".bold(),
    );
    println!("  Dashboard: {}", "http://localcoast:31415".bold(),);
    println!("  Or just run: {}", "coast ui".bold(),);
    Ok(())
}

fn status() -> Result<()> {
    let exists = std::path::Path::new(RESOLVER_PATH).exists();

    if exists {
        println!(
            "{} localcoast DNS resolver is {} at {}",
            "ok".green().bold(),
            "configured".green(),
            RESOLVER_PATH,
        );

        if let Ok(content) = std::fs::read_to_string(RESOLVER_PATH) {
            for line in content.lines() {
                println!("  {line}");
            }
        }
    } else {
        println!(
            "{} localcoast DNS resolver is {}",
            "!!".yellow().bold(),
            "not configured".yellow(),
        );
        println!("  Run {} to set it up.", "coast dns setup".bold(),);
    }
    Ok(())
}

async fn remove() -> Result<()> {
    if cfg!(not(target_os = "macos")) {
        bail!("coast dns remove is currently macOS-only.");
    }

    if !std::path::Path::new(RESOLVER_PATH).exists() {
        println!(
            "{} No resolver file at {} — nothing to remove.",
            "ok".green().bold(),
            RESOLVER_PATH,
        );
        return Ok(());
    }

    println!(
        "{}",
        "Removing localcoast DNS resolver (requires sudo)...".bold()
    );

    let status = std::process::Command::new("sudo")
        .args(["rm", RESOLVER_PATH])
        .status()?;

    if !status.success() {
        bail!("failed to remove {RESOLVER_PATH}");
    }

    println!("{} localcoast DNS resolver removed.", "ok".green().bold(),);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: DnsArgs,
    }

    #[test]
    fn test_dns_setup_parse() {
        let cli = TestCli::try_parse_from(["test", "setup"]).unwrap();
        assert!(matches!(cli.args.action, DnsAction::Setup));
    }

    #[test]
    fn test_dns_status_parse() {
        let cli = TestCli::try_parse_from(["test", "status"]).unwrap();
        assert!(matches!(cli.args.action, DnsAction::Status));
    }

    #[test]
    fn test_dns_remove_parse() {
        let cli = TestCli::try_parse_from(["test", "remove"]).unwrap();
        assert!(matches!(cli.args.action, DnsAction::Remove));
    }

    #[test]
    fn test_dns_missing_subcommand() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
