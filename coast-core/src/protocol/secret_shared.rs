use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to manage per-instance secrets.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SecretRequest {
    /// Set a per-instance secret override.
    Set {
        instance: String,
        project: String,
        name: String,
        value: String,
    },
    /// List secrets for an instance.
    List { instance: String, project: String },
}

/// Info about a secret.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecretInfo {
    pub name: String,
    pub extractor: String,
    pub inject: String,
    pub is_override: bool,
}

/// Response for secret operations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecretResponse {
    pub message: String,
    pub secrets: Vec<SecretInfo>,
}

/// Request to manage shared services.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum SharedRequest {
    /// Show shared service status (docker ps equivalent).
    Ps { project: String },
    /// Stop a shared service container (None = all services).
    Stop {
        project: String,
        service: Option<String>,
    },
    /// Start a stopped shared service container (None = all services).
    Start {
        project: String,
        service: Option<String>,
    },
    /// Restart a shared service container (None = all services).
    Restart {
        project: String,
        service: Option<String>,
    },
    /// Remove a shared service.
    Rm { project: String, service: String },
}

/// Info about a shared service.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedServiceInfo {
    pub name: String,
    pub container_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub ports: Option<String>,
}

/// Response for shared service operations.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedResponse {
    pub message: String,
    pub services: Vec<SharedServiceInfo>,
}
