/// Handlers for `coast mcp` subcommands and MCP runtime installation.
///
/// Reads MCP configuration from the stored Coastfile and queries the live
/// container state to determine installation status and tool listings.
/// Also provides config generation for MCP client connectors.
use tracing::info;

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    McpLocationSummary, McpLocationsRequest, McpLocationsResponse, McpLsRequest, McpLsResponse,
    McpServerSummary, McpToolInfo, McpToolSummary, McpToolsRequest, McpToolsResponse,
};
use coast_core::types::{InstanceStatus, McpClientFormat, McpServerConfig};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

fn read_coastfile_for_project(project: &str) -> Result<Coastfile> {
    let home =
        dirs::home_dir().ok_or_else(|| CoastError::state("Cannot determine home directory"))?;

    let latest = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if latest.exists() {
        return Coastfile::from_file(&latest);
    }

    Err(CoastError::coastfile(format!(
        "No Coastfile found for project '{project}'. Run `coast build` first."
    )))
}

/// Handle `coast mcp <name> ls`.
pub async fn handle_ls(req: McpLsRequest, state: &AppState) -> Result<McpLsResponse> {
    info!(name = %req.name, project = %req.project, "handling mcp ls request");

    let coastfile = read_coastfile_for_project(&req.project)?;

    let container_id = resolve_container_if_running(state, &req.project, &req.name).await;

    let mut servers = Vec::new();
    for mcp in &coastfile.mcp_servers {
        let is_host = mcp.is_host_proxied();
        let status = if is_host {
            "proxied".to_string()
        } else if let Some(ref cid) = container_id {
            check_mcp_installed(state, cid, &mcp.name).await
        } else {
            "unknown".to_string()
        };

        let command_display = mcp.command.clone().or_else(|| {
            if is_host {
                Some("(from host config)".to_string())
            } else {
                None
            }
        });

        servers.push(McpServerSummary {
            name: mcp.name.clone(),
            proxy: mcp.proxy.as_ref().map(|p| p.as_str().to_string()),
            command: command_display,
            args: mcp.args.clone(),
            status,
        });
    }

    Ok(McpLsResponse {
        name: req.name,
        servers,
    })
}

/// Handle `coast mcp <name> tools <server>`.
pub async fn handle_tools(req: McpToolsRequest, state: &AppState) -> Result<McpToolsResponse> {
    info!(name = %req.name, project = %req.project, server = %req.server, "handling mcp tools request");

    let coastfile = read_coastfile_for_project(&req.project)?;

    let mcp_config = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == req.server)
        .ok_or_else(|| {
            CoastError::state(format!(
                "MCP server '{}' not found in Coastfile. Available: {}",
                req.server,
                coastfile
                    .mcp_servers
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

    if mcp_config.is_host_proxied() {
        return Err(CoastError::state(format!(
            "MCP server '{}' is host-proxied. Tool listing for host-proxied servers \
             is not yet supported. Query the MCP server directly on the host.",
            req.server
        )));
    }

    let container_id = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
            CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            }
        })?;

        if instance.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped. Start it first to query MCP tools.",
                req.name
            )));
        }

        instance.container_id.ok_or_else(|| {
            CoastError::state(format!("Instance '{}' has no container ID.", req.name))
        })?
    };

    let command = mcp_config.command.as_deref().unwrap_or("node");
    let args_str = mcp_config
        .args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");

    let jsonrpc_script = format!(
        concat!(
            "cd /mcp/{server} && ",
            "echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"capabilities\":{{}}}}}}' | ",
            "{cmd} {args} 2>/dev/null | head -1 > /dev/null && ",
            "echo '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{{}}}}' | ",
            "{cmd} {args} 2>/dev/null | head -1"
        ),
        server = req.server,
        cmd = command,
        args = args_str,
    );

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is not available."))?;

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let exec_result = runtime
        .exec_in_coast(&container_id, &["sh", "-c", &jsonrpc_script])
        .await
        .map_err(|e| CoastError::docker(format!("Failed to query MCP tools: {e}")))?;

    let output = exec_result.stdout.trim();
    let tools = parse_tools_list_response(output);

    let tool_info = if let Some(ref tool_name) = req.tool {
        tools
            .iter()
            .find(|t| t.name == *tool_name)
            .map(|t| McpToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
    } else {
        None
    };

    let tool_summaries = tools
        .iter()
        .map(|t| McpToolSummary {
            name: t.name.clone(),
            description: t.description.clone(),
        })
        .collect();

    Ok(McpToolsResponse {
        server: req.server,
        tools: tool_summaries,
        tool_info,
    })
}

struct ParsedTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

fn parse_tools_list_response(output: &str) -> Vec<ParsedTool> {
    let parsed: serde_json::Value = match serde_json::from_str(output) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let Some(tools) = parsed
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
    else {
        return Vec::new();
    };

    tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            let description = t
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = t
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            Some(ParsedTool {
                name,
                description,
                input_schema,
            })
        })
        .collect()
}

