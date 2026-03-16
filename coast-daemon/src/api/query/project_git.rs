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
///
/// Includes worktrees under the project root and any configured external
/// worktree directories. Excludes the main worktree (the project root itself)
/// and worktrees outside all known directories.
async fn list_worktree_dirs(project: &str, project_root: &std::path::Path) -> Vec<String> {
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

    let external_dirs = load_external_worktree_dirs(project, project_root);

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_porcelain_worktrees(&stdout, project_root, &external_dirs)
}

/// Load resolved external worktree directory paths.
///
/// Reads the **live** Coastfile from the project root first (picks up edits
/// immediately). Falls back to the cached build artifact if the live file
/// is missing or unparseable.
fn load_external_worktree_dirs(project: &str, project_root: &std::path::Path) -> Vec<PathBuf> {
    use coast_core::coastfile::Coastfile;

    let worktree_dirs = load_worktree_dirs_from_live_or_cached(project, project_root);
    worktree_dirs
        .iter()
        .filter(|d| Coastfile::is_external_worktree_dir(d))
        .map(|d| Coastfile::resolve_worktree_dir(project_root, d))
        .collect()
}

/// Read `worktree_dirs` from the live Coastfile on disk, falling back to the
/// cached build artifact at `~/.coast/images/{project}/latest/coastfile.toml`.
fn load_worktree_dirs_from_live_or_cached(
    project: &str,
    project_root: &std::path::Path,
) -> Vec<String> {
    use coast_core::coastfile::Coastfile;

    let live_path = project_root.join("Coastfile");
    if let Ok(cf) = Coastfile::from_file(&live_path) {
        return cf.worktree_dirs;
    }

    let home = dirs::home_dir().unwrap_or_default();
    let cached_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if let Ok(cf) = Coastfile::from_file(&cached_path) {
        return cf.worktree_dirs;
    }

    vec![".worktrees".to_string()]
}

