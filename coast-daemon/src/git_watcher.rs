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

/// Read `worktree_dirs` from the live Coastfile on disk, falling back to the
/// cached build artifact at `~/.coast/images/{project}/latest/coastfile.toml`.
fn read_worktree_dirs(project: &str) -> Vec<String> {
    use coast_core::coastfile::Coastfile;

    if let Some(root) = resolve_project_root(project) {
        let live_path =
            Coastfile::find_coastfile(&root, "Coastfile").unwrap_or_else(|| root.join("Coastfile"));
        if let Ok(cf) = Coastfile::from_file(&live_path) {
            return cf.worktree_dirs;
        }
    }

    let Some(home) = dirs::home_dir() else {
        return vec![".worktrees".to_string()];
    };
    let cf_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if let Ok(cf) = Coastfile::from_file(&cf_path) {
        return cf.worktree_dirs;
    }
    vec![".worktrees".to_string()]
}

/// Read the contents of `.git/HEAD` for a project root.
async fn read_git_head(project_root: &Path) -> Option<String> {
    let head_path = project_root.join(".git").join("HEAD");
    tokio::fs::read_to_string(&head_path)
        .await
        .ok()
        .map(|s| s.trim().to_string())
}

/// Read the repository's complete worktree inventory. An unavailable inventory
/// must never be interpreted as proof that an assigned worktree was deleted.
async fn list_worktree_dirs(project_root: &Path, wt_dir_names: &[String]) -> Option<Vec<String>> {
    use coast_core::coastfile::Coastfile;

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::process::Command::new("git")
            .args(["worktree", "list", "--porcelain", "-z"])
            .current_dir(project_root)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let porcelain = std::str::from_utf8(&output.stdout).ok()?;
    if !porcelain.ends_with("\0\0") {
        return None;
    }
    let project_root = tokio::fs::canonicalize(project_root).await.ok()?;
    let mut roots: Vec<PathBuf> = wt_dir_names
        .iter()
        .filter(|dir| !Coastfile::is_external_worktree_dir(dir))
        .map(|dir| project_root.join(dir))
        .collect();
    roots.extend(
        Coastfile::resolve_external_worktree_dirs_expanded(wt_dir_names, &project_root)
            .into_iter()
            .map(|dir| dir.resolved_path),
    );
    for root in &mut roots {
        match tokio::fs::canonicalize(&root).await {
            Ok(path) => *root = path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }

    let mut names = Vec::new();
    for record in porcelain.split("\0\0").filter(|record| !record.is_empty()) {
        let mut fields = record.split('\0');
        let path = Path::new(fields.next()?.strip_prefix("worktree ")?);
        let path = match tokio::fs::canonicalize(path).await {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if path == project_root {
            continue;
        }
        for root in &roots {
            if let Ok(relative) = path.strip_prefix(root) {
                if !relative.as_os_str().is_empty() {
                    names.push(relative.to_str()?.to_string());
                }
            }
        }
        // Assign can also discover an internal parent absent from the Coastfile.
        // Keep that alias without narrowing the inventory to that one parent.
        if let Ok(relative) = path.strip_prefix(&project_root) {
            let mut components = relative.components();
            components.next();
            let name = components.as_path();
            if !name.as_os_str().is_empty() {
                names.push(name.to_str()?.to_string());
            }
        }
        for field in fields {
            if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                names.push(branch.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
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
        .filter(|inst| inst.remote_host.is_none())
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

/// Try to get the container_id for an instance using a non-blocking DB lock.
fn try_get_container_id(state: &AppState, project: &str, instance: &str) -> Option<String> {
    let db = state.db.try_lock().ok()?;
    match db.get_instance(project, instance) {
        Ok(Some(inst)) => inst.container_id.clone(),
        _ => None,
    }
}

/// Stop, start, and wait for the inner daemon to recover.
async fn restart_and_wait_for_daemon(docker: &bollard::Docker, cid: &str, instance: &str) -> bool {
    use coast_docker::runtime::Runtime;

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
    true
}

/// Restart a DinD container and wait for its inner daemon to become ready.
/// Returns `true` on success, `false` if any step fails.
async fn restart_container_for_recovery(state: &AppState, project: &str, instance: &str) -> bool {
    let Some(container_id) = try_get_container_id(state, project, instance) else {
        warn!(
            instance,
            project, "no container ID or DB lock failed, cannot recover"
        );
        return false;
    };
    let Some(docker) = state.docker.as_ref() else {
        warn!(instance, project, "no Docker client, cannot recover");
        return false;
    };

    info!(instance, project, "restarting DinD container for recovery");
    if restart_and_wait_for_daemon(&docker, &container_id, instance).await {
        info!(
            instance,
            project, "DinD container restarted, inner daemon ready"
        );
        true
    } else {
        false
    }
}

/// Attempt to auto-unassign an instance whose worktree was deleted.
///
/// First tries a direct unassign. If that fails (e.g. inner daemon unhealthy
/// because the bind-mounted directory was removed from the host), restarts the
/// DinD container to recover, then retries the unassign.
/// Try to unassign, logging the result. Returns `true` on success.
async fn try_unassign_with_logging(
    state: &AppState,
    project: &str,
    instance: &str,
    context: &str,
) -> bool {
    match try_unassign(state, project, instance).await {
        Ok(resp) => {
            info!(instance, project, branch = %resp.worktree, "{context}");
            true
        }
        Err(e) => {
            warn!(instance, project, error = %e, "{context} failed");
            false
        }
    }
}

async fn auto_unassign_with_recovery(
    state: &AppState,
    project: &str,
    instance: &str,
    expected_worktree: &str,
) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let semaphore = state.project_semaphore(project).await;
    let Ok(_permit) = semaphore.acquire().await else {
        return;
    };
    let Some(root) = resolve_project_root(project) else {
        return;
    };
    let dirs = read_worktree_dirs(project);
    if !assignment_is_still_orphaned(state, project, instance, expected_worktree, &root, &dirs)
        .await
    {
        return;
    }

    if try_unassign_with_logging(
        state,
        project,
        instance,
        "auto-unassigned after worktree deletion",
    )
    .await
    {
        return;
    }

    if !assignment_is_still_orphaned(state, project, instance, expected_worktree, &root, &dirs)
        .await
    {
        return;
    }

    if !restart_container_for_recovery(state, project, instance).await {
        return;
    }

    if !assignment_is_still_orphaned(state, project, instance, expected_worktree, &root, &dirs)
        .await
    {
        return;
    }

    try_unassign_with_logging(
        state,
        project,
        instance,
        "auto-unassigned after recovery restart",
    )
    .await;
}

/// Called under the project operation permit so a queued deletion cannot undo
/// a newer assignment. Re-read Git after the delay, including before recovery.
async fn assignment_is_still_orphaned(
    state: &AppState,
    project: &str,
    instance: &str,
    expected_worktree: &str,
    root: &Path,
    dirs: &[String],
) -> bool {
    let current = {
        let db = state.db.lock().await;
        let Ok(Some(current)) = db.get_instance(project, instance) else {
            return false;
        };
        current
    };
    if current.remote_host.is_some()
        || current.worktree_name.as_deref() != Some(expected_worktree)
        || !matches!(
            current.status,
            InstanceStatus::Running | InstanceStatus::Idle | InstanceStatus::Unassigning
        )
    {
        return false;
    }
    let Some(listing) = list_worktree_dirs(root, dirs).await else {
        return false;
    };
    !listing.iter().any(|name| name == expected_worktree)
}

/// One-time startup scan for worktrees that were deleted while the daemon
/// was not running. Spawns background auto-unassign tasks for any orphans
/// found, then returns immediately.
pub async fn reconcile_orphaned_worktrees(state: &Arc<AppState>) {
    let instances = {
        let db = state.db.lock().await;
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
        let wt_dirs = read_worktree_dirs(project);
        let Some(listing) = list_worktree_dirs(&project_root, &wt_dirs).await else {
            warn!(
                project,
                "worktree inventory unavailable, preserving assignments"
            );
            continue;
        };

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
                auto_unassign_with_recovery(&s, &p, &inst_name, &wt_name).await;
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

                let wt_dirs = read_worktree_dirs(project);
                if let Some(listing) = list_worktree_dirs(&entry.project_root, &wt_dirs).await {
                    if entry.last_worktree_listing.as_ref() != Some(&listing) {
                        if entry.last_worktree_listing.is_some() {
                            debug!(project, "worktree directory changed");
                            changed = true;

                            // Check for instances assigned to worktrees that no longer exist
                            let project_instances = {
                                let db = state.db.lock().await;
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
                                        &wt_name,
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
            remote_host: None,
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

    #[test]
    fn test_find_orphaned_worktrees_ignores_remote() {
        let mut remote_inst = make_instance("dev-1", InstanceStatus::Running, Some("feature-x"));
        remote_inst.remote_host = Some("10.0.0.1".to_string());

        let local_inst = make_instance("dev-2", InstanceStatus::Running, Some("feature-x"));

        let instances = vec![remote_inst, local_inst];
        let listing: Vec<String> = vec![];
        let orphans = find_orphaned_worktrees(&instances, &listing);
        assert_eq!(orphans.len(), 1, "only local instance should be orphaned");
        assert_eq!(orphans[0].0, "dev-2");
    }

    #[tokio::test]
    async fn test_listing_preserves_nested_external_assignments_when_internal_worktree_is_added() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        init_fixture_repo(&root);
        let external = fixture.path().join("external");
        let first = external.join("session-a/project");
        let second = external.join("session-b/project");
        add_fixture_worktree(&root, &first, "first");
        add_fixture_worktree(&root, &second, "second");
        let dirs = vec![".worktrees".into(), external.to_str().unwrap().into()];
        let instances = vec![
            make_instance("one", InstanceStatus::Running, Some("session-a/project")),
            make_instance("two", InstanceStatus::Running, Some("session-b/project")),
        ];
        let before = list_worktree_dirs(&root, &dirs).await.unwrap();
        assert!(find_orphaned_worktrees(&instances, &before).is_empty());

        add_fixture_worktree(&root, &root.join(".worktrees/third"), "third");
        let after = list_worktree_dirs(&root, &dirs).await.unwrap();
        assert!(find_orphaned_worktrees(&instances, &after).is_empty());
        assert!(after.contains(&"third".into()));
        assert!(first.join(".git").is_file());
        assert!(second.join(".git").is_file());
    }

    #[tokio::test]
    async fn test_listing_recognizes_branch_assignment_and_detached_worktree() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        init_fixture_repo(&root);
        let attached = root.join(".worktrees/task dir");
        add_fixture_worktree(&root, &attached, "feature/topic");
        let detached = root.join(".worktrees/session/project");
        git_fixture(
            &root,
            &["worktree", "add", "--detach", detached.to_str().unwrap()],
        );
        let listing = list_worktree_dirs(&root, &[".worktrees".into()])
            .await
            .unwrap();
        for name in ["task dir", "feature/topic", "session/project"] {
            assert!(
                listing.contains(&name.to_string()),
                "missing {name}: {listing:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_listing_failure_is_not_an_empty_inventory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".worktrees")).unwrap();
        assert!(list_worktree_dirs(dir.path(), &[".worktrees".into()])
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_listing_detects_deletion_of_last_external_worktree() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        init_fixture_repo(&root);
        let external = fixture.path().join("external");
        let worktree = external.join("task");
        add_fixture_worktree(&root, &worktree, "feature/task");
        let dirs = vec![external.to_str().unwrap().into()];
        assert!(!list_worktree_dirs(&root, &dirs).await.unwrap().is_empty());
        std::fs::remove_dir_all(&worktree).unwrap();
        assert_eq!(list_worktree_dirs(&root, &dirs).await, Some(vec![]));
    }

    // --- try_get_container_id tests ---

    #[tokio::test]
    async fn test_try_get_container_id_found() {
        use crate::state::StateDb;
        use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "inst".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("cid-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);
        assert_eq!(
            try_get_container_id(&state, "proj", "inst"),
            Some("cid-123".to_string())
        );
    }

    #[tokio::test]
    async fn test_try_get_container_id_not_found() {
        use crate::state::StateDb;
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        assert_eq!(try_get_container_id(&state, "proj", "ghost"), None);
    }

    #[tokio::test]
    async fn test_listing_ignores_foreign_repositories_and_unregistered_directories() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let other = fixture.path().join("other");
        let external = fixture.path().join("external");
        init_fixture_repo(&root);
        init_fixture_repo(&other);
        add_fixture_worktree(&root, &external.join("mine"), "mine");
        add_fixture_worktree(&other, &external.join("theirs"), "theirs");
        std::fs::create_dir_all(root.join(".worktrees/not-a-worktree")).unwrap();
        let listing = list_worktree_dirs(
            &root,
            &[".worktrees".into(), external.to_str().unwrap().into()],
        )
        .await
        .unwrap();
        assert_eq!(listing, vec!["mine"]);
    }

    #[tokio::test]
    async fn test_listing_distinguishes_deleted_worktrees_with_identical_basenames() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let external = fixture.path().join("external");
        init_fixture_repo(&root);
        let first = external.join("first/project");
        add_fixture_worktree(&root, &first, "first-branch");
        add_fixture_worktree(&root, &external.join("second/project"), "second-branch");
        std::fs::remove_dir_all(first).unwrap();
        let listing = list_worktree_dirs(&root, &[external.to_str().unwrap().into()])
            .await
            .unwrap();
        let instances = vec![
            make_instance("first", InstanceStatus::Running, Some("first/project")),
            make_instance("second", InstanceStatus::Running, Some("second/project")),
        ];
        assert_eq!(
            find_orphaned_worktrees(&instances, &listing),
            vec![("first".into(), "first/project".into())]
        );
    }

    #[tokio::test]
    async fn test_listing_handles_symlinked_roots_and_newlines_in_paths() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        let external = fixture.path().join("external");
        init_fixture_repo(&root);
        add_fixture_worktree(&root, &external.join("session\nname/project"), "topic");
        let alias = fixture.path().join("alias");
        std::os::unix::fs::symlink(&external, &alias).unwrap();
        let listing = list_worktree_dirs(&root, &[alias.to_str().unwrap().into()])
            .await
            .unwrap();
        assert_eq!(listing, vec!["session\nname/project", "topic"]);
    }

    #[tokio::test]
    async fn test_listing_preserves_git_detected_internal_parent() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        init_fixture_repo(&root);
        add_fixture_worktree(&root, &root.join("custom/nested/task"), "topic");
        let listing = list_worktree_dirs(&root, &[".worktrees".into()])
            .await
            .unwrap();
        assert_eq!(listing, vec!["nested/task", "topic"]);
    }

    #[tokio::test]
    async fn test_orphan_recheck_preserves_changed_assignment_and_restored_worktree() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("repo");
        init_fixture_repo(&root);
        let db = crate::state::StateDb::open_in_memory().unwrap();
        db.insert_instance(&make_instance(
            "preview",
            InstanceStatus::Running,
            Some("new"),
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);
        let dirs = vec![".worktrees".into()];
        assert!(
            !assignment_is_still_orphaned(&state, "test-project", "preview", "old", &root, &dirs)
                .await
        );
        assert!(
            assignment_is_still_orphaned(&state, "test-project", "preview", "new", &root, &dirs)
                .await
        );
        add_fixture_worktree(&root, &root.join(".worktrees/new"), "new");
        assert!(
            !assignment_is_still_orphaned(&state, "test-project", "preview", "new", &root, &dirs)
                .await
        );
    }

    #[tokio::test]
    async fn test_orphan_recheck_preserves_assignment_when_git_fails() {
        let root = tempfile::tempdir().unwrap();
        let db = crate::state::StateDb::open_in_memory().unwrap();
        db.insert_instance(&make_instance(
            "preview",
            InstanceStatus::Running,
            Some("task"),
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);
        assert!(
            !assignment_is_still_orphaned(
                &state,
                "test-project",
                "preview",
                "task",
                root.path(),
                &[".worktrees".into()]
            )
            .await
        );
    }

    fn init_fixture_repo(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        git_fixture(root, &["init", "-q"]);
        git_fixture(
            root,
            &[
                "-c",
                "user.name=Coast test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
    }

    fn add_fixture_worktree(root: &Path, path: &Path, branch: &str) {
        git_fixture(
            root,
            &["worktree", "add", "-b", branch, path.to_str().unwrap()],
        );
    }

    fn git_fixture(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
