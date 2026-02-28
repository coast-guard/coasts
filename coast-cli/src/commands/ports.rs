/// `coast ports` command — show and manage port allocations for a coast instance.
///
/// Displays a table of canonical and dynamic port mappings including
/// the logical service name for each port. Supports setting/unsetting
/// a primary service.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{PortsRequest, Request, Response};

/// Arguments for `coast ports`.
#[derive(Debug, Args)]
pub struct PortsArgs {
    /// Name of the coast instance.
    pub name: String,
    /// Port subcommand (omit to list ports).
    #[command(subcommand)]
    pub action: Option<PortsAction>,
}

/// Subcommands for `coast ports`.
#[derive(Debug, Subcommand)]
pub enum PortsAction {
    /// Mark a service as the primary for this instance.
    #[command(name = "set-primary")]
    SetPrimary {
        /// Logical service name.
        service: String,
    },
    /// Remove the primary service designation.
    #[command(name = "unset-primary")]
    UnsetPrimary,
}

/// Execute the `coast ports` command.
pub async fn execute(args: &PortsArgs, project: &str) -> Result<()> {
    let request = match &args.action {
        None => Request::Ports(PortsRequest::List {
            name: args.name.clone(),
            project: project.to_string(),
        }),
        Some(PortsAction::SetPrimary { service }) => Request::Ports(PortsRequest::SetPrimary {
            name: args.name.clone(),
            project: project.to_string(),
            service: service.clone(),
        }),
        Some(PortsAction::UnsetPrimary) => Request::Ports(PortsRequest::UnsetPrimary {
            name: args.name.clone(),
            project: project.to_string(),
        }),
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Ports(resp) => {
            if let Some(ref msg) = resp.message {
                println!("{} {}", "ok".green().bold(), msg);
            } else {
                println!(
                    "{} Port allocations for '{}':",
                    "ok".green().bold(),
                    resp.name,
                );
            }
            println!(
                "{}",
                super::format_port_table(&resp.ports, resp.subdomain_host.as_deref())
            );
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
    use coast_core::types::PortMapping;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: PortsArgs,
    }

    #[test]
    fn test_ports_list_default() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.action.is_none());
    }

    #[test]
    fn test_ports_set_primary() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "set-primary", "web"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        match cli.args.action {
            Some(PortsAction::SetPrimary { service }) => {
                assert_eq!(service, "web");
            }
            _ => panic!("expected SetPrimary"),
        }
    }

    #[test]
    fn test_ports_unset_primary() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "unset-primary"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(matches!(cli.args.action, Some(PortsAction::UnsetPrimary)));
    }

    #[test]
    fn test_ports_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_port_table_list_with_ports() {
        let ports = vec![
            PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: false,
            },
            PortMapping {
                logical_name: "api".to_string(),
                canonical_port: 8080,
                dynamic_port: 52341,
                is_primary: true,
            },
        ];
        let output = super::super::format_port_table(&ports, None);
        assert!(output.contains("web"));
        assert!(output.contains("3000"));
        assert!(output.contains("52340"));
        assert!(output.contains("api"));
        assert!(output.contains("8080"));
        assert!(output.contains("52341"));
        assert!(output.contains("★"));
    }

    #[test]
    fn test_format_port_table_with_subdomain_host() {
        let ports = vec![PortMapping {
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            is_primary: false,
        }];
        let output = super::super::format_port_table(&ports, Some("dev-1.localhost"));
        assert!(output.contains("dev-1.localhost:52340"));
    }

    #[test]
    fn test_format_port_table_empty_ports() {
        let output = super::super::format_port_table(&[], None);
        assert_eq!(output, "  No port mappings configured.");
    }
}
