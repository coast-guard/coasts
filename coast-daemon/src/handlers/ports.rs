/// Handler for the `coast ports` command.
///
/// Queries the state DB for port allocations for a given instance
/// and returns them. Also handles set/unset primary service.
///
/// The primary port is stored per-build in the settings table
/// (`primary_port:{project}:{build_id}`) rather than per-instance.
use std::collections::HashMap;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, PortsRequest, PortsResponse};
use coast_core::types::PortMapping;

use crate::server::AppState;
use crate::state::StateDb;

/// Convert a logical service name into a shell-safe dynamic-port env var key.
///
/// Examples:
/// - `web` -> `WEB_DYNAMIC_PORT`
/// - `backend-test` -> `BACKEND_TEST_DYNAMIC_PORT`
/// - `svc.v2` -> `SVC_V2_DYNAMIC_PORT`
pub(crate) fn service_to_dynamic_port_env_key(service_name: &str) -> String {
    let mut sanitized = String::with_capacity(service_name.len());
    for ch in service_name.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_uppercase());
        } else {
            sanitized.push('_');
        }
    }
    if sanitized.is_empty() {
        sanitized.push_str("SERVICE");
    }
    if sanitized
        .as_bytes()
        .first()
        .map(u8::is_ascii_digit)
        .unwrap_or(false)
    {
        sanitized.insert(0, '_');
    }
    format!("{sanitized}_DYNAMIC_PORT")
}

/// Build environment variables for dynamic port mappings.
///
/// Returns a map where keys are derived from `logical_name` and values are
/// stringified dynamic ports.
pub(crate) fn dynamic_port_env_vars_from_mappings(
    mappings: &[PortMapping],
) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    for mapping in mappings {
        env_vars.insert(
            service_to_dynamic_port_env_key(&mapping.logical_name),
            mapping.dynamic_port.to_string(),
        );
    }
    env_vars
}

/// Build the settings key for a build's primary port.
pub(crate) fn primary_port_settings_key(project: &str, build_id: &str) -> String {
    format!("primary_port:{project}:{build_id}")
}

/// Resolve the subdomain host for an instance if subdomain routing is enabled.
fn resolve_subdomain_host(db: &StateDb, project: &str, name: &str) -> Option<String> {
    let enabled = db
        .get_setting(&format!("subdomain_routing:{project}"))
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    if enabled {
        Some(format!("{name}.localhost"))
    } else {
        None
    }
}

/// Set `is_primary` on each port by comparing against the primary service name.
fn apply_primary_flag(ports: &mut [PortMapping], primary_service: Option<&str>) {
    for p in ports.iter_mut() {
        p.is_primary = primary_service == Some(p.logical_name.as_str());
    }
}

/// Look up the instance's build_id, then resolve the primary port from settings.
fn resolve_primary_for_instance(db: &StateDb, project: &str, name: &str) -> Result<Option<String>> {
    let instance = db.get_instance(project, name)?;
    let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
        name: name.to_string(),
        project: project.to_string(),
    })?;

    if let Some(ref build_id) = instance.build_id {
        let key = primary_port_settings_key(project, build_id);
        db.get_setting(&key)
    } else {
        Ok(None)
    }
}

/// Query port allocations, resolve primary flag, return enriched `PortMapping` list.
fn get_ports_with_primary(db: &StateDb, project: &str, name: &str) -> Result<Vec<PortMapping>> {
    let allocs = db.get_port_allocations(project, name)?;
    let mut ports: Vec<PortMapping> = allocs.iter().map(PortMapping::from).collect();
    let primary = resolve_primary_for_instance(db, project, name)?;
    apply_primary_flag(&mut ports, primary.as_deref());
    Ok(ports)
}

