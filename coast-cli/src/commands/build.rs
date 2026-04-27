/// `coast build` command — build a coast image from a Coastfile.
///
/// Parses the Coastfile, caches images, extracts secrets, and creates
/// the coast image artifact at `~/.coast/images/{project}/`.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use coast_core::protocol::{BuildProgressEvent, BuildRequest, Request, Response};

/// Arguments for `coast build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Path to the Coastfile (default: ./Coastfile).
    /// Mutually exclusive with --type.
    #[arg(short = 'f', long = "file", default_value = "Coastfile")]
    pub coastfile_path: PathBuf,

    /// Build a typed Coastfile variant (e.g. --type light -> Coastfile.light).
    /// Mutually exclusive with -f/--file when a non-default path is given.
    #[arg(short = 't', long = "type")]
    pub coastfile_type: Option<String>,

    /// Re-extract secrets and re-pull images even if cached.
    #[arg(long)]
    pub refresh: bool,

    /// Which remote to build on (only used with --type remote).
    #[arg(long)]
    pub remote: Option<String>,

    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose build detail (e.g., docker build logs).
    #[arg(short = 'v', long)]
    pub verbose: bool,

    // --- Coastfile-less build flags ---
    /// Project name (required when building without a Coastfile).
    #[arg(long = "name")]
    pub project_name: Option<String>,

    /// Path to docker-compose file (required when building without a Coastfile).
    #[arg(long)]
    pub compose: Option<PathBuf>,

    /// Inline docker-compose content (alternative to --compose).
    #[arg(long = "compose-content", conflicts_with = "compose")]
    pub compose_content: Option<String>,

    /// Container runtime: dind, sysbox, or podman.
    #[arg(long)]
    pub runtime: Option<String>,

    /// Port mapping (repeatable). Format: NAME=PORT (e.g. --port web=3000).
    #[arg(long = "port", value_name = "NAME=PORT")]
    pub ports: Vec<String>,

    /// Disable auto-start of compose services during `coast run`.
    #[arg(long)]
    pub no_autostart: bool,

    /// Primary port service name.
    #[arg(long = "primary-port")]
    pub primary_port: Option<String>,

    /// Inline TOML config for complex sections (secrets, volumes, etc.).
    /// Merged on top of Coastfile and individual flags.
    #[arg(long)]
    pub config: Option<String>,
}

/// Verbosity level for progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verbosity {
    Silent,
    Default,
    Verbose,
}

// ---------------------------------------------------------------------------
// Interactive build display — renders a live-updating checklist to stderr
// using ANSI cursor movement. Falls back to simple linear output when stderr
// is not a TTY.
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone, Copy)]
enum StepStatus {
    Pending,
    InProgress,
    Ok,
    Warn,
    Fail,
    Skip,
}

struct DisplayItem {
    detail: String,
    icon: String,
    verbose: Option<String>,
}

struct DisplayStep {
    name: String,
    number: u32,
    total: u32,
    status: StepStatus,
    items: Vec<DisplayItem>,
}

pub(crate) struct ProgressDisplay {
    steps: Vec<DisplayStep>,
    lines_rendered: usize,
    interactive: bool,
    verbosity: Verbosity,
}

impl ProgressDisplay {
    pub(crate) fn new(verbosity: Verbosity) -> Self {
        Self {
            steps: Vec::new(),
            lines_rendered: 0,
            interactive: std::io::stderr().is_terminal(),
            verbosity,
        }
    }

