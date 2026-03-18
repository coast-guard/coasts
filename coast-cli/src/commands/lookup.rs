/// `coast lookup` command — find coast instances for the caller's current worktree.
///
/// Detects which worktree the user is inside (based on cwd relative to the
/// project's `worktree_dir`), queries the daemon for matching instances, and
/// outputs the results in one of three formats: default (human-readable),
/// `--compact` (JSON name array), or `--json` (full structured JSON).
///
/// Designed primarily for AI coding agents that need to discover which coast
/// instance(s) correspond to the directory they are working in.
use std::path::Path;

use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{LookupRequest, Request, Response};

/// Arguments for `coast lookup`.
#[derive(Debug, Args)]
pub struct LookupArgs {
    /// Output only instance names as a JSON array.
    #[arg(long)]
    pub compact: bool,
    /// Output full structured JSON.
    #[arg(long, conflicts_with = "compact")]
    pub json: bool,
}

/// Execute the `coast lookup` command.
pub async fn execute(args: &LookupArgs, project: &str) -> Result<()> {
    let worktree = detect_worktree()?;

    let request = Request::Lookup(LookupRequest {
        project: project.to_string(),
        worktree: worktree.clone(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Lookup(resp) => {
            if args.compact {
                let names: Vec<&str> = resp.instances.iter().map(|i| i.name.as_str()).collect();
                println!("{}", serde_json::to_string(&names).unwrap_or_default());
            } else if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resp).unwrap_or_default()
                );
            } else {
                // Human-readable output
                let wt_display = match &resp.worktree {
                    Some(w) => w.as_str(),
                    None => "project root",
                };

                if resp.instances.is_empty() {
                    match &resp.worktree {
                        Some(w) => println!(
                            "{}",
                            t!(
                                "cli.lookup.no_instances",
                                worktree = w,
                                project = resp.project
                            )
                        ),
                        None => println!(
                            "{}",
                            t!("cli.lookup.no_instances_root", project = resp.project)
                        ),
                    }

                    if let Some(ref w) = resp.worktree {
                        println!("\n  {}", t!("cli.lookup.run_hint", worktree = w));
                    }
                } else {
                    match &resp.worktree {
                        Some(w) => println!(
                            "{}",
                            t!("cli.lookup.header", worktree = w, project = resp.project)
                        ),
                        None => {
                            println!("{}", t!("cli.lookup.header_root", project = resp.project))
                        }
                    }

                    for (i, inst) in resp.instances.iter().enumerate() {
                        if i > 0 {
                            println!("\n  {}", "─".repeat(45));
                        }
                        println!();

                        let status_str = format!("{:?}", inst.status).to_lowercase();
                        let checked = if inst.checked_out {
                            format!("  {} checked out", "★".yellow())
                        } else {
                            String::new()
                        };
                        println!("  {}  {}{}", inst.name.bold(), status_str.green(), checked);

                        if let Some(ref url) = inst.primary_url {
                            println!("\n  {}  {}", "Primary URL:".bold(), url);
                        }

                        if !inst.ports.is_empty() {
                            println!();
                            println!("{}", super::format_port_table(&inst.ports, None));
                        }

                        println!(
                            "\n  {} (exec starts at the workspace root where your Coastfile is, cd to your target directory first):",
                            "Examples".bold()
                        );
                        println!(
                            "    coast exec {} -- sh -c \"cd <dir> && <command>\"",
                            inst.name
                        );
                        println!("    coast logs {} --service <service>", inst.name);
                        println!("    coast ps {}", inst.name);
                    }
                }

                println!();

                let _ = wt_display; // used in header above
            }

            if resp.instances.is_empty() {
                std::process::exit(1);
            }

            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("{}", t!("error.unexpected_response"));
        }
    }
}

/// Detect which worktree the user is in.
///
/// First tries the path-based approach (cwd under `{project_root}/{worktree_dir}/{name}`).
/// If that fails, falls back to git worktree detection via the `.git` file, which
/// handles external worktrees (e.g., `~/.codex/worktrees/`, `~/.t3/worktrees/`).
pub fn detect_worktree() -> Result<Option<String>> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;

    if let Ok((project_root, worktree_dirs)) = find_project_root_and_worktree_dirs(&cwd) {
        for dir in &worktree_dirs {
            let resolved =
                coast_core::coastfile::Coastfile::resolve_worktree_dir(&project_root, dir);
            if let Some(name) = detect_worktree_from_paths(&cwd, &resolved)? {
                return Ok(Some(name));
            }
        }
    }

    if let Some(name) = detect_worktree_via_git(&cwd)? {
        return Ok(Some(name));
    }

    Ok(None)
}

