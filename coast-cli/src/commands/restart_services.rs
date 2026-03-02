/// `coast restart-services` command — restart all services inside a coast instance.
///
/// Tears down and restarts compose services (`compose down` + `compose up -d`)
/// or bare services (`stop-all.sh` + `start-all.sh`). Respects `autostart = false`.
/// Does not affect shared services.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{Request, Response, RestartServicesRequest};

/// Arguments for `coast restart-services`.
#[derive(Debug, Args)]
pub struct RestartServicesArgs {
    /// Name of the coast instance to restart services for.
    pub name: String,
}

/// Execute the `coast restart-services` command.
pub async fn execute(args: &RestartServicesArgs, project: &str) -> Result<()> {
    let request = Request::RestartServices(RestartServicesRequest {
        name: args.name.clone(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::RestartServices(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.services_restarted", name = resp.name),
            );

            if !resp.services_restarted.is_empty() {
                println!(
                    "  Services restarted: {}",
                    resp.services_restarted.join(", ")
                );
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
        args: RestartServicesArgs,
    }

    #[test]
    fn test_restart_services_args() {
        let cli = TestCli::try_parse_from(["test", "dev-1"]).unwrap();
        assert_eq!(cli.args.name, "dev-1");
    }

    #[test]
    fn test_restart_services_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
