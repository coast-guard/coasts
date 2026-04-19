//! `coast ssg` command — manage the singleton Shared Service Group.
//!
//! Phase: ssg-phase-2 wires `build` and `ps`. Phase 3+ adds lifecycle
//! verbs (`run`, `stop`, `start`, `restart`, `rm`, `logs`, `exec`,
//! `ports`) and Phase 6 adds `checkout` / `uncheckout`.
//!
//! Alias: `coast shared-service-group` also points here per
//! `coast-ssg/DESIGN.md §7`.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{
    BuildProgressEvent, Request, Response, SsgPortInfo, SsgRequest, SsgResponse, SsgServiceInfo,
};

/// Arguments for `coast ssg`.
#[derive(Debug, Args)]
pub struct SsgArgs {
    #[command(subcommand)]
    pub action: SsgAction,

    /// Suppress progress output; print only the final summary or errors.
    #[arg(short = 's', long, global = true)]
    pub silent: bool,
}

#[derive(Debug, Subcommand)]
pub enum SsgAction {
    /// Build the SSG from `Coastfile.shared_service_groups`.
    ///
    /// Pulls each service's image into the shared cache, synthesizes
    /// an inner `compose.yml`, writes the build artifact to
    /// `~/.coast/ssg/builds/{build_id}/`, and flips the `latest`
    /// symlink. Prunes to the 5 most recent builds.
    Build {
        /// Path to the SSG Coastfile. Defaults to cwd lookup.
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,

        /// Directory used to locate the Coastfile when `-f` is absent.
        /// Defaults to cwd.
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,

        /// Inline TOML (alternative to `-f`).
        #[arg(long)]
        config: Option<String>,
    },
    /// Show the current SSG build's service list (read from
    /// `~/.coast/ssg/latest/manifest.json`; no container inspection).
    Ps,
}

pub async fn execute(args: &SsgArgs) -> Result<()> {
    match &args.action {
        SsgAction::Build {
            file,
            working_dir,
            config,
        } => {
            execute_build(
                file.clone(),
                working_dir.clone(),
                config.clone(),
                args.silent,
            )
            .await
        }
        SsgAction::Ps => execute_ps(args.silent).await,
    }
}

async fn execute_build(
    file: Option<PathBuf>,
    working_dir: Option<PathBuf>,
    config: Option<String>,
    silent: bool,
) -> Result<()> {
    let request = Request::Ssg(SsgRequest::Build {
        file,
        working_dir,
        config,
    });

    let response = super::send_build_request(request, |event| {
        if silent {
            return;
        }
        render_progress(event);
    })
    .await?;

    match response {
        Response::Ssg(resp) => {
            if !silent {
                print_build_summary(&resp);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

async fn execute_ps(silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest::Ps)).await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                print_ps(&resp);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

fn render_progress(event: &BuildProgressEvent) {
    // Compact renderer — enough for humans to see where they are but
    // no fancy spinners. `coast ssg build` is less frequent than
    // `coast build`, so the full ProgressDisplay would be overkill.
    let step = if let (Some(n), Some(total)) = (event.step_number, event.total_steps) {
        format!("[{n}/{total}]")
    } else {
        "      ".to_string()
    };

    let detail = event.detail.as_deref().unwrap_or("");

    match event.status.as_str() {
        "plan" => {
            if let Some(ref plan) = event.plan {
                println!("{} {}", "plan:".dimmed(), plan.join(" -> ").dimmed());
            }
        }
        "started" => {
            println!("{} {} {}", step, "...".yellow(), event.step);
        }
        "ok" | "done" | "cached" => {
            let status = if event.status == "cached" {
                "cached".cyan()
            } else {
                "ok".green()
            };
            let suffix = if detail.is_empty() {
                String::new()
            } else {
                format!(" ({detail})").dimmed().to_string()
            };
            println!("{} {} {}{}", step, status, event.step, suffix);
        }
        other => {
            println!("{} [{}] {} {}", step, other.yellow(), event.step, detail);
        }
    }
}

fn print_build_summary(resp: &SsgResponse) {
    println!();
    println!("{} {}", "ok".green().bold(), resp.message);
    if !resp.services.is_empty() {
        println!();
        println!("{}", format_services_table(&resp.services));
    }
}

fn print_ps(resp: &SsgResponse) {
    println!("{}", resp.message);
    if !resp.services.is_empty() {
        println!();
        println!("{}", format_services_table(&resp.services));
    }
    if !resp.ports.is_empty() {
        println!();
        println!("{}", format_ports_table(&resp.ports));
    }
}

fn format_services_table(services: &[SsgServiceInfo]) -> String {
    let mut lines = Vec::with_capacity(services.len() + 1);
    lines.push(format!(
        "  {:<20} {:<30} {:<10} {}",
        "SERVICE".bold(),
        "IMAGE".bold(),
        "PORT".bold(),
        "STATUS".bold(),
    ));
    for svc in services {
        let port_str = if svc.inner_port == 0 {
            "-".to_string()
        } else {
            svc.inner_port.to_string()
        };
        lines.push(format!(
            "  {:<20} {:<30} {:<10} {}",
            svc.name, svc.image, port_str, svc.status
        ));
    }
    lines.join("\n")
}

fn format_ports_table(ports: &[SsgPortInfo]) -> String {
    let mut lines = Vec::with_capacity(ports.len() + 1);
    lines.push(format!(
        "  {:<20} {:<15} {:<15} {}",
        "SERVICE".bold(),
        "CANONICAL".bold(),
        "DYNAMIC".bold(),
        "CHECKED OUT".bold(),
    ));
    for port in ports {
        lines.push(format!(
            "  {:<20} {:<15} {:<15} {}",
            port.service, port.canonical_port, port.dynamic_host_port, port.checked_out
        ));
    }
    lines.join("\n")
}