/// Detect the worktree name by tracing the `.git` file back to the main repo
/// and matching cwd against `git worktree list --porcelain`.
///
/// This handles external worktrees where cwd is outside the project root.
fn detect_worktree_via_git(cwd: &Path) -> Result<Option<String>> {
    let real_project_root = resolve_project_root_from_git_file(cwd)?;

    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&real_project_root)
        .output()
        .context("Failed to run git worktree list")?;

    if !output.status.success() {
        return Ok(None);
    }

    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let canonical_root = real_project_root
        .canonicalize()
        .unwrap_or_else(|_| real_project_root.clone());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let worktree_dirs = load_worktree_dirs_from_project(&real_project_root);
    let external_dirs: Vec<std::path::PathBuf> = worktree_dirs
        .iter()
        .filter(|d| coast_core::coastfile::Coastfile::is_external_worktree_dir(d))
        .map(|d| coast_core::coastfile::Coastfile::resolve_worktree_dir(&real_project_root, d))
        .collect();

    let mut current_path: Option<std::path::PathBuf> = None;

    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            current_path = Some(std::path::PathBuf::from(path_str));
        } else if line.starts_with("branch ") || line == "detached" {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                if wt_canonical == canonical_root {
                    continue;
                }
                if !canonical_cwd.starts_with(&wt_canonical) {
                    continue;
                }

                if let Some(branch_ref) = line.strip_prefix("branch ") {
                    let name = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
                    return Ok(Some(name.to_string()));
                }

                for ext_dir in &external_dirs {
                    let canon_ext = ext_dir.canonicalize().unwrap_or_else(|_| ext_dir.clone());
                    if let Ok(relative) = wt_canonical.strip_prefix(&canon_ext) {
                        let name = relative.display().to_string();
                        if !name.is_empty() {
                            return Ok(Some(name));
                        }
                    }
                }

                if let Some(name) = wt_path.file_name().and_then(|n| n.to_str()) {
                    return Ok(Some(name.to_string()));
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    Ok(None)
}

/// Resolve the real project root by reading the `.git` file in cwd (or an ancestor)
/// and following the `gitdir:` pointer back to the main repository.
fn resolve_project_root_from_git_file(start: &Path) -> Result<std::path::PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let dot_git = dir.join(".git");
        if dot_git.is_file() {
            let content = std::fs::read_to_string(&dot_git)
                .with_context(|| format!("Failed to read {}", dot_git.display()))?;
            if let Some(gitdir_str) = content.lines().find_map(|l| l.strip_prefix("gitdir: ")) {
                let gitdir_str = gitdir_str.trim();
                let gitdir_path = if std::path::Path::new(gitdir_str).is_absolute() {
                    std::path::PathBuf::from(gitdir_str)
                } else {
                    dir.join(gitdir_str)
                };
                let canonical = gitdir_path
                    .canonicalize()
                    .unwrap_or_else(|_| gitdir_path.clone());
                let mut ancestor = canonical.as_path();
                while let Some(parent) = ancestor.parent() {
                    if parent.file_name().map(|n| n == ".git").unwrap_or(false) {
                        if let Some(project_root) = parent.parent() {
                            return Ok(project_root.to_path_buf());
                        }
                    }
                    ancestor = parent;
                }
                bail!("Could not resolve project root from gitdir: {}", gitdir_str);
            }
        } else if dot_git.is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    bail!(
        "No .git file or directory found walking up from {}",
        start.display()
    );
}

/// Load worktree_dirs from the Coastfile at the given project root.
fn load_worktree_dirs_from_project(project_root: &Path) -> Vec<String> {
    let coastfile_path = project_root.join("Coastfile");
    if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
        return cf.worktree_dirs;
    }
    vec![".worktrees".to_string()]
}

