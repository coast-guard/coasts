/// `coast assign` command — assign (or reassign) a worktree to an existing coast instance.
///
/// Switches the worktree in an existing slot without recreating the DinD container.
/// This is the fast path for worktree switching (~5s vs ~19s for rm + run).
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{AssignRequest, Request, Response};

use super::build::{ProgressDisplay, Verbosity};

/// Arguments for `coast assign`.
#[derive(Debug, Args)]
pub struct AssignArgs {
    /// Name of the coast instance (slot) to assign to.
    pub name: String,

    /// Git worktree to assign to this instance.
    #[arg(short = 'w', long)]
    pub worktree: Option<String>,

    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose detail (e.g., docker build logs).
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Execute the `coast assign` command.
pub async fn execute(args: &AssignArgs, project: &str) -> Result<()> {
    let worktree = match args.worktree.as_deref() {
        Some(w) => w.to_string(),
        None => {
            bail!(
                "A worktree must be specified. Usage: coast assign <name> --worktree <worktree>\n\
                 Shorthand: coast assign <name> -w <worktree>"
            );
        }
    };

    let commit_sha = std::process::Command::new("git")
        .args(["rev-parse", &worktree])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let request = Request::Assign(AssignRequest {
        name: args.name.clone(),
        project: project.to_string(),
        worktree: worktree.clone(),
        commit_sha,
    });

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut display = ProgressDisplay::new(verbosity);

    let response = super::send_assign_request(request, |event| {
        display.handle_event(event);
    })
    .await;

    display.finalize();

    match response {
        Ok(Response::Assign(resp)) => {
            if verbosity != Verbosity::Silent {
                eprintln!();
            }
            println!(
                "{} {}",
                "ok".green().bold(),
                t!(
                    "cli.ok.instance_assigned",
                    worktree = resp.worktree,
                    name = resp.name
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
        args: AssignArgs,
    }

    #[test]
    fn test_assign_args_worktree() {
        let cli =
            TestCli::try_parse_from(["test", "dev-1", "--worktree", "feature/oauth"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
        assert_eq!(cli.args.worktree.as_deref(), Some("feature/oauth"));
    }

    #[test]
    fn test_assign_args_worktree_short() {
        let cli = TestCli::try_parse_from(["test", "dev-2", "-w", "main"]).unwrap();
        assert_eq!(cli.args.name, "dev-2");
        assert_eq!(cli.args.worktree.as_deref(), Some("main"));
    }

    #[test]
    fn test_assign_args_missing_name() {
        let result = TestCli::try_parse_from(["test", "--worktree", "main"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_assign_args_no_worktree() {
        let cli = TestCli::try_parse_from(["test", "dev-1"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
        assert!(cli.args.worktree.is_none());
    }

    #[test]
    fn test_assign_args_silent() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-w", "main", "--silent"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_assign_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-w", "main", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_assign_args_verbose_short() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-w", "main", "-v"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_assign_args_branch_rejected() {
        let result = TestCli::try_parse_from(["test", "dev-1", "--branch", "main"]);
        assert!(result.is_err());
    }
}
