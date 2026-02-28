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

    /// Suppress all progress output; only print the final summary (or errors).
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Show verbose build detail (e.g., docker build logs).
    #[arg(short = 'v', long)]
    pub verbose: bool,
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

/// Execute the `coast build` command.
///
/// The project name is derived from the Coastfile, not from the `--project` flag,
/// since the Coastfile itself defines the project name.
pub async fn execute(args: &BuildArgs) -> Result<()> {
    if let Some(ref t) = args.coastfile_type {
        if t == "default" {
            bail!(
                "'--type default' is not allowed. \
                 The base 'Coastfile' is the default type. \
                 Run 'coast build' without --type."
            );
        }
    }

    let coastfile_path = if let Some(ref t) = args.coastfile_type {
        let has_custom_file = args.coastfile_path != Path::new("Coastfile");
        if has_custom_file {
            bail!(
                "--type and -f/--file are mutually exclusive. \
                 Use --type to pick a variant (e.g. Coastfile.light), \
                 or -f to specify an explicit path."
            );
        }
        let filename = format!("Coastfile.{t}");
        std::env::current_dir()?.join(filename)
    } else if args.coastfile_path.is_absolute() {
        args.coastfile_path.clone()
    } else {
        std::env::current_dir()?.join(&args.coastfile_path)
    };

    let request = Request::Build(BuildRequest {
        coastfile_path,
        refresh: args.refresh,
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
}
