/// Handler for the `coast ls` command.
///
/// Lists all coast instances from the state DB, optionally filtered
/// by project name.
use tracing::info;

use coast_core::artifact::coast_home;
use coast_core::error::Result;
use coast_core::protocol::{InstanceSummary, KnownProject, LsRequest, LsResponse};
use coast_core::types::InstanceStatus;

use crate::server::AppState;

/// Handle an ls request.
///
/// Queries the state DB for all instances, optionally filtering by project.
/// Returns a list of `InstanceSummary` entries with name, project, status,
/// branch, runtime, checkout status, and project root.
pub async fn handle(req: LsRequest, state: &AppState) -> Result<LsResponse> {
    info!(project = ?req.project, "handling ls request");

    let db = state.db.lock().await;

    // Query instances
    let rows = match &req.project {
        Some(project) => db.list_instances_for_project(project)?,
        None => db.list_instances()?,
    };

    // Resolve project_root from manifest.json for each unique project
    let mut project_roots: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    let mut port_counts: std::collections::HashMap<String, std::collections::HashMap<String, u32>> =
        std::collections::HashMap::new();
    for row in &rows {
        if !project_roots.contains_key(&row.project) {
            let root = resolve_project_root(&row.project);
            project_roots.insert(row.project.clone(), root);
        }
        if !port_counts.contains_key(&row.project) {
            let counts = db.port_counts_for_project(&row.project).unwrap_or_default();
            port_counts.insert(row.project.clone(), counts);
        }
    }

    // Read cached service health data
    let health_cache = state.service_health_cache.lock().await;

    // Convert DB rows to InstanceSummary
    let instances: Vec<InstanceSummary> = rows
        .iter()
        .map(|row| {
            let checked_out = row.status == InstanceStatus::CheckedOut;
            let project_root = project_roots.get(&row.project).cloned().flatten();
            let port_count = port_counts
                .get(&row.project)
                .and_then(|m| m.get(&row.name))
                .copied()
                .unwrap_or(0);

            let pp = resolve_primary_port(
                &db,
                &row.project,
                &row.name,
                row.build_id.as_deref(),
                checked_out,
            );

            let health_key = format!("{}:{}", row.project, row.name);
            let down_service_count = health_cache.get(&health_key).copied().unwrap_or(0);

            InstanceSummary {
                name: row.name.clone(),
                project: row.project.clone(),
                status: row.status.clone(),
                branch: row.branch.clone(),
                runtime: row.runtime.clone(),
                checked_out,
                project_root,
                worktree: row.worktree_name.clone(),
                build_id: row.build_id.clone(),
                coastfile_type: row.coastfile_type.clone(),
                port_count,
                primary_port_service: pp.service,
                primary_port_canonical: pp.canonical,
                primary_port_dynamic: pp.dynamic,
                primary_port_url: pp.url,
                down_service_count,
                remote_name: row.remote_name.clone(),
            }
        })
        .collect();

    let archived_set = db.list_archived_projects().unwrap_or_default();
    drop(health_cache);
    drop(db);

    let known_projects = scan_known_projects(&archived_set);

    info!(
        count = instances.len(),
        known = known_projects.len(),
        "listing instances"
    );

    Ok(LsResponse {
        instances,
        known_projects,
    })
}

/// Scan ~/.coast/images/ for built projects.
fn scan_known_projects(archived_set: &std::collections::HashSet<String>) -> Vec<KnownProject> {
    let Ok(home) = coast_home() else {
        return Vec::new();
    };
    let images_dir = home.join("images");
    let Ok(entries) = std::fs::read_dir(&images_dir) else {
        return Vec::new();
    };
    let mut projects = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let project_root = resolve_project_root(&name);
        let archived = archived_set.contains(&name);
        projects.push(KnownProject {
            name,
            project_root,
            archived,
        });
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    projects
}

/// Resolved primary port info for an instance.
struct PrimaryPortInfo {
    service: Option<String>,
    canonical: Option<u16>,
    dynamic: Option<u16>,
    url: Option<String>,
}

