/// `coast checkout` command — check out a coast instance.
///
/// Binds canonical ports to the specified instance via socat forwarding.
/// Use `--none` to unbind all canonical ports. This operation is instant
/// because it only kills and spawns socat processes, not containers.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{CheckoutRequest, Request, Response};

/// Arguments for `coast checkout`.
#[derive(Debug, Args)]
pub struct CheckoutArgs {
    /// Name of the coast instance to check out.
    #[arg(required_unless_present = "none")]
    pub name: Option<String>,

    /// Unbind all canonical ports (check out nothing).
    #[arg(long, conflicts_with = "name")]
    pub none: bool,
}

/// Execute the `coast checkout` command.
pub async fn execute(args: &CheckoutArgs, project: &str) -> Result<()> {
    let name = if args.none { None } else { args.name.clone() };

    let request = Request::Checkout(CheckoutRequest {
        name,
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Checkout(resp) => {
            match &resp.checked_out {
                Some(instance) => {
                    println!(
                        "{} {}",
                        "ok".green().bold(),
                        t!("cli.ok.instance_checked_out", name = instance),
                    );

                    if !resp.ports.is_empty() {
                        println!(
                            "\n  {} {}",
                            t!("cli.info.ports_header").to_string().bold(),
                            t!("cli.info.ports_canonical_active"),
                        );
                        println!("{}", super::format_port_table(&resp.ports, None));
                    }
                }
                None => {
                    println!("{} {}", "ok".green().bold(), t!("cli.ok.ports_unbound"),);
                }
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
        args: CheckoutArgs,
    }

    #[test]
    fn test_checkout_args_with_name() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, Some("feature-oauth".to_string()));
        assert!(!cli.args.none);
    }

    #[test]
    fn test_checkout_args_none() {
        let cli = TestCli::try_parse_from(["test", "--none"]).unwrap();
        assert!(cli.args.none);
        assert!(cli.args.name.is_none());
    }

    #[test]
    fn test_checkout_args_missing_both() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