/// Detect the worktree name given explicit paths (for testability).
///
/// Handles branch names containing slashes (e.g. `testing/assign-speed`)
/// which create nested directories under the worktree base. Walks into the
/// relative path looking for a checkout root (`.git` or `Coastfile`), and
/// falls back to the first path component if no marker is found.
pub fn detect_worktree_from_paths(cwd: &Path, worktree_base: &Path) -> Result<Option<String>> {
    let Ok(canonical_cwd) = cwd.canonicalize() else {
        return Ok(None);
    };
    let Ok(canonical_wt) = worktree_base.canonicalize() else {
        return Ok(None);
    };

    if let Ok(relative) = canonical_cwd.strip_prefix(&canonical_wt) {
        let components: Vec<_> = relative.components().collect();
        if components.is_empty() {
            return Ok(None);
        }

        let mut accumulated = std::path::PathBuf::new();
        for component in &components {
            accumulated.push(component);
            let candidate = canonical_wt.join(&accumulated);
            if candidate.join(".git").exists() || candidate.join("Coastfile").exists() {
                let name = accumulated.to_string_lossy().to_string();
                if !name.is_empty() {
                    return Ok(Some(name));
                }
            }
        }

        if let Some(first) = components.first() {
            let name = first.as_os_str().to_string_lossy().to_string();
            if !name.is_empty() {
                return Ok(Some(name));
            }
        }
    }

    Ok(None)
}

