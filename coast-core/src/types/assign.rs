use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// What `coast assign` should do to a compose service when switching branches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AssignAction {
    None,
    /// Swap the filesystem only; rely on mount propagation + file watchers.
    Hot,
    #[default]
    Restart,
    Rebuild,
}

impl AssignAction {
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "hot" => Some(Self::Hot),
            "restart" => Some(Self::Restart),
            "rebuild" => Some(Self::Rebuild),
            _ => Option::None,
        }
    }
}

impl std::fmt::Display for AssignAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Hot => write!(f, "hot"),
            Self::Restart => write!(f, "restart"),
            Self::Rebuild => write!(f, "rebuild"),
        }
    }
}

/// Per-project configuration for `coast assign` behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignConfig {
    pub default: AssignAction,
    pub services: HashMap<String, AssignAction>,
    pub rebuild_triggers: HashMap<String, Vec<String>>,
    /// Paths to exclude from worktree sync (e.g. large irrelevant subdirectories).
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

impl Default for AssignConfig {
    fn default() -> Self {
        Self {
            default: AssignAction::Restart,
            services: HashMap::new(),
            rebuild_triggers: HashMap::new(),
            exclude_paths: Vec::new(),
        }
    }
}

impl AssignConfig {
    pub fn action_for_service(&self, service: &str) -> AssignAction {
        self.services
            .get(service)
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }
}

/// Services and volumes to omit from the compose file inside coast containers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OmitConfig {
    pub services: Vec<String>,
    pub volumes: Vec<String>,
}

impl OmitConfig {
    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && self.volumes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hot_action_from_str() {
        assert_eq!(AssignAction::from_str_value("hot"), Some(AssignAction::Hot));
    }

    #[test]
    fn test_hot_action_display() {
        assert_eq!(AssignAction::Hot.to_string(), "hot");
    }

    #[test]
    fn test_hot_action_serde_roundtrip() {
        let action = AssignAction::Hot;
        let json = serde_json::to_string(&action).unwrap();
        let back: AssignAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AssignAction::Hot);
    }

    #[test]
    fn test_action_for_service_hot() {
        let config = AssignConfig {
            default: AssignAction::Restart,
            services: [("web".to_string(), AssignAction::Hot)]
                .into_iter()
                .collect(),
            rebuild_triggers: Default::default(),
            exclude_paths: Default::default(),
        };
        assert_eq!(config.action_for_service("web"), AssignAction::Hot);
        assert_eq!(config.action_for_service("api"), AssignAction::Restart);
    }

    #[test]
    fn test_action_for_service_default_hot() {
        let config = AssignConfig {
            default: AssignAction::Hot,
            services: Default::default(),
            rebuild_triggers: Default::default(),
            exclude_paths: Default::default(),
        };
        assert_eq!(config.action_for_service("anything"), AssignAction::Hot);
    }

    #[test]
    fn test_exclude_paths_default_empty() {
        let config = AssignConfig::default();
        assert!(config.exclude_paths.is_empty());
    }
}
