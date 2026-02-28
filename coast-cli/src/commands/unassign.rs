/// `coast unassign` command — return an instance to the repo's default branch.
///
/// Detects the default branch (main/master) and delegates to the assign flow
/// to perform the worktree switch. The instance stays in its current lifecycle
/// state (Running, CheckedOut, etc.).
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{Request, Response, UnassignRequest};

use super::build::{ProgressDisplay, Verbosity};

/// Arguments for `coast unassign`.
#[derive(Debug, Args)]
pub struct UnassignArgs {
    /// Name of the coast instance to unassign.
    pub name: String,

    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose detail (e.g., docker build logs).
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Execute the `coast unassign` command.
pub async fn execute(args: &UnassignArgs, project: &str) -> Result<()> {
    let request = Request::Unassign(UnassignRequest {
        name: args.name.clone(),
        project: project.to_string(),
    });

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut display = ProgressDisplay::new(verbosity);

    let response = super::send_unassign_request(request, |event| {
        display.handle_event(event);
    })
    .await;

    display.finalize();

    match response {
        Ok(Response::Unassign(resp)) => {
            if verbosity != Verbosity::Silent {
                eprintln!();
            }
            println!(
                "{} {}",
                "ok".green().bold(),
                t!(
                    "cli.ok.instance_unassigned",
                    name = resp.name,
                    worktree = resp.worktree
                ),
            );
            if let Some(ref prev) = resp.previous_worktree {
                println!("   Previous: {}", prev);
            }
            let elapsed_secs = resp.time_elapsed_ms as f64 / 1000.0;
            println!("   Elapsed:  {:.1}s", elapsed_secs);
            Ok(())
        }
        Ok(Response::Error(e)) => {
            bail!("{}", e.error);
        }
        Err(e) => {
            bail!("{}", e);
        }
        _ => {
            bail!("{}", t!("error.unexpected_response"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: UnassignArgs,
    }

    #[test]
    fn test_unassign_args_basic() {
        let cli = TestCli::try_parse_from(["test", "dev-1"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_unassign_args_silent() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--silent"]).unwrap();
        assert!(cli.args.silent);
    }

    #[test]
    fn test_unassign_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_unassign_args_verbose_short() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-v"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_unassign_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
