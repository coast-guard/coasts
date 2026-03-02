/// `coast update` — check for updates and apply them.
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;

/// Arguments for the `coast update` command.
#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// The update action to perform.
    #[command(subcommand)]
    pub action: UpdateAction,
}

/// Available update actions.
#[derive(Debug, Subcommand)]
pub enum UpdateAction {
    /// Check for available updates.
    Check,
    /// Download and apply the latest update.
    Apply,
}

/// Execute the `coast update` command.
pub async fn execute(args: &UpdateArgs) -> Result<()> {
    match &args.action {
        UpdateAction::Check => execute_check().await,
        UpdateAction::Apply => execute_apply().await,
    }
}

async fn execute_check() -> Result<()> {
    let info = coast_update::check_for_updates().await;

    println!("  {} {}", "current:".bold(), info.current_version);

    match &info.latest_version {
        Some(latest) => {
            println!("  {} {}", "latest:".bold(), latest);
            if latest == &info.current_version {
                println!("\n{}", "You are up to date!".green());
            } else {
                println!(
                    "\n{}",
                    format!("Update available: {} -> {}", info.current_version, latest).yellow()
                );
                println!("  Run: {}", "coast update apply".bold());
            }
        }
        None => {
            println!(
                "  {} {}",
                "latest:".bold(),
                "(could not reach GitHub)".dimmed()
            );
        }
    }

    println!("  {} {}", "policy:".bold(), info.policy.policy);

    Ok(())
}

async fn execute_apply() -> Result<()> {
    println!("Checking for updates...");

    let latest = coast_update::checker::check_latest_version(coast_update::DOWNLOAD_TIMEOUT).await;

    let Some(latest) = latest else {
        anyhow::bail!(
            "Could not reach GitHub to check for updates. Check your internet connection."
        );
    };

    let current = coast_update::version::current_version()?;

    if !coast_update::version::is_newer(&current, &latest) {
        println!("{}", "Already up to date!".green());
        return Ok(());
    }

    println!("Downloading coast v{} ...", latest);

    let tarball =
        coast_update::updater::download_release(&latest, coast_update::DOWNLOAD_TIMEOUT).await?;

    println!("Applying update...");
    coast_update::updater::apply_update(&tarball)?;

    println!(
        "{}",
        format!("Updated coast: {} -> {}", current, latest).green()
    );

    // Restart the daemon so it picks up the new binary
    if let Err(e) = super::daemon::restart_daemon_if_running().await {
        eprintln!("{} Failed to restart daemon: {e}", "warning:".yellow());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: UpdateArgs,
    }

    #[test]
    fn test_update_check_parse() {
        let cli = TestCli::try_parse_from(["test", "check"]).unwrap();
        assert!(matches!(cli.args.action, UpdateAction::Check));
    }

    #[test]
    fn test_update_apply_parse() {
        let cli = TestCli::try_parse_from(["test", "apply"]).unwrap();
        assert!(matches!(cli.args.action, UpdateAction::Apply));
    }

    #[test]
    fn test_update_missing_subcommand() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