/// Parse `git worktree list --porcelain` output, accepting worktrees under
/// the project root or any of the provided external directories.
fn parse_porcelain_worktrees(
    porcelain: &str,
    project_root: &std::path::Path,
    external_dirs: &[PathBuf],
) -> Vec<String> {
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_externals: Vec<PathBuf> = external_dirs
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();

    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;

    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path));
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                if is_known_worktree(&wt_canonical, &canonical_root, &canonical_externals) {
                    let name = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                    worktrees.push(name.to_string());
                }
            }
        } else if line == "detached" {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                if is_known_worktree(&wt_canonical, &canonical_root, &canonical_externals) {
                    let name = detached_worktree_name(
                        &wt_canonical,
                        &canonical_root,
                        &canonical_externals,
                    );
                    if !name.is_empty() {
                        worktrees.push(name);
                    }
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    worktrees.sort();
    worktrees.dedup();
    worktrees
}

/// Check if a worktree path is under the project root or any external dir,
/// but is not the project root itself.
fn is_known_worktree(
    wt_canonical: &std::path::Path,
    canonical_root: &std::path::Path,
    canonical_externals: &[PathBuf],
) -> bool {
    if wt_canonical == canonical_root {
        return false;
    }
    if wt_canonical.starts_with(canonical_root) {
        return true;
    }
    canonical_externals
        .iter()
        .any(|ext| wt_canonical.starts_with(ext))
}

/// Compute the display name for a detached HEAD worktree.
///
/// For internal worktrees (under project root), uses `file_name()`.
/// For external worktrees, uses the relative path within the external dir
/// (e.g., `a0db/coastguard-platform`) to produce unique, distinguishable names.
fn detached_worktree_name(
    wt_canonical: &std::path::Path,
    canonical_root: &std::path::Path,
    canonical_externals: &[PathBuf],
) -> String {
    for ext in canonical_externals {
        if let Ok(relative) = wt_canonical.strip_prefix(ext) {
            return relative.display().to_string();
        }
    }
    if let Ok(relative) = wt_canonical.strip_prefix(canonical_root) {
        return relative
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
    }
    wt_canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_known_worktree_under_root() {
        let root = PathBuf::from("/projects/app");
        let externals: Vec<PathBuf> = vec![];
        assert!(is_known_worktree(
            &PathBuf::from("/projects/app/.worktrees/feat"),
            &root,
            &externals,
        ));
    }

    #[test]
    fn test_is_known_worktree_is_root() {
        let root = PathBuf::from("/projects/app");
        let externals: Vec<PathBuf> = vec![];
        assert!(!is_known_worktree(&root, &root, &externals));
    }

    #[test]
    fn test_is_known_worktree_under_external() {
        let root = PathBuf::from("/projects/app");
        let externals = vec![PathBuf::from("/home/user/.codex/worktrees")];
        assert!(is_known_worktree(
            &PathBuf::from("/home/user/.codex/worktrees/abc123/my-app"),
            &root,
            &externals,
        ));
    }

    #[test]
    fn test_is_known_worktree_unknown_location() {
        let root = PathBuf::from("/projects/app");
        let externals = vec![PathBuf::from("/home/user/.codex/worktrees")];
        assert!(!is_known_worktree(
            &PathBuf::from("/tmp/random/worktree"),
            &root,
            &externals,
        ));
    }

    #[test]
    fn test_parse_porcelain_internal_only() {
        let root = tempfile::tempdir().unwrap();
        let wt_dir = root.path().join(".worktrees").join("feat-a");
        std::fs::create_dir_all(&wt_dir).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc123\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def456\nbranch refs/heads/feat-a\n\n",
            root.path().display(),
            wt_dir.display(),
        );
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &[]);
        assert_eq!(result, vec!["feat-a"]);
    }

    #[test]
    fn test_parse_porcelain_external_accepted() {
        let root = tempfile::tempdir().unwrap();
        let ext_dir = tempfile::tempdir().unwrap();
        let wt = ext_dir.path().join("abc").join("my-project");
        std::fs::create_dir_all(&wt).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc123\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def456\nbranch refs/heads/ext-feat\n\n",
            root.path().display(),
            wt.display(),
        );
        let externals = vec![ext_dir.path().to_path_buf()];
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &externals);
        assert_eq!(result, vec!["ext-feat"]);
    }

    #[test]
    fn test_parse_porcelain_unknown_rejected() {
        let root = tempfile::tempdir().unwrap();
        let unknown = tempfile::tempdir().unwrap();
        let wt = unknown.path().join("stray-worktree");
        std::fs::create_dir_all(&wt).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc123\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def456\nbranch refs/heads/stray\n\n",
            root.path().display(),
            wt.display(),
        );
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_porcelain_detached_in_external() {
        let root = tempfile::tempdir().unwrap();
        let ext_dir = tempfile::tempdir().unwrap();
        let wt = ext_dir.path().join("hash123").join("my-project");
        std::fs::create_dir_all(&wt).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc123\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def456\ndetached\n\n",
            root.path().display(),
            wt.display(),
        );
        let externals = vec![ext_dir.path().to_path_buf()];
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &externals);
        assert_eq!(result, vec!["hash123/my-project"]);
    }

    #[test]
    fn test_parse_porcelain_dedup_across_internal_and_external() {
        let root = tempfile::tempdir().unwrap();
        let ext_dir = tempfile::tempdir().unwrap();
        let internal_wt = root.path().join(".worktrees").join("feat");
        let external_wt = ext_dir.path().join("feat");
        std::fs::create_dir_all(&internal_wt).unwrap();
        std::fs::create_dir_all(&external_wt).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def\nbranch refs/heads/feat\n\n\
             worktree {}\nHEAD ghi\nbranch refs/heads/feat\n\n",
            root.path().display(),
            internal_wt.display(),
            external_wt.display(),
        );
        let externals = vec![ext_dir.path().to_path_buf()];
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &externals);
        assert_eq!(result, vec!["feat"]);
    }

    #[test]
    fn test_parse_porcelain_mixed_internal_and_external() {
        let root = tempfile::tempdir().unwrap();
        let ext_dir = tempfile::tempdir().unwrap();
        let internal_wt = root.path().join(".worktrees").join("local-feat");
        let external_wt = ext_dir.path().join("hash").join("project");
        std::fs::create_dir_all(&internal_wt).unwrap();
        std::fs::create_dir_all(&external_wt).unwrap();

        let porcelain = format!(
            "worktree {}\nHEAD abc\nbranch refs/heads/main\n\n\
             worktree {}\nHEAD def\nbranch refs/heads/local-feat\n\n\
             worktree {}\nHEAD ghi\nbranch refs/heads/ext-feat\n\n",
            root.path().display(),
            internal_wt.display(),
            external_wt.display(),
        );
        let externals = vec![ext_dir.path().to_path_buf()];
        let result = parse_porcelain_worktrees(&porcelain, root.path(), &externals);
        assert_eq!(result, vec!["ext-feat", "local-feat"]);
    }
}
