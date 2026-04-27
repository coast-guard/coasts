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
    BuildProgressEvent, Request, Response, SsgAction as ProtoSsgAction, SsgDoctorFinding,
    SsgListing, SsgPortInfo, SsgRequest, SsgResponse, SsgServiceInfo,
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
    /// List every per-project SSG known to the daemon.
    ///
    /// Cross-project command: runs from any cwd, does NOT require a
    /// Coastfile. Prints one row per known SSG with project, status,
    /// build id, container id, and service count. See
    /// `coast-ssg/DESIGN.md §23` — Phase 22.
    Ls,
    /// Bind the canonical host port of an SSG service (or all of them)
    /// via a socat forwarder so host-side callers (psql from the host,
    /// Coastguard previews, MCPs) can reach the service at its
    /// canonical name/port.
    ///
    /// If a coast instance currently holds the canonical port, it is
    /// displaced with a clear warning. If the port is held by a process
    /// outside Coast's tracking, the command errors out — use the
    /// usual host-side tools to free it. See `coast-ssg/DESIGN.md §12`.
    Checkout {
        /// Service name (e.g. `postgres`). Positional form per
        /// DESIGN.md §12. Mutually exclusive with `--all`.
        #[arg(value_name = "SERVICE")]
        service: Option<String>,
        /// Alternative long-form for the positional `<SERVICE>`.
        /// Preserved for backward compat; prefer the positional form.
        #[arg(long = "service", value_name = "SERVICE")]
        service_flag: Option<String>,
        /// Check out every SSG service.
        #[arg(long)]
        all: bool,
    },
    /// Tear down the canonical-port socat for an SSG service (or all
    /// of them). Does NOT auto-restore any coast instance previously
    /// displaced by a checkout; rebind it with `coast checkout <instance>`
    /// if you want it back on the canonical port.
    Uncheckout {
        /// Service name (e.g. `postgres`). Positional form per
        /// DESIGN.md §12. Mutually exclusive with `--all`.
        #[arg(value_name = "SERVICE")]
        service: Option<String>,
        /// Alternative long-form for the positional `<SERVICE>`.
        #[arg(long = "service", value_name = "SERVICE")]
        service_flag: Option<String>,
        #[arg(long)]
        all: bool,
    },
    /// Read-only permission check on host bind mounts of the active
    /// SSG's known-image services (postgres, mysql, mariadb, mongo).
    ///
    /// Reports one finding per `(service, host-bind-mount)` pair:
    /// `ok` when the directory owner matches the image's expected
    /// UID/GID, `warn` when it diverges (with the `chown` command to
    /// fix it), or `info` when the directory does not exist yet.
    /// Does not modify anything. See `coast-ssg/DESIGN.md §10.5`.
    Doctor,
    /// Zero-copy migration: resolve a host Docker named volume's
    /// mountpoint and add it to the SSG Coastfile as a bind-mount
    /// entry for a given service.
    ///
    /// Default prints the TOML snippet to paste. Pass `--apply` to
    /// rewrite the SSG Coastfile in place (with a `.bak` backup).
    /// Use `--file` / `--working-dir` / `--config` to point at the
    /// SSG Coastfile; discovery mirrors `coast ssg build`.
    /// See `coast-ssg/DESIGN.md §10.7`.
    ImportHostVolume {
        /// Host Docker named volume name (must already exist).
        #[arg(value_name = "VOLUME")]
        volume: String,
        /// Target `[shared_services.<name>]` section.
        #[arg(long)]
        service: String,
        /// Absolute container path to bind the volume mountpoint at.
        #[arg(long)]
        mount: PathBuf,
        /// Path to the SSG Coastfile (overrides discovery).
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
        /// Working directory for SSG Coastfile discovery.
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        /// Inline TOML config for the SSG Coastfile (cannot combine
        /// with `--apply`; the helper has nothing to write back to).
        #[arg(long)]
        config: Option<String>,
        /// Rewrite the SSG Coastfile in place with a `.bak` backup.
        #[arg(long)]
        apply: bool,
    },
    /// Pin this project's consumer coast to a specific SSG build.
    ///
    /// Drift checks and auto-start on `coast run` use the pinned
    /// build instead of `latest`, so the SSG can be rebuilt without
    /// disturbing your consumer until you explicitly unpin or repin.
    /// Pinned builds also survive `auto_prune`. See
    /// `coast-ssg/DESIGN.md §17-9 SETTLED #41`.
    CheckoutBuild {
        /// SSG build id to pin to. Validated against
        /// `~/.coast/ssg/builds/<id>/manifest.json` at pin time.
        #[arg(value_name = "BUILD_ID")]
        build_id: String,
        /// Project name override. Defaults to `[coast].name` read
        /// from the consumer Coastfile in `--working-dir` / cwd.
        #[arg(long)]
        project: Option<String>,
        /// Working directory for Coastfile discovery. Defaults to
        /// cwd. Ignored when `--project` is set.
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        /// Path to the consumer Coastfile. Ignored when `--project`
        /// is set.
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// Clear the SSG build pin for this project. Idempotent.
    UncheckoutBuild {
        /// Project name override. Defaults to `[coast].name` read
        /// from the consumer Coastfile in `--working-dir` / cwd.
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// Show the current SSG build pin for this project, if any.
    ShowPin {
        /// Project name override. Defaults to `[coast].name` read
        /// from the consumer Coastfile in `--working-dir` / cwd.
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// List SSG build artifacts for this project.
    ///
    /// Walks `~/.coast/ssg/builds/` (the globally-shared SSG
    /// artifact pool) and filters to builds whose
    /// `coastfile_hash` matches the project's current
    /// `latest_build_id`'s hash. Marks the latest build with
    /// `LATEST` and the currently-pinned build (if any) with
    /// `PINNED`. The same data backs the SPA's "SHARED SERVICE
    /// GROUPS" subsection. See `coast-ssg/DESIGN.md §9.1` for
    /// the artifact layout.
    BuildsLs {
        /// Project name override. Defaults to `[coast].name`
        /// read from the consumer Coastfile in `--working-dir` /
        /// cwd. Same fallback chain as `coast ssg show-pin`.
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    /// Manage the SSG's encrypted secret store.
    ///
    /// Phase 33: SSGs declare `[secrets.<name>]` blocks the same
    /// way regular Coastfiles do. `coast ssg build` extracts and
    /// encrypts the values into the shared keystore under
    /// `coast_image = "ssg:<project>"`; `coast ssg run` decrypts
    /// and injects them via a per-run `compose.override.yml`. The
    /// keystore is NEVER auto-purged: only the explicit
    /// `coast ssg secrets clear` verb removes entries. See
    /// `DESIGN.md §33`.
    Secrets {
        #[command(subcommand)]
        cmd: SsgSecretsCommand,
    },
}

/// Subcommands of `coast ssg secrets`.
#[derive(Debug, Clone, clap::Subcommand)]
pub enum SsgSecretsCommand {
    /// Clear every encrypted SSG secret for this project.
    /// Idempotent. Subsequent `coast ssg run` will still start
    /// the SSG container, but services that depend on a missing
    /// env-var or file path will fail at compose-up time.
    Clear {
        /// Project name override. Defaults to `[coast].name`
        /// read from the consumer Coastfile in `--working-dir` /
        /// cwd. Same fallback chain as `coast ssg show-pin`.
        #[arg(long)]
        project: Option<String>,
        #[arg(long = "working-dir")]
        working_dir: Option<PathBuf>,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
}

pub async fn execute(args: &SsgArgs, cli_working_dir: &Option<PathBuf>) -> Result<()> {
    match &args.action {
        SsgAction::Build {
            file,
            working_dir,
            config,
        } => dispatch_build(file, working_dir, config, args.silent, cli_working_dir).await,
        SsgAction::ImportHostVolume { .. } => {
            dispatch_import_host_volume(&args.action, args.silent, cli_working_dir).await
        }
        SsgAction::CheckoutBuild { .. }
        | SsgAction::UncheckoutBuild { .. }
        | SsgAction::ShowPin { .. } => {
            execute_pin_action(&args.action, args.silent, cli_working_dir).await
        }
        // Cross-project: no `resolve_consumer_project` call, no cwd
        // Coastfile required. Daemon handler ignores `req.project`.
        SsgAction::Ls => execute_ls(args.silent).await,
        SsgAction::BuildsLs {
            project,
            working_dir,
            file,
        } => {
            let resolved_working_dir = working_dir.clone().or_else(|| cli_working_dir.clone());
            let resolved_project = resolve_consumer_project(project, &resolved_working_dir, file)?;
            execute_builds_ls(resolved_project, args.silent).await
        }
        // Phase 33: SSG-native secrets. `secrets clear` is the only
        // verb today; future additions (e.g. `secrets list`) slot
        // into the same `Secrets { cmd }` enum.
        SsgAction::Secrets {
            cmd:
                SsgSecretsCommand::Clear {
                    project,
                    working_dir,
                    file,
                },
        } => {
            let resolved_working_dir = working_dir.clone().or_else(|| cli_working_dir.clone());
            let resolved_project = resolve_consumer_project(project, &resolved_working_dir, file)?;
            execute_secrets_clear(resolved_project, args.silent).await
        }
        other => dispatch_simple_or_lifecycle(other, args.silent, cli_working_dir).await,
    }
}

/// `coast ssg build` — resolves project from the SSG Coastfile's
/// directory (which also contains the sibling `Coastfile`).
async fn dispatch_build(
    file: &Option<PathBuf>,
    working_dir: &Option<PathBuf>,
    config: &Option<String>,
    silent: bool,
    cli_working_dir: &Option<PathBuf>,
) -> Result<()> {
    // Subcommand-level `--working-dir` wins; fall back to the global
    // `coast --working-dir` flag so `coast --working-dir <dir> ssg
    // build` works (DESIGN.md §5). Phase 25: if neither flag is set,
    // use the CLI process's cwd — this matches the Phase 22 cwd-based
    // project-resolution contract and keeps the daemon from falling
    // back to its own cwd (e.g. `/workspace` in dindind, where no
    // `Coastfile.shared_service_groups` exists).
    let resolved_working_dir = working_dir
        .clone()
        .or_else(|| cli_working_dir.clone())
        .or_else(|| std::env::current_dir().ok());
    // Per-project SSG (§23): project comes from the sibling
    // `Coastfile`'s `[coast].name` in the SSG Coastfile's dir.
    let project = resolve_consumer_project(&None, &resolved_working_dir, &None)?;
    execute_build(
        project,
        file.clone(),
        resolved_working_dir,
        config.clone(),
        silent,
    )
    .await
}

/// `coast ssg import-host-volume` — resolves project the same way as
/// `build` since they share the same `--working-dir` / `-f` /
/// `--config` discovery triplet. Takes `&SsgAction` so the dispatcher
/// stays under clippy's `too_many_arguments` gate.
async fn dispatch_import_host_volume(
    action: &SsgAction,
    silent: bool,
    cli_working_dir: &Option<PathBuf>,
) -> Result<()> {
    let SsgAction::ImportHostVolume {
        volume,
        service,
        mount,
        file,
        working_dir,
        config,
        apply,
    } = action
    else {
        unreachable!("dispatch_import_host_volume is only called with SsgAction::ImportHostVolume")
    };
    // Phase 25: fall back to the CLI process's cwd so the daemon
    // doesn't resolve relative to its own cwd. See `dispatch_build`
    // for the full rationale.
    let resolved_working_dir = working_dir
        .clone()
        .or_else(|| cli_working_dir.clone())
        .or_else(|| std::env::current_dir().ok());
    let project = resolve_consumer_project(&None, &resolved_working_dir, &None)?;
    execute_simple(
        SsgRequest {
            project,
            action: ProtoSsgAction::ImportHostVolume {
                volume: volume.clone(),
                service: service.clone(),
                mount: mount.clone(),
                file: file.clone(),
                working_dir: resolved_working_dir,
                config: config.clone(),
                apply: *apply,
            },
        },
        silent,
    )
    .await
}

/// Dispatcher for every non-build, non-import, non-pin verb. Each
/// resolves the project from cwd once and then forwards to either
/// `execute_lifecycle` (Run/Start/Restart, streams progress) or
/// `execute_simple` (everything else).
async fn dispatch_simple_or_lifecycle(
    action: &SsgAction,
    silent: bool,
    cli_working_dir: &Option<PathBuf>,
) -> Result<()> {
    let project = resolve_consumer_project(&None, cli_working_dir, &None)?;
    match action {
        SsgAction::Ps => execute_ps(project, silent).await,
        SsgAction::Ports => execute_ports(project, silent).await,
        SsgAction::Doctor => execute_doctor(project, silent).await,
        SsgAction::Run => {
            execute_lifecycle(wrap(project, ProtoSsgAction::Run), "Run", silent).await
        }
        SsgAction::Start => {
            execute_lifecycle(wrap(project, ProtoSsgAction::Start), "Start", silent).await
        }
        SsgAction::Restart => {
            execute_lifecycle(wrap(project, ProtoSsgAction::Restart), "Restart", silent).await
        }
        SsgAction::Stop { force } => {
            execute_simple(
                wrap(project, ProtoSsgAction::Stop { force: *force }),
                silent,
            )
            .await
        }
        SsgAction::Rm { with_data, force } => {
            execute_simple(
                wrap(
                    project,
                    ProtoSsgAction::Rm {
                        with_data: *with_data,
                        force: *force,
                    },
                ),
                silent,
            )
            .await
        }
        SsgAction::Logs {
            service,
            tail,
            follow,
        } => {
            if *follow {
                execute_logs_follow(project, service.clone(), *tail).await
            } else {
                execute_simple(
                    wrap(
                        project,
                        ProtoSsgAction::Logs {
                            service: service.clone(),
                            tail: *tail,
                            follow: false,
                        },
                    ),
                    silent,
                )
                .await
            }
        }
        SsgAction::Exec { service, command } => {
            execute_simple(
                wrap(
                    project,
                    ProtoSsgAction::Exec {
                        service: service.clone(),
                        command: command.clone(),
                    },
                ),
                silent,
            )
            .await
        }
        SsgAction::Checkout {
            service,
            service_flag,
            all,
        } => {
            let resolved = resolve_checkout_service(service, service_flag, *all)?;
            execute_simple(
                wrap(
                    project,
                    ProtoSsgAction::Checkout {
                        service: resolved,
                        all: *all,
                    },
                ),
                silent,
            )
            .await
        }
        SsgAction::Uncheckout {
            service,
            service_flag,
            all,
        } => {
            let resolved = resolve_checkout_service(service, service_flag, *all)?;
            execute_simple(
                wrap(
                    project,
                    ProtoSsgAction::Uncheckout {
                        service: resolved,
                        all: *all,
                    },
                ),
                silent,
            )
            .await
        }
        SsgAction::Build { .. }
        | SsgAction::ImportHostVolume { .. }
        | SsgAction::CheckoutBuild { .. }
        | SsgAction::UncheckoutBuild { .. }
        | SsgAction::ShowPin { .. }
        | SsgAction::Ls
        | SsgAction::BuildsLs { .. }
        | SsgAction::Secrets { .. } => {
            unreachable!("dispatch_simple_or_lifecycle only handles non-build/non-pin/non-ls verbs")
        }
    }
}

/// Shorthand: `SsgRequest { project, action }` constructor.
fn wrap(project: String, action: ProtoSsgAction) -> SsgRequest {
    SsgRequest { project, action }
}

/// Dispatch for the Phase 16 pinning verbs. Extracted out of
/// [`execute`] to keep that function under the
/// `clippy::too_many_lines` threshold. Each variant resolves the
/// project name with [`resolve_consumer_project`] and forwards to
/// `execute_simple`.
async fn execute_pin_action(
    action: &SsgAction,
    silent: bool,
    cli_working_dir: &Option<PathBuf>,
) -> Result<()> {
    match action {
        SsgAction::CheckoutBuild {
            build_id,
            project,
            working_dir,
            file,
        } => {
            let resolved_working_dir = working_dir.clone().or_else(|| cli_working_dir.clone());
            let resolved_project = resolve_consumer_project(project, &resolved_working_dir, file)?;
            execute_simple(
                SsgRequest {
                    project: resolved_project,
                    action: ProtoSsgAction::CheckoutBuild {
                        build_id: build_id.clone(),
                    },
                },
                silent,
            )
            .await
        }
        SsgAction::UncheckoutBuild {
            project,
            working_dir,
            file,
        } => {
            let resolved_working_dir = working_dir.clone().or_else(|| cli_working_dir.clone());
            let resolved_project = resolve_consumer_project(project, &resolved_working_dir, file)?;
            execute_simple(
                SsgRequest {
                    project: resolved_project,
                    action: ProtoSsgAction::UncheckoutBuild,
                },
                silent,
            )
            .await
        }
        SsgAction::ShowPin {
            project,
            working_dir,
            file,
        } => {
            let resolved_working_dir = working_dir.clone().or_else(|| cli_working_dir.clone());
            let resolved_project = resolve_consumer_project(project, &resolved_working_dir, file)?;
            execute_simple(
                SsgRequest {
                    project: resolved_project,
                    action: ProtoSsgAction::ShowPin,
                },
                silent,
            )
            .await
        }
        _ => unreachable!("execute_pin_action only handles CheckoutBuild/UncheckoutBuild/ShowPin"),
    }
}

/// Resolve the consumer project name for `coast ssg
/// checkout-build / uncheckout-build / show-pin`:
/// - If `--project` is set, use it verbatim.
/// - Else parse the consumer's Coastfile (by `-f` or by discovery
///   rooted at `--working-dir` / cwd) and use `[coast].name`.
/// - Else hard-error with guidance.
///
/// Mirrors the fallback chain `coast run` uses for its `project`
/// field so users don't have to repeat `--project` when they're
/// already in the consumer's checkout.
fn resolve_consumer_project(
    project: &Option<String>,
    working_dir: &Option<PathBuf>,
    file: &Option<PathBuf>,
) -> Result<String> {
    if let Some(p) = project {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            bail!("coast ssg: --project cannot be empty.");
        }
        return Ok(trimmed.to_string());
    }

    let cwd = match working_dir {
        Some(p) => p.clone(),
        None => std::env::current_dir().map_err(|e| {
            anyhow::anyhow!("failed to read current directory for Coastfile discovery: {e}")
        })?,
    };

    let coastfile_path = if let Some(f) = file {
        if f.is_absolute() {
            f.clone()
        } else {
            cwd.join(f)
        }
    } else {
        coast_core::coastfile::Coastfile::find_coastfile(&cwd, "Coastfile").ok_or_else(|| {
            anyhow::anyhow!(
                "coast ssg: could not resolve the consumer project name. No Coastfile found \
                 near '{}'. Run from the consumer's Coastfile directory, pass --working-dir, \
                 -f <path>, or --project <name>.",
                cwd.display()
            )
        })?
    };

    let coastfile = coast_core::coastfile::Coastfile::from_file(&coastfile_path).map_err(|e| {
        anyhow::anyhow!(
            "coast ssg: failed to parse Coastfile '{}': {e}",
            coastfile_path.display()
        )
    })?;
    if coastfile.name.trim().is_empty() {
        bail!(
            "coast ssg: Coastfile at '{}' has an empty [coast].name. Pass --project <name> \
             explicitly.",
            coastfile_path.display()
        );
    }
    Ok(coastfile.name)
}

/// Resolve `coast ssg checkout [service|--service <s>|--all]` inputs into
/// the `Option<String>` the daemon protocol expects. DESIGN.md §12 uses
/// a positional argument; the `--service` flag is a backward-compat
/// alias for the same value. Rejects the case where both are set with
/// conflicting values.
fn resolve_checkout_service(
    positional: &Option<String>,
    flag: &Option<String>,
    all: bool,
) -> Result<Option<String>> {
    match (positional, flag, all) {
        (Some(p), Some(f), _) if p != f => {
            bail!("conflicting service name: positional '{p}' vs --service '{f}'. Use one form.")
        }
        (Some(s), _, true) | (_, Some(s), true) => {
            let _ = s;
            bail!("--all and a specific service are mutually exclusive.")
        }
        (Some(s), _, false) | (_, Some(s), false) => Ok(Some(s.clone())),
        (None, None, _) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_resolver_positional_only() {
        assert_eq!(
            resolve_checkout_service(&Some("postgres".into()), &None, false).unwrap(),
            Some("postgres".into())
        );
    }

    #[test]
    fn checkout_resolver_flag_only() {
        assert_eq!(
            resolve_checkout_service(&None, &Some("postgres".into()), false).unwrap(),
            Some("postgres".into())
        );
    }

    #[test]
    fn checkout_resolver_same_value_on_both_is_fine() {
        assert_eq!(
            resolve_checkout_service(&Some("postgres".into()), &Some("postgres".into()), false,)
                .unwrap(),
            Some("postgres".into())
        );
    }

    #[test]
    fn checkout_resolver_conflicting_values_errors() {
        let err = resolve_checkout_service(&Some("postgres".into()), &Some("redis".into()), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("conflicting service name"), "got: {err}");
    }

    #[test]
    fn checkout_resolver_all_with_service_errors() {
        let err = resolve_checkout_service(&Some("postgres".into()), &None, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("mutually exclusive"), "got: {err}");
    }

    #[test]
    fn checkout_resolver_all_alone_returns_none() {
        assert_eq!(resolve_checkout_service(&None, &None, true).unwrap(), None);
    }

    // --- Phase 16: resolve_consumer_project ---

    #[test]
    fn resolve_consumer_project_prefers_explicit_project() {
        let out = resolve_consumer_project(&Some("my-proj".into()), &None, &None).unwrap();
        assert_eq!(out, "my-proj");
    }

    #[test]
    fn resolve_consumer_project_trims_whitespace() {
        let out = resolve_consumer_project(&Some("  trimmed  ".into()), &None, &None).unwrap();
        assert_eq!(out, "trimmed");
    }

    #[test]
    fn resolve_consumer_project_rejects_empty_explicit() {
        let err = resolve_consumer_project(&Some("   ".into()), &None, &None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be empty"), "got: {err}");
    }

    #[test]
    fn resolve_consumer_project_reads_name_from_explicit_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cf_path = tmp.path().join("Coastfile");
        std::fs::write(
            &cf_path,
            r#"[coast]
name = "my-consumer"
runtime = "dind"
compose = "docker-compose.yml"
"#,
        )
        .unwrap();
        // Empty compose file to satisfy parser.
        std::fs::write(tmp.path().join("docker-compose.yml"), "services: {}\n").unwrap();

        let out = resolve_consumer_project(&None, &None, &Some(cf_path)).unwrap();
        assert_eq!(out, "my-consumer");
    }

    #[test]
    fn resolve_consumer_project_reads_name_via_working_dir_discovery() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Coastfile"),
            r#"[coast]
name = "discovered"
runtime = "dind"
compose = "docker-compose.yml"
"#,
        )
        .unwrap();
        std::fs::write(tmp.path().join("docker-compose.yml"), "services: {}\n").unwrap();

        let out = resolve_consumer_project(&None, &Some(tmp.path().to_path_buf()), &None).unwrap();
        assert_eq!(out, "discovered");
    }

    #[test]
    fn resolve_consumer_project_errors_when_no_coastfile_found() {
        let tmp = tempfile::tempdir().unwrap();
        // Fresh tempdir, no Coastfile.
        let err = resolve_consumer_project(&None, &Some(tmp.path().to_path_buf()), &None)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("could not resolve the consumer project name"),
            "got: {err}"
        );
    }

    // --- Phase 31: format_ports_table virtual port column ---

    fn port_with(virtual_port: Option<u16>) -> SsgPortInfo {
        SsgPortInfo {
            service: "postgres".to_string(),
            canonical_port: 5432,
            dynamic_host_port: 60001,
            virtual_port,
            checked_out: false,
        }
    }

    /// Strip ANSI escape codes from a string. The header is bolded
    /// for the terminal but the unit test only cares about plaintext
    /// matches.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_escape = false;
        for ch in s.chars() {
            if ch == '\u{1b}' {
                in_escape = true;
                continue;
            }
            if in_escape {
                if ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn format_ports_table_header_includes_virtual_column() {
        let table = format_ports_table(&[]);
        let plain = strip_ansi(&table);
        assert!(plain.contains("VIRTUAL"), "header missing VIRTUAL: {plain}");
        // Phase 31 column order — DYNAMIC stays before VIRTUAL so
        // existing scripts that read col 3 keep getting the dyn port.
        let dyn_idx = plain.find("DYNAMIC").expect("DYNAMIC missing");
        let vport_idx = plain.find("VIRTUAL").expect("VIRTUAL missing");
        let status_idx = plain.find("STATUS").expect("STATUS missing");
        assert!(
            dyn_idx < vport_idx && vport_idx < status_idx,
            "expected DYNAMIC < VIRTUAL < STATUS, got positions {dyn_idx}, {vport_idx}, {status_idx}"
        );
    }

    #[test]
    fn format_ports_table_renders_virtual_port_when_some() {
        let table = format_ports_table(&[port_with(Some(42001))]);
        let plain = strip_ansi(&table);
        assert!(
            plain.contains("42001"),
            "expected virtual port 42001 in output: {plain}"
        );
        // Dyn port still present in the same row.
        assert!(
            plain.contains("60001"),
            "expected dynamic port 60001 in output: {plain}"
        );
    }

    // --- format_builds_table (coast ssg builds-ls) ---

    fn build_entry(
        build_id: &str,
        services: &[&str],
        latest: bool,
        pinned: bool,
        created_at_unix: i64,
    ) -> coast_core::protocol::SsgBuildEntry {
        coast_core::protocol::SsgBuildEntry {
            build_id: build_id.to_string(),
            project: "cg".to_string(),
            created_at_unix,
            services: services.iter().map(|s| (*s).to_string()).collect(),
            services_count: services.len() as u32,
            pinned,
            latest,
        }
    }

    #[test]
    fn format_builds_table_header_has_expected_columns() {
        let table = format_builds_table(&[]);
        let plain = strip_ansi(&table);
        for col in ["BUILD", "SERVICES", "CREATED", "STATUS"] {
            assert!(plain.contains(col), "header missing {col}: {plain}");
        }
    }

    #[test]
    fn format_builds_table_marks_latest_and_pinned() {
        let table = format_builds_table(&[
            build_entry("abc_20260422000000", &["postgres", "redis"], true, false, 1),
            build_entry("abc_20260421000000", &["postgres"], false, true, 0),
            build_entry("abc_20260420000000", &["postgres"], false, false, 0),
        ]);
        let plain = strip_ansi(&table);
        let latest_line = plain
            .lines()
            .find(|l| l.contains("abc_20260422000000"))
            .expect("missing latest row");
        assert!(latest_line.contains("LATEST"), "got: {latest_line}");
        let pinned_line = plain
            .lines()
            .find(|l| l.contains("abc_20260421000000"))
            .expect("missing pinned row");
        assert!(pinned_line.contains("PINNED"), "got: {pinned_line}");
        assert!(
            !pinned_line.contains("LATEST"),
            "pinned row should not be LATEST: {pinned_line}"
        );
        let plain_line = plain
            .lines()
            .find(|l| l.contains("abc_20260420000000"))
            .expect("missing plain row");
        assert!(
            !plain_line.contains("LATEST") && !plain_line.contains("PINNED"),
            "plain row should not be flagged: {plain_line}"
        );
    }

    #[test]
    fn format_builds_table_renders_services_csv() {
        let table = format_builds_table(&[build_entry(
            "abc_20260422000000",
            &["postgres", "redis"],
            false,
            false,
            0,
        )]);
        let plain = strip_ansi(&table);
        assert!(
            plain.contains("postgres,redis"),
            "expected services CSV in row: {plain}"
        );
    }

    #[test]
    fn format_builds_table_truncates_long_service_list() {
        // 6 service names of 8 chars each + commas = > 24 chars.
        let table = format_builds_table(&[build_entry(
            "abc_20260422000000",
            &[
                "postgres", "redis123", "mongo123", "kafka123", "elastic1", "minio123",
            ],
            false,
            false,
            0,
        )]);
        let plain = strip_ansi(&table);
        let row = plain
            .lines()
            .find(|l| l.contains("abc_20260422000000"))
            .expect("row missing");
        assert!(row.contains("..."), "expected truncation marker in: {row}");
    }

    #[test]
    fn format_builds_table_renders_dash_when_no_timestamp() {
        let table = format_builds_table(&[build_entry(
            "abc_20260422000000",
            &["postgres"],
            false,
            false,
            0, // missing built_at -> falls back to 0 -> "-"
        )]);
        let plain = strip_ansi(&table);
        let row = plain
            .lines()
            .find(|l| l.contains("abc_20260422000000"))
            .expect("row missing");
        // Two-space gap before STATUS column means the CREATED slot
        // contains a literal "-" character.
        assert!(
            row.contains(" - "),
            "expected '-' placeholder when timestamp is 0: {row}"
        );
    }

    #[test]
    fn format_ports_table_renders_dash_when_virtual_port_none() {
        let table = format_ports_table(&[port_with(None)]);
        let plain = strip_ansi(&table);
        // Look for "-- " in the data line (after dyn port). The header
        // doesn't contain "-- ", and the dyn port is 60001.
        let data_line = plain
            .lines()
            .find(|l| l.contains("60001"))
            .expect("data line not found");
        assert!(
            data_line.contains("--"),
            "expected '--' rendering for None virtual_port: {data_line}"
        );
    }
}

async fn execute_build(
    project: String,
    file: Option<PathBuf>,
    working_dir: Option<PathBuf>,
    config: Option<String>,
    silent: bool,
) -> Result<()> {
    let request = Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::Build {
            file,
            working_dir,
            config,
        },
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

async fn execute_ps(project: String, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::Ps,
    }))
    .await?;
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

async fn execute_ports(project: String, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::Ports,
    }))
    .await?;
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