    pub(crate) fn handle_event(&mut self, event: &BuildProgressEvent) {
        if self.verbosity == Verbosity::Silent {
            return;
        }

        match event.status.as_str() {
            "plan" => {
                if let Some(ref names) = event.plan {
                    let total = names.len() as u32;
                    self.steps = names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| DisplayStep {
                            name: name.clone(),
                            number: (i + 1) as u32,
                            total,
                            status: StepStatus::Pending,
                            items: Vec::new(),
                        })
                        .collect();
                    if self.interactive {
                        self.render();
                    }
                }
            }
            "started" => {
                if event.detail.is_some() {
                    return;
                }
                for step in &mut self.steps {
                    if step.status == StepStatus::InProgress {
                        step.status = StepStatus::Ok;
                    }
                }
                if let Some(step) = self.steps.iter_mut().find(|s| s.name == event.step) {
                    step.status = StepStatus::InProgress;
                }
                if self.interactive && !self.steps.is_empty() {
                    self.render();
                } else {
                    self.linear_started(event);
                }
            }
            "ok" | "warn" | "fail" | "skip" => {
                let status = match event.status.as_str() {
                    "ok" => StepStatus::Ok,
                    "warn" => StepStatus::Warn,
                    "fail" => StepStatus::Fail,
                    "skip" => StepStatus::Skip,
                    _ => StepStatus::Ok,
                };

                if let Some(ref detail) = event.detail {
                    let icon = match event.status.as_str() {
                        "ok" => "✓".green().to_string(),
                        "warn" => "⚠".yellow().to_string(),
                        "fail" => "✗".red().to_string(),
                        "skip" => "–".dimmed().to_string(),
                        _ => event.status.clone(),
                    };
                    if let Some(step) = self.steps.iter_mut().find(|s| s.name == event.step) {
                        step.items.push(DisplayItem {
                            detail: detail.clone(),
                            icon,
                            verbose: event.verbose_detail.clone(),
                        });
                    }
                } else if let Some(step) = self.steps.iter_mut().find(|s| s.name == event.step) {
                    step.status = status;
                }

                if self.interactive && !self.steps.is_empty() {
                    self.render();
                } else {
                    self.linear_result(event);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn finalize(&mut self) {
        for step in &mut self.steps {
            if step.status == StepStatus::InProgress {
                step.status = StepStatus::Ok;
            }
        }
        if self.interactive && !self.steps.is_empty() {
            self.render();
            self.print_detail_log();
        }
    }

    // ---- Interactive (TTY) renderer ----
    //
    // Fixed-size block: exactly one line per step. Sub-items are shown
    // inline on the step's line (latest item while in-progress, count
    // summary when done). The block never grows, so cursor-up always
    // covers the same N lines and nothing leaks into scrollback.

    fn render(&mut self) {
        let mut buf: Vec<u8> = Vec::with_capacity(2048);

        if self.lines_rendered > 0 {
            write!(&mut buf, "\x1b[{}F", self.lines_rendered).unwrap();
        }

        let lines = self.steps.len();
        for step in &self.steps {
            let icon = match step.status {
                StepStatus::Pending => "○".dimmed().to_string(),
                StepStatus::InProgress => "●".cyan().to_string(),
                StepStatus::Ok => "✓".green().to_string(),
                StepStatus::Warn => "⚠".yellow().to_string(),
                StepStatus::Fail => "✗".red().to_string(),
                StepStatus::Skip => "–".dimmed().to_string(),
            };

            let label = format!("[{}/{}]", step.number, step.total).dimmed();

            let name = match step.status {
                StepStatus::Pending | StepStatus::Skip => step.name.dimmed().to_string(),
                StepStatus::Warn => step.name.yellow().to_string(),
                StepStatus::Fail => step.name.red().to_string(),
                _ => step.name.clone(),
            };

            let suffix = self.step_suffix(step);

            writeln!(&mut buf, "\x1b[2K  {} {} {}{}", icon, label, name, suffix).unwrap();
        }

        write!(&mut buf, "\x1b[J").unwrap();

        let mut stderr = std::io::stderr().lock();
        stderr.write_all(&buf).ok();
        stderr.flush().ok();

        self.lines_rendered = lines;
    }

    /// Build the inline suffix for a step line.
    ///
    /// - In-progress with items: show the latest item detail and its icon.
    /// - Completed with items: show a count summary like "(3 ok, 1 warn)".
    /// - Otherwise: empty.
    fn step_suffix(&self, step: &DisplayStep) -> String {
        if step.items.is_empty() {
            return String::new();
        }

        match step.status {
            StepStatus::InProgress => {
                if let Some(last) = step.items.last() {
                    format!(" {} {}", format!("— {}", last.detail).dimmed(), last.icon)
                } else {
                    String::new()
                }
            }
            StepStatus::Ok | StepStatus::Warn | StepStatus::Fail => {
                let mut ok = 0usize;
                let mut warn = 0usize;
                let mut fail = 0usize;
                for item in &step.items {
                    match item.icon.as_str() {
                        i if i.contains('✓') => ok += 1,
                        i if i.contains('⚠') => warn += 1,
                        i if i.contains('✗') => fail += 1,
                        _ => ok += 1,
                    }
                }
                let mut parts = Vec::new();
                if ok > 0 {
                    parts.push(format!("{ok} ok"));
                }
                if warn > 0 {
                    parts.push(format!("{warn} warn"));
                }
                if fail > 0 {
                    parts.push(format!("{fail} fail"));
                }
                format!(" {}", format!("({})", parts.join(", ")).dimmed())
            }
            _ => String::new(),
        }
    }

    /// Print detailed sub-item log below the fixed block after build completes.
    fn print_detail_log(&self) {
        let steps_with_items: Vec<_> = self.steps.iter().filter(|s| !s.items.is_empty()).collect();

        if steps_with_items.is_empty() {
            return;
        }

        let mut stderr = std::io::stderr().lock();
        writeln!(stderr).ok();
        for step in steps_with_items {
            writeln!(stderr, "  {}:", step.name.bold()).ok();
            for item in &step.items {
                writeln!(stderr, "    {}  {}", item.detail, item.icon).ok();
                if self.verbosity == Verbosity::Verbose {
                    if let Some(ref v) = item.verbose {
                        for vline in v.lines() {
                            writeln!(stderr, "      {}", vline.dimmed()).ok();
                        }
                    }
                }
            }
        }
        stderr.flush().ok();
    }

    // ---- Linear (non-TTY) fallback — same output as before ----

    fn linear_started(&self, event: &BuildProgressEvent) {
        if let (Some(n), Some(t)) = (event.step_number, event.total_steps) {
            eprint!("  {} {}...", format!("[{}/{}]", n, t).dimmed(), event.step);
        } else {
            eprint!("  {}...", event.step);
        }
    }

    fn linear_result(&self, event: &BuildProgressEvent) {
        let icon = match event.status.as_str() {
            "ok" => "ok".green().to_string(),
            "warn" => "warn".yellow().to_string(),
            "fail" => "FAIL".red().to_string(),
            "skip" => "skip".dimmed().to_string(),
            _ => event.status.clone(),
        };

        if let Some(ref detail) = event.detail {
            eprintln!("    {}  {}", detail, icon);
        } else {
            eprintln!("  {}", icon);
        }

        if self.verbosity == Verbosity::Verbose {
            if let Some(ref verbose_detail) = event.verbose_detail {
                for line in verbose_detail.lines() {
                    eprintln!("      {}", line.dimmed());
                }
            }
        }
    }
}

/// Validate a user-supplied `--type <coastfile_type>` argument.
///
/// Rejects:
/// - `default` / `toml` — reserved degenerate names.
/// - SSG variants (`shared_service_groups`) — built via `coast ssg build`,
///   not through this command. See `coast-ssg/DESIGN.md`.
///
/// Remote variants (`remote*`) are intentionally NOT rejected here:
/// `coast build --type remote.foo` is a valid invocation that the
/// downstream pipeline handles. Only SSG is fundamentally incompatible
/// with `coast build`.
pub(crate) fn validate_coastfile_type_arg(t: &str) -> Result<()> {
    if t == "default" {
        bail!(
            "'--type default' is not allowed. \
             The base 'Coastfile' is the default type. \
             Run 'coast build' without --type."
        );
    }
    if t == "toml" {
        bail!(
            "'--type toml' is not allowed. \
             'toml' is a reserved name. Use 'Coastfile.toml' for the default type \
             with syntax highlighting, or choose a different type name."
        );
    }
    if coast_core::coastfile::Coastfile::is_ssg_type(Some(t)) {
        bail!(
            "'--type {t}' is a Shared Service Group variant; \
             build it with 'coast ssg build' instead of 'coast build --type {t}'. \
             SSGs are a separate build product (see coast-ssg/DESIGN.md)."
        );
    }
    Ok(())
}

/// Execute the `coast build` command.
///
/// The project name is derived from the Coastfile, not from the `--project` flag,
/// since the Coastfile itself defines the project name.
pub async fn execute(args: &BuildArgs, global_working_dir: &Option<PathBuf>) -> Result<()> {
    if let Some(ref t) = args.coastfile_type {
        validate_coastfile_type_arg(t.as_str())?;
    }

    let has_inline_flags = args.project_name.is_some()
        || args.compose.is_some()
        || args.compose_content.is_some()
        || !args.ports.is_empty()
        || args.runtime.is_some()
        || args.no_autostart
        || args.primary_port.is_some()
        || args.config.is_some();

    let cwd = std::env::current_dir()?;
    let has_custom_file = args.coastfile_path != Path::new("Coastfile");

    let coastfile_on_disk = if has_custom_file {
        true
    } else {
        coast_core::coastfile::Coastfile::find_coastfile(&cwd, "Coastfile").is_some()
    };

    let (coastfile_path, coastfile_content) = if has_inline_flags && !coastfile_on_disk {
        args.project_name.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "--name is required when building without a Coastfile. \
                 Provide a project name with --name <NAME>."
            )
        })?;
        let toml_content = build_toml_from_flags(args, &cwd)?;
        (cwd.join("Coastfile"), Some(toml_content))
    } else if has_inline_flags && coastfile_on_disk {
        let base_path = resolve_coastfile_path(args, &cwd)?;
        let base_content = std::fs::read_to_string(&base_path)?;
        let merged = merge_flags_into_toml(&base_content, args, &cwd)?;
        (base_path, Some(merged))
    } else if !coastfile_on_disk && !has_custom_file {
        bail!(
            "No Coastfile found in the current directory.\n\
             Either create a Coastfile, or build without one using:\n  \
             coast build --name <NAME> [--compose <PATH>] [--port NAME=PORT ...]"
        );
    } else {
        (resolve_coastfile_path(args, &cwd)?, None)
    };

