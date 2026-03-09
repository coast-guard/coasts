use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::AssignRequest;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

use super::classify::classify_services;
use super::gitignored_sync::SYNC_EXCLUDE_DIRS;
use super::util::{check_has_bare_install, load_coastfile_data, read_project_root};
use super::worktree::detect_worktree_dir_from_git;

pub(super) async fn count_tracked_files(root: &std::path::Path, exclude_paths: &[String]) -> usize {
    let output = tokio::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()
        .await
        .ok();
    output
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !exclude_paths.iter().any(|p| l.starts_with(p)))
                .count()
        })
        .unwrap_or(0)
}

pub(super) async fn count_gitignored_files(
    root: &std::path::Path,
    exclude_paths: &[String],
) -> usize {
    let output = tokio::process::Command::new("git")
        .args(["ls-files", "--others", "--ignored", "--exclude-standard"])
        .current_dir(root)
        .output()
        .await
        .ok();
    let exclude_patterns: Vec<&str> = exclude_paths
        .iter()
        .map(std::string::String::as_str)
        .chain(SYNC_EXCLUDE_DIRS.iter().copied())
        .collect();
    output
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !exclude_patterns.iter().any(|p| l.starts_with(p)))
                .count()
        })
        .unwrap_or(0)
}

/// Handle an explain-only assign request. Performs analysis without executing.
pub async fn handle_explain(
    req: AssignRequest,
    state: &AppState,
) -> Result<coast_core::protocol::AssignExplainResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        worktree = %req.worktree,
        "handling assign --explain request"
    );

    let db = state.db.lock().await;
    let instance =
        db.get_instance(&req.project, &req.name)?
            .ok_or_else(|| CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            })?;
    let previous_branch = instance.branch.clone();
    let build_id = instance.build_id.clone();
    let container_id = instance.container_id.clone().unwrap_or_default();
    drop(db);

    let cf_data = load_coastfile_data(&req.project);
    let assign_config = cf_data.assign;
    let project_root = read_project_root(&req.project);

    let all_service_names: Vec<String> = if cf_data.has_compose {
        if let Some(ref docker) = state.docker {
            let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            let svc_ctx =
                crate::handlers::compose_context_for_build(&req.project, build_id.as_deref());
            let svc_cmd = svc_ctx.compose_shell("config --services");
            let svc_refs: Vec<&str> = svc_cmd.iter().map(std::string::String::as_str).collect();
            tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                rt.exec_in_coast(&container_id, &svc_refs),
            )
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .filter(coast_docker::runtime::ExecResult::success)
            .map(|r| {
                r.stdout
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let changed_files: Vec<String> = if !assign_config.rebuild_triggers.is_empty() {
        if let (Some(ref root), Some(ref prev)) = (&project_root, &previous_branch) {
            tokio::process::Command::new("git")
                .args(["diff", "--name-only", &format!("{prev}..{}", req.worktree)])
                .current_dir(root)
                .output()
                .await
                .ok()
                .filter(|o| o.status.success())
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let service_actions = classify_services(&all_service_names, &assign_config, &changed_files);

    let (worktree_exists, worktree_synced) = if let Some(ref root) = project_root {
        let wt_dir =
            detect_worktree_dir_from_git(root).unwrap_or_else(|| cf_data.worktree_dir.clone());
        let wt_path = root.join(&wt_dir).join(&req.worktree);
        let exists = wt_path.exists();
        (exists, exists && wt_path.join(".coast-synced").exists())
    } else {
        (false, false)
    };

    let tracked_file_count = match project_root {
        Some(ref root) => count_tracked_files(root, &assign_config.exclude_paths).await,
        None => 0,
    };
    let gitignored_file_count = match project_root {
        Some(ref root) => count_gitignored_files(root, &assign_config.exclude_paths).await,
        None => 0,
    };
    let has_bare_install = check_has_bare_install(&req.project, build_id.as_deref());

    let mut services: Vec<coast_core::protocol::AssignExplainService> = service_actions
        .iter()
        .map(
            |(name, action)| coast_core::protocol::AssignExplainService {
                name: name.clone(),
                action: format!("{action:?}").to_lowercase(),
            },
        )
        .collect();
    services.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(coast_core::protocol::AssignExplainResponse {
        name: req.name,
        worktree: req.worktree,
        current_branch: previous_branch,
        services,
        exclude_paths: assign_config.exclude_paths,
        tracked_file_count,
        gitignored_file_count,
        worktree_exists,
        worktree_synced,
        has_bare_install,
        changed_files_count: changed_files.len(),
    })
}
