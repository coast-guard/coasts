/// `coast stop` command — stop a running coast instance.
///
/// Stops the inner compose stack, stops the coast container, and kills
/// all associated socat port-forwarding processes.
///
/// With `--all`, stops every running instance for the resolved project.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use crate::commands::build::{ProgressDisplay, Verbosity};
use coast_core::protocol::{LsRequest, Request, Response, StopRequest};
use coast_core::types::InstanceStatus;

/// Arguments for `coast stop`.
#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("target").required(true))]
pub struct StopArgs {
    /// Name of the coast instance to stop.
    #[arg(group = "target")]
    pub name: Option<String>,

    /// Stop all instances for the current project.
    #[arg(long, group = "target")]
    pub all: bool,

    /// Suppress all progress output; only show the final result.
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose detail for each step.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Execute the `coast stop` command.
pub async fn execute(args: &StopArgs, project: &str) -> Result<()> {
    if args.all {
        return execute_all(args, project).await;
    }

    let name = args.name.as_deref().unwrap();
    let request = Request::Stop(StopRequest {
        name: name.to_string(),
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

    let response = super::send_stop_request(request, |event| {
        display.handle_event(event);
    })
    .await?;

    display.finalize();

    match response {
        Response::Stop(resp) => {
            if verbosity != Verbosity::Silent {
                eprintln!();
            }
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.instance_stopped", name = resp.name),
            );
            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("{}", t!("error.unexpected_response"));
        }
    }
}

/// Stop all running instances for a project.
async fn execute_all(args: &StopArgs, project: &str) -> Result<()> {
    let ls_resp = super::send_request(Request::Ls(LsRequest {
        project: Some(project.to_string()),
    }))
    .await?;

    let instances = match ls_resp {
        Response::Ls(resp) => resp.instances,
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    };

    let stoppable: Vec<_> = instances
        .into_iter()
        .filter(|i| {
            matches!(
                i.status,
                InstanceStatus::Running | InstanceStatus::CheckedOut
            )
        })
        .collect();

    if stoppable.is_empty() {
        println!("{}", t!("cli.info.no_running_instances", project = project));
        return Ok(());
    }

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut errors = 0u32;
    for inst in &stoppable {
        let request = Request::Stop(StopRequest {
            name: inst.name.clone(),
            project: project.to_string(),
        });

        let mut display = ProgressDisplay::new(verbosity);

        match super::send_stop_request(request, |event| {
            display.handle_event(event);
        })
        .await
        {
            Ok(Response::Stop(resp)) => {
                display.finalize();
                println!(
                    "{} {}",
                    "ok".green().bold(),
                    t!("cli.ok.instance_stopped", name = resp.name),
                );
            }
            Ok(Response::Error(e)) => {
                display.finalize();
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!("cli.err.failed_to_stop", name = inst.name, error = e.error),
                );
                errors += 1;
            }
            Ok(_) => {
                display.finalize();
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!("cli.err.unexpected_response", name = inst.name),
                );
                errors += 1;
            }
            Err(e) => {
                display.finalize();
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!(
                        "cli.err.failed_to_stop",
                        name = inst.name,
                        error = format!("{:#}", e)
                    ),
                );
                errors += 1;
            }
        }
    }

    if errors > 0 {
        bail!("{}", t!("cli.err.instances_failed_stop", count = errors));
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
        args: StopArgs,
    }

    #[test]
    fn test_stop_args_name() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, Some("feature-oauth".to_string()));
        assert!(!cli.args.all);
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_stop_args_all() {
        let cli = TestCli::try_parse_from(["test", "--all"]).unwrap();
        assert!(cli.args.name.is_none());
        assert!(cli.args.all);
    }

    #[test]
    fn test_stop_args_name_and_all_conflict() {
        let result = TestCli::try_parse_from(["test", "feature-oauth", "--all"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_args_neither() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_stop_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_stop_args_silent() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--silent"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }
}