async fn execute_logs_follow(
    project: String,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<()> {
    let request = Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::Logs {
            service,
            tail,
            follow: true,
        },
    });
    super::stream_ssg_log_chunks(request, |chunk| {
        println!("{chunk}");
    })
    .await
}

async fn execute_ls(silent: bool) -> Result<()> {
    // `Ls` is cross-project: the daemon handler ignores `req.project`,
    // so an empty-string placeholder is fine and lets the command run
    // from any cwd (no Coastfile required).
    let response = super::send_request(Request::Ssg(SsgRequest {
        project: String::new(),
        action: ProtoSsgAction::Ls,
    }))
    .await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                println!("{}", resp.message);
                if !resp.listings.is_empty() {
                    println!();
                    println!("{}", format_listings_table(&resp.listings));
                }
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

/// `coast ssg builds-ls` — list build artifacts for a single
/// project. Uses [`resolve_consumer_project`]'s same fallback chain
/// as the pin verbs so the user can omit `--project` from inside
/// their consumer checkout. The daemon scopes the response to
/// builds whose `coastfile_hash` matches the project's anchor (the
/// `coastfile_hash` of its current `latest_build_id`); see
/// `handle_builds_ls` in `coast-daemon/src/handlers/ssg/mod.rs`.
async fn execute_builds_ls(project: String, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::BuildsLs,
    }))
    .await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                println!("{}", resp.message);
                if !resp.builds.is_empty() {
                    println!();
                    println!("{}", format_builds_table(&resp.builds));
                }
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

