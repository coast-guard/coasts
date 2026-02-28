use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::Query;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use coast_core::protocol::ProjectGitResponse;

use crate::server::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/project/git", get(project_git))
}

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ProjectGitParams {
    pub project: String,
}

async fn project_git(
    Query(params): Query<ProjectGitParams>,
) -> Result<Json<ProjectGitResponse>, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let Some(project_root) = resolve_project_root(&params.project) else {
        return Ok(Json(ProjectGitResponse {
            is_git_repo: false,
            current_branch: None,
            local_branches: Vec::new(),
            worktrees: Vec::new(),
        }));
    };

    if !is_git_repo(&project_root).await {
        return Ok(Json(ProjectGitResponse {
            is_git_repo: false,
            current_branch: None,
            local_branches: Vec::new(),
            worktrees: Vec::new(),
        }));
    }

    let worktrees = list_worktree_dirs(&params.project, &project_root).await;

    Ok(Json(ProjectGitResponse {
        is_git_repo: true,
        current_branch: resolve_current_branch(&project_root).await,
        local_branches: list_local_branches(&project_root).await,
        worktrees,
    }))
}

/// List existing worktree branch names using `git worktree list --porcelain`.
/// Excludes the main worktree (the project root itself).
async fn list_worktree_dirs(_project: &str, project_root: &std::path::Path) -> Vec<String> {
    let output = tokio::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()
        .await;

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                if wt_canonical != canonical_root {
                    let name = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                    worktrees.push(name.to_string());
                }
            }
        } else if line == "detached" {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                if wt_canonical != canonical_root {
                    if let Some(name) = wt_path.file_name().and_then(|n| n.to_str()) {
                        worktrees.push(name.to_string());
                    }
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    worktrees.sort();
    worktrees
}

pub(crate) fn resolve_project_root(project: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    let root = manifest.get("project_root")?.as_str()?;
    Some(PathBuf::from(root))
}

async fn is_git_repo(project_root: &PathBuf) -> bool {
    tokio::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project_root)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn resolve_current_branch(project_root: &PathBuf) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_root)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

async fn list_local_branches(project_root: &PathBuf) -> Vec<String> {
    let output = tokio::process::Command::new("git")
        .args(["for-each-ref", "refs/heads", "--format=%(refname:short)"])
        .current_dir(project_root)
        .output()
        .await;
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
