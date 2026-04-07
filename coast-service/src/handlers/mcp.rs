use tracing::info;

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    McpLsRequest, McpLsResponse, McpServerSummary, McpToolInfo, McpToolSummary, McpToolsRequest,
    McpToolsResponse,
};
use coast_docker::runtime::Runtime;

use crate::state::ServiceState;

fn read_coastfile_for_project(project: &str) -> Result<Coastfile> {
    let home = crate::state::service_home();
    let project_dir = home.join("images").join(project);

    for prefix in &["latest-remote", "latest"] {
        let dir = project_dir.join(prefix);
        let path = dir.join("coastfile.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(|e| {
                CoastError::coastfile(format!("failed to read {}: {e}", path.display()))
            })?;
            return Coastfile::parse(&content, &dir);
        }
    }

    Err(CoastError::coastfile(format!(
        "No Coastfile found for project '{project}' on remote."
    )))
}

pub async fn handle_ls(req: McpLsRequest, state: &ServiceState) -> Result<McpLsResponse> {
    info!(name = %req.name, project = %req.project, "remote mcp ls request");

    let coastfile = read_coastfile_for_project(&req.project)?;

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?;
    drop(db);

    let container_id = instance.and_then(|i| i.container_id);

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

        servers.push(McpServerSummary {
            name: mcp.name.clone(),
            proxy: mcp.proxy.as_ref().map(|p| p.as_str().to_string()),
            command: mcp.command.clone(),
            args: mcp.args.clone(),
            status,
        });
    }

    Ok(McpLsResponse {
        name: req.name,
        servers,
    })
}

pub async fn handle_tools(req: McpToolsRequest, state: &ServiceState) -> Result<McpToolsResponse> {
    info!(name = %req.name, project = %req.project, server = %req.server, "remote mcp tools request");

    let coastfile = read_coastfile_for_project(&req.project)?;

    let mcp_config = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == req.server)
        .ok_or_else(|| {
            CoastError::state(format!(
                "MCP server '{}' not found in Coastfile",
                req.server
            ))
        })?;

    if mcp_config.is_host_proxied() {
        return Err(CoastError::state(format!(
            "MCP server '{}' is host-proxied. Query it from the host.",
            req.server
        )));
    }

    let db = state.db.lock().await;
    let instance = db
        .get_instance(&req.project, &req.name)?
        .ok_or_else(|| CoastError::state(format!("instance '{}' not found", req.name)))?;
    drop(db);

    let container_id = instance
        .container_id
        .ok_or_else(|| CoastError::state(format!("instance '{}' has no container", req.name)))?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::state("Docker not available"))?;

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

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let exec_result = rt
        .exec_in_coast(&container_id, &["sh", "-c", &jsonrpc_script])
        .await
        .map_err(|e| CoastError::docker(format!("Failed to query MCP tools: {e}")))?;

    let output = exec_result.stdout.trim();
    let tools = parse_tools_list_response(output);

    let tool_info = req.tool.as_ref().and_then(|tool_name| {
        tools
            .iter()
            .find(|t| t.name == *tool_name)
            .map(|t| McpToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
    });

    Ok(McpToolsResponse {
        server: req.server,
        tools: tools
            .iter()
            .map(|t| McpToolSummary {
                name: t.name.clone(),
                description: t.description.clone(),
            })
            .collect(),
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

async fn check_mcp_installed(
    state: &ServiceState,
    container_id: &str,
    server_name: &str,
) -> String {
    let Some(docker) = state.docker.as_ref() else {
        return "unknown".to_string();
    };

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let check_cmd = format!("test -d /mcp/{} && echo yes || echo no", server_name);
    match rt
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