/// `coast ssg secrets clear` — drop every keystore entry whose
/// `coast_image == "ssg:<project>"`. Idempotent. Phase 33.
async fn execute_secrets_clear(project: String, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::SecretsClear,
    }))
    .await?;
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

async fn execute_doctor(project: String, silent: bool) -> Result<()> {
    let response = super::send_request(Request::Ssg(SsgRequest {
        project,
        action: ProtoSsgAction::Doctor,
    }))
    .await?;
    match response {
        Response::Ssg(resp) => {
            if !silent {
                print_doctor(&resp);
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        other => bail!("unexpected response from daemon: {other:?}"),
    }
}

fn print_doctor(resp: &SsgResponse) {
    println!("{}", resp.message);
    if resp.findings.is_empty() {
        return;
    }
    println!();
    println!("{}", format_findings_table(&resp.findings));
}

fn format_findings_table(findings: &[SsgDoctorFinding]) -> String {
    let mut lines = Vec::with_capacity(findings.len() + 1);
    lines.push(format!(
        "  {:<7} {:<20} {:<40} {}",
        "LEVEL".bold(),
        "SERVICE".bold(),
        "PATH".bold(),
        "MESSAGE".bold(),
    ));
    for f in findings {
        // Pad the uncolored severity to a fixed width first, then
        // colorize. ANSI escape codes count toward `{:<N}` width and
        // would throw the column alignment off otherwise.
        let padded = format!("{:<5}", f.severity);
        let level = match f.severity.as_str() {
            "ok" => padded.green().to_string(),
            "warn" => padded.yellow().to_string(),
            "info" => padded.cyan().to_string(),
            _ => padded,
        };
        let path = if f.path.is_empty() {
            "-".to_string()
        } else {
            f.path.clone()
        };
        lines.push(format!(
            "  {} {:<20} {:<40} {}",
            level, f.service, path, f.message
        ));
    }
    lines.join("\n")
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
    // Phase 31: VIRTUAL column appended after DYNAMIC so existing
    // scripts that parse `awk '{print $3}'` keep reading the dynamic
    // port (a daemon-internal sanity check). Consumer-routing
    // assertions read column 4. See `coast-ssg/DESIGN.md §24.5`.
    lines.push(format!(
        "  {:<20} {:<15} {:<15} {:<15} {}",
        "SERVICE".bold(),
        "CANONICAL".bold(),
        "DYNAMIC".bold(),
        "VIRTUAL".bold(),
        "STATUS".bold(),
    ));
    for port in ports {
        // Phase 6: show "(checked out)" when a host-side socat is
        // bound on the canonical port; otherwise blank so the eye
        // skips straight to the port numbers.
        let status = if port.checked_out {
            "(checked out)"
        } else {
            ""
        };
        // Phase 31: render `--` when the virtual port hasn't been
        // allocated yet (SSG not running, or pre-Phase 28 daemon).
        let virtual_port_display = match port.virtual_port {
            Some(v) => v.to_string(),
            None => "--".to_string(),
        };
        lines.push(format!(
            "  {:<20} {:<15} {:<15} {:<15} {}",
            port.service, port.canonical_port, port.dynamic_host_port, virtual_port_display, status,
        ));
    }
    lines.join("\n")
}

/// Phase 22 — render the `coast ssg ls` listings. One row per
/// per-project SSG known to the daemon.
fn format_listings_table(listings: &[SsgListing]) -> String {
    let mut lines = Vec::with_capacity(listings.len() + 1);
    lines.push(format!(
        "  {:<20} {:<10} {:<32} {:<14} {:<8}  {}",
        "PROJECT".bold(),
        "STATUS".bold(),
        "BUILD".bold(),
        "CONTAINER".bold(),
        "SERVICES".bold(),
        "CREATED".bold(),
    ));
    for row in listings {
        // Build and container id columns are opaque — truncate the
        // hash prefix to the first 12 chars so the row stays readable
        // without losing the identifying prefix.
        let build = row
            .build_id
            .as_deref()
            .map(truncate_for_table)
            .unwrap_or_else(|| "-".to_string());
        let container = row
            .container_id
            .as_deref()
            .map(truncate_for_table)
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "  {:<20} {:<10} {:<32} {:<14} {:<8}  {}",
            row.project, row.status, build, container, row.service_count, row.created_at,
        ));
    }
    lines.join("\n")
}

