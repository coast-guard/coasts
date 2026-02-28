/// `coast rerun-extractors` command — re-run secret extraction only.
///
/// Uses the Coastfile cached in the build artifact and updates the keystore
/// without rebuilding artifacts or images.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;

use crate::commands::build::{ProgressDisplay, Verbosity};
use coast_core::protocol::{Request, RerunExtractorsRequest, Response};

/// Arguments for `coast rerun-extractors`.
#[derive(Debug, Args)]
pub struct RerunExtractorsArgs {
    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose extraction detail.
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Optional build ID to target (defaults to the project's latest build).
    #[arg(long = "build-id", value_name = "BUILD_ID")]
    pub build_id: Option<String>,
}

/// Execute the `coast rerun-extractors` command.
pub async fn execute(args: &RerunExtractorsArgs, project: &str) -> Result<()> {
    let request = Request::RerunExtractors(RerunExtractorsRequest {
        project: project.to_string(),
        build_id: args.build_id.clone(),
    });

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut display = ProgressDisplay::new(verbosity);

    let response = super::send_build_request(request, |event| {
        display.handle_event(event);
    })
    .await?;

    display.finalize();

    match response {
        Response::RerunExtractors(resp) => {
            if verbosity != Verbosity::Silent {
                eprintln!();
            }
            println!(
                "{} Re-extracted secrets for '{}'",
                "ok".green().bold(),
                resp.project,
            );
            println!("   Secrets: {} extracted", resp.secrets_extracted);
            for warning in &resp.warnings {
                println!("   {}: {}", "warning".yellow().bold(), warning);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: RerunExtractorsArgs,
    }

    #[test]
    fn test_rerun_extractors_args_defaults() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
        assert!(cli.args.build_id.is_none());
    }

    #[test]
    fn test_rerun_extractors_args_silent() {
        let cli = TestCli::try_parse_from(["test", "--silent"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_rerun_extractors_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_rerun_extractors_args_with_build_id() {
        let cli = TestCli::try_parse_from(["test", "--build-id", "a3c7d783"]).unwrap();
        assert_eq!(cli.args.build_id.as_deref(), Some("a3c7d783"));
    }
}