/// Handle a ports request.
#[allow(clippy::cognitive_complexity)]
pub async fn handle(req: PortsRequest, state: &AppState) -> Result<PortsResponse> {
    match req {
        PortsRequest::List { name, project } => {
            info!(name = %name, project = %project, "handling ports list request");
            let db = state.db.lock().await;
            let ports = get_ports_with_primary(&db, &project, &name)?;
            let subdomain_host = resolve_subdomain_host(&db, &project, &name);
            info!(name = %name, port_count = ports.len(), "returning port allocations");
            Ok(PortsResponse {
                name,
                ports,
                message: None,
                subdomain_host,
            })
        }
        PortsRequest::SetPrimary {
            name,
            project,
            service,
        } => {
            info!(name = %name, project = %project, service = %service, "handling set-primary request");
            let db = state.db.lock().await;

            let instance = db.get_instance(&project, &name)?;
            let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
                name: name.clone(),
                project: project.clone(),
            })?;

            let build_id = instance.build_id.ok_or_else(|| CoastError::Port {
                message: format!(
                    "instance '{name}' has no build_id. Re-run the instance to associate it with a build."
                ),
                source: None,
            })?;

            // Verify the service exists in this instance's ports
            let allocs = db.get_port_allocations(&project, &name)?;
            if !allocs.iter().any(|a| a.logical_name == service) {
                return Err(CoastError::Port {
                    message: format!(
                        "no port allocation '{service}' found for '{project}/{name}'. \
                         Available services: {}",
                        allocs
                            .iter()
                            .map(|a| a.logical_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    source: None,
                });
            }

            let key = primary_port_settings_key(&project, &build_id);
            db.set_setting(&key, &service)?;

            let mut ports: Vec<PortMapping> = allocs.iter().map(PortMapping::from).collect();
            apply_primary_flag(&mut ports, Some(&service));
            let subdomain_host = resolve_subdomain_host(&db, &project, &name);

            drop(db);
            state.emit_event(CoastEvent::PortPrimaryChanged {
                name: name.clone(),
                project: project.clone(),
                service: Some(service.clone()),
            });

            Ok(PortsResponse {
                name,
                ports,
                message: Some(format!("Primary service set to '{service}'")),
                subdomain_host,
            })
        }
        PortsRequest::UnsetPrimary { name, project } => {
            info!(name = %name, project = %project, "handling unset-primary request");
            let db = state.db.lock().await;

            let instance = db.get_instance(&project, &name)?;
            let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
                name: name.clone(),
                project: project.clone(),
            })?;

            if let Some(ref build_id) = instance.build_id {
                let key = primary_port_settings_key(&project, build_id);
                db.delete_setting(&key)?;
            }

            let allocs = db.get_port_allocations(&project, &name)?;
            let ports: Vec<PortMapping> = allocs.iter().map(PortMapping::from).collect();
            let subdomain_host = resolve_subdomain_host(&db, &project, &name);

            drop(db);
            state.emit_event(CoastEvent::PortPrimaryChanged {
                name: name.clone(),
                project: project.clone(),
                service: None,
            });

            Ok(PortsResponse {
                name,
                ports,
                message: Some("Primary service unset".to_string()),
                subdomain_host,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str, build_id: Option<&str>) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: build_id.map(|s| s.to_string()),
            coastfile_type: None,
        }
    }

    fn pm(name: &str, canonical: u16, dynamic: u16) -> PortMapping {
        PortMapping {
            logical_name: name.to_string(),
            canonical_port: canonical,
            dynamic_port: dynamic,
            is_primary: false,
        }
    }

    #[tokio::test]
    async fn test_ports_list_with_allocations() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("web", 3000, 52340))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("postgres", 5432, 52341))
                .unwrap();
        }

        let req = PortsRequest::List {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.name, "feat-a");
        assert_eq!(resp.ports.len(), 2);
        assert!(resp.message.is_none());
        assert!(resp.ports.iter().all(|p| !p.is_primary));
    }

    #[tokio::test]
    async fn test_ports_list_with_primary_from_settings() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("web", 3000, 52340))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("postgres", 5432, 52341))
                .unwrap();
            db.set_setting("primary_port:my-app:build-1", "web")
                .unwrap();
        }

        let req = PortsRequest::List {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        let web = resp.ports.iter().find(|p| p.logical_name == "web").unwrap();
        let pg = resp
            .ports
            .iter()
            .find(|p| p.logical_name == "postgres")
            .unwrap();
        assert!(web.is_primary);
        assert!(!pg.is_primary);
    }

    #[tokio::test]
    async fn test_set_primary_stores_in_settings() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("web", 3000, 52340))
                .unwrap();
        }

        let req = PortsRequest::SetPrimary {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
            service: "web".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.message.unwrap().contains("web"));
        assert!(resp.ports[0].is_primary);

        // Verify it's in settings
        let db = state.db.lock().await;
        let val = db
            .get_setting("primary_port:my-app:build-1")
            .unwrap()
            .unwrap();
        assert_eq!(val, "web");
    }

    #[tokio::test]
    async fn test_set_primary_shared_across_instances() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("inst-a", "proj", Some("build-1")))
                .unwrap();
            db.insert_instance(&make_instance("inst-b", "proj", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("proj", "inst-a", &pm("web", 3000, 52340))
                .unwrap();
            db.insert_port_allocation("proj", "inst-b", &pm("web", 3000, 52341))
                .unwrap();
        }

        // Set primary via inst-a
        let req = PortsRequest::SetPrimary {
            name: "inst-a".to_string(),
            project: "proj".to_string(),
            service: "web".to_string(),
        };
        handle(req, &state).await.unwrap();

        // inst-b should also see it as primary (same build_id)
        let req = PortsRequest::List {
            name: "inst-b".to_string(),
            project: "proj".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.ports[0].is_primary);
    }

    #[tokio::test]
    async fn test_unset_primary() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("web", 3000, 52340))
                .unwrap();
            db.set_setting("primary_port:my-app:build-1", "web")
                .unwrap();
        }

        let req = PortsRequest::UnsetPrimary {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.ports.iter().all(|p| !p.is_primary));

        let db = state.db.lock().await;
        assert!(db
            .get_setting("primary_port:my-app:build-1")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_ports_no_allocations() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("no-ports", "my-app", None))
                .unwrap();
        }

        let req = PortsRequest::List {
            name: "no-ports".to_string(),
            project: "my-app".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.ports.is_empty());
    }

    #[tokio::test]
    async fn test_ports_nonexistent_instance() {
        let state = test_state();
        let req = PortsRequest::List {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_set_primary_nonexistent_service() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", Some("build-1")))
                .unwrap();
            db.insert_port_allocation("my-app", "feat-a", &pm("web", 3000, 52340))
                .unwrap();
        }

        let req = PortsRequest::SetPrimary {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
            service: "ghost".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_service_to_dynamic_port_env_key() {
        assert_eq!(
            service_to_dynamic_port_env_key("web"),
            "WEB_DYNAMIC_PORT".to_string()
        );
        assert_eq!(
            service_to_dynamic_port_env_key("backend-test"),
            "BACKEND_TEST_DYNAMIC_PORT".to_string()
        );
        assert_eq!(
            service_to_dynamic_port_env_key("svc.v2"),
            "SVC_V2_DYNAMIC_PORT".to_string()
        );
    }

    #[test]
    fn test_service_to_dynamic_port_env_key_edge_cases() {
        assert_eq!(
            service_to_dynamic_port_env_key(""),
            "SERVICE_DYNAMIC_PORT".to_string()
        );
        assert_eq!(
            service_to_dynamic_port_env_key("9svc"),
            "_9SVC_DYNAMIC_PORT".to_string()
        );
    }

    #[test]
    fn test_dynamic_port_env_vars_from_mappings() {
        let mappings = vec![
            PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: false,
            },
            PortMapping {
                logical_name: "backend-test".to_string(),
                canonical_port: 8080,
                dynamic_port: 52341,
                is_primary: false,
            },
        ];

        let env = dynamic_port_env_vars_from_mappings(&mappings);
        assert_eq!(env.get("WEB_DYNAMIC_PORT"), Some(&"52340".to_string()));
        assert_eq!(
            env.get("BACKEND_TEST_DYNAMIC_PORT"),
            Some(&"52341".to_string())
        );
    }

    #[test]
    fn test_apply_primary_flag() {
        let mut ports = vec![pm("web", 3000, 52340), pm("db", 5432, 52341)];
        apply_primary_flag(&mut ports, Some("web"));
        assert!(ports[0].is_primary);
        assert!(!ports[1].is_primary);

        apply_primary_flag(&mut ports, None);
        assert!(!ports[0].is_primary);
        assert!(!ports[1].is_primary);
    }
}
