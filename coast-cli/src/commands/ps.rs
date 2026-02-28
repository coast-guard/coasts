/// `coast ps` command — show inner compose service status.
///
/// Displays a table of services running inside the coast container,
/// including their status and exposed ports.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;

use coast_core::protocol::{PsRequest, Request, Response, ServiceStatus};

/// Arguments for `coast ps`.
#[derive(Debug, Args)]
pub struct PsArgs {
    /// Name of the coast instance.
    pub name: String,
}

/// Execute the `coast ps` command.
pub async fn execute(args: &PsArgs, project: &str) -> Result<()> {
    let request = Request::Ps(PsRequest {
        name: args.name.clone(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Ps(resp) => {
            println!("Services in coast instance '{}':", resp.name,);
            println!("{}", format_service_table(&resp.services));
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

/// Format a table of inner compose services for display.
pub fn format_service_table(services: &[ServiceStatus]) -> String {
    if services.is_empty() {
        return "  No services running.".to_string();
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "  {:<25} {:<20} {}",
        "NAME".bold(),
        "STATUS".bold(),
        "PORTS".bold(),
    ));

    for svc in services {
        let status_colored = colorize_status(&svc.status);
        lines.push(format!(
            "  {:<25} {:<20} {}",
            svc.name, status_colored, svc.ports,
        ));
    }

    lines.join("\n")
}

/// Apply color to a service status string.
fn colorize_status(status: &str) -> String {
    let lower = status.to_lowercase();
    if lower.contains("up") || lower.contains("running") || lower.contains("healthy") {
        status.green().to_string()
    } else if lower.contains("exit") || lower.contains("dead") || lower.contains("error") {
        status.red().to_string()
    } else {
        status.yellow().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: PsArgs,
    }

    #[test]
    fn test_ps_args_name() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
    }

    #[test]
    fn test_ps_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_service_table_empty() {
        let output = format_service_table(&[]);
        assert_eq!(output, "  No services running.");
    }

    #[test]
    fn test_format_service_table_with_services() {
        let services = vec![
            ServiceStatus {
                name: "web".to_string(),
                status: "Up 5 minutes".to_string(),
                ports: "0.0.0.0:3000->3000/tcp".to_string(),
                image: "nginx:latest".to_string(),
                kind: None,
            },
            ServiceStatus {
                name: "db".to_string(),
                status: "Up 5 minutes (healthy)".to_string(),
                ports: "5432/tcp".to_string(),
                image: "postgres:16".to_string(),
                kind: None,
            },
        ];

        let output = format_service_table(&services);
        assert!(output.contains("web"));
        assert!(output.contains("db"));
        assert!(output.contains("3000"));
        assert!(output.contains("5432"));
    }

    #[test]
    fn test_format_service_table_has_header() {
        let services = vec![ServiceStatus {
            name: "web".to_string(),
            status: "Up".to_string(),
            ports: "3000/tcp".to_string(),
            image: "nginx:latest".to_string(),
            kind: None,
        }];

        let output = format_service_table(&services);
        assert!(output.contains("NAME"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("PORTS"));
    }

    #[test]
    fn test_colorize_status_up() {
        let colored = colorize_status("Up 5 minutes");
        // Should contain ANSI green escape codes
        assert!(colored.contains("Up 5 minutes"));
    }

    #[test]
    fn test_colorize_status_exit() {
        let colored = colorize_status("Exited (1)");
        assert!(colored.contains("Exited (1)"));
    }

    #[test]
    fn test_colorize_status_other() {
        let colored = colorize_status("Restarting");
        assert!(colored.contains("Restarting"));
    }
}
