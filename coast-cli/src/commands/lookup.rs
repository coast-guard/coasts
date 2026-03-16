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

/// Detect which worktree the user is in by examining the cwd relative to
/// the project root and its configured `worktree_dir` entries.
///
/// Returns `Some(name)` if the cwd is inside `{project_root}/{worktree_dir}/{name}/...`,
/// or `None` if the cwd is the project root (or not inside any worktree).
pub fn detect_worktree() -> Result<Option<String>> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let (project_root, worktree_dirs) = find_project_root_and_worktree_dirs(&cwd)?;

    for dir in &worktree_dirs {
        let wt_path = project_root.join(dir);
        if let Some(name) = detect_worktree_from_paths(&cwd, &wt_path)? {
            return Ok(Some(name));
        }
    }
    Ok(None)
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
}
