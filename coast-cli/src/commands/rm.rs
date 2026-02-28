/// `coast rm` command — remove a coast instance.
///
/// Stops the instance if running, removes the coast container, deletes
/// isolated volumes, and removes the instance from state.
/// Shared service data is always preserved.
///
/// With `--all`, removes every instance for the resolved project.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{LsRequest, Request, Response, RmRequest};

/// Arguments for `coast rm`.
#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("target").required(true))]
pub struct RmArgs {
    /// Name of the coast instance to remove.
    #[arg(group = "target")]
    pub name: Option<String>,

    /// Remove all instances for the current project.
    #[arg(long, group = "target")]
    pub all: bool,
}

/// Execute the `coast rm` command.
pub async fn execute(args: &RmArgs, project: &str) -> Result<()> {
    if args.all {
        return execute_all(project).await;
    }

    let name = args.name.as_deref().unwrap();
    let request = Request::Rm(RmRequest {
        name: name.to_string(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Rm(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.instance_removed", name = resp.name),
            );

            println!(
                "  {}: {}",
                "note".cyan().bold(),
                t!("cli.warn.shared_data_preserved"),
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

/// Remove all instances for a project.
async fn execute_all(project: &str) -> Result<()> {
    let ls_resp = super::send_request(Request::Ls(LsRequest {
        project: Some(project.to_string()),
    }))
    .await?;

    let instances = match ls_resp {
        Response::Ls(resp) => resp.instances,
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    };

    if instances.is_empty() {
        println!(
            "{}",
            t!("cli.info.no_instances_to_remove", project = project)
        );
        return Ok(());
    }

    println!(
        "Removing {} instance(s) for project '{project}'...",
        instances.len(),
    );

    let mut errors = 0u32;
    for inst in &instances {
        let request = Request::Rm(RmRequest {
            name: inst.name.clone(),
            project: project.to_string(),
        });

        match super::send_request(request).await {
            Ok(Response::Rm(resp)) => {
                println!(
                    "{} {}",
                    "ok".green().bold(),
                    t!("cli.ok.instance_removed", name = resp.name),
                );
            }
            Ok(Response::Error(e)) => {
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!(
                        "cli.err.failed_to_remove",
                        name = inst.name,
                        error = e.error
                    ),
                );
                errors += 1;
            }
            Ok(_) => {
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!("cli.err.unexpected_response", name = inst.name),
                );
                errors += 1;
            }
            Err(e) => {
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!(
                        "cli.err.failed_to_remove",
                        name = inst.name,
                        error = format!("{:#}", e)
                    ),
                );
                errors += 1;
            }
        }
    }

    println!(
        "\n  {}: {}",
        "note".cyan().bold(),
        t!("cli.warn.shared_data_preserved"),
    );

    if errors > 0 {
        bail!("{}", t!("cli.err.instances_failed_remove", count = errors));
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
        args: RmArgs,
    }

    #[test]
    fn test_rm_args_name_only() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, Some("feature-oauth".to_string()));
        assert!(!cli.args.all);
    }

    #[test]
    fn test_rm_args_all() {
        let cli = TestCli::try_parse_from(["test", "--all"]).unwrap();
        assert!(cli.args.name.is_none());
        assert!(cli.args.all);
    }

    #[test]
    fn test_rm_args_name_and_all_conflict() {
        let result = TestCli::try_parse_from(["test", "feature-oauth", "--all"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_rm_args_neither() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
