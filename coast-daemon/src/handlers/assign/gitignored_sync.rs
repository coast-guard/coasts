/// Directories excluded from gitignored file sync (heavy or generated dirs).
pub(super) const SYNC_EXCLUDE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "__pycache__",
    "dist",
    ".next",
    ".nuxt",
    "target",
    ".cache",
    ".worktrees",
    ".coasts",
    ".coast-synced",
    "__debug_bin",
];

/// Build the shell script that syncs gitignored files from the project root
/// into a worktree. Uses `git ls-files --others --ignored` to enumerate only
/// the files that need syncing, then either hardlinks them via rsync
/// `--files-from` or copies them via the tar pipeline. This avoids traversing
/// the entire project tree (which is extremely slow in large repos).
/// Touches `.coast-synced` on success so subsequent assigns skip the copy.
pub(super) fn build_gitignored_sync_script(
    root: &str,
    wt_path: &str,
    extra_excludes: &[String],
) -> String {
    let mut grep_parts: Vec<String> = SYNC_EXCLUDE_DIRS
        .iter()
        .filter(|d| **d != ".git" && **d != ".coast-synced")
        .map(|d| d.replace('.', "\\."))
        .collect();
    for path in extra_excludes {
        grep_parts.push(path.replace('.', "\\."));
    }
    let grep_excludes = grep_parts.join("|");

    format!(
        "cd '{root}' && \
         git ls-files --others --ignored --exclude-standard 2>/dev/null | \
         grep -v -E '{grep_excludes}' > /tmp/.coast-sync-filelist 2>/dev/null; \
         if [ -s /tmp/.coast-sync-filelist ]; then \
           if command -v rsync >/dev/null 2>&1; then \
             rsync -a --link-dest='{root}' --files-from=/tmp/.coast-sync-filelist \
               '{root}/' '{wt_path}/' 2>/dev/null; \
           else \
             tar -T /tmp/.coast-sync-filelist -cf - 2>/dev/null | \
             tar -xf - -C '{wt_path}' 2>/dev/null; \
           fi; \
         fi; \
         rm -f /tmp/.coast-sync-filelist; \
         touch '{wt_path}/.coast-synced'; true"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uses_git_ls_files() {
        let script =
            build_gitignored_sync_script("/home/user/project", "/home/user/.worktrees/feat", &[]);
        assert!(
            script.contains("git ls-files --others --ignored --exclude-standard"),
            "should use git ls-files to enumerate gitignored files"
        );
    }

    #[test]
    fn test_uses_rsync_files_from() {
        let script =
            build_gitignored_sync_script("/home/user/project", "/home/user/.worktrees/feat", &[]);
        assert!(
            script.contains("--files-from="),
            "should use rsync --files-from for targeted sync"
        );
        assert!(
            script.contains("--link-dest='/home/user/project'"),
            "should use rsync with --link-dest pointing to project root"
        );
        assert!(
            script.contains("'/home/user/project/' '/home/user/.worktrees/feat/'"),
            "should rsync from root/ to wt_path/"
        );
    }

    #[test]
    fn test_grep_excludes_heavy_dirs() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        let grep_idx = script.find("grep -v -E").expect("should have grep");
        let grep_section = &script[grep_idx..];
        for dir in SYNC_EXCLUDE_DIRS {
            if *dir == ".git" || *dir == ".coast-synced" {
                continue;
            }
            let escaped = dir.replace('.', "\\.");
            assert!(
                grep_section.contains(&escaped),
                "grep pattern should exclude '{dir}'"
            );
        }
    }

    #[test]
    fn test_creates_marker() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        assert!(
            script.contains("touch '/wt/.coast-synced'"),
            "should create .coast-synced marker on success"
        );
    }

    #[test]
    fn test_has_tar_fallback() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        assert!(
            script.contains("if command -v rsync"),
            "should check for rsync availability"
        );
        assert!(
            script.contains("tar -T"),
            "should fall back to tar pipeline when rsync is missing"
        );
    }

    #[test]
    fn test_exclude_paths_in_sync_script() {
        let extras = vec!["apps/ide".to_string(), "apps/extension".to_string()];
        let script = build_gitignored_sync_script("/root", "/wt", &extras);
        let grep_idx = script.find("grep -v -E").expect("should have grep");
        let grep_section = &script[grep_idx..];
        assert!(
            grep_section.contains("apps/ide"),
            "grep pattern should contain 'apps/ide'"
        );
        assert!(
            grep_section.contains("apps/extension"),
            "grep pattern should contain 'apps/extension'"
        );
    }

    #[test]
    fn test_marker_skip_when_exists() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".coast-synced");
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists(), "marker should exist");
    }

    #[test]
    fn test_marker_not_present_initially() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".coast-synced");
        assert!(!marker.exists(), "marker should not exist in fresh dir");
    }

    #[test]
    fn test_empty_extra_excludes() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        assert!(script.contains("grep -v -E"));
    }

    #[test]
    fn test_special_regex_chars_in_paths() {
        let extras = vec!["foo.bar".to_string(), "baz+qux".to_string()];
        let script = build_gitignored_sync_script("/root", "/wt", &extras);
        let grep_idx = script.find("grep -v -E").expect("should have grep");
        let grep_section = &script[grep_idx..];
        assert!(
            grep_section.contains("foo\\.bar"),
            "dots in paths should be escaped"
        );
    }
}
