// Coast CLI — the user-facing command-line interface for managing coast instances.
//
// Connects to the `coastd` daemon over a Unix domain socket at `~/.coast/coastd.sock`
// and sends JSON-encoded requests for each subcommand.
rust_i18n::i18n!("../coast-i18n/locales", fallback = "en");

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use tracing_subscriber::EnvFilter;

pub mod commands;
pub mod i18n_helper;

/// Coast: Containerized Host — run isolated dev environments on a single machine.
#[derive(Debug, Parser)]
#[command(
    name = "coast",
    version,
    about = "Manage isolated development environments"
)]
pub struct Cli {
    /// Project name (overrides auto-detection from Coastfile in current directory).
    #[arg(long, global = true)]
    project: Option<String>,

    /// The subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

/// All available coast subcommands.
///
/// Ordered by group: global commands, project-explicit commands, then
/// project-context commands (roughly following the docs concept flow).
#[derive(Debug, Subcommand)]
pub enum Commands {
    // --- Global commands (no project context) ---
    /// Manage the coastd daemon process.
    Daemon(commands::daemon::DaemonArgs),
    /// Diagnose and repair orphaned state (missing containers).
    Doctor(commands::doctor::DoctorArgs),
    /// Open the Coast dashboard in your browser.
    Ui(commands::ui::UiArgs),
    /// Manage the localcoast DNS resolver.
    Dns(commands::dns::DnsArgs),
    /// Manage Coast configuration.
    Config(commands::config::ConfigArgs),
    /// Check for updates and apply them.
    Update(commands::update::UpdateArgs),
    /// List docs tree or print docs markdown content.
    Docs(commands::docs::DocsArgs),
    /// Search docs with hybrid semantic + keyword ranking.
    #[command(name = "search-docs")]
    SearchDocs(commands::search_docs::SearchDocsArgs),
    /// Print the Coastfile installation prompt for AI coding agents.
    #[command(name = "installation-prompt")]
    InstallationPrompt(commands::installation_prompt::InstallationPromptArgs),
    /// Print the Coast runtime skills prompt for AI coding agents.
    #[command(name = "skills-prompt")]
    SkillsPrompt(commands::skills_prompt::SkillsPromptArgs),

    // --- Project-explicit commands (project as positional arg or self-resolved) ---
    /// Build a coast image from a Coastfile.
    Build(commands::build::BuildArgs),
    /// Remove a project's build artifact and associated Docker resources.
    #[command(name = "rm-build")]
    RmBuild(commands::rm_build::RmBuildArgs),
    /// Inspect build artifacts.
    Builds(commands::builds::BuildsArgs),
    /// Archive a project (stop instances/services, hide from list).
    Archive(commands::archive::ArchiveArgs),
    /// Unarchive a project (restore to main list).
    Unarchive(commands::archive::UnarchiveArgs),

