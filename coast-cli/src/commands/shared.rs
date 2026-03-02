/// `coast shared-services` command -- manage shared services.
///
/// Provides subcommands to list status, start, stop, restart, and remove
/// shared services that run on the host Docker daemon and are shared
/// across coast instances.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{Request, Response, SharedRequest, SharedServiceInfo};

/// Arguments for `coast shared-services`.
#[derive(Debug, Args)]
pub struct SharedArgs {
    /// Shared service subcommand.
    #[command(subcommand)]
    pub action: SharedAction,

    /// Suppress all output; only exit code indicates success or failure.
    #[arg(short = 's', long, global = true)]
    pub silent: bool,

    /// Show verbose detail (container IDs, ports, images).
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
}

/// Subcommands for `coast shared-services`.
#[derive(Debug, Subcommand)]
pub enum SharedAction {
    /// Show shared service status (like docker ps for shared services).
    Ps,
    /// Start a shared service, or all with --all.
    Start {
        /// Name of the shared service to start.
        service: Option<String>,
        /// Start all shared services.
        #[arg(long)]
        all: bool,
    },
    /// Stop a shared service, or all with --all.
    Stop {
        /// Name of the shared service to stop.
        service: Option<String>,
        /// Stop all shared services.
        #[arg(long)]
        all: bool,
    },
    /// Restart a shared service, or all with --all.
    Restart {
        /// Name of the shared service to restart.
        service: Option<String>,
        /// Restart all shared services.
        #[arg(long)]
        all: bool,
    },
    /// Remove a shared service.
    Rm {
        /// Name of the shared service to remove.
        service: String,
    },
}

/// Resolve the service target for start/stop/restart.
/// Returns `Some(name)` for a single service, `None` for --all.
fn resolve_service_target(service: &Option<String>, all: bool) -> Result<Option<String>> {
    match (service, all) {
        (Some(s), false) => Ok(Some(s.clone())),
        (None, true) => Ok(None),
        (Some(_), true) => {
            bail!("Cannot specify both a service name and --all. Use one or the other.");
        }
        (None, false) => {
            bail!("Specify a service name or use --all to target every shared service.");
        }
    }
}

/// Verbosity level derived from CLI flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verbosity {
    Silent,
    Default,
    Verbose,
}

