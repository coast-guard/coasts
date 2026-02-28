/// `coast rebuild` command — rebuild images and restart services inside a DinD container.
///
/// Runs `docker compose build` + `docker compose up -d` inside the DinD container,
/// picking up code changes from the bind-mounted `/workspace`.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{RebuildRequest, Request, Response};

/// Arguments for `coast rebuild`.
#[derive(Debug, Args)]
pub struct RebuildArgs {
    /// Name of the coast instance to rebuild.
    pub name: String,
}

/// Execute the `coast rebuild` command.
pub async fn execute(args: &RebuildArgs, project: &str) -> Result<()> {
    let request = Request::Rebuild(RebuildRequest {
        name: args.name.clone(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Rebuild(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.instance_rebuilt", name = resp.name),
            );

            if !resp.services_rebuilt.is_empty() {
                println!("  Services rebuilt: {}", resp.services_rebuilt.join(", "));
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
        args: RebuildArgs,
    }

    #[test]
    fn test_rebuild_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
    }

    #[test]
    fn test_rebuild_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
