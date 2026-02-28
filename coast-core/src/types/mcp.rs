use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Whether an MCP server is proxied from the host or runs internally in the coast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpProxyMode {
    Host,
}

impl McpProxyMode {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "host" => Some(Self::Host),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Host => "host",
        }
    }
}

impl std::fmt::Display for McpProxyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for an MCP server declared in the Coastfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub proxy: Option<McpProxyMode>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub install: Vec<String>,
    pub source: Option<String>,
}

impl McpServerConfig {
    pub fn is_host_proxied(&self) -> bool {
        self.proxy.is_some()
    }
}

/// Built-in MCP client config formats that coast can generate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpClientFormat {
    ClaudeCode,
    Cursor,
}

impl McpClientFormat {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "claude-code" => Some(Self::ClaudeCode),
            "cursor" => Some(Self::Cursor),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Cursor => "cursor",
        }
    }

    pub fn default_config_path(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "/root/.claude/mcp_servers.json",
            Self::Cursor => "/workspace/.cursor/mcp.json",
        }
    }
}

impl std::fmt::Display for McpClientFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for an MCP client connector declared in the Coastfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpClientConnectorConfig {
    pub name: String,
    pub format: Option<McpClientFormat>,
    pub config_path: Option<String>,
    pub run: Option<String>,
}

impl McpClientConnectorConfig {
    pub fn resolved_config_path(&self) -> Option<&str> {
        if let Some(ref p) = self.config_path {
            Some(p.as_str())
        } else {
            self.format
                .as_ref()
                .map(McpClientFormat::default_config_path)
        }
    }

    pub fn is_command_based(&self) -> bool {
        self.run.is_some()
    }
}
