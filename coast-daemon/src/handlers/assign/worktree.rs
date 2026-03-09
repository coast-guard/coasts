use coast_core::error::{CoastError, Result};

/// Detect the worktree parent directory from existing git worktrees.
///
/// Runs `git worktree list --porcelain`, collects non-main worktree paths,
/// and returns the first relative path component shared by all worktrees.
/// For example, worktrees at `.worktrees/feat-a` and `.worktrees/testing/speed`
/// both have `.worktrees` as their first component, so `.worktrees` is returned.
/// Returns `None` if there are no non-main worktrees or they disagree on the first component.
pub fn detect_worktree_dir_from_git(project_root: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let canonical_root = project_root.canonicalize().ok()?;

    let mut first_components: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            let path = std::path::PathBuf::from(path_str);
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if canonical == canonical_root {
                continue;
            }
            if let Ok(relative) = canonical.strip_prefix(&canonical_root) {
                if let Some(first) = relative.components().next() {
                    first_components.push(first.as_os_str().to_string_lossy().to_string());
                }
            }
        }
    }

    if first_components.is_empty() {
        return None;
    }

    let first = &first_components[0];
    if first_components.iter().all(|c| c == first) {
        Some(first.clone())
    } else {
        None
    }
}

/// Fallback worktree creation when `git worktree add <path> <branch>` fails.
/// Checks whether the branch already exists: if so, uses `--force` to reuse it;
/// if not, creates a new branch with `-b`.
pub(super) async fn create_worktree_fallback(
    root: &std::path::Path,
    worktree_path: &std::path::Path,
    branch: &str,
) -> Result<std::process::Output> {
    let branch_exists = tokio::process::Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(root)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if branch_exists {
        tokio::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--force",
                &worktree_path.to_string_lossy(),
                branch,
            ])
            .current_dir(root)
            .output()
            .await
            .map_err(|e| CoastError::git(format!("Failed to create worktree: {e}")))
    } else {
        tokio::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                branch,
                &worktree_path.to_string_lossy(),
            ])
            .current_dir(root)
            .output()
            .await
            .map_err(|e| CoastError::git(format!("Failed to create worktree: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_in(root: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed to start");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn test_no_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);

        let result = detect_worktree_dir_from_git(root);
        assert_eq!(result, None, "should return None when no worktrees exist");
    }

    #[test]
    fn test_with_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "feat-a"]);
        git_in(root, &["branch", "feat-b"]);

        let wt_parent = root.join(".worktrees");
        std::fs::create_dir_all(&wt_parent).unwrap();
        git_in(
            root,
            &[
                "worktree",
                "add",
                &wt_parent.join("feat-a").to_string_lossy(),
                "feat-a",
            ],
        );
        git_in(
            root,
            &[
                "worktree",
                "add",
                &wt_parent.join("feat-b").to_string_lossy(),
                "feat-b",
            ],
        );

        let result = detect_worktree_dir_from_git(root);
        assert_eq!(
            result,
            Some(".worktrees".to_string()),
            "should detect .worktrees as the common parent"
        );
    }

    #[test]
    fn test_non_git() {
        let dir = tempfile::tempdir().unwrap();
        let result = detect_worktree_dir_from_git(dir.path());
        assert_eq!(result, None, "should return None for non-git directory");
    }

    #[test]
    fn test_with_slash_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "testing/assign-speed"]);

        let wt_path = root.join(".worktrees").join("testing").join("assign-speed");
        std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
        git_in(
            root,
            &[
                "worktree",
                "add",
                &wt_path.to_string_lossy(),
                "testing/assign-speed",
            ],
        );

        let result = detect_worktree_dir_from_git(root);
        assert_eq!(
            result,
            Some(".worktrees".to_string()),
            "slash branch at .worktrees/testing/assign-speed should return .worktrees"
        );
    }

    #[test]
    fn test_multiple_slash_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "feature/auth"]);
        git_in(root, &["branch", "testing/speed"]);

        let wt_a = root.join(".worktrees").join("feature").join("auth");
        let wt_b = root.join(".worktrees").join("testing").join("speed");
        std::fs::create_dir_all(wt_a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(wt_b.parent().unwrap()).unwrap();
        git_in(
            root,
            &["worktree", "add", &wt_a.to_string_lossy(), "feature/auth"],
        );
        git_in(
            root,
            &["worktree", "add", &wt_b.to_string_lossy(), "testing/speed"],
        );

        let result = detect_worktree_dir_from_git(root);
        assert_eq!(
            result,
            Some(".worktrees".to_string()),
            "multiple slash branches should return .worktrees"
        );
    }

    #[test]
    fn test_mixed_flat_and_slash() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "feat-a"]);
        git_in(root, &["branch", "testing/speed"]);

        let wt_flat = root.join(".worktrees").join("feat-a");
        let wt_slash = root.join(".worktrees").join("testing").join("speed");
        std::fs::create_dir_all(wt_flat.parent().unwrap()).unwrap();
        std::fs::create_dir_all(wt_slash.parent().unwrap()).unwrap();
        git_in(
            root,
            &["worktree", "add", &wt_flat.to_string_lossy(), "feat-a"],
        );
        git_in(
            root,
            &[
                "worktree",
                "add",
                &wt_slash.to_string_lossy(),
                "testing/speed",
            ],
        );

        let result = detect_worktree_dir_from_git(root);
        assert_eq!(
            result,
            Some(".worktrees".to_string()),
            "mixed flat and slash branches should return .worktrees"
        );
    }

    #[tokio::test]
    async fn test_fallback_creates_new_branch() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);

        let wt_path = root.join(".worktrees").join("new-feature");
        std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();

        let result = create_worktree_fallback(root, &wt_path, "new-feature").await;
        assert!(result.is_ok());
        assert!(result.unwrap().status.success());
        assert!(wt_path.exists());
    }
}