/// Truncate an opaque id (build id, container id) to a short prefix
/// that's still distinctive for visual scanning but fits in a table
/// column. Full ids remain available in the JSON protocol.
fn truncate_for_table(value: &str) -> String {
    const MAX: usize = 30;
    if value.len() <= MAX {
        value.to_string()
    } else {
        format!("{}...", &value[..MAX.saturating_sub(3)])
    }
}

/// Render `coast ssg builds-ls` rows. One row per build artifact
/// belonging to the requested project (filtered server-side by
/// `coastfile_hash`). The CREATED column shows the build's
/// `built_at` timestamp converted to local time; STATUS shows
/// `LATEST` / `PINNED` / both / blank.
fn format_builds_table(builds: &[coast_core::protocol::SsgBuildEntry]) -> String {
    let mut lines = Vec::with_capacity(builds.len() + 1);
    lines.push(format!(
        "  {:<35} {:<24} {:<22}  {}",
        "BUILD".bold(),
        "SERVICES".bold(),
        "CREATED".bold(),
        "STATUS".bold(),
    ));
    for row in builds {
        let services = if row.services.is_empty() {
            row.services_count.to_string()
        } else {
            row.services.join(",")
        };
        // Truncate at the field width minus the trailing ellipsis so
        // a service list like `postgres,redis,mongo,elasticsearch`
        // doesn't blow out the column. Full list is in the JSON.
        let services = if services.chars().count() > 24 {
            let mut shorter: String = services.chars().take(21).collect();
            shorter.push_str("...");
            shorter
        } else {
            services
        };
        let created = if row.created_at_unix > 0 {
            chrono::DateTime::<chrono::Local>::from(
                chrono::DateTime::<chrono::Utc>::from_timestamp(row.created_at_unix, 0)
                    .unwrap_or_default(),
            )
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
        } else {
            "-".to_string()
        };
        let status = match (row.latest, row.pinned) {
            (true, true) => "LATEST PINNED".green().to_string(),
            (true, false) => "LATEST".green().to_string(),
            (false, true) => "PINNED".yellow().to_string(),
            (false, false) => String::new(),
        };
        lines.push(format!(
            "  {:<35} {:<24} {:<22}  {}",
            row.build_id, services, created, status,
        ));
    }
    lines.join("\n")
}
