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
    /// Create the SSG singleton DinD and start all its services.
    ///
    /// Allocates dynamic host ports for each service, writes them to
    /// the state DB, and publishes them on the outer DinD container.
    /// Streams progress events while booting.
    Run,
    /// Start a previously-created-then-stopped SSG.
    Start,
    /// Stop the SSG DinD (inner `docker compose down` + outer stop).
    ///
    /// With `--force`, proceeds even if remote consumer coasts are
    /// currently consuming the SSG; the daemon tears down their
    /// reverse SSH tunnels before stopping.
    Stop {
        /// Proceed even when remote shadow coasts reference the SSG.
        /// Tears down the reverse-tunnel ssh children first.
        #[arg(long)]
        force: bool,
    },
    /// Stop then start the SSG. Preserves the existing container id
    /// and dynamic port mappings.
    Restart,
    /// Remove the SSG DinD container.
    ///
    /// With `--with-data`, inner named volumes (postgres WAL, etc.)
    /// are also removed before tearing down the DinD. Host bind mount
    /// contents are never touched. With `--force`, proceeds even if
    /// remote consumer coasts are consuming the SSG.
    Rm {
        /// Also remove inner named volumes. Host bind mount contents
        /// are unaffected.
        #[arg(long = "with-data")]
        with_data: bool,
        /// Proceed even when remote shadow coasts reference the SSG.
        /// Tears down the reverse-tunnel ssh children first.
        #[arg(long)]
        force: bool,
    },
    /// Tail logs from the outer DinD or a specific inner service.
    Logs {
        /// Inner service name. When omitted, shows the outer DinD
        /// container's stdout.
        #[arg(long)]
        service: Option<String>,
        /// Number of trailing lines to include. Defaults to 200.
        #[arg(short = 't', long)]
        tail: Option<u32>,
        /// Stream new lines as they arrive.
        #[arg(short = 'f', long)]
        follow: bool,
    },
    /// Exec a command inside the outer DinD or a named inner service.
    ///
    /// With `--service <name>`, runs `docker compose exec -T <name>
    /// <cmd...>` inside the outer DinD. Without, execs directly on
    /// the outer DinD container.
    Exec {
        /// Inner service name (e.g. `postgres`).
        #[arg(long)]
        service: Option<String>,
        /// Command to run. Everything after `--` is passed verbatim.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Show the per-service dynamic host port mapping.
    Ports,
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
        SsgAction::Run => execute_lifecycle(SsgRequest::Run, "Run", args.silent).await,
        SsgAction::Start => execute_lifecycle(SsgRequest::Start, "Start", args.silent).await,
        SsgAction::Restart => execute_lifecycle(SsgRequest::Restart, "Restart", args.silent).await,
        SsgAction::Stop { force } => {
            execute_simple(SsgRequest::Stop { force: *force }, args.silent).await
        }
        SsgAction::Rm { with_data, force } => {
            execute_simple(
                SsgRequest::Rm {
                    with_data: *with_data,
                    force: *force,
                },
                args.silent,
            )
            .await
        }
        SsgAction::Logs {
            service,
            tail,
            follow,
        } => {
            if *follow {
                execute_logs_follow(service.clone(), *tail).await
            } else {
                execute_simple(
                    SsgRequest::Logs {
                        service: service.clone(),
                        tail: *tail,
                        follow: false,
                    },
                    args.silent,
                )
                .await
            }
        }
        SsgAction::Exec { service, command } => {
            execute_simple(
                SsgRequest::Exec {
                    service: service.clone(),
                    command: command.clone(),
                },
                args.silent,
            )
            .await
        }
        SsgAction::Ports => execute_ports(args.silent).await,
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

async fn execute_lifecycle(req: SsgRequest, verb: &str, silent: bool) -> Result<()> {
    let response = super::send_build_request(Request::Ssg(req), |event| {
        if silent {
            return;
        }
        render_progress(event);
    })
    .await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                print_lifecycle_summary(verb, &resp);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

async fn execute_simple(req: SsgRequest, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(req)).await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                println!("{}", resp.message);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

async fn execute_ports(silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest::Ports)).await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                println!("{}", resp.message);
                if !resp.ports.is_empty() {
                    println!();
                    println!("{}", format_ports_table(&resp.ports));
                }
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

async fn execute_logs_follow(service: Option<String>, tail: Option<u32>) -> Result<()> {
    let request = Request::Ssg(SsgRequest::Logs {
        service,
        tail,
        follow: true,
    });
    super::stream_ssg_log_chunks(request, |chunk| {
        println!("{chunk}");
    })
    .await
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

fn print_lifecycle_summary(verb: &str, resp: &SsgResponse) {
    println!();
    println!(
        "{} {} — {}",
        verb.green().bold(),
        "done".green(),
        resp.message
    );
    if !resp.services.is_empty() {
        println!();
        println!("{}", format_services_table(&resp.services));
    }
    if !resp.ports.is_empty() {
        println!();
        println!("{}", format_ports_table(&resp.ports));
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
