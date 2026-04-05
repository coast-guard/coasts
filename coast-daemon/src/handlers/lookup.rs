/// Handler for the `coast lookup` command.
///
/// Finds coast instances assigned to a specific worktree (or the project root),
/// returning full port data for each match. Designed for agents and scripts
/// that need to discover which coast instances correspond to their working directory.
use tracing::info;

use coast_core::error::Result;
use coast_core::protocol::{LookupInstance, LookupRequest, LookupResponse};
use coast_core::types::{InstanceStatus, PortMapping};

use crate::server::AppState;

/// Handle a lookup request.
///
/// Queries all instances for the given project, filters to those whose
/// `worktree_name` matches the request (None = project root, Some = specific
/// worktree), and returns each with its full port allocations and primary URL.
pub async fn handle(req: LookupRequest, state: &AppState) -> Result<LookupResponse> {
    info!(
        project = %req.project,
        worktree = ?req.worktree,
        "handling lookup request"
    );

    let db = state.db.lock().await;

    let rows = db.list_instances_for_project(&req.project)?;

    let matching: Vec<_> = rows
        .into_iter()
        .filter(|row| row.worktree_name == req.worktree)
        .collect();

    let project_root = resolve_project_root(&req.project);

    let instances: Vec<LookupInstance> = matching
        .iter()
        .map(|row| {
            let checked_out = row.status == InstanceStatus::CheckedOut;

            let allocs = db
                .get_port_allocations(&req.project, &row.name)
                .unwrap_or_default();
            let ports: Vec<PortMapping> = allocs.iter().map(PortMapping::from).collect();

            let primary_url = resolve_primary_url(
                &db,
                &req.project,
                &row.name,
                row.build_id.as_deref(),
                checked_out,
            );

            LookupInstance {
                name: row.name.clone(),
                status: row.status.clone(),
                checked_out,
                branch: row.branch.clone(),
                primary_url,
                ports,
            }
        })
        .collect();

    info!(
        count = instances.len(),
        project = %req.project,
        worktree = ?req.worktree,
        "lookup complete"
    );

    Ok(LookupResponse {
        project: req.project,
        worktree: req.worktree,
        project_root,
        instances,
    })
}

/// Resolve the primary service URL for an instance.
///
/// Reuses the same settings-key logic as `ls.rs` to find the primary port
/// service name, then builds a URL from the port template and subdomain config.
fn resolve_primary_url(
    db: &crate::state::StateDb,
    project: &str,
    instance_name: &str,
    build_id: Option<&str>,
    checked_out: bool,
) -> Option<String> {
    let bid = build_id?;
    let key = super::ports::primary_port_settings_key(project, bid);
    let service = db.get_setting(&key).ok()??;

    let allocs = db.get_port_allocations(project, instance_name).ok()?;
    let alloc = allocs.iter().find(|a| a.logical_name == service)?;

    let subdomain_enabled = db
        .get_setting(&format!("subdomain_routing:{project}"))
        .ok()
        .flatten()
        .as_deref()
        == Some("true");

    let template = db
        .get_setting(&format!("port_url:{project}:{service}"))
        .ok()
        .flatten()
        .unwrap_or_else(|| "http://localhost:<port>".to_string());

    if checked_out {
        let resolved = template.replace("<port>", &alloc.canonical_port.to_string());
        Some(resolved)
    } else {
        let resolved = template.replace("<port>", &alloc.dynamic_port.to_string());
        if subdomain_enabled {
            Some(resolved.replace("localhost:", &format!("{instance_name}.localhost:")))
        } else {
            Some(resolved)
        }
    }
}