/// Walk up from `start` to find the true project root and `worktree_dirs`.
///
/// A worktree directory contains a copy of the project, including its
/// Coastfile. So we collect every directory containing a Coastfile while
/// walking up, and pick the **outermost** (highest ancestor) as the true
/// project root. This ensures that if cwd is inside
/// `{project_root}/{worktree_dir}/{name}/...`, we resolve the actual
/// project root rather than the worktree copy.
fn find_project_root_and_worktree_dirs(start: &Path) -> Result<(std::path::PathBuf, Vec<String>)> {
    let mut dir = start.to_path_buf();
    let mut outermost: Option<(std::path::PathBuf, Vec<String>)> = None;
    loop {
        let coastfile_path = dir.join("Coastfile");
        if coastfile_path.exists() {
            if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
                outermost = Some((dir.clone(), cf.worktree_dirs));
            }
        }
        if !dir.pop() {
            break;
        }
    }
    outermost.ok_or_else(|| anyhow::anyhow!("{}", t!("cli.info.project_resolve_error")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: LookupArgs,
    }

    #[test]
    fn test_lookup_args_no_flags() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(!cli.args.compact);
        assert!(!cli.args.json);
    }

    #[test]
    fn test_lookup_args_compact() {
        let cli = TestCli::try_parse_from(["test", "--compact"]).unwrap();
        assert!(cli.args.compact);
        assert!(!cli.args.json);
    }

    #[test]
    fn test_lookup_args_json() {
        let cli = TestCli::try_parse_from(["test", "--json"]).unwrap();
        assert!(!cli.args.compact);
        assert!(cli.args.json);
    }

    #[test]
    fn test_lookup_args_compact_json_conflict() {
        let result = TestCli::try_parse_from(["test", "--compact", "--json"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_worktree_from_paths_in_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let feat_dir = wt_base.join("feature-alpha").join("src");
        std::fs::create_dir_all(&feat_dir).unwrap();

        let result = detect_worktree_from_paths(&feat_dir, &wt_base).unwrap();
        assert_eq!(result, Some("feature-alpha".to_string()));
    }

    #[test]
    fn test_detect_worktree_from_paths_in_worktree_root() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let feat_dir = wt_base.join("feature-beta");
        std::fs::create_dir_all(&feat_dir).unwrap();

        let result = detect_worktree_from_paths(&feat_dir, &wt_base).unwrap();
        assert_eq!(result, Some("feature-beta".to_string()));
    }

    #[test]
    fn test_detect_worktree_from_paths_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        let wt_base = project_root.join(".worktrees");
        std::fs::create_dir_all(&wt_base).unwrap();

        let result = detect_worktree_from_paths(project_root, &wt_base).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_worktree_from_paths_no_worktree_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path();
        let wt_base = project_root.join(".worktrees");
        // Don't create wt_base — it doesn't exist

        let result = detect_worktree_from_paths(project_root, &wt_base).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_worktree_from_paths_inside_worktree_dir_but_not_specific() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        std::fs::create_dir_all(&wt_base).unwrap();

        let result = detect_worktree_from_paths(&wt_base, &wt_base).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_worktree_from_paths_deeply_nested_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let deep = wt_base.join("feat").join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();

        let result = detect_worktree_from_paths(&deep, &wt_base).unwrap();
        assert_eq!(result, Some("feat".to_string()));
    }

    #[test]
    fn test_detect_worktree_from_paths_slash_branch_name() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let wt_dir = wt_base.join("testing").join("assign-speed");
        std::fs::create_dir_all(&wt_dir).unwrap();
        // Simulate a git worktree root marker
        std::fs::write(wt_dir.join(".git"), "gitdir: /fake/path").unwrap();

        let result = detect_worktree_from_paths(&wt_dir, &wt_base).unwrap();
        assert_eq!(result, Some("testing/assign-speed".to_string()));
    }

    #[test]
    fn test_detect_worktree_from_paths_slash_branch_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let wt_dir = wt_base.join("testing").join("assign-speed");
        let subdir = wt_dir.join("src").join("lib");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(wt_dir.join(".git"), "gitdir: /fake/path").unwrap();

        let result = detect_worktree_from_paths(&subdir, &wt_base).unwrap();
        assert_eq!(result, Some("testing/assign-speed".to_string()));
    }

    #[test]
    fn test_detect_worktree_from_paths_triple_slash_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let wt_base = tmp.path().join(".worktrees");
        let wt_dir = wt_base.join("team").join("feature").join("oauth");
        std::fs::create_dir_all(&wt_dir).unwrap();
        std::fs::write(wt_dir.join(".git"), "gitdir: /fake/path").unwrap();

        let result = detect_worktree_from_paths(&wt_dir, &wt_base).unwrap();
        assert_eq!(result, Some("team/feature/oauth".to_string()));
    }

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
    fn test_resolve_project_root_from_git_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "feat"]);

        let ext_dir = tempfile::tempdir().unwrap();
        let wt_path = ext_dir.path().join("feat-wt");
        git_in(
            root,
            &["worktree", "add", &wt_path.to_string_lossy(), "feat"],
        );

        let resolved = resolve_project_root_from_git_file(&wt_path).unwrap();
        assert_eq!(
            resolved.canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_resolve_project_root_from_git_file_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);

        let resolved = resolve_project_root_from_git_file(root).unwrap();
        assert_eq!(
            resolved.canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_resolve_project_root_from_git_file_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_project_root_from_git_file(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_worktree_via_git_branch_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);
        git_in(root, &["branch", "my-feature"]);

        std::fs::write(root.join("Coastfile"), "[coast]\nname = \"test\"\n").unwrap();

        let ext_dir = tempfile::tempdir().unwrap();
        let wt_path = ext_dir.path().join("my-feature");
        git_in(
            root,
            &["worktree", "add", &wt_path.to_string_lossy(), "my-feature"],
        );

        let result = detect_worktree_via_git(&wt_path).unwrap();
        assert_eq!(result, Some("my-feature".to_string()));
    }

    #[test]
    fn test_detect_worktree_via_git_detached_with_external_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);

        let ext_base = tempfile::tempdir().unwrap();
        let wt_path = ext_base.path().join("abc123").join("my-project");
        std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
        git_in(
            root,
            &["worktree", "add", "--detach", &wt_path.to_string_lossy()],
        );

        std::fs::write(
            root.join("Coastfile"),
            format!(
                "[coast]\nname = \"test\"\nworktree_dir = [\".worktrees\", \"{}\"]\n",
                ext_base.path().display()
            ),
        )
        .unwrap();

        let result = detect_worktree_via_git(&wt_path).unwrap();
        assert_eq!(result, Some("abc123/my-project".to_string()));
    }

    #[test]
    fn test_detect_worktree_via_git_at_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git_in(root, &["init", "-b", "main"]);
        git_in(root, &["commit", "--allow-empty", "-m", "init"]);

        std::fs::write(root.join("Coastfile"), "[coast]\nname = \"test\"\n").unwrap();

        let result = detect_worktree_via_git(root).unwrap();
        assert_eq!(result, None);
    }
}
