use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to list MCP servers for a coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLsRequest {
    pub name: String,
    pub project: String,
}

/// Summary of a single MCP server declaration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpServerSummary {
    pub name: String,
    pub proxy: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub status: String,
}

/// Response listing all MCP servers for an instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLsResponse {
    pub name: String,
    pub servers: Vec<McpServerSummary>,
}

/// Request to list tools for a specific MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpToolsRequest {
    pub name: String,
    pub project: String,
    pub server: String,
    pub tool: Option<String>,
}

/// Summary of an MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpToolSummary {
    pub name: String,
    pub description: String,
}

/// Detailed info about a single MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Response listing tools for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpToolsResponse {
    pub server: String,
    pub tools: Vec<McpToolSummary>,
    pub tool_info: Option<McpToolInfo>,
}

/// Request to list MCP client connector locations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLocationsRequest {
    pub name: String,
    pub project: String,
}

/// Summary of an MCP client connector location.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLocationSummary {
    pub client: String,
    pub format: String,
    pub config_path: String,
}

/// Response listing MCP client connector locations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpLocationsResponse {
    pub name: String,
    pub locations: Vec<McpLocationSummary>,
}