    let working_dir = global_working_dir.as_ref().map(|wd| {
        if wd.is_absolute() {
            wd.clone()
        } else {
            cwd.join(wd)
        }
    });

    let request = Request::Build(BuildRequest {
        coastfile_path,
        refresh: args.refresh,
        remote: args.remote.clone(),
        coastfile_content,
        working_dir,
    });

    let verbosity = if args.silent {
        Verbosity::Silent
    } else if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Default
    };

    let mut display = ProgressDisplay::new(verbosity);

    let response = super::send_build_request(request, |event| {
        display.handle_event(event);
    })
    .await?;

    display.finalize();

    match response {
        Response::Build(resp) => {
            if verbosity != Verbosity::Silent {
                eprintln!();
            }
            println!(
                "{} {}",
                "ok".green().bold(),
                t!("cli.ok.build_complete", project = resp.project),
            );
            println!("   Artifact: {}", resp.artifact_path.display());
            println!(
                "   Images: {} cached, {} built",
                resp.images_cached, resp.images_built
            );
            println!("   Secrets: {} extracted", resp.secrets_extracted);
            if let Some(ref coast_image) = resp.coast_image {
                println!("   Coast image: {}", coast_image);
            }

            for warning in &resp.warnings {
                println!("   {}: {}", "warning".yellow().bold(), warning);
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

/// Resolve the coastfile path from args when NOT using inline flags.
fn resolve_coastfile_path(args: &BuildArgs, cwd: &Path) -> anyhow::Result<PathBuf> {
    if let Some(ref t) = args.coastfile_type {
        let has_custom_file = args.coastfile_path != Path::new("Coastfile");
        if has_custom_file {
            bail!(
                "--type and -f/--file are mutually exclusive. \
                 Use --type to pick a variant (e.g. Coastfile.light), \
                 or -f to specify an explicit path."
            );
        }
        Ok(
            coast_core::coastfile::Coastfile::find_coastfile_for_type(cwd, Some(t))
                .unwrap_or_else(|| cwd.join(format!("Coastfile.{t}"))),
        )
    } else if args.coastfile_path == Path::new("Coastfile") {
        Ok(
            coast_core::coastfile::Coastfile::find_coastfile(cwd, "Coastfile")
                .unwrap_or_else(|| cwd.join("Coastfile")),
        )
    } else if args.coastfile_path.is_absolute() {
        Ok(args.coastfile_path.clone())
    } else {
        Ok(cwd.join(&args.coastfile_path))
    }
}

/// Construct a full TOML string from CLI flags (coastfile-less build).
fn build_toml_from_flags(args: &BuildArgs, cwd: &Path) -> anyhow::Result<String> {
    let mut toml = String::new();

    toml.push_str("[coast]\n");
    if let Some(ref name) = args.project_name {
        toml.push_str(&format!("name = \"{}\"\n", escape_toml_string(name)));
    }
    if let Some(ref compose) = args.compose {
        let compose_path = if compose.is_absolute() {
            compose.clone()
        } else {
            cwd.join(compose)
        };
        toml.push_str(&format!(
            "compose = \"{}\"\n",
            escape_toml_string(&compose_path.display().to_string())
        ));
    }
    if let Some(ref runtime) = args.runtime {
        toml.push_str(&format!("runtime = \"{}\"\n", escape_toml_string(runtime)));
    }
    if args.no_autostart {
        toml.push_str("autostart = false\n");
    }
    if let Some(ref primary) = args.primary_port {
        toml.push_str(&format!(
            "primary_port = \"{}\"\n",
            escape_toml_string(primary)
        ));
    }
    toml.push('\n');

    if !args.ports.is_empty() {
        toml.push_str("[ports]\n");
        for port_spec in &args.ports {
            let (name, port) = parse_port_spec(port_spec)?;
            toml.push_str(&format!("{name} = {port}\n"));
        }
        toml.push('\n');
    }

    if let Some(ref config) = args.config {
        toml.push_str(config);
        toml.push('\n');
    }

    Ok(toml)
}

/// Merge CLI flag overrides into existing Coastfile TOML content.
fn merge_flags_into_toml(
    base_content: &str,
    args: &BuildArgs,
    cwd: &Path,
) -> anyhow::Result<String> {
    let mut overlay = String::new();

    let has_coast_overrides = args.project_name.is_some()
        || args.compose.is_some()
        || args.runtime.is_some()
        || args.no_autostart
        || args.primary_port.is_some();

    if has_coast_overrides {
        overlay.push_str("[coast]\n");
        if let Some(ref name) = args.project_name {
            overlay.push_str(&format!("name = \"{}\"\n", escape_toml_string(name)));
        }
        if let Some(ref compose) = args.compose {
            let compose_path = if compose.is_absolute() {
                compose.clone()
            } else {
                cwd.join(compose)
            };
            overlay.push_str(&format!(
                "compose = \"{}\"\n",
                escape_toml_string(&compose_path.display().to_string())
            ));
        }
        if let Some(ref runtime) = args.runtime {
            overlay.push_str(&format!("runtime = \"{}\"\n", escape_toml_string(runtime)));
        }
        if args.no_autostart {
            overlay.push_str("autostart = false\n");
        }
        if let Some(ref primary) = args.primary_port {
            overlay.push_str(&format!(
                "primary_port = \"{}\"\n",
                escape_toml_string(primary)
            ));
        }
        overlay.push('\n');
    }

    if !args.ports.is_empty() {
        overlay.push_str("[ports]\n");
        for port_spec in &args.ports {
            let (name, port) = parse_port_spec(port_spec)?;
            overlay.push_str(&format!("{name} = {port}\n"));
        }
        overlay.push('\n');
    }

    if let Some(ref config) = args.config {
        overlay.push_str(config);
        overlay.push('\n');
    }

    if overlay.is_empty() {
        return Ok(base_content.to_string());
    }

    // Parse both as TOML tables and merge (overlay wins).
    let mut base_table: toml::Table = toml::from_str(base_content)?;
    let overlay_table: toml::Table = toml::from_str(&overlay)?;
    merge_toml_tables(&mut base_table, &overlay_table);
    Ok(toml::to_string_pretty(&base_table)?)
}

fn merge_toml_tables(base: &mut toml::Table, overlay: &toml::Table) {
    for (key, value) in overlay {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(base_sub)), toml::Value::Table(overlay_sub)) => {
                merge_toml_tables(base_sub, overlay_sub);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

fn parse_port_spec(spec: &str) -> anyhow::Result<(String, u16)> {
    let parts: Vec<&str> = spec.splitn(2, '=').collect();
    if parts.len() != 2 {
        bail!(
            "invalid port format '{}'. Expected NAME=PORT (e.g. web=3000)",
            spec
        );
    }
    let name = parts[0].trim().to_string();
    let port: u16 = parts[1].trim().parse().map_err(|_| {
        anyhow::anyhow!(
            "invalid port number '{}' in '{}'. Port must be 1-65535",
            parts[1].trim(),
            spec
        )
    })?;
    if port == 0 {
        bail!("port 0 is not allowed in '{}'", spec);
    }
    Ok((name, port))
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Wrapper to test BuildArgs parsing.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: BuildArgs,
    }

    #[test]
    fn test_build_args_defaults() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert_eq!(cli.args.coastfile_path, PathBuf::from("Coastfile"));
        assert!(!cli.args.refresh);
        assert!(!cli.args.silent);
        assert!(!cli.args.verbose);
    }

    #[test]
    fn test_build_args_refresh() {
        let cli = TestCli::try_parse_from(["test", "--refresh"]).unwrap();
        assert!(cli.args.refresh);
    }

    #[test]
    fn test_build_args_silent() {
        let cli = TestCli::try_parse_from(["test", "--silent"]).unwrap();
        assert!(cli.args.silent);
    }

    #[test]
    fn test_build_args_silent_short() {
        let cli = TestCli::try_parse_from(["test", "-s"]).unwrap();
        assert!(cli.args.silent);
    }

    #[test]
    fn test_build_args_verbose() {
        let cli = TestCli::try_parse_from(["test", "--verbose"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_build_args_verbose_short() {
        let cli = TestCli::try_parse_from(["test", "-v"]).unwrap();
        assert!(cli.args.verbose);
    }

    #[test]
    fn test_build_args_custom_file() {
        let cli = TestCli::try_parse_from(["test", "-f", "/path/to/Coastfile"]).unwrap();
        assert_eq!(cli.args.coastfile_path, PathBuf::from("/path/to/Coastfile"));
    }

    #[test]
    fn test_build_args_long_file() {
        let cli = TestCli::try_parse_from(["test", "--file", "my/Coastfile"]).unwrap();
        assert_eq!(cli.args.coastfile_path, PathBuf::from("my/Coastfile"));
    }

    // -------------------------------------------------------------------
    // validate_coastfile_type_arg tests
    // -------------------------------------------------------------------

    #[test]
    fn test_validate_coastfile_type_arg_rejects_default() {
        let err = validate_coastfile_type_arg("default").unwrap_err();
        assert!(err.to_string().contains("'--type default' is not allowed"));
    }

    #[test]
    fn test_validate_coastfile_type_arg_rejects_toml() {
        let err = validate_coastfile_type_arg("toml").unwrap_err();
        assert!(err.to_string().contains("'--type toml' is not allowed"));
    }

    #[test]
    fn test_validate_coastfile_type_arg_rejects_shared_service_groups() {
        let err = validate_coastfile_type_arg("shared_service_groups").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Shared Service Group"),
            "expected SSG mention, got: {msg}"
        );
        assert!(
            msg.contains("coast ssg build"),
            "expected pointer to 'coast ssg build', got: {msg}"
        );
    }

    #[test]
    fn test_validate_coastfile_type_arg_accepts_user_variants() {
        assert!(validate_coastfile_type_arg("light").is_ok());
        assert!(validate_coastfile_type_arg("e2e").is_ok());
        // Remote variants pass through `validate_coastfile_type_arg` —
        // they're handled downstream by the remote-build pipeline.
        assert!(validate_coastfile_type_arg("remote").is_ok());
        assert!(validate_coastfile_type_arg("remote.light").is_ok());
    }

    // -------------------------------------------------------------------
    // ProgressDisplay state-machine tests
    // -------------------------------------------------------------------

    fn make_event(status: &str, step: &str) -> BuildProgressEvent {
        BuildProgressEvent {
            step: step.to_string(),
            detail: None,
            status: status.to_string(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        }
    }

    fn make_plan_event(steps: Vec<&str>) -> BuildProgressEvent {
        BuildProgressEvent {
            step: String::new(),
            detail: None,
            status: "plan".to_string(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: Some(steps.iter().map(|s| s.to_string()).collect()),
        }
    }

    #[test]
    fn test_progress_display_new_initial_state() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        assert!(pd.steps.is_empty());
        assert_eq!(pd.lines_rendered, 0);
        assert_eq!(pd.verbosity, Verbosity::Default);
    }

    #[test]
    fn test_progress_display_new_silent() {
        let pd = ProgressDisplay::new(Verbosity::Silent);
        assert!(pd.steps.is_empty());
        assert_eq!(pd.verbosity, Verbosity::Silent);
    }

    #[test]
    fn test_handle_event_plan_sets_steps() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec![
            "Pull images",
            "Extract secrets",
            "Build artifact",
        ]));

        assert_eq!(pd.steps.len(), 3);
        assert_eq!(pd.steps[0].name, "Pull images");
        assert_eq!(pd.steps[0].number, 1);
        assert_eq!(pd.steps[0].total, 3);
        assert_eq!(pd.steps[0].status, StepStatus::Pending);
        assert_eq!(pd.steps[1].name, "Extract secrets");
        assert_eq!(pd.steps[1].number, 2);
        assert_eq!(pd.steps[2].name, "Build artifact");
        assert_eq!(pd.steps[2].number, 3);
        assert_eq!(pd.steps[2].total, 3);
        for step in &pd.steps {
            assert_eq!(step.status, StepStatus::Pending);
            assert!(step.items.is_empty());
        }
    }

    #[test]
    fn test_handle_event_plan_without_plan_field_is_noop() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        let event = BuildProgressEvent {
            step: String::new(),
            detail: None,
            status: "plan".to_string(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        };
        pd.handle_event(&event);
        assert!(pd.steps.is_empty());
    }

    #[test]
    fn test_handle_event_started_sets_in_progress() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A", "Step B"]));
        pd.handle_event(&make_event("started", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::InProgress);
        assert_eq!(pd.steps[1].status, StepStatus::Pending);
    }

    #[test]
    fn test_handle_event_started_auto_completes_previous() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A", "Step B"]));
        pd.handle_event(&make_event("started", "Step A"));
        pd.handle_event(&make_event("started", "Step B"));

        assert_eq!(pd.steps[0].status, StepStatus::Ok);
        assert_eq!(pd.steps[1].status, StepStatus::InProgress);
    }

    #[test]
    fn test_handle_event_started_with_detail_is_ignored() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        let event = BuildProgressEvent {
            step: "Step A".to_string(),
            detail: Some("sub-item".to_string()),
            status: "started".to_string(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        };
        pd.handle_event(&event);
        assert_eq!(pd.steps[0].status, StepStatus::Pending);
    }

    #[test]
    fn test_handle_event_ok_marks_complete() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        pd.handle_event(&make_event("started", "Step A"));
        pd.handle_event(&make_event("ok", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::Ok);
    }

    #[test]
    fn test_handle_event_fail_marks_failed() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        pd.handle_event(&make_event("started", "Step A"));
        pd.handle_event(&make_event("fail", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::Fail);
    }

    #[test]
    fn test_handle_event_skip_marks_skipped() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        pd.handle_event(&make_event("skip", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::Skip);
    }

    #[test]
    fn test_handle_event_warn_marks_warn() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        pd.handle_event(&make_event("started", "Step A"));
        pd.handle_event(&make_event("warn", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::Warn);
    }

    #[test]
    fn test_handle_event_detail_adds_item_without_changing_step_status() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Pull images"]));
        pd.handle_event(&make_event("started", "Pull images"));

        let event = BuildProgressEvent {
            step: "Pull images".to_string(),
            detail: Some("postgres:16".to_string()),
            status: "ok".to_string(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        };
        pd.handle_event(&event);

        assert_eq!(pd.steps[0].status, StepStatus::InProgress);
        assert_eq!(pd.steps[0].items.len(), 1);
        assert_eq!(pd.steps[0].items[0].detail, "postgres:16");
    }

    #[test]
    fn test_handle_event_multiple_detail_items() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Pull images"]));
        pd.handle_event(&make_event("started", "Pull images"));

        for image in &["postgres:16", "redis:7", "node:20"] {
            pd.handle_event(&BuildProgressEvent {
                step: "Pull images".to_string(),
                detail: Some(image.to_string()),
                status: "ok".to_string(),
                verbose_detail: None,
                step_number: None,
                total_steps: None,
                plan: None,
            });
        }

        assert_eq!(pd.steps[0].items.len(), 3);
        assert_eq!(pd.steps[0].items[0].detail, "postgres:16");
        assert_eq!(pd.steps[0].items[1].detail, "redis:7");
        assert_eq!(pd.steps[0].items[2].detail, "node:20");
    }

    #[test]
    fn test_handle_event_verbose_detail_stored_on_item() {
        let mut pd = ProgressDisplay::new(Verbosity::Verbose);
        pd.handle_event(&make_plan_event(vec!["Build images"]));
        pd.handle_event(&make_event("started", "Build images"));

        let event = BuildProgressEvent {
            step: "Build images".to_string(),
            detail: Some("frontend".to_string()),
            status: "ok".to_string(),
            verbose_detail: Some("Step 1/5 : FROM node:20\nStep 2/5 : COPY . .".to_string()),
            step_number: None,
            total_steps: None,
            plan: None,
        };
        pd.handle_event(&event);

        assert_eq!(pd.steps[0].items.len(), 1);
        assert_eq!(
            pd.steps[0].items[0].verbose.as_deref(),
            Some("Step 1/5 : FROM node:20\nStep 2/5 : COPY . .")
        );
    }

    #[test]
    fn test_handle_event_fail_detail_item() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Build images"]));
        pd.handle_event(&make_event("started", "Build images"));

        let event = BuildProgressEvent {
            step: "Build images".to_string(),
            detail: Some("broken-service".to_string()),
            status: "fail".to_string(),
            verbose_detail: Some("error: Dockerfile not found".to_string()),
            step_number: None,
            total_steps: None,
            plan: None,
        };
        pd.handle_event(&event);

        assert_eq!(pd.steps[0].items.len(), 1);
        assert!(pd.steps[0].items[0].icon.contains('✗'));
        assert_eq!(
            pd.steps[0].items[0].verbose.as_deref(),
            Some("error: Dockerfile not found")
        );
    }

    #[test]
    fn test_finalize_marks_in_progress_as_ok() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A", "Step B"]));
        pd.handle_event(&make_event("started", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::InProgress);
        pd.finalize();
        assert_eq!(pd.steps[0].status, StepStatus::Ok);
        assert_eq!(pd.steps[1].status, StepStatus::Pending);
    }

    #[test]
    fn test_finalize_preserves_completed_statuses() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A", "Step B", "Step C"]));
        pd.handle_event(&make_event("ok", "Step A"));
        pd.handle_event(&make_event("fail", "Step B"));
        pd.handle_event(&make_event("started", "Step C"));

        pd.finalize();

        assert_eq!(pd.steps[0].status, StepStatus::Ok);
        assert_eq!(pd.steps[1].status, StepStatus::Fail);
        assert_eq!(pd.steps[2].status, StepStatus::Ok);
    }

    #[test]
    fn test_finalize_no_panic_on_empty() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.finalize();
    }

    #[test]
    fn test_silent_mode_ignores_all_events() {
        let mut pd = ProgressDisplay::new(Verbosity::Silent);
        pd.handle_event(&make_plan_event(vec!["Step A", "Step B"]));
        pd.handle_event(&make_event("started", "Step A"));
        pd.handle_event(&make_event("ok", "Step A"));

        assert!(pd.steps.is_empty());
    }

    #[test]
    fn test_unknown_status_is_ignored() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);
        pd.handle_event(&make_plan_event(vec!["Step A"]));
        pd.handle_event(&make_event("unknown_status", "Step A"));

        assert_eq!(pd.steps[0].status, StepStatus::Pending);
    }

    // -------------------------------------------------------------------
    // step_suffix unit tests
    // -------------------------------------------------------------------

    #[test]
    fn test_step_suffix_empty_items_returns_empty() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "test".to_string(),
            number: 1,
            total: 1,
            status: StepStatus::Ok,
            items: Vec::new(),
        };
        assert_eq!(pd.step_suffix(&step), "");
    }

    #[test]
    fn test_step_suffix_in_progress_shows_latest_item() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "Pull images".to_string(),
            number: 1,
            total: 2,
            status: StepStatus::InProgress,
            items: vec![
                DisplayItem {
                    detail: "postgres:16".to_string(),
                    icon: "✓".to_string(),
                    verbose: None,
                },
                DisplayItem {
                    detail: "node:20".to_string(),
                    icon: "✓".to_string(),
                    verbose: None,
                },
            ],
        };
        let suffix = pd.step_suffix(&step);
        assert!(suffix.contains("node:20"));
        assert!(!suffix.contains("postgres:16"));
    }

    #[test]
    fn test_step_suffix_completed_shows_count_summary() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "Pull images".to_string(),
            number: 1,
            total: 2,
            status: StepStatus::Ok,
            items: vec![
                DisplayItem {
                    detail: "postgres:16".to_string(),
                    icon: "✓".to_string(),
                    verbose: None,
                },
                DisplayItem {
                    detail: "broken".to_string(),
                    icon: "⚠".to_string(),
                    verbose: None,
                },
                DisplayItem {
                    detail: "redis:7".to_string(),
                    icon: "✗".to_string(),
                    verbose: None,
                },
            ],
        };
        let suffix = pd.step_suffix(&step);
        assert!(suffix.contains("1 ok"));
        assert!(suffix.contains("1 warn"));
        assert!(suffix.contains("1 fail"));
    }

    #[test]
    fn test_step_suffix_pending_with_items_returns_empty() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "test".to_string(),
            number: 1,
            total: 1,
            status: StepStatus::Pending,
            items: vec![DisplayItem {
                detail: "something".to_string(),
                icon: "✓".to_string(),
                verbose: None,
            }],
        };
        assert_eq!(pd.step_suffix(&step), "");
    }

    #[test]
    fn test_step_suffix_skip_with_items_returns_empty() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "test".to_string(),
            number: 1,
            total: 1,
            status: StepStatus::Skip,
            items: vec![DisplayItem {
                detail: "something".to_string(),
                icon: "–".to_string(),
                verbose: None,
            }],
        };
        assert_eq!(pd.step_suffix(&step), "");
    }

    #[test]
    fn test_step_suffix_fail_status_shows_summary() {
        let pd = ProgressDisplay::new(Verbosity::Default);
        let step = DisplayStep {
            name: "Build images".to_string(),
            number: 2,
            total: 3,
            status: StepStatus::Fail,
            items: vec![
                DisplayItem {
                    detail: "frontend".to_string(),
                    icon: "✓".to_string(),
                    verbose: None,
                },
                DisplayItem {
                    detail: "backend".to_string(),
                    icon: "✗".to_string(),
                    verbose: None,
                },
            ],
        };
        let suffix = pd.step_suffix(&step);
        assert!(suffix.contains("1 ok"));
        assert!(suffix.contains("1 fail"));
    }

    // -------------------------------------------------------------------
    // Full lifecycle integration
    // -------------------------------------------------------------------

    #[test]
    fn test_full_build_lifecycle() {
        let mut pd = ProgressDisplay::new(Verbosity::Default);

        pd.handle_event(&make_plan_event(vec![
            "Pull images",
            "Extract secrets",
            "Build artifact",
        ]));
        assert_eq!(pd.steps.len(), 3);

        pd.handle_event(&make_event("started", "Pull images"));
        assert_eq!(pd.steps[0].status, StepStatus::InProgress);

        pd.handle_event(&BuildProgressEvent {
            step: "Pull images".to_string(),
            detail: Some("postgres:16".to_string()),
            status: "ok".to_string(),
            verbose_detail: None,
            step_number: Some(1),
            total_steps: Some(3),
            plan: None,
        });
        assert_eq!(pd.steps[0].items.len(), 1);

        pd.handle_event(&make_event("started", "Extract secrets"));
        assert_eq!(pd.steps[0].status, StepStatus::Ok);
        assert_eq!(pd.steps[1].status, StepStatus::InProgress);

        pd.handle_event(&make_event("ok", "Extract secrets"));
        assert_eq!(pd.steps[1].status, StepStatus::Ok);

        pd.handle_event(&make_event("started", "Build artifact"));
        pd.handle_event(&make_event("ok", "Build artifact"));
        assert_eq!(pd.steps[2].status, StepStatus::Ok);

        pd.finalize();
        for step in &pd.steps {
            assert_ne!(step.status, StepStatus::InProgress);
        }
    }

    // -------------------------------------------------------------------
    // Coastfile-less build flag tests
    // -------------------------------------------------------------------

    #[test]
    fn test_build_args_name_flag() {
        let cli = TestCli::try_parse_from(["test", "--name", "my-project"]).unwrap();
        assert_eq!(cli.args.project_name, Some("my-project".to_string()));
    }

    #[test]
    fn test_build_args_compose_flag() {
        let cli = TestCli::try_parse_from(["test", "--compose", "./dc.yml"]).unwrap();
        assert_eq!(cli.args.compose, Some(PathBuf::from("./dc.yml")));
    }

    #[test]
    fn test_build_args_port_flags() {
        let cli =
            TestCli::try_parse_from(["test", "--port", "web=3000", "--port", "api=8080"]).unwrap();
        assert_eq!(cli.args.ports, vec!["web=3000", "api=8080"]);
    }

    #[test]
    fn test_build_args_config_flag() {
        let cli = TestCli::try_parse_from([
            "test",
            "--config",
            "[secrets.key]\nextractor = \"env\"\ninject = \"env:KEY\"",
        ])
        .unwrap();
        assert!(cli.args.config.is_some());
    }

    #[test]
    fn test_build_args_no_autostart() {
        let cli = TestCli::try_parse_from(["test", "--no-autostart"]).unwrap();
        assert!(cli.args.no_autostart);
    }

    #[test]
    fn test_build_args_runtime_flag() {
        let cli = TestCli::try_parse_from(["test", "--runtime", "sysbox"]).unwrap();
        assert_eq!(cli.args.runtime, Some("sysbox".to_string()));
    }

    #[test]
    fn test_build_args_primary_port_flag() {
        let cli = TestCli::try_parse_from(["test", "--primary-port", "web"]).unwrap();
        assert_eq!(cli.args.primary_port, Some("web".to_string()));
    }

    #[test]
    fn test_build_args_compose_content_conflicts_compose() {
        let result = TestCli::try_parse_from([
            "test",
            "--compose",
            "./dc.yml",
            "--compose-content",
            "services: {}",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_args_all_new_flags_together() {
        let cli = TestCli::try_parse_from([
            "test",
            "--name",
            "proj",
            "--compose",
            "./dc.yml",
            "--runtime",
            "dind",
            "--port",
            "web=3000",
            "--no-autostart",
            "--primary-port",
            "web",
        ])
        .unwrap();
        assert_eq!(cli.args.project_name, Some("proj".to_string()));
        assert_eq!(cli.args.compose, Some(PathBuf::from("./dc.yml")));
        assert_eq!(cli.args.runtime, Some("dind".to_string()));
        assert_eq!(cli.args.ports, vec!["web=3000"]);
        assert!(cli.args.no_autostart);
        assert_eq!(cli.args.primary_port, Some("web".to_string()));
    }

    // -------------------------------------------------------------------
    // TOML construction and merge tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_port_spec_valid() {
        let (name, port) = parse_port_spec("web=3000").unwrap();
        assert_eq!(name, "web");
        assert_eq!(port, 3000);
    }

    #[test]
    fn test_parse_port_spec_with_spaces() {
        let (name, port) = parse_port_spec("api = 8080").unwrap();
        assert_eq!(name, "api");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_port_spec_invalid_format() {
        assert!(parse_port_spec("web:3000").is_err());
    }

    #[test]
    fn test_parse_port_spec_invalid_port() {
        assert!(parse_port_spec("web=abc").is_err());
    }

    #[test]
    fn test_parse_port_spec_zero_port() {
        assert!(parse_port_spec("web=0").is_err());
    }

    #[test]
    fn test_escape_toml_string() {
        assert_eq!(escape_toml_string("hello"), "hello");
        assert_eq!(escape_toml_string(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_toml_string(r"path\to"), r"path\\to");
    }

    #[test]
    fn test_build_toml_from_flags_minimal() {
        let args = BuildArgs {
            coastfile_path: PathBuf::from("Coastfile"),
            coastfile_type: None,
            refresh: false,
            remote: None,
            silent: false,
            verbose: false,
            project_name: Some("test-proj".to_string()),
            compose: Some(PathBuf::from("/abs/path/docker-compose.yml")),
            compose_content: None,
            runtime: None,
            ports: vec![],
            no_autostart: false,
            primary_port: None,
            config: None,
        };
        let cwd = PathBuf::from("/does-not-matter");
        let toml = build_toml_from_flags(&args, &cwd).unwrap();
        assert!(toml.contains("name = \"test-proj\""));
        assert!(toml.contains("compose = \"/abs/path/docker-compose.yml\""));
    }

    #[test]
    fn test_build_toml_from_flags_with_ports_and_config() {
        let args = BuildArgs {
            coastfile_path: PathBuf::from("Coastfile"),
            coastfile_type: None,
            refresh: false,
            remote: None,
            silent: false,
            verbose: false,
            project_name: Some("test".to_string()),
            compose: Some(PathBuf::from("/abs/dc.yml")),
            compose_content: None,
            runtime: Some("sysbox".to_string()),
            ports: vec!["web=3000".to_string(), "api=8080".to_string()],
            no_autostart: true,
            primary_port: Some("web".to_string()),
            config: Some(
                "[secrets.key]\nextractor = \"env\"\ninject = \"env:KEY\"\nvar = \"MY_KEY\""
                    .to_string(),
            ),
        };
        let cwd = PathBuf::from("/tmp");
        let toml = build_toml_from_flags(&args, &cwd).unwrap();
        assert!(toml.contains("name = \"test\""));
        assert!(toml.contains("compose = \"/abs/dc.yml\""));
        assert!(toml.contains("runtime = \"sysbox\""));
        assert!(toml.contains("autostart = false"));
        assert!(toml.contains("primary_port = \"web\""));
        assert!(toml.contains("web = 3000"));
        assert!(toml.contains("api = 8080"));
        assert!(toml.contains("[secrets.key]"));
    }

    #[test]
    fn test_merge_flags_into_toml_name_override() {
        let base = r#"
[coast]
name = "original"
compose = "./dc.yml"
"#;
        let args = BuildArgs {
            coastfile_path: PathBuf::from("Coastfile"),
            coastfile_type: None,
            refresh: false,
            remote: None,
            silent: false,
            verbose: false,
            project_name: Some("overridden".to_string()),
            compose: None,
            compose_content: None,
            runtime: None,
            ports: vec![],
            no_autostart: false,
            primary_port: None,
            config: None,
        };
        let cwd = PathBuf::from("/tmp");
        let merged = merge_flags_into_toml(base, &args, &cwd).unwrap();
        assert!(merged.contains("overridden"));
    }

    #[test]
    fn test_merge_flags_into_toml_adds_ports() {
        let base = r#"
[coast]
name = "proj"
compose = "./dc.yml"
"#;
        let args = BuildArgs {
            coastfile_path: PathBuf::from("Coastfile"),
            coastfile_type: None,
            refresh: false,
            remote: None,
            silent: false,
            verbose: false,
            project_name: None,
            compose: None,
            compose_content: None,
            runtime: None,
            ports: vec!["web=3000".to_string()],
            no_autostart: false,
            primary_port: None,
            config: None,
        };
        let cwd = PathBuf::from("/tmp");
        let merged = merge_flags_into_toml(base, &args, &cwd).unwrap();
        assert!(merged.contains("web = 3000") || merged.contains("web = 3_000"));
    }

    #[test]
    fn test_merge_flags_empty_overlay_returns_base() {
        let base = "[coast]\nname = \"proj\"\n";
        let args = BuildArgs {
            coastfile_path: PathBuf::from("Coastfile"),
            coastfile_type: None,
            refresh: false,
            remote: None,
            silent: false,
            verbose: false,
            project_name: None,
            compose: None,
            compose_content: None,
            runtime: None,
            ports: vec![],
            no_autostart: false,
            primary_port: None,
            config: None,
        };
        let cwd = PathBuf::from("/tmp");
        let merged = merge_flags_into_toml(base, &args, &cwd).unwrap();
        assert_eq!(merged, base);
    }

    #[test]
    fn test_merge_toml_tables_deep_merge() {
        let mut base: toml::Table =
            toml::from_str("[coast]\nname = \"a\"\nruntime = \"dind\"\n[ports]\nweb = 3000\n")
                .unwrap();
        let overlay: toml::Table =
            toml::from_str("[coast]\nname = \"b\"\n[ports]\napi = 8080\n").unwrap();
        merge_toml_tables(&mut base, &overlay);
        assert_eq!(base["coast"]["name"].as_str(), Some("b"),);
        assert_eq!(base["coast"]["runtime"].as_str(), Some("dind"),);
        assert_eq!(base["ports"]["web"].as_integer(), Some(3000));
        assert_eq!(base["ports"]["api"].as_integer(), Some(8080));
    }
}
