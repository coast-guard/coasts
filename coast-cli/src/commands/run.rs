/// `coast run` command — create and start a new coast instance.
///
/// Starts a coast container with an inner Docker daemon, loads cached images,
/// and runs `docker compose up`. The host project root is bind-mounted into
/// the DinD container.
///
/// Supports batch creation: `coast run dev-{n} --n=5` creates dev-1..dev-5.
/// The git branch is always auto-detected from the current HEAD.
use std::io::Write;

use anyhow::{bail, Context, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{Request, Response, RunRequest};

use super::build::{ProgressDisplay, Verbosity};

/// Arguments for `coast run`.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Name for the new coast instance. Use {n} for batch numbering.
    pub name: String,

    /// Worktree to assign after provisioning completes.
    #[arg(short = 'w', long)]
    pub worktree: Option<String>,

    /// Number of instances to create. Name must contain {n}.
    #[arg(long = "n")]
    pub count: Option<usize>,

    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose detail (e.g., docker build logs).
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Coastfile type to use for build resolution (e.g. --type light).
    #[arg(short = 't', long = "type")]
    pub coastfile_type: Option<String>,

    /// Which registered remote to use (only for --type remote).
    #[arg(long)]
    pub remote: Option<String>,

    /// Force-remove any dangling Docker container with the same name before creating.
    #[arg(long)]
    pub force_remove_dangling: bool,
}

/// Validate that batch arguments are consistent.
///
/// - If `{n}` is in the name, `--n` must be provided
/// - If `--n` is provided, `{n}` must be in the name
/// - `count` must be > 0
pub fn validate_batch_args(name: &str, count: Option<usize>) -> Result<()> {
    let has_template = name.contains("{n}");

    match (has_template, count) {
        (true, None) => {
            bail!("{}", t!("cli.batch.template_no_count"));
        }
        (false, Some(_)) => {
            bail!("{}", t!("cli.batch.count_no_template"));
        }
        (_, Some(0)) => {
            bail!("{}", t!("cli.batch.count_zero"));
        }
        _ => Ok(()),
    }
}

/// Expand a name template into a list of instance names.
///
/// - If `count` is `None`, returns a single-element vec with the original name.
/// - If `count` is `Some(n)`, replaces `{n}` with 1..=n.
pub fn expand_names(name: &str, count: Option<usize>) -> Vec<String> {
    match count {
        None => vec![name.to_string()],
        Some(n) => (1..=n)
            .map(|i| name.replace("{n}", &i.to_string()))
            .collect(),
    }
}