    // --- Project-context commands (project resolved from cwd or --project) ---
    /// Create and start a new coast instance.
    Run(commands::run::RunArgs),
    /// List all coast instances.
    Ls(commands::ls::LsArgs),
    /// Find coast instances for your current worktree.
    Lookup(commands::lookup::LookupArgs),
    /// Show port allocations for an instance.
    Ports(commands::ports::PortsArgs),
    /// Check out an instance (bind canonical ports to it).
    Checkout(commands::checkout::CheckoutArgs),
    /// Assign a worktree to an existing coast instance (fast worktree switch).
    Assign(commands::assign::AssignArgs),
    /// Return an instance to the repo's default branch.
    Unassign(commands::unassign::UnassignArgs),
    /// Stop a running instance.
    Stop(commands::stop::StopArgs),
    /// Start a stopped instance.
    Start(commands::start::StartArgs),
    /// Remove an instance.
    Rm(commands::rm::RmArgs),
    /// Rebuild images and restart services inside a coast instance.
    Rebuild(commands::rebuild::RebuildArgs),
    /// Re-run secret extractors using the cached build Coastfile.
    #[command(name = "rerun-extractors")]
    RerunExtractors(commands::rerun_extractors::RerunExtractorsArgs),
    /// Manage shared services.
    #[command(name = "shared-services")]
    SharedServices(commands::shared::SharedArgs),
    /// Manage per-instance secret overrides.
    Secret(commands::secret::SecretArgs),
    /// Stream logs from a coast instance.
    Logs(commands::logs::LogsArgs),
    /// Show inner compose service status.
    Ps(commands::ps::PsArgs),
    /// Execute a command inside a coast container.
    Exec(commands::exec::ExecArgs),
    /// Run a docker command inside a coast instance's inner daemon.
    Docker(commands::docker::DockerArgs),
    /// Manage per-instance agent shells.
    #[command(name = "agent-shell")]
    AgentShell(commands::agent_shell::AgentShellArgs),
    /// Inspect MCP server configuration and tools.
    Mcp(commands::mcp::McpArgs),
}

/// Resolve the project name from the CLI flag, or by walking up from cwd to
/// find the nearest Coastfile.
fn resolve_project(cli_project: &Option<String>) -> Result<String> {
    if let Some(project) = cli_project {
        return Ok(project.clone());
    }
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    resolve_project_from(&cwd)
}

/// Walk up from `start` looking for a Coastfile and return the project name.
fn resolve_project_from(start: &std::path::Path) -> Result<String> {
    let mut dir = start.to_path_buf();
    loop {
        let coastfile_path = dir.join("Coastfile");
        if coastfile_path.exists() {
            let coastfile = coast_core::coastfile::Coastfile::from_file(&coastfile_path)
                .context("Failed to parse Coastfile")?;
            return Ok(coastfile.name);
        }
        if !dir.pop() {
            break;
        }
    }
    bail!("{}", rust_i18n::t!("cli.info.project_resolve_error"))
}

/// Recursively walk the command tree and replace `about` strings with
/// translated versions. Builds dotted key paths like `cli.daemon.status.about`
/// so nested subcommands are translated automatically.
fn localize_clap_help(cmd: &mut clap::Command, lang: &str) {
    cmd.build();
    localize_recursive(cmd, "cli.app", lang);
}

fn localize_recursive(cmd: &mut clap::Command, prefix: &str, lang: &str) {
    use rust_i18n::t;

    // Recurse into subcommands first (before any clone that might lose the built tree).
    let sub_names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();

    for name in &sub_names {
        let key_prefix = if prefix == "cli.app" {
            format!("cli.{}", name.replace('-', "_"))
        } else {
            format!("{prefix}.{}", name.replace('-', "_"))
        };
        let sub = cmd.find_subcommand_mut(name).unwrap();
        localize_recursive(sub, &key_prefix, lang);
    }

    // Translate this command's own about. We use mem::take to move the
    // command out, apply .about(), and move it back -- avoiding a clone
    // that would discard the already-translated children.
    let self_key = format!("{prefix}.about");
    let translated = t!(&self_key, locale = lang);
    if !translated.contains(&self_key) {
        let taken = std::mem::take(cmd);
        *cmd = taken.about(translated.to_string());
    }
}

/// Print a clap error with translated chrome (error label, help hint).
///
/// Clap's internal error messages are English-only. We intercept them and
/// replace the structural bits ("error:", "Usage:", "For more information…")
/// with translated equivalents, leaving the specifics (arg names, values)
/// untouched since those are identifiers.
/// Print a clap error with translated chrome (error label, help hint).
///
/// NOTE: Clap handles `--help` and missing-subcommand cases internally
/// (calls `exit()` directly for nested subcommands), so those never reach
/// this function. We can only translate errors that clap returns to us via
/// `try_get_matches()`, such as invalid argument values. For nested
/// subcommand help text, clap rebuilds the Command tree from derive macros
/// internally, so our `localize_clap_help` translations only take effect
/// for the top-level `--help` output.
fn print_localized_clap_error(
    err: &clap::error::Error,
    lang: &str,
    _localized_cmd: &clap::Command,
) {
    use clap::error::ErrorKind;
    use rust_i18n::t;

    if matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        err.print().ok();
        return;
    }

