/// `coast archive` and `coast unarchive` commands.
///
/// Archive hides a project from the main list after stopping all running
/// instances and shared services. Unarchive restores it.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{ArchiveProjectRequest, Request, Response, UnarchiveProjectRequest};

/// Arguments for `coast archive`.
#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Project name to archive.
    pub project: String,
}

/// Arguments for `coast unarchive`.
#[derive(Debug, Args)]
pub struct UnarchiveArgs {
    /// Project name to unarchive.
    pub project: String,
}

/// Execute the `coast archive` command.
pub async fn execute_archive(args: &ArchiveArgs) -> Result<()> {
    let request = Request::ArchiveProject(ArchiveProjectRequest {
        project: args.project.clone(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::ArchiveProject(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.project_archived", project = resp.project),
            );
            if resp.instances_stopped > 0 {
                println!("  Stopped {} instance(s)", resp.instances_stopped);
            }
            if resp.shared_services_stopped > 0 {
                println!(
                    "  Stopped {} shared service(s)",
                    resp.shared_services_stopped
                );
            }
            println!(
                "  {}: Use `coast unarchive {}` to restore.",
                "note".cyan().bold(),
                resp.project,
            );
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("{}", t!("error.unexpected_response")),
    }
}

/// Execute the `coast unarchive` command.
pub async fn execute_unarchive(args: &UnarchiveArgs) -> Result<()> {
    let request = Request::UnarchiveProject(UnarchiveProjectRequest {
        project: args.project.clone(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::UnarchiveProject(resp) => {
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.project_unarchived", project = resp.project),
            );
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("{}", t!("error.unexpected_response")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestArchiveCli {
        #[command(flatten)]
        args: ArchiveArgs,
    }

    #[derive(Debug, Parser)]
    struct TestUnarchiveCli {
        #[command(flatten)]
        args: UnarchiveArgs,
    }

    #[test]
    fn test_archive_args() {
        let cli = TestArchiveCli::try_parse_from(["test", "my-app"]).unwrap();
        assert_eq!(cli.args.project, "my-app");
    }

    #[test]
    fn test_archive_missing_project() {
        let result = TestArchiveCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_unarchive_args() {
        let cli = TestUnarchiveCli::try_parse_from(["test", "my-app"]).unwrap();
        assert_eq!(cli.args.project, "my-app");
    }

    #[test]
    fn test_unarchive_missing_project() {
        let result = TestUnarchiveCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }
}
