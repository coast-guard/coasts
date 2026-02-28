use serde::{Deserialize, Serialize};

/// Restart policy for bare process services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum RestartPolicy {
    #[default]
    No,
    OnFailure,
    Always,
}

impl RestartPolicy {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "no" => Some(Self::No),
            "on-failure" | "on_failure" => Some(Self::OnFailure),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::No => "no",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

impl std::fmt::Display for RestartPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for a bare process service (no Docker compose).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BareServiceConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default)]
    pub install: Vec<String>,
}