    let raw = err.to_string();
    let lbl_error = t!("clap.error", locale = lang);
    let lbl_missing = t!("clap.missing_required", locale = lang);
    let lbl_usage = t!("clap.usage", locale = lang);
    let lbl_info = t!("clap.for_more_info", locale = lang);

    let translated = raw
        .replace("error:", &format!("{lbl_error}:"))
        .replace(
            "the following required arguments were not provided:",
            &lbl_missing,
        )
        .replace("Usage:", &lbl_usage)
        .replace("For more information, try '--help'.", &lbl_info);

    eprint!("{translated}");
}

/// Dispatch the parsed CLI command to the appropriate handler.
async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        // --- Global commands ---
        Commands::Daemon(args) => commands::daemon::execute(&args).await,
        Commands::Doctor(args) => commands::doctor::execute(&args).await,
        Commands::Ui(args) => commands::ui::execute(&args).await,
        Commands::Dns(args) => commands::dns::execute(&args).await,
        Commands::Config(args) => commands::config::execute(&args).await,
        Commands::Update(args) => commands::update::execute(&args).await,
        Commands::Docs(args) => commands::docs::execute(&args).await,
        Commands::SearchDocs(args) => commands::search_docs::execute(&args).await,
        Commands::InstallationPrompt(args) => commands::installation_prompt::execute(&args).await,
        Commands::SkillsPrompt(args) => commands::skills_prompt::execute(&args).await,

        // --- Project-explicit commands ---
        Commands::Build(args) => commands::build::execute(&args).await,
        Commands::RmBuild(args) => commands::rm_build::execute(&args).await,
        Commands::Builds(args) => commands::builds::execute(&args, &cli.project).await,
        Commands::Archive(args) => commands::archive::execute_archive(&args).await,
        Commands::Unarchive(args) => commands::archive::execute_unarchive(&args).await,
        Commands::Ls(args) => commands::ls::execute(&args, &cli.project).await,

        // --- Project-context commands (require project resolution) ---
        cmd => dispatch_project_command(cmd, &cli.project).await,
    }
}

/// Dispatch commands that require resolving a project from `--project` or cwd.
async fn dispatch_project_command(cmd: Commands, project_flag: &Option<String>) -> Result<()> {
    let project = resolve_project(project_flag)?;
    match cmd {
        Commands::Run(args) => commands::run::execute(&args, &project).await,
        Commands::Lookup(args) => commands::lookup::execute(&args, &project).await,
        Commands::Ports(args) => commands::ports::execute(&args, &project).await,
        Commands::Checkout(args) => commands::checkout::execute(&args, &project).await,
        Commands::Assign(args) => commands::assign::execute(&args, &project).await,
        Commands::Unassign(args) => commands::unassign::execute(&args, &project).await,
        Commands::Stop(args) => commands::stop::execute(&args, &project).await,
        Commands::Start(args) => commands::start::execute(&args, &project).await,
        Commands::Rm(args) => commands::rm::execute(&args, &project).await,
        Commands::Rebuild(args) => commands::rebuild::execute(&args, &project).await,
        Commands::RerunExtractors(args) => {
            commands::rerun_extractors::execute(&args, &project).await
        }
        Commands::SharedServices(args) => commands::shared::execute(&args, &project).await,
        Commands::Secret(args) => commands::secret::execute(&args, &project).await,
        Commands::Logs(args) => commands::logs::execute(&args, &project).await,
        Commands::Ps(args) => commands::ps::execute(&args, &project).await,
        Commands::Exec(args) => commands::exec::execute(&args, &project).await,
        Commands::Docker(args) => commands::docker::execute(&args, &project).await,
        Commands::AgentShell(args) => commands::agent_shell::execute(&args, &project).await,
        Commands::Mcp(args) => commands::mcp::execute(&args, &project).await,
        _ => unreachable!("non-project commands handled in dispatch()"),
    }
}