/// Handle `coast mcp <name> locations`.
pub async fn handle_locations(
    req: McpLocationsRequest,
    _state: &AppState,
) -> Result<McpLocationsResponse> {
    info!(name = %req.name, project = %req.project, "handling mcp locations request");

    let coastfile = read_coastfile_for_project(&req.project)?;

    let locations = coastfile
        .mcp_clients
        .iter()
        .filter_map(|client| {
            let config_path = client.resolved_config_path()?;
            let format = client
                .format
                .as_ref()
                .map(|f| f.as_str().to_string())
                .unwrap_or_else(|| {
                    if client.run.is_some() {
                        "custom".to_string()
                    } else {
                        "external".to_string()
                    }
                });
            Some(McpLocationSummary {
                client: client.name.clone(),
                format,
                config_path: config_path.to_string(),
            })
        })
        .collect();

    Ok(McpLocationsResponse {
        name: req.name,
        locations,
    })
}

/// Generate MCP client config JSON for a given built-in format.
///
/// Produces the JSON string that should be written to the config file path
/// so the AI tool discovers all declared MCP servers.
pub fn generate_mcp_client_config(servers: &[McpServerConfig], format: &McpClientFormat) -> String {
    match format {
        McpClientFormat::ClaudeCode => generate_claude_code_config(servers),
        McpClientFormat::Cursor => generate_cursor_config(servers),
    }
}

fn generate_claude_code_config(servers: &[McpServerConfig]) -> String {
    let mut mcp_servers = serde_json::Map::new();

    for server in servers {
        let mut entry = serde_json::Map::new();

        if server.is_host_proxied() {
            entry.insert(
                "command".to_string(),
                serde_json::Value::String("coast-mcp-proxy".to_string()),
            );
            entry.insert(
                "args".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::String(server.name.clone())]),
            );
        } else {
            if let Some(ref cmd) = server.command {
                entry.insert(
                    "command".to_string(),
                    serde_json::Value::String(cmd.clone()),
                );
            }
            let args: Vec<serde_json::Value> = server
                .args
                .iter()
                .map(|a| serde_json::Value::String(a.clone()))
                .collect();
            entry.insert("args".to_string(), serde_json::Value::Array(args));
            entry.insert(
                "cwd".to_string(),
                serde_json::Value::String(format!("/mcp/{}/", server.name)),
            );
        }

        if !server.env.is_empty() {
            let env_map: serde_json::Map<String, serde_json::Value> = server
                .env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            entry.insert("env".to_string(), serde_json::Value::Object(env_map));
        }

        mcp_servers.insert(server.name.clone(), serde_json::Value::Object(entry));
    }

    let root = serde_json::json!({ "mcpServers": mcp_servers });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
}

fn generate_cursor_config(servers: &[McpServerConfig]) -> String {
    let mut mcp_servers = serde_json::Map::new();

    for server in servers {
        let mut entry = serde_json::Map::new();

        if server.is_host_proxied() {
            entry.insert(
                "command".to_string(),
                serde_json::Value::String("coast-mcp-proxy".to_string()),
            );
            entry.insert(
                "args".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::String(server.name.clone())]),
            );
        } else {
            if let Some(ref cmd) = server.command {
                entry.insert(
                    "command".to_string(),
                    serde_json::Value::String(cmd.clone()),
                );
            }
            let args: Vec<serde_json::Value> = server
                .args
                .iter()
                .map(|a| serde_json::Value::String(a.clone()))
                .collect();
            entry.insert("args".to_string(), serde_json::Value::Array(args));
            entry.insert(
                "cwd".to_string(),
                serde_json::Value::String(format!("/mcp/{}/", server.name)),
            );
        }

        if !server.env.is_empty() {
            let env_map: serde_json::Map<String, serde_json::Value> = server
                .env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            entry.insert("env".to_string(), serde_json::Value::Object(env_map));
        }

        mcp_servers.insert(server.name.clone(), serde_json::Value::Object(entry));
    }

    let root = serde_json::json!({ "mcpServers": mcp_servers });
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
}

async fn resolve_container_if_running(
    state: &AppState,
    project: &str,
    name: &str,
) -> Option<String> {
    let db = state.db.lock().await;
    let instance = db.get_instance(project, name).ok()??;
    if instance.status == InstanceStatus::Stopped {
        return None;
    }
    instance.container_id
}

async fn check_mcp_installed(state: &AppState, container_id: &str, server_name: &str) -> String {
    let Some(docker) = state.docker.as_ref() else {
        return "unknown".to_string();
    };

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let check_cmd = format!("test -d /mcp/{} && echo yes || echo no", server_name);
    match runtime
        .exec_in_coast(container_id, &["sh", "-c", &check_cmd])
        .await
    {
        Ok(result) => {
            if result.stdout.trim() == "yes" {
                "installed".to_string()
            } else {
                "not-installed".to_string()
            }
        }
        Err(_) => "unknown".to_string(),
    }
}
