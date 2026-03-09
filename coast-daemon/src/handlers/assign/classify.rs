use std::collections::HashMap;

use tracing::info;

use coast_core::types::{AssignAction, AssignConfig};

/// Classify each compose service into an AssignAction based on the config,
/// then apply rebuild_triggers optimization (downgrade rebuild -> restart
/// if no trigger files changed between branches).
pub(super) fn classify_services(
    service_names: &[String],
    config: &AssignConfig,
    changed_files: &[String],
) -> HashMap<String, AssignAction> {
    let mut result = HashMap::new();
    for svc in service_names {
        let mut action = config.action_for_service(svc);

        if action == AssignAction::Rebuild {
            if let Some(triggers) = config.rebuild_triggers.get(svc) {
                if !triggers.is_empty() {
                    let any_trigger_changed = triggers.iter().any(|trigger| {
                        changed_files
                            .iter()
                            .any(|f| f == trigger || f.ends_with(trigger))
                    });
                    if !any_trigger_changed {
                        info!(
                            service = %svc,
                            "no rebuild trigger files changed, downgrading rebuild -> restart"
                        );
                        action = AssignAction::Restart;
                    }
                }
            }
        }

        result.insert(svc.clone(), action);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_default_restart() {
        let config = AssignConfig::default();
        let services = vec!["api".to_string(), "db".to_string(), "redis".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
        assert_eq!(result.get("db"), Some(&AssignAction::Restart));
        assert_eq!(result.get("redis"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_with_overrides() {
        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("db".to_string(), AssignAction::None);
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: HashMap::new(),
            exclude_paths: vec![],
        };

        let services = vec!["api".to_string(), "db".to_string(), "worker".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
        assert_eq!(result.get("db"), Some(&AssignAction::None));
        assert_eq!(result.get("worker"), Some(&AssignAction::Rebuild));
    }

    #[test]
    fn test_rebuild_trigger_downgrade() {
        let mut triggers = HashMap::new();
        triggers.insert(
            "worker".to_string(),
            vec!["Dockerfile".to_string(), "package.json".to_string()],
        );

        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: triggers,
            exclude_paths: vec![],
        };

        let changed = vec!["src/main.rs".to_string(), "README.md".to_string()];
        let result = classify_services(
            &["worker".to_string(), "api".to_string()],
            &config,
            &changed,
        );
        assert_eq!(result.get("worker"), Some(&AssignAction::Restart));
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_rebuild_trigger_keeps_rebuild() {
        let mut triggers = HashMap::new();
        triggers.insert(
            "worker".to_string(),
            vec!["Dockerfile".to_string(), "package.json".to_string()],
        );

        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: triggers,
            exclude_paths: vec![],
        };

        let changed = vec!["Dockerfile".to_string(), "src/main.rs".to_string()];
        let result = classify_services(&["worker".to_string()], &config, &changed);
        assert_eq!(result.get("worker"), Some(&AssignAction::Rebuild));
    }

    #[test]
    fn test_default_none() {
        let config = AssignConfig {
            default: AssignAction::None,
            services: HashMap::new(),
            rebuild_triggers: HashMap::new(),
            exclude_paths: vec![],
        };

        let services = vec!["api".to_string(), "db".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::None));
        assert_eq!(result.get("db"), Some(&AssignAction::None));
    }

    #[test]
    fn test_hot() {
        let config = AssignConfig {
            default: AssignAction::Hot,
            services: Default::default(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("web"), Some(&AssignAction::Hot));
        assert_eq!(result.get("api"), Some(&AssignAction::Hot));
    }

    #[test]
    fn test_mixed_hot_restart() {
        let config = AssignConfig {
            default: AssignAction::Restart,
            services: [("web".to_string(), AssignAction::Hot)]
                .into_iter()
                .collect(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("web"), Some(&AssignAction::Hot));
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_hot_excluded_from_restart_and_rebuild_lists() {
        let config = AssignConfig {
            default: AssignAction::Hot,
            services: [("db".to_string(), AssignAction::Restart)]
                .into_iter()
                .collect(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string(), "db".to_string()];
        let result = classify_services(&services, &config, &[]);
        let restart: Vec<&str> = result
            .iter()
            .filter(|(_, a)| **a == AssignAction::Restart)
            .map(|(s, _)| s.as_str())
            .collect();
        let hot: Vec<&str> = result
            .iter()
            .filter(|(_, a)| **a == AssignAction::Hot)
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(restart, vec!["db"]);
        assert!(hot.contains(&"web"));
        assert!(hot.contains(&"api"));
    }

    #[test]
    fn test_empty_service_list() {
        let config = AssignConfig::default();
        let result = classify_services(&[], &config, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_all_triggers_match() {
        let mut triggers = HashMap::new();
        triggers.insert(
            "worker".to_string(),
            vec!["Dockerfile".to_string(), "Gemfile".to_string()],
        );

        let config = AssignConfig {
            default: AssignAction::Rebuild,
            services: HashMap::new(),
            rebuild_triggers: triggers,
            exclude_paths: vec![],
        };

        let changed = vec!["Dockerfile".to_string(), "Gemfile".to_string()];
        let result = classify_services(&["worker".to_string()], &config, &changed);
        assert_eq!(result.get("worker"), Some(&AssignAction::Rebuild));
    }

    #[test]
    fn test_rebuild_without_triggers_stays_rebuild() {
        let config = AssignConfig {
            default: AssignAction::Rebuild,
            services: HashMap::new(),
            rebuild_triggers: HashMap::new(),
            exclude_paths: vec![],
        };

        let result = classify_services(&["api".to_string()], &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::Rebuild));
    }
}