/// Resolve the project root directory from manifest.json.
fn resolve_project_root(project: &str) -> Option<String> {
    let home = dirs::home_dir()?;
    let manifest_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("project_root")
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(
        name: &str,
        project: &str,
        worktree: Option<&str>,
        branch: Option<&str>,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: InstanceStatus::Running,
            branch: branch.map(str::to_string),
            commit_sha: None,
            container_id: Some(format!("cid-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: worktree.map(str::to_string),
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    #[tokio::test]
    async fn test_lookup_empty_project() {
        let state = test_state();
        let req = LookupRequest {
            project: "no-such-project".to_string(),
            worktree: None,
        };
        let result = handle(req, &state).await.unwrap();
        assert!(result.instances.is_empty());
        assert_eq!(result.project, "no-such-project");
        assert!(result.worktree.is_none());
    }

    #[tokio::test]
    async fn test_lookup_filters_by_worktree_none() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dev-1", "app", None, Some("main")))
                .unwrap();
            db.insert_instance(&make_instance(
                "dev-2",
                "app",
                Some("feature-alpha"),
                Some("feature-alpha"),
            ))
            .unwrap();
            db.insert_instance(&make_instance("dev-3", "app", None, Some("main")))
                .unwrap();
        }

        let req = LookupRequest {
            project: "app".to_string(),
            worktree: None,
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 2);
        let names: Vec<&str> = result.instances.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"dev-1"));
        assert!(names.contains(&"dev-3"));
    }

    #[tokio::test]
    async fn test_lookup_filters_by_worktree_some() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dev-1", "app", None, Some("main")))
                .unwrap();
            db.insert_instance(&make_instance(
                "dev-2",
                "app",
                Some("feature-alpha"),
                Some("feature-alpha"),
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "dev-3",
                "app",
                Some("feature-alpha"),
                Some("feature-alpha"),
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "dev-4",
                "app",
                Some("feature-beta"),
                Some("feature-beta"),
            ))
            .unwrap();
        }

        let req = LookupRequest {
            project: "app".to_string(),
            worktree: Some("feature-alpha".to_string()),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 2);
        let names: Vec<&str> = result.instances.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"dev-2"));
        assert!(names.contains(&"dev-3"));
    }

    #[tokio::test]
    async fn test_lookup_no_match_different_worktree() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "dev-1",
                "app",
                Some("feature-alpha"),
                Some("feature-alpha"),
            ))
            .unwrap();
        }

        let req = LookupRequest {
            project: "app".to_string(),
            worktree: Some("feature-beta".to_string()),
        };
        let result = handle(req, &state).await.unwrap();
        assert!(result.instances.is_empty());
    }

    #[tokio::test]
    async fn test_lookup_includes_ports() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dev-1", "app", None, Some("main")))
                .unwrap();
            db.insert_port_allocation(
                "app",
                "dev-1",
                &PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 52340,
                    is_primary: false,
                },
            )
            .unwrap();
            db.insert_port_allocation(
                "app",
                "dev-1",
                &PortMapping {
                    logical_name: "db".to_string(),
                    canonical_port: 5432,
                    dynamic_port: 55681,
                    is_primary: false,
                },
            )
            .unwrap();
        }

        let req = LookupRequest {
            project: "app".to_string(),
            worktree: None,
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.instances[0].ports.len(), 2);
        let port_names: Vec<&str> = result.instances[0]
            .ports
            .iter()
            .map(|p| p.logical_name.as_str())
            .collect();
        assert!(port_names.contains(&"db"));
        assert!(port_names.contains(&"web"));
    }

    #[tokio::test]
    async fn test_lookup_checked_out_status() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            let mut inst = make_instance("dev-1", "app", None, Some("main"));
            inst.status = InstanceStatus::CheckedOut;
            db.insert_instance(&inst).unwrap();
        }

        let req = LookupRequest {
            project: "app".to_string(),
            worktree: None,
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 1);
        assert!(result.instances[0].checked_out);
    }

    #[tokio::test]
    async fn test_lookup_response_fields() {
        let state = test_state();
        let req = LookupRequest {
            project: "my-proj".to_string(),
            worktree: Some("feat".to_string()),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.project, "my-proj");
        assert_eq!(result.worktree, Some("feat".to_string()));
    }

    #[tokio::test]
    async fn test_lookup_mixed_worktrees() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dev-1", "app", None, Some("main")))
                .unwrap();
            db.insert_instance(&make_instance(
                "dev-2",
                "app",
                Some("feat-a"),
                Some("feat-a"),
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "dev-3",
                "app",
                Some("feat-b"),
                Some("feat-b"),
            ))
            .unwrap();
            db.insert_instance(&make_instance("dev-4", "other", None, Some("main")))
                .unwrap();
        }

        // Lookup project root
        let result = handle(
            LookupRequest {
                project: "app".to_string(),
                worktree: None,
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.instances[0].name, "dev-1");

        // Lookup feat-a
        let result = handle(
            LookupRequest {
                project: "app".to_string(),
                worktree: Some("feat-a".to_string()),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.instances[0].name, "dev-2");

        // Lookup feat-b
        let result = handle(
            LookupRequest {
                project: "app".to_string(),
                worktree: Some("feat-b".to_string()),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.instances[0].name, "dev-3");
    }
}
