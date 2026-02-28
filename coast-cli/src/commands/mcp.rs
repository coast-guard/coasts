/// `coast mcp` command — inspect MCP server configuration and tools.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{McpLocationsRequest, McpLsRequest, McpToolsRequest, Request, Response};

/// Arguments for `coast mcp`.
#[derive(Debug, Args)]
pub struct McpArgs {
    /// Name of the coast instance.
    pub name: String,

    #[command(subcommand)]
    pub command: McpCommand,
}

/// MCP subcommands.
#[derive(Debug, Subcommand)]
pub enum McpCommand {
    /// List declared MCP servers and their status.
    Ls,
    /// List tools exposed by an MCP server.
    Tools(McpToolsArgs),
    /// Show MCP client config file locations.
    Locations,
}

/// Arguments for `coast mcp <name> tools`.
#[derive(Debug, Args)]
pub struct McpToolsArgs {
    /// MCP server name.
    pub server: String,

    #[command(subcommand)]
    pub subcommand: Option<McpToolsSubcommand>,
}

/// Subcommands under `coast mcp <name> tools <server>`.
#[derive(Debug, Subcommand)]
pub enum McpToolsSubcommand {
    /// Show detailed info about a specific tool.
    Info(McpToolInfoArgs),
}

/// Arguments for `coast mcp <name> tools <server> info`.
#[derive(Debug, Args)]
pub struct McpToolInfoArgs {
    /// Tool name to inspect.
    pub tool: String,
}

pub async fn execute(args: &McpArgs, project: &str) -> Result<()> {
    match &args.command {
        McpCommand::Ls => execute_ls(&args.name, project).await,
        McpCommand::Tools(tools_args) => execute_tools(&args.name, project, tools_args).await,
        McpCommand::Locations => execute_locations(&args.name, project).await,
    }
}

async fn execute_ls(name: &str, project: &str) -> Result<()> {
    let request = Request::McpLs(McpLsRequest {
        name: name.to_string(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::McpLs(resp) => {
            if resp.servers.is_empty() {
                println!("No MCP servers configured for instance '{}'.", resp.name);
                return Ok(());
            }

            println!("MCP servers in coast instance '{}':", resp.name);
            println!(
                "  {:<20} {:<12} {:<35} {}",
                "NAME".bold(),
                "TYPE".bold(),
                "COMMAND".bold(),
                "STATUS".bold(),
            );
            for server in &resp.servers {
                let type_str = match server.proxy.as_deref() {
                    Some("host") => "host".yellow().to_string(),
                    _ => "internal".cyan().to_string(),
                };
                let cmd = server.command.as_deref().unwrap_or("-");
                let cmd_with_args = if server.args.is_empty() {
                    cmd.to_string()
                } else {
                    format!("{} {}", cmd, server.args.join(" "))
                };
                let cmd_display = if cmd_with_args.len() > 33 {
                    format!("{}...", &cmd_with_args[..30])
                } else {
                    cmd_with_args
                };
                let status = colorize_status(&server.status);
                println!(
                    "  {:<20} {:<12} {:<35} {}",
                    server.name, type_str, cmd_display, status,
                );
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    }
}

async fn execute_tools(name: &str, project: &str, args: &McpToolsArgs) -> Result<()> {
    let tool_name = args
        .subcommand
        .as_ref()
        .map(|McpToolsSubcommand::Info(info_args)| info_args.tool.clone());

    let request = Request::McpTools(McpToolsRequest {
        name: name.to_string(),
        project: project.to_string(),
        server: args.server.clone(),
        tool: tool_name.clone(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::McpTools(resp) => {
            if let Some(ref info) = resp.tool_info {
                println!("Tool: {}", info.name.bold());
                println!("Server: {}", resp.server);
                println!("Description: {}", info.description);
                println!();
                println!("{}:", "Input Schema".bold());
                let schema_pretty =
                    serde_json::to_string_pretty(&info.input_schema).unwrap_or_default();
                for line in schema_pretty.lines() {
                    println!("  {}", line);
                }
                return Ok(());
            }

            if resp.tools.is_empty() {
                println!("No tools found for MCP server '{}'.", resp.server);
                return Ok(());
            }

            println!("Tools for MCP server '{}':", resp.server);
            println!("  {:<25} {}", "NAME".bold(), "DESCRIPTION".bold(),);
            for tool in &resp.tools {
                println!("  {:<25} {}", tool.name, tool.description);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    }
}

async fn execute_locations(name: &str, project: &str) -> Result<()> {
    let request = Request::McpLocations(McpLocationsRequest {
        name: name.to_string(),
        project: project.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::McpLocations(resp) => {
            if resp.locations.is_empty() {
                println!(
                    "No MCP client connectors configured for instance '{}'.",
                    resp.name
                );
                return Ok(());
            }

            println!("MCP client config locations for '{}':", resp.name);
            println!(
                "  {:<20} {:<16} {}",
                "CLIENT".bold(),
                "FORMAT".bold(),
                "PATH".bold(),
            );
            for loc in &resp.locations {
                println!(
                    "  {:<20} {:<16} {}",
                    loc.client, loc.format, loc.config_path,
                );
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    }
}

fn colorize_status(status: &str) -> String {
    match status {
        "installed" => "installed".green().to_string(),
        "proxied" => "proxied".yellow().to_string(),
        "not-installed" => "not-installed".red().to_string(),
        other => other.dimmed().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colorize_status_installed() {
        let result = colorize_status("installed");
        assert!(result.contains("installed"));
    }

    #[test]
    fn test_colorize_status_proxied() {
        let result = colorize_status("proxied");
        assert!(result.contains("proxied"));
    }

    #[test]
    fn test_colorize_status_not_installed() {
        let result = colorize_status("not-installed");
        assert!(result.contains("not-installed"));
    }

    #[test]
    fn test_colorize_status_unknown() {
        let result = colorize_status("broken");
        assert!(result.contains("broken"));
    }
}
