/// `coast rm-build` command -- remove a project's build artifact and Docker resources.
///
/// Removes the artifact directory at `~/.coast/images/{project}/`, plus
/// any associated stopped containers, volumes, and images on the host daemon.
/// Requires confirmation unless `--force` is passed.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;
use std::io::{self, Write};

use crate::commands::build::{ProgressDisplay, Verbosity};
use coast_core::protocol::{Request, Response, RmBuildRequest};

/// Arguments for `coast rm-build`.
#[derive(Debug, Args)]
pub struct RmBuildArgs {
    /// Project name whose build artifact should be removed.
    pub project: String,

    /// Skip confirmation prompt.
    #[arg(long)]
    pub force: bool,

    /// Suppress all progress output.
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose detail for each step.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Execute the `coast rm-build` command.
pub async fn execute(args: &RmBuildArgs) -> Result<()> {
    if !args.force {
        print!(
            "{} Remove build for project '{}'? This will delete the artifact, images, volumes, and containers. [y/N] ",
            "confirm".yellow().bold(),
            args.project,
        );
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            println!("{}", t!("cli.info.aborted"));
            return Ok(());
        }
    }

    let request = Request::RmBuild(RmBuildRequest {
        project: args.project.clone(),
        build_ids: Vec::new(),
    });

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut display = ProgressDisplay::new(verbosity);

    let response = super::send_rm_build_request(request, |event| {
        display.handle_event(event);
    })
    .await?;

    match response {
        Response::RmBuild(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.build_removed", project = resp.project),
            );
            if resp.containers_removed > 0 {
                println!("  Containers removed: {}", resp.containers_removed);
            }
            if resp.volumes_removed > 0 {
                println!("  Volumes removed: {}", resp.volumes_removed);
            }
            if resp.images_removed > 0 {
                println!("  Images removed: {}", resp.images_removed);
            }
            if resp.artifact_removed {
                println!("  Artifact directory removed");
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: RmBuildArgs,
    }

    #[test]
    fn test_rm_build_args_basic() {
        let cli = TestCli::try_parse_from(["test", "my-app"]).unwrap();
        assert_eq!(cli.args.project, "my-app");
        assert!(!cli.args.force);
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_rm_build_args_force() {
        let cli = TestCli::try_parse_from(["test", "my-app", "--force"]).unwrap();
        assert_eq!(cli.args.project, "my-app");
        assert!(cli.args.force);
    }

    #[test]
    fn test_rm_build_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "my-app", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_rm_build_args_silent() {
        let cli = TestCli::try_parse_from(["test", "my-app", "--silent"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_rm_build_args_missing_project() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