/// Detect the current git branch and HEAD commit SHA.
///
/// Returns `(branch_name, commit_sha)`.
pub fn detect_current_branch() -> Result<(String, String)> {
    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context(
            "Failed to run `git rev-parse --abbrev-ref HEAD`. Is git installed and is this a git repo?",
        )?;

    if !branch_output.status.success() {
        let stderr = String::from_utf8_lossy(&branch_output.stderr);
        bail!(
            "{}",
            t!("cli.git.branch_detect_failed", error = stderr.trim())
        );
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    let sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("Failed to run `git rev-parse HEAD`.")?;

    let sha = if sha_output.status.success() {
        String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string()
    } else {
        String::new()
    };

    Ok((branch, sha))
}

/// Prompt the user to confirm creating a large batch.
///
/// Returns `true` if the user confirms, `false` otherwise.
pub fn confirm_large_batch(count: usize) -> Result<bool> {
    print!("{} ", t!("cli.info.confirm_large_batch", count = count));
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();
    Ok(trimmed == "y" || trimmed == "yes")
}

/// Execute the `coast run` command.
pub async fn execute(args: &RunArgs, project: &str) -> Result<()> {
    // Step 1: Validate batch args
    validate_batch_args(&args.name, args.count)?;

    // Step 2: Detect branch from git HEAD
    let (branch, sha) = detect_current_branch()?;
    let commit_sha = if sha.is_empty() { None } else { Some(sha) };

    // Step 3: Expand names
    let names = expand_names(&args.name, args.count);

    // Step 4: Confirm large batches
    if names.len() > 10 && !confirm_large_batch(names.len())? {
        println!("{}", t!("cli.info.aborted"));
        return Ok(());
    }

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    // Step 5: Create instances
    let mut successes = 0usize;
    let mut failures = Vec::new();

    for name in &names {
        let request = Request::Run(RunRequest {
            name: name.clone(),
            project: project.to_string(),
            branch: Some(branch.clone()),
            commit_sha: commit_sha.clone(),
            worktree: args.worktree.clone(),
            build_id: None,
            coastfile_type: args.coastfile_type.clone(),
            remote: args.remote.clone(),
            force_remove_dangling: args.force_remove_dangling,
            shared_service_ports: Vec::new(),
        });

        let mut display = ProgressDisplay::new(verbosity);

        let response = super::send_run_request(request, |event| {
            display.handle_event(event);
        })
        .await;

        display.finalize();

        match response {
            Ok(Response::Run(resp)) => {
                successes += 1;
                if verbosity != Verbosity::Silent {
                    eprintln!();
                }
                println!(
                    "{} {}",
                    "ok".green().bold(),
                    t!(
                        "cli.ok.instance_created",
                        name = resp.name,
                        container = &resp.container_id[..12.min(resp.container_id.len())]
                    ),
                );

                if !resp.ports.is_empty() && names.len() == 1 {
                    println!("\n  {}", t!("cli.info.dynamic_ports").to_string().bold());
                    println!("{}", super::format_port_table(&resp.ports, None));
                }
            }
            Ok(Response::Error(e)) => {
                failures.push((name.clone(), e.error.clone()));
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!("cli.err.failed_to_create", name = name, error = e.error),
                );
                if e.error.contains("dangling Docker container") {
                    eprintln!(
                        "\n{} {}",
                        "hint".yellow().bold(),
                        t!("cli.hint.dangling_container", name = name),
                    );
                }
            }
            Ok(_) => {
                failures.push((name.clone(), t!("error.unexpected_response").to_string()));
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!(
                        "cli.err.failed_to_create",
                        name = name,
                        error = t!("error.unexpected_response")
                    ),
                );
            }
            Err(e) => {
                failures.push((name.clone(), e.to_string()));
                eprintln!(
                    "{} {}",
                    "err".red().bold(),
                    t!("cli.err.failed_to_create", name = name, error = e),
                );
            }
        }
    }

    // Step 6: Summary
    if names.len() > 1 {
        println!(
            "\n{}",
            t!(
                "cli.info.batch_summary",
                successes = successes,
                total = names.len()
            ),
        );
    }

    if !failures.is_empty() {
        bail!(
            "{}",
            t!("cli.err.instances_failed_create", count = failures.len())
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: RunArgs,
    }

    #[test]
    fn test_run_args_name_only() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.worktree.is_none());
        assert!(cli.args.count.is_none());
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_run_args_with_count() {
        let cli = TestCli::try_parse_from(["test", "dev-{n}", "--n", "5"]).unwrap();
        assert_eq!(cli.args.name, "dev-{n}");
        assert_eq!(cli.args.count, Some(5));
    }

    #[test]
    fn test_run_args_with_worktree_and_count() {
        let cli =
            TestCli::try_parse_from(["test", "dev-{n}", "--worktree", "feature/x", "--n", "3"])
                .unwrap();
        assert_eq!(cli.args.name, "dev-{n}");
        assert_eq!(cli.args.worktree, Some("feature/x".to_string()));
        assert_eq!(cli.args.count, Some(3));
    }

    #[test]
    fn test_run_args_branch_rejected() {
        let result = TestCli::try_parse_from(["test", "feature-x", "--branch", "feature/x"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_args_silent() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--silent"]).unwrap();
        assert!(cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_run_args_silent_short() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-s"]).unwrap();
        assert!(cli.args.silent);
    }

    #[test]
    fn test_run_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
        assert!(!cli.args.silent);
    }

    #[test]
    fn test_run_args_verbose_short() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-v"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_run_args_with_worktree() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--worktree", "my-feature"]).unwrap();
        assert_eq!(cli.args.worktree, Some("my-feature".to_string()));
    }

    #[test]
    fn test_run_args_with_worktree_short() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "-w", "my-feature"]).unwrap();
        assert_eq!(cli.args.worktree, Some("my-feature".to_string()));
    }

    // --- validate_batch_args tests ---

    #[test]
    fn test_validate_batch_args_template_no_count() {
        let result = validate_batch_args("dev-{n}", None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("{n}"));
        assert!(err.contains("--n"));
    }

    #[test]
    fn test_validate_batch_args_count_no_template() {
        let result = validate_batch_args("dev-1", Some(5));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("{n}"));
    }

    #[test]
    fn test_validate_batch_args_valid_batch() {
        assert!(validate_batch_args("dev-{n}", Some(5)).is_ok());
    }

    #[test]
    fn test_validate_batch_args_single() {
        assert!(validate_batch_args("my-coast", None).is_ok());
    }

    #[test]
    fn test_validate_batch_args_zero_count() {
        let result = validate_batch_args("dev-{n}", Some(0));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 1"));
    }

    // --- expand_names tests ---

    #[test]
    fn test_expand_names_single() {
        let names = expand_names("my-coast", None);
        assert_eq!(names, vec!["my-coast"]);
    }

    #[test]
    fn test_expand_names_batch() {
        let names = expand_names("dev-{n}", Some(3));
        assert_eq!(names, vec!["dev-1", "dev-2", "dev-3"]);
    }

    #[test]
    fn test_expand_names_template_in_middle() {
        let names = expand_names("coast-{n}-test", Some(2));
        assert_eq!(names, vec!["coast-1-test", "coast-2-test"]);
    }

    // --- force_remove_dangling flag tests ---

    #[test]
    fn test_run_args_force_remove_dangling() {
        let cli = TestCli::try_parse_from(["test", "dev-1", "--force-remove-dangling"]).unwrap();
        assert!(cli.args.force_remove_dangling);
    }

    #[test]
    fn test_run_args_force_remove_dangling_default() {
        let cli = TestCli::try_parse_from(["test", "dev-1"]).unwrap();
        assert!(!cli.args.force_remove_dangling);
    }

    #[test]
    fn test_run_args_force_remove_dangling_with_other_flags() {
        let cli = TestCli::try_parse_from([
            "test",
            "dev-1",
            "--worktree",
            "main",
            "--force-remove-dangling",
            "-v",
        ])
        .unwrap();
        assert!(cli.args.force_remove_dangling);
        assert_eq!(cli.args.worktree, Some("main".to_string()));
        assert!(cli.args.verbose);
    }
}
