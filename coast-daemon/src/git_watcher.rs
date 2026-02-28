/// Watches `.git/HEAD` for known projects and emits `ProjectGitChanged`
/// events when the current branch changes. Also watches the worktree
/// directory for structural changes (worktree added/removed).
///
/// When a worktree directory is deleted while an instance is still
/// assigned to it, the watcher automatically triggers an unassign
/// (returning the instance to the default branch).
///
/// Uses lightweight polling (every 2 seconds) rather than a full
/// file-watcher dependency, since there are typically only 1-3 projects.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{debug, info, warn};

use coast_core::protocol::CoastEvent;
use coast_core::types::{CoastInstance, InstanceStatus};

use crate::server::AppState;

/// Cached state for a single project's git info.
struct ProjectGitState {
    project_root: PathBuf,
    last_head: Option<String>,
    last_worktree_listing: Option<Vec<String>>,
}

/// Resolve the project root from `~/.coast/images/{project}/manifest.json`.
fn resolve_project_root(project: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("project_root")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
}

/// Read the worktree_dir from the project's cached Coastfile (default ".coasts").
fn read_worktree_dir(project: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return ".coasts".to_string();
    };
    let cf_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&cf_path) {
        return cf.worktree_dir;
    }
    ".coasts".to_string()
}

/// Read the contents of `.git/HEAD` for a project root.
async fn read_git_head(project_root: &Path) -> Option<String> {
    let head_path = project_root.join(".git").join("HEAD");
    tokio::fs::read_to_string(&head_path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
}

/// List worktree subdirectory names.
async fn list_worktree_dirs(project_root: &Path, wt_dir_name: &str) -> Option<Vec<String>> {
    let wt_path = project_root.join(wt_dir_name);
    let Ok(mut entries) = tokio::fs::read_dir(&wt_path).await else {
        return None;
    };
    let mut names = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(ft) = entry.file_type().await {
            if ft.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Some(names)
}

/// Find instances whose assigned worktree directory no longer exists on disk.
///
/// Returns `(instance_name, worktree_name)` pairs for running/idle instances
/// that reference a worktree not present in the current directory listing.
pub fn find_orphaned_worktrees(
    instances: &[CoastInstance],
    worktree_listing: &[String],
) -> Vec<(String, String)> {
    instances
        .iter()
        .filter(|inst| matches!(inst.status, InstanceStatus::Running | InstanceStatus::Idle))
        .filter_map(|inst| {
            let wt = inst.worktree_name.as_ref()?;
            if worktree_listing.contains(wt) {
                None
            } else {
                Some((inst.name.clone(), wt.clone()))
            }
        })
        .collect()
}

/// Send an unassign request and return the result.
async fn try_unassign(
    state: &AppState,
    project: &str,
    instance: &str,
) -> Result<coast_core::protocol::UnassignResponse, coast_core::error::CoastError> {
    let req = coast_core::protocol::UnassignRequest {
        name: instance.to_string(),
        project: project.to_string(),
    };
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    crate::handlers::unassign::handle(req, state, tx).await
}

/// Restart a DinD container and wait for its inner daemon to become ready.
/// Returns `true` on success, `false` if any step fails.
#[allow(clippy::cognitive_complexity)]
async fn restart_container_for_recovery(state: &AppState, project: &str, instance: &str) -> bool {
    use coast_docker::runtime::Runtime;

    let container_id = {
        let Ok(db) = state.db.try_lock() else {
            warn!(
                instance,
                project, "could not acquire DB lock for recovery restart"
            );
            return false;
        };
        match db.get_instance(project, instance) {
            Ok(Some(inst)) => inst.container_id.clone(),
            _ => None,
        }
    };

    let (Some(ref docker), Some(ref cid)) = (&state.docker, &container_id) else {
        warn!(
            instance,
            project, "no Docker client or container ID, cannot recover"
        );
        return false;
    };

    info!(instance, project, "restarting DinD container for recovery");
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    if let Err(e) = rt.stop_coast_container(cid).await {
        warn!(instance, error = %e, "stop failed during recovery (may already be stopped)");
    }
    if let Err(e) = rt.start_coast_container(cid).await {
        warn!(instance, error = %e, "start failed during recovery");
        return false;
    }
    let mgr = coast_docker::container::ContainerManager::new(
        coast_docker::dind::DindRuntime::with_client(docker.clone()),
    );
    if let Err(e) = mgr.wait_for_inner_daemon(cid).await {
        warn!(instance, error = %e, "inner daemon did not recover after restart");
        return false;
    }
    info!(
        instance,
        project, "DinD container restarted, inner daemon ready"
    );
    true
}

/// Attempt to auto-unassign an instance whose worktree was deleted.
///
/// First tries a direct unassign. If that fails (e.g. inner daemon unhealthy
/// because the bind-mounted directory was removed from the host), restarts the
/// DinD container to recover, then retries the unassign.
#[allow(clippy::cognitive_complexity)]
pub(crate) async fn auto_unassign_with_recovery(state: &AppState, project: &str, instance: &str) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    match try_unassign(state, project, instance).await {
        Ok(resp) => {
            info!(instance, project, branch = %resp.worktree, "auto-unassigned after worktree deletion");
            return;
        }
        Err(e) => {
            warn!(instance, project, error = %e, "auto-unassign attempt 1 failed, will restart container");
        }
    }

    if !restart_container_for_recovery(state, project, instance).await {
        return;
    }

    match try_unassign(state, project, instance).await {
        Ok(resp) => {
            info!(instance, project, branch = %resp.worktree, "auto-unassigned after recovery restart");
        }
        Err(e) => {
            warn!(instance, project, error = %e, "auto-unassign failed even after container restart");
        }
    }
}

/// One-time startup scan for worktrees that were deleted while the daemon
/// was not running. Spawns background auto-unassign tasks for any orphans
/// found, then returns immediately.
pub async fn reconcile_orphaned_worktrees(state: &Arc<AppState>) {
    let instances = {
        let Ok(db) = state.db.try_lock() else {
            warn!("startup reconcile: could not acquire DB lock");
            return;
        };
        db.list_instances().unwrap_or_default()
    };

    let mut by_project: HashMap<String, Vec<CoastInstance>> = HashMap::new();
    for inst in instances {
        by_project
            .entry(inst.project.clone())
            .or_default()
            .push(inst);
    }

    for (project, project_instances) in &by_project {
        let Some(project_root) = resolve_project_root(project) else {
            continue;
        };
        let wt_dir = read_worktree_dir(project);
        let listing = list_worktree_dirs(&project_root, &wt_dir)
            .await
            .unwrap_or_default();

        let orphans = find_orphaned_worktrees(project_instances, &listing);
        for (inst_name, wt_name) in orphans {
            info!(
                project,
                instance = %inst_name,
                worktree = %wt_name,
                "startup: orphaned worktree detected, auto-unassigning"
            );
            let s = Arc::clone(state);
            let p = project.clone();
            tokio::spawn(async move {
                auto_unassign_with_recovery(&s, &p, &inst_name).await;
            });
        }
    }
}

/// Spawn the background git watcher task.
///
/// Polls every 2 seconds, discovers projects from the state DB,
/// and emits `ProjectGitChanged` events when HEAD or worktree
/// directory contents change.
pub fn spawn_git_watcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut cache: HashMap<String, ProjectGitState> = HashMap::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            let projects = {
                let Ok(db) = state.db.try_lock() else {
                    continue;
                };
                let instances = db.list_instances().unwrap_or_default();
                let mut seen = std::collections::HashSet::new();
                instances
                    .into_iter()
                    .filter_map(|inst| {
                        if seen.insert(inst.project.clone()) {
                            Some(inst.project)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for project in &projects {
                let entry = cache.entry(project.clone()).or_insert_with(|| {
                    let project_root = resolve_project_root(project)
                        .unwrap_or_else(|| PathBuf::from("/nonexistent"));
                    ProjectGitState {
                        project_root,
                        last_head: None,
                        last_worktree_listing: None,
                    }
                });

                if !entry.project_root.exists() {
                    if let Some(root) = resolve_project_root(project) {
                        entry.project_root = root;
                    } else {
                        continue;
                    }
                }

                let mut changed = false;

                if let Some(head) = read_git_head(&entry.project_root).await {
                    if entry.last_head.as_ref() != Some(&head) {
                        if entry.last_head.is_some() {
                            debug!(project, old = ?entry.last_head, new = %head, "git HEAD changed");
                            changed = true;
                        }
                        entry.last_head = Some(head);
                    }
                }

                let wt_dir = read_worktree_dir(project);
                if let Some(listing) = list_worktree_dirs(&entry.project_root, &wt_dir).await {
                    if entry.last_worktree_listing.as_ref() != Some(&listing) {
                        if entry.last_worktree_listing.is_some() {
                            debug!(project, "worktree directory changed");
                            changed = true;

                            // Check for instances assigned to worktrees that no longer exist
                            let project_instances = {
                                let Ok(db) = state.db.try_lock() else {
                                    entry.last_worktree_listing = Some(listing.clone());
                                    continue;
                                };
                                db.list_instances_for_project(project).unwrap_or_default()
                            };
                            let orphans = find_orphaned_worktrees(&project_instances, &listing);
                            for (inst_name, wt_name) in orphans {
                                info!(
                                    project,
                                    instance = %inst_name,
                                    worktree = %wt_name,
                                    "worktree deleted, auto-unassigning instance"
                                );
                                let unassign_state = Arc::clone(&state);
                                let unassign_project = project.clone();
                                tokio::spawn(async move {
                                    auto_unassign_with_recovery(
                                        &unassign_state,
                                        &unassign_project,
                                        &inst_name,
                                    )
                                    .await;
                                });
                            }
                        }
                        entry.last_worktree_listing = Some(listing);
                    }
                }

                if changed {
                    state.emit_event(CoastEvent::ProjectGitChanged {
                        project: project.clone(),
                    });
                }
            }

            cache.retain(|k, _| projects.contains(k));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use coast_core::types::RuntimeType;

    fn make_instance(name: &str, status: InstanceStatus, worktree: Option<&str>) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: "test-project".to_string(),
            status,
            branch: worktree.map(String::from),
            commit_sha: None,
            container_id: Some(format!("container-{name}")),
            runtime: RuntimeType::Dind,
            created_at: Utc::now(),
            worktree_name: worktree.map(String::from),
            build_id: None,
            coastfile_type: None,
        }
    }

    #[test]
    fn test_find_orphaned_worktrees_detects_missing() {
        let instances = vec![make_instance(
            "dev-1",
            InstanceStatus::Running,
            Some("feature-x"),
        )];
        let listing = vec!["feature-y".to_string()];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0], ("dev-1".to_string(), "feature-x".to_string()));
    }

    #[test]
    fn test_find_orphaned_worktrees_no_orphans() {
        let instances = vec![make_instance(
            "dev-1",
            InstanceStatus::Running,
            Some("feature-x"),
        )];
        let listing = vec!["feature-x".to_string(), "feature-y".to_string()];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_find_orphaned_worktrees_ignores_none_worktree() {
        let instances = vec![make_instance("dev-1", InstanceStatus::Running, None)];
        let listing = vec!["feature-x".to_string()];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_find_orphaned_worktrees_ignores_stopped() {
        let instances = vec![make_instance(
            "dev-1",
            InstanceStatus::Stopped,
            Some("feature-x"),
        )];
        let listing = vec!["feature-y".to_string()];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert!(orphans.is_empty());
    }

    #[test]
    fn test_find_orphaned_worktrees_empty_listing() {
        let instances = vec![
            make_instance("dev-1", InstanceStatus::Running, Some("feature-a")),
            make_instance("dev-2", InstanceStatus::Idle, Some("feature-b")),
        ];
        let listing: Vec<String> = vec![];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert_eq!(orphans.len(), 2);
    }

    #[test]
    fn test_find_orphaned_worktrees_mixed_statuses() {
        let instances = vec![
            make_instance("dev-1", InstanceStatus::Running, Some("feature-x")),
            make_instance("dev-2", InstanceStatus::Stopped, Some("feature-x")),
            make_instance("dev-3", InstanceStatus::Idle, Some("feature-y")),
            make_instance("dev-4", InstanceStatus::Running, None),
        ];
        let listing = vec!["feature-y".to_string()];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].0, "dev-1");
    }
}