/// Execute the `coast shared-services` command.
pub async fn execute(args: &SharedArgs, project: &str) -> Result<()> {
    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let request = match &args.action {
        SharedAction::Ps => Request::Shared(SharedRequest::Ps {
            project: project.to_string(),
        }),
        SharedAction::Start { service, all } => {
            let target = resolve_service_target(service, *all)?;
            Request::Shared(SharedRequest::Start {
                project: project.to_string(),
                service: target,
            })
        }
        SharedAction::Stop { service, all } => {
            let target = resolve_service_target(service, *all)?;
            Request::Shared(SharedRequest::Stop {
                project: project.to_string(),
                service: target,
            })
        }
        SharedAction::Restart { service, all } => {
            let target = resolve_service_target(service, *all)?;
            Request::Shared(SharedRequest::Restart {
                project: project.to_string(),
                service: target,
            })
        }
        SharedAction::Rm { service } => Request::Shared(SharedRequest::Rm {
            project: project.to_string(),
            service: service.clone(),
        }),
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Shared(resp) => {
            if verbosity == Verbosity::Silent {
                return Ok(());
            }

            if !resp.message.is_empty() {
                println!("{} {}", "ok".green().bold(), resp.message);
            }

            if !resp.services.is_empty() {
                println!("{}", format_shared_service_table(&resp.services, verbosity));
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

/// Format a table of shared services for display.
fn format_shared_service_table(services: &[SharedServiceInfo], verbosity: Verbosity) -> String {
    if services.is_empty() {
        return "  No shared services.".to_string();
    }

    let mut lines = Vec::new();

    if verbosity == Verbosity::Verbose {
        lines.push(format!(
            "  {:<20} {:<15} {:<25} {:<15} {}",
            "SERVICE".bold(),
            "STATUS".bold(),
            "IMAGE".bold(),
            "CONTAINER".bold(),
            "PORTS".bold(),
        ));

        for svc in services {
            let status_colored = colorize_shared_status(&svc.status);
            let image = svc.image.as_deref().unwrap_or("-");
            let container = svc
                .container_id
                .as_deref()
                .map(|id| &id[..12.min(id.len())])
                .unwrap_or("-");
            let ports = svc.ports.as_deref().unwrap_or("-");

            lines.push(format!(
                "  {:<20} {:<15} {:<25} {:<15} {}",
                svc.name, status_colored, image, container, ports,
            ));
        }
    } else {
        lines.push(format!(
            "  {:<20} {:<15} {}",
            "SERVICE".bold(),
            "STATUS".bold(),
            "IMAGE".bold(),
        ));

        for svc in services {
            let status_colored = colorize_shared_status(&svc.status);
            let image = svc.image.as_deref().unwrap_or("-");

            lines.push(format!(
                "  {:<20} {:<15} {}",
                svc.name, status_colored, image,
            ));
        }
    }

    lines.join("\n")
}

/// Apply color to a shared service status string.
fn colorize_shared_status(status: &str) -> String {
    let lower = status.to_lowercase();
    if lower.contains("running") {
        status.green().to_string()
    } else if lower.contains("stopped") || lower.contains("exited") {
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
        args: SharedArgs,
    }

    #[test]
    fn test_shared_ps_args() {
        let cli = TestCli::try_parse_from(["test", "ps"]).unwrap();
        assert!(matches!(cli.args.action, SharedAction::Ps));
    }

    #[test]
    fn test_shared_start_single() {
        let cli = TestCli::try_parse_from(["test", "start", "postgres"]).unwrap();
        match cli.args.action {
            SharedAction::Start { service, all } => {
                assert_eq!(service, Some("postgres".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected Start action"),
        }
    }

    #[test]
    fn test_shared_start_all() {
        let cli = TestCli::try_parse_from(["test", "start", "--all"]).unwrap();
        match cli.args.action {
            SharedAction::Start { service, all } => {
                assert!(service.is_none());
                assert!(all);
            }
            _ => panic!("Expected Start action"),
        }
    }

    #[test]
    fn test_shared_stop_single() {
        let cli = TestCli::try_parse_from(["test", "stop", "redis"]).unwrap();
        match cli.args.action {
            SharedAction::Stop { service, all } => {
                assert_eq!(service, Some("redis".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected Stop action"),
        }
    }

    #[test]
    fn test_shared_stop_all() {
        let cli = TestCli::try_parse_from(["test", "stop", "--all"]).unwrap();
        match cli.args.action {
            SharedAction::Stop { service, all } => {
                assert!(service.is_none());
                assert!(all);
            }
            _ => panic!("Expected Stop action"),
        }
    }

    #[test]
    fn test_shared_restart_single() {
        let cli = TestCli::try_parse_from(["test", "restart", "mongodb"]).unwrap();
        match cli.args.action {
            SharedAction::Restart { service, all } => {
                assert_eq!(service, Some("mongodb".to_string()));
                assert!(!all);
            }
            _ => panic!("Expected Restart action"),
        }
    }

    #[test]
    fn test_shared_restart_all() {
        let cli = TestCli::try_parse_from(["test", "restart", "--all"]).unwrap();
        match cli.args.action {
            SharedAction::Restart { service, all } => {
                assert!(service.is_none());
                assert!(all);
            }
            _ => panic!("Expected Restart action"),
        }
    }

    #[test]
    fn test_shared_rm_args() {
        let cli = TestCli::try_parse_from(["test", "rm", "postgres"]).unwrap();
        match cli.args.action {
            SharedAction::Rm { service } => {
                assert_eq!(service, "postgres");
            }
            _ => panic!("Expected Rm action"),
        }
    }

    #[test]
    fn test_shared_rm_missing_service() {
        let result = TestCli::try_parse_from(["test", "rm"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_shared_verbose_flag() {
        let cli = TestCli::try_parse_from(["test", "--verbose", "ps"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_shared_silent_flag() {
        let cli = TestCli::try_parse_from(["test", "--silent", "ps"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_shared_verbose_short_flag() {
        let cli = TestCli::try_parse_from(["test", "-v", "ps"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_shared_silent_short_flag() {
        let cli = TestCli::try_parse_from(["test", "-s", "ps"]).unwrap();
        assert!(cli.args.silent);
    }

    #[test]
    fn test_resolve_service_target_single() {
        let result = resolve_service_target(&Some("postgres".to_string()), false).unwrap();
        assert_eq!(result, Some("postgres".to_string()));
    }

    #[test]
    fn test_resolve_service_target_all() {
        let result = resolve_service_target(&None, true).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_service_target_both() {
        let result = resolve_service_target(&Some("pg".to_string()), true);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_service_target_neither() {
        let result = resolve_service_target(&None, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_shared_service_table_empty() {
        let output = format_shared_service_table(&[], Verbosity::Default);
        assert_eq!(output, "  No shared services.");
    }

    #[test]
    fn test_format_shared_service_table_default() {
        let services = vec![
            SharedServiceInfo {
                name: "postgres".to_string(),
                container_id: Some("abc123def456789".to_string()),
                status: "running".to_string(),
                image: Some("postgres:15".to_string()),
                ports: Some("0.0.0.0:5432->5432/tcp".to_string()),
            },
            SharedServiceInfo {
                name: "redis".to_string(),
                container_id: None,
                status: "stopped".to_string(),
                image: Some("redis:7".to_string()),
                ports: None,
            },
        ];

        let output = format_shared_service_table(&services, Verbosity::Default);
        assert!(output.contains("postgres"));
        assert!(output.contains("redis"));
        assert!(output.contains("SERVICE"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("IMAGE"));
        assert!(!output.contains("CONTAINER"));
        assert!(!output.contains("PORTS"));
    }

    #[test]
    fn test_format_shared_service_table_verbose() {
        let services = vec![SharedServiceInfo {
            name: "postgres".to_string(),
            container_id: Some("abc123def456789".to_string()),
            status: "running".to_string(),
            image: Some("postgres:15".to_string()),
            ports: Some("0.0.0.0:5432->5432/tcp".to_string()),
        }];

        let output = format_shared_service_table(&services, Verbosity::Verbose);
        assert!(output.contains("SERVICE"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("IMAGE"));
        assert!(output.contains("CONTAINER"));
        assert!(output.contains("PORTS"));
        assert!(output.contains("abc123def456"));
        assert!(output.contains("5432"));
    }

    #[test]
    fn test_colorize_shared_status_running() {
        let colored = colorize_shared_status("running");
        assert!(colored.contains("running"));
    }

    #[test]
    fn test_colorize_shared_status_stopped() {
        let colored = colorize_shared_status("stopped");
        assert!(colored.contains("stopped"));
    }

    #[test]
    fn test_colorize_shared_status_other() {
        let colored = colorize_shared_status("creating");
        assert!(colored.contains("creating"));
    }
}