/// Look up the primary port for an instance from the per-build settings.
fn resolve_primary_port(
    db: &crate::state::StateDb,
    project: &str,
    instance_name: &str,
    build_id: Option<&str>,
    checked_out: bool,
) -> PrimaryPortInfo {
    let empty = PrimaryPortInfo {
        service: None,
        canonical: None,
        dynamic: None,
        url: None,
    };
    let Some(bid) = build_id else {
        return empty;
    };
    let key = super::ports::primary_port_settings_key(project, bid);
    let Ok(Some(service)) = db.get_setting(&key) else {
        return empty;
    };
    let Ok(allocs) = db.get_port_allocations(project, instance_name) else {
        return PrimaryPortInfo {
            service: Some(service),
            canonical: None,
            dynamic: None,
            url: None,
        };
    };
    let Some(alloc) = allocs.iter().find(|a| a.logical_name == service) else {
        return PrimaryPortInfo {
            service: Some(service),
            canonical: None,
            dynamic: None,
            url: None,
        };
    };

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

    let url = if checked_out {
        let resolved = template.replace("<port>", &alloc.canonical_port.to_string());
        Some(resolved)
    } else {
        let resolved = template.replace("<port>", &alloc.dynamic_port.to_string());
        if subdomain_enabled {
            Some(resolved.replace("localhost:", &format!("{instance_name}.localhost:")))
        } else {
            Some(resolved)
        }
    };

    PrimaryPortInfo {
        service: Some(service),
        canonical: Some(alloc.canonical_port),
        dynamic: Some(alloc.dynamic_port),
        url,
    }
}

/// Resolve the project root directory from manifest.json.
fn resolve_project_root(project: &str) -> Option<String> {
    let home = coast_home().ok()?;
    let manifest_path = home
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
        status: InstanceStatus,
        runtime: RuntimeType,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("cid-{name}")),
            runtime,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    #[tokio::test]
    async fn test_ls_empty() {
        let state = test_state();
        let req = LsRequest { project: None };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        assert!(result.unwrap().instances.is_empty());
    }

    #[tokio::test]
    async fn test_ls_multiple_instances() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&CoastInstance {
                name: "feat-a".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::Running,
                branch: Some("feature/a".to_string()),
                commit_sha: None,
                container_id: Some("cid-1".to_string()),
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_name: None,
            })
            .unwrap();
            db.insert_instance(&CoastInstance {
                name: "feat-b".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::CheckedOut,
                branch: Some("feature/b".to_string()),
                commit_sha: None,
                container_id: Some("cid-2".to_string()),
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_name: None,
            })
            .unwrap();
            db.insert_instance(&CoastInstance {
                name: "main".to_string(),
                project: "other-app".to_string(),
                status: InstanceStatus::Stopped,
                branch: Some("main".to_string()),
                commit_sha: None,
                container_id: None,
                runtime: RuntimeType::Sysbox,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_name: None,
            })
            .unwrap();
        }

        // List all
        let req = LsRequest { project: None };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 3);

        // Find the checked-out one
        let checked_out = result
            .instances
            .iter()
            .find(|i| i.name == "feat-b")
            .unwrap();
        assert!(checked_out.checked_out);
        assert_eq!(checked_out.status, InstanceStatus::CheckedOut);
        assert_eq!(checked_out.branch, Some("feature/b".to_string()));
    }

    #[tokio::test]
    async fn test_ls_filtered_by_project() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                "my-app",
                InstanceStatus::Running,
                RuntimeType::Dind,
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "main",
                "other-app",
                InstanceStatus::Running,
                RuntimeType::Dind,
            ))
            .unwrap();
        }

        let req = LsRequest {
            project: Some("my-app".to_string()),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 1);
        assert_eq!(result.instances[0].name, "feat-a");
        assert_eq!(result.instances[0].project, "my-app");
    }

    #[tokio::test]
    async fn test_ls_filtered_by_nonexistent_project() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                "my-app",
                InstanceStatus::Running,
                RuntimeType::Dind,
            ))
            .unwrap();
        }

        let req = LsRequest {
            project: Some("no-such-project".to_string()),
        };
        let result = handle(req, &state).await.unwrap();
        assert!(result.instances.is_empty());
    }

    #[tokio::test]
    async fn test_ls_runtime_types() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "dind-inst",
                "app",
                InstanceStatus::Running,
                RuntimeType::Dind,
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "sysbox-inst",
                "app",
                InstanceStatus::Running,
                RuntimeType::Sysbox,
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "podman-inst",
                "app",
                InstanceStatus::Running,
                RuntimeType::Podman,
            ))
            .unwrap();
        }

        let req = LsRequest { project: None };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.instances.len(), 3);

        let dind = result
            .instances
            .iter()
            .find(|i| i.name == "dind-inst")
            .unwrap();
        assert_eq!(dind.runtime, RuntimeType::Dind);

        let sysbox = result
            .instances
            .iter()
            .find(|i| i.name == "sysbox-inst")
            .unwrap();
        assert_eq!(sysbox.runtime, RuntimeType::Sysbox);

        let podman = result
            .instances
            .iter()
            .find(|i| i.name == "podman-inst")
            .unwrap();
        assert_eq!(podman.runtime, RuntimeType::Podman);
    }
}