/// Main entry point for the coast CLI. Call this from your binary's main().
pub async fn run() -> Result<()> {
    // Initialize tracing (respects RUST_LOG env var)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let lang = i18n_helper::cli_lang();
    rust_i18n::set_locale(lang);
    let mut cmd = Cli::command();
    localize_clap_help(&mut cmd, lang);
    let localized_cmd = cmd.clone();

    let matches = match cmd.try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            print_localized_clap_error(&e, lang, &localized_cmd);
            std::process::exit(2);
        }
    };
    let cli = Cli::from_arg_matches(&matches)?;

    // Pre-dispatch update policy check (skip for update commands themselves)
    let is_update_cmd = matches!(cli.command, Commands::Update(_));
    let policy_action = if is_update_cmd {
        None
    } else {
        tokio::time::timeout(
            coast_update::POLICY_CHECK_TIMEOUT,
            coast_update::enforce_update_policy(coast_update::POLICY_CHECK_TIMEOUT),
        )
        .await
        .ok()
    };

    // Block execution if policy requires it
    if let Some(coast_update::policy::PolicyAction::Required {
        current,
        minimum,
        message,
    }) = &policy_action
    {
        eprintln!(
            "{} {}",
            colored::Colorize::bold(colored::Colorize::red("error:")),
            coast_update::format_required_message(current, minimum, message)
        );
        std::process::exit(1);
    }

    // Auto-update: download and replace binaries before running the command
    if let Some(coast_update::policy::PolicyAction::AutoUpdate {
        current,
        latest,
        message: _,
    }) = &policy_action
    {
        if let Ok(latest_ver) = coast_update::version::parse_version(latest) {
            eprintln!(
                "{} Updating coast {} -> {} ...",
                colored::Colorize::bold(colored::Colorize::cyan("auto-update:")),
                current,
                latest
            );
            match coast_update::updater::download_release(
                &latest_ver,
                coast_update::DOWNLOAD_TIMEOUT,
            )
            .await
            {
                Ok(tarball) => match coast_update::updater::apply_update(&tarball) {
                    Ok(()) => {
                        // Restart daemon so it picks up the new binary
                        let _ = commands::daemon::restart_daemon_if_running().await;
                        eprintln!(
                            "{} coast updated to {}. Re-run your command.",
                            colored::Colorize::green("done:"),
                            latest
                        );
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!(
                            "{} Auto-update failed: {e}. Continuing with current version.",
                            colored::Colorize::yellow("warning:")
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "{} Auto-update failed: {e}. Continuing with current version.",
                        colored::Colorize::yellow("warning:")
                    );
                }
            }
        }
    }

    let result = dispatch(cli).await;

    // Post-command nudge message (only on success, only for nudge policy)
    if result.is_ok() {
        if let Some(coast_update::policy::PolicyAction::Nudge {
            current,
            latest,
            message,
        }) = &policy_action
        {
            eprintln!(
                "\n{}",
                colored::Colorize::dimmed(
                    coast_update::format_nudge_message(current, latest, message).as_str()
                )
            );
        }
    }

    if let Err(e) = result {
        eprintln!("{}: {:#}", colored::Colorize::red("error"), e);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parses_successfully() {
        // Verify the CLI definition is valid
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_build_subcommand() {
        let cli = Cli::try_parse_from(["coast", "build"]).unwrap();
        assert!(matches!(cli.command, Commands::Build(_)));
        assert!(cli.project.is_none());
    }

    #[test]
    fn test_cli_agent_shell_subcommand() {
        let cli = Cli::try_parse_from(["coast", "agent-shell", "dev-1", "ls"]).unwrap();
        if let Commands::AgentShell(args) = cli.command {
            assert_eq!(args.name, "dev-1");
        } else {
            panic!("Expected AgentShell command");
        }
    }

    #[test]
    fn test_cli_build_with_refresh() {
        let cli = Cli::try_parse_from(["coast", "build", "--refresh"]).unwrap();
        if let Commands::Build(args) = cli.command {
            assert!(args.refresh);
        } else {
            panic!("Expected Build command");
        }
    }

    #[test]
    fn test_cli_global_project_flag() {
        let cli = Cli::try_parse_from(["coast", "--project", "my-app", "ls"]).unwrap();
        assert_eq!(cli.project, Some("my-app".to_string()));
    }

    #[test]
    fn test_cli_run_subcommand() {
        let cli = Cli::try_parse_from(["coast", "run", "feature-oauth"]).unwrap();
        if let Commands::Run(args) = cli.command {
            assert_eq!(args.name, "feature-oauth");
            assert!(args.count.is_none());
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_cli_run_with_batch() {
        let cli = Cli::try_parse_from(["coast", "run", "dev-{n}", "--n", "5"]).unwrap();
        if let Commands::Run(args) = cli.command {
            assert_eq!(args.name, "dev-{n}");
            assert_eq!(args.count, Some(5));
        } else {
            panic!("Expected Run command");
        }
    }

    #[test]
    fn test_cli_checkout_none() {
        let cli = Cli::try_parse_from(["coast", "checkout", "--none"]).unwrap();
        if let Commands::Checkout(args) = cli.command {
            assert!(args.none);
            assert!(args.name.is_none());
        } else {
            panic!("Expected Checkout command");
        }
    }

    #[test]
    fn test_cli_assign_subcommand() {
        let cli = Cli::try_parse_from(["coast", "assign", "dev-1", "--worktree", "feature/oauth"])
            .unwrap();
        if let Commands::Assign(args) = cli.command {
            assert_eq!(args.name, "dev-1");
            assert_eq!(args.worktree.as_deref(), Some("feature/oauth"));
        } else {
            panic!("Expected Assign command");
        }
    }

    #[test]
    fn test_cli_unassign_subcommand() {
        let cli = Cli::try_parse_from(["coast", "unassign", "dev-1"]).unwrap();
        if let Commands::Unassign(args) = cli.command {
            assert_eq!(args.name, "dev-1");
        } else {
            panic!("Expected Unassign command");
        }
    }

    #[test]
    fn test_cli_assign_no_worktree_allowed() {
        // --worktree is now optional at the clap level; the execute function
        // will return an error if not provided.
        let result = Cli::try_parse_from(["coast", "assign", "dev-1"]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cli_docker_subcommand() {
        let cli = Cli::try_parse_from(["coast", "docker", "dev-1", "ps"]).unwrap();
        if let Commands::Docker(args) = cli.command {
            assert_eq!(args.name, "dev-1");
            assert_eq!(args.command, vec!["ps"]);
        } else {
            panic!("Expected Docker command");
        }
    }

    #[test]
    fn test_cli_docker_compose_passthrough() {
        let cli =
            Cli::try_parse_from(["coast", "docker", "dev-1", "compose", "logs", "-f"]).unwrap();
        if let Commands::Docker(args) = cli.command {
            assert_eq!(args.name, "dev-1");
            assert_eq!(args.command, vec!["compose", "logs", "-f"]);
        } else {
            panic!("Expected Docker command");
        }
    }

    #[test]
    fn test_cli_missing_required_arg() {
        let result = Cli::try_parse_from(["coast", "run"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_stop_all() {
        let cli = Cli::try_parse_from(["coast", "stop", "--all"]).unwrap();
        if let Commands::Stop(args) = cli.command {
            assert!(args.all);
            assert!(args.name.is_none());
        } else {
            panic!("Expected Stop command");
        }
    }

    #[test]
    fn test_cli_start_all() {
        let cli = Cli::try_parse_from(["coast", "start", "--all"]).unwrap();
        if let Commands::Start(args) = cli.command {
            assert!(args.all);
            assert!(args.name.is_none());
        } else {
            panic!("Expected Start command");
        }
    }

    #[test]
    fn test_cli_rm_all() {
        let cli = Cli::try_parse_from(["coast", "rm", "--all"]).unwrap();
        if let Commands::Rm(args) = cli.command {
            assert!(args.all);
            assert!(args.name.is_none());
        } else {
            panic!("Expected Rm command");
        }
    }

    #[test]
    fn test_cli_docs_subcommand() {
        let cli = Cli::try_parse_from(["coast", "docs", "--path", "coastfiles"]).unwrap();
        if let Commands::Docs(args) = cli.command {
            assert_eq!(args.path.as_deref(), Some("coastfiles"));
        } else {
            panic!("Expected Docs command");
        }
    }

    #[test]
    fn test_cli_installation_prompt_subcommand() {
        let cli = Cli::try_parse_from(["coast", "installation-prompt"]).unwrap();
        assert!(matches!(cli.command, Commands::InstallationPrompt(_)));
    }

    #[test]
    fn test_cli_skills_prompt_subcommand() {
        let cli = Cli::try_parse_from(["coast", "skills-prompt"]).unwrap();
        assert!(matches!(cli.command, Commands::SkillsPrompt(_)));
    }

    #[test]
    fn test_cli_search_docs_subcommand() {
        let cli = Cli::try_parse_from(["coast", "search-docs", "shared", "services"]).unwrap();
        if let Commands::SearchDocs(args) = cli.command {
            assert_eq!(args.query, vec!["shared", "services"]);
        } else {
            panic!("Expected SearchDocs command");
        }
    }

    #[test]
    fn test_cli_daemon_status() {
        let cli = Cli::try_parse_from(["coast", "daemon", "status"]).unwrap();
        assert!(matches!(cli.command, Commands::Daemon(_)));
    }

    #[test]
    fn test_cli_daemon_kill() {
        let cli = Cli::try_parse_from(["coast", "daemon", "kill"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Kill { force: false }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_kill_force() {
        let cli = Cli::try_parse_from(["coast", "daemon", "kill", "--force"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Kill { force: true }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_kill_force_short() {
        let cli = Cli::try_parse_from(["coast", "daemon", "kill", "-f"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Kill { force: true }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_start() {
        let cli = Cli::try_parse_from(["coast", "daemon", "start"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(args.action, commands::daemon::DaemonAction::Start));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_restart() {
        let cli = Cli::try_parse_from(["coast", "daemon", "restart"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Restart { force: false }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_restart_force() {
        let cli = Cli::try_parse_from(["coast", "daemon", "restart", "--force"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Restart { force: true }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_logs() {
        let cli = Cli::try_parse_from(["coast", "daemon", "logs"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Logs { tail: false }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_logs_tail() {
        let cli = Cli::try_parse_from(["coast", "daemon", "logs", "--tail"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Logs { tail: true }
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_install() {
        let cli = Cli::try_parse_from(["coast", "daemon", "install"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Install
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_uninstall() {
        let cli = Cli::try_parse_from(["coast", "daemon", "uninstall"]).unwrap();
        if let Commands::Daemon(args) = cli.command {
            assert!(matches!(
                args.action,
                commands::daemon::DaemonAction::Uninstall
            ));
        } else {
            panic!("Expected Daemon command");
        }
    }

    #[test]
    fn test_cli_daemon_missing_subcommand() {
        let result = Cli::try_parse_from(["coast", "daemon"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_unknown_subcommand() {
        let result = Cli::try_parse_from(["coast", "unknown"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_project_with_explicit() {
        let result = resolve_project(&Some("my-app".to_string())).unwrap();
        assert_eq!(result, "my-app");
    }

    fn write_minimal_coastfile(dir: &std::path::Path, name: &str) {
        std::fs::write(
            dir.join("Coastfile"),
            format!("[coast]\nname = \"{name}\"\nruntime = \"dind\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn test_resolve_project_from_cwd_with_coastfile() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_coastfile(tmp.path(), "test-proj");
        let result = resolve_project_from(tmp.path()).unwrap();
        assert_eq!(result, "test-proj");
    }

    #[test]
    fn test_resolve_project_from_parent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_coastfile(tmp.path(), "parent-proj");
        let child = tmp.path().join("src").join("api");
        std::fs::create_dir_all(&child).unwrap();
        let result = resolve_project_from(&child).unwrap();
        assert_eq!(result, "parent-proj");
    }

    #[test]
    fn test_resolve_project_from_grandparent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_coastfile(tmp.path(), "deep-proj");
        let deep = tmp.path().join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&deep).unwrap();
        let result = resolve_project_from(&deep).unwrap();
        assert_eq!(result, "deep-proj");
    }

    #[test]
    fn test_resolve_project_from_no_coastfile_anywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("no").join("coastfile").join("here");
        std::fs::create_dir_all(&child).unwrap();
        let result = resolve_project_from(&child);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Coastfile"),
            "Error should mention Coastfile, got: {err}"
        );
    }

    #[test]
    fn test_resolve_project_from_nearest_coastfile_wins() {
        let tmp = tempfile::tempdir().unwrap();
        write_minimal_coastfile(tmp.path(), "outer");
        let inner = tmp.path().join("subproject");
        std::fs::create_dir_all(&inner).unwrap();
        write_minimal_coastfile(&inner, "inner");
        let deep = inner.join("src");
        std::fs::create_dir_all(&deep).unwrap();
        let result = resolve_project_from(&deep).unwrap();
        assert_eq!(result, "inner");
    }

    #[test]
    fn test_resolve_project_explicit_overrides_walk_up() {
        let result = resolve_project(&Some("override-proj".to_string())).unwrap();
        assert_eq!(result, "override-proj");
    }

    #[test]
    fn test_localize_clap_help_translates_about_es() {
        let mut cmd = Cli::command();
        localize_clap_help(&mut cmd, "es");
        let build_sub = cmd.find_subcommand("build").unwrap();
        let about = build_sub.get_about().unwrap().to_string();
        assert_ne!(about, "Build a coast image from a Coastfile");
        assert!(!about.is_empty());
    }

    #[test]
    fn test_daemon_subcommand_tree_depth() {
        let mut cmd = Cli::command();
        cmd.build();
        let daemon = cmd.find_subcommand("daemon").unwrap();
        let daemon_subs: Vec<String> = daemon
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        eprintln!("daemon subs: {daemon_subs:?}");
        assert!(
            daemon_subs.contains(&"status".to_string()),
            "daemon should have 'status' subcommand after build(), got: {daemon_subs:?}"
        );
    }

    #[test]
    fn test_localize_nested_daemon_subcommands() {
        let mut cmd = Cli::command();
        localize_clap_help(&mut cmd, "zh");

        // Check the original after localization
        let daemon = cmd.find_subcommand("daemon").unwrap();
        eprintln!(
            "daemon after localize has {} subs",
            daemon.get_subcommands().count()
        );
        for sub in daemon.get_subcommands() {
            let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
            eprintln!("  {} => {}", sub.get_name(), about);
        }
        let status = daemon.find_subcommand("status");
        assert!(
            status.is_some(),
            "daemon should still have 'status' after localize"
        );
        let status_about = status.unwrap().get_about().unwrap().to_string();
        assert_ne!(
            status_about, "Check if the daemon is running",
            "status.about should be translated to zh"
        );

        // Check the clone preserves translations
        cmd.build();
        let cloned = cmd.clone();
        let daemon_cloned = cloned.find_subcommand("daemon").unwrap();
        eprintln!(
            "daemon CLONED has {} subs",
            daemon_cloned.get_subcommands().count()
        );
        for sub in daemon_cloned.get_subcommands() {
            let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
            eprintln!("  [clone] {} => {}", sub.get_name(), about);
        }
        let status_clone = daemon_cloned.find_subcommand("status").unwrap();
        let status_clone_about = status_clone.get_about().unwrap().to_string();
        assert_ne!(
            status_clone_about, "Check if the daemon is running",
            "clone should preserve translations"
        );
    }

    #[test]
    fn test_localize_clap_help_preserves_english() {
        let mut cmd = Cli::command();
        localize_clap_help(&mut cmd, "en");
        let build_sub = cmd.find_subcommand("build").unwrap();
        let about = build_sub.get_about().unwrap().to_string();
        assert_eq!(about, "Build a coast image from a Coastfile");
    }

    #[test]
    fn test_cli_update_check() {
        let cli = Cli::try_parse_from(["coast", "update", "check"]).unwrap();
        assert!(matches!(cli.command, Commands::Update(_)));
    }

    #[test]
    fn test_cli_update_apply() {
        let cli = Cli::try_parse_from(["coast", "update", "apply"]).unwrap();
        assert!(matches!(cli.command, Commands::Update(_)));
    }

    #[test]
    fn test_cli_update_missing_subcommand() {
        let result = Cli::try_parse_from(["coast", "update"]);
        assert!(result.is_err());
    }
}
