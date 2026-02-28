use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Volume isolation strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeStrategy {
    Isolated,
    Shared,
}

impl VolumeStrategy {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "isolated" => Some(Self::Isolated),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }
}

/// Configuration for a volume declared in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeConfig {
    pub name: String,
    pub strategy: VolumeStrategy,
    pub service: String,
    pub mount: PathBuf,
    pub snapshot_source: Option<String>,
}

/// Configuration for a shared service in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedServiceConfig {
    pub name: String,
    pub image: String,
    pub ports: Vec<u16>,
    pub volumes: Vec<String>,
    pub env: HashMap<String, String>,
    pub auto_create_db: bool,
    pub inject: Option<InjectType>,
}

/// Configuration for a secret in the Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretConfig {
    pub name: String,
    pub extractor: String,
    pub params: HashMap<String, String>,
    pub inject: InjectType,
    pub ttl: Option<String>,
}

/// How a secret or connection detail is injected into a coast container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectType {
    Env(String),
    File(PathBuf),
}

impl InjectType {
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(var) = s.strip_prefix("env:") {
            if var.is_empty() {
                return Err(
                    "inject env target cannot be empty. Use format: env:VAR_NAME".to_string(),
                );
            }
            Ok(Self::Env(var.to_string()))
        } else if let Some(path) = s.strip_prefix("file:") {
            if path.is_empty() {
                return Err(
                    "inject file target cannot be empty. Use format: file:/path/in/container"
                        .to_string(),
                );
            }
            Ok(Self::File(PathBuf::from(path)))
        } else {
            Err(format!(
                "invalid inject format '{}'. Expected 'env:VAR_NAME' or 'file:/path/in/container'",
                s
            ))
        }
    }

    pub fn to_inject_string(&self) -> String {
        match self {
            Self::Env(var) => format!("env:{var}"),
            Self::File(path) => format!("file:{}", path.display()),
        }
    }
}
