//! Phase 27 / 28 (§24): daemon-managed host socat supervisor.
//!
//! One long-lived `socat` process per
//! `(project, service_name, container_port)` SSG service port.
//! Listens on the stable `virtual_port` (from `ssg_virtual_ports`,
//! allocated in Phase 26) and forwards to
//! `host.docker.internal:<current_ssg_dyn_port>`. Consumers always
//! resolve their socat upstream to this virtual port — the host
//! supervisor is the single place where "current SSG dyn port"
//! surfaces, so an SSG rebuild is a one-process-argv swap here and
//! invisible to consumers.
//!
//! Phase 28 keys on `container_port` (not just service name) so
//! multi-port services (e.g. minio's 9000+9001) get one host socat
//! per published port, in lockstep with the per-port virtual port
//! allocation.
//!
//! Mirrors the in-DinD shell script pattern from
//! [`coast_docker::shared_service_routing::build_proxy_setup_script`]:
//! `nohup socat <listen> <upstream> >log 2>&1 & echo $! > <pidfile>`.
//! Keeping the same shape on both sides means one mental model for
//! debugging "which process terminates which tcp endpoint."

use std::path::{Path, PathBuf};

use tokio::process::Command;
use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};
use coast_ssg::state::SsgStateExt;

use crate::handlers::run::paths::{host_socat_paths, host_socats_dir};
use crate::handlers::ssg::virtual_port_allocator::{allocate_or_reuse, AllocatorConfig};
use crate::server::AppState;

// --- Public surface -------------------------------------------------

/// Spawn (or update) the host socat for
/// `(project, service, container_port)` so it forwards
/// `virtual_port` → `host.docker.internal:<dyn_port>`.
///
/// Idempotent. If a live pid already holds the target virtual port
/// AND its recorded upstream matches `dyn_port`, the call is a
/// no-op. Otherwise, kills the stale pid (if any) and respawns.
///
/// Errors:
/// - `socat` missing from host PATH.
/// - `socats/` directory cannot be created.
/// - Spawned socat died within the liveness window — reports the
///   logfile tail so the caller sees why.
pub async fn spawn_or_update(
    project: &str,
    service: &str,
    container_port: u16,
    virtual_port: u16,
    dyn_port: u16,
) -> Result<()> {
    preflight::check_socat_available()?;
    ensure_socats_dir()?;

    let (pidfile, logfile) = host_socat_paths(project, service, container_port);
    let args = socat_spawn_args(project, service, virtual_port, dyn_port, &pidfile, &logfile);

    // Idempotency: same live pid, same recorded argv → no-op.
    if is_already_running(&args) {
        debug!(
            project = %project,
            service = %service,
            container_port = container_port,
            virtual_port = virtual_port,
            dyn_port = dyn_port,
            "host socat already running with matching argv; skipping"
        );
        return Ok(());
    }

    // Different upstream or stale pidfile: kill-and-respawn.
    kill_if_alive(&pidfile);

    spawn_with_args(&args).await?;

    // Liveness probe: give socat ~100ms to fault. If it immediately
    // died (e.g. bad args, permission denied on the listen port),
    // surface the logfile tail so the caller gets a readable error.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let Some(pid) = pidfile::read_pid(&pidfile) else {
        return Err(CoastError::docker(format!(
            "host socat for '{project}/{service}:{container_port}' exited before writing \
             pidfile. Logfile tail:\n{}",
            tail_logfile(&logfile)
        )));
    };
    if !pidfile::is_alive(pid) {
        return Err(CoastError::docker(format!(
            "host socat for '{project}/{service}:{container_port}' (pid {pid}) exited \
             immediately. Logfile tail:\n{}",
            tail_logfile(&logfile)
        )));
    }

    info!(
        project = %project,
        service = %service,
        container_port = container_port,
        pid = pid,
        virtual_port = virtual_port,
        dyn_port = dyn_port,
        "spawned host socat"
    );
    Ok(())
}

/// Kill the host socat for `(project, service, container_port)` and
/// remove its pidfile. Idempotent — succeeds if the pidfile is
/// absent or the recorded pid is already dead.
pub fn kill(project: &str, service: &str, container_port: u16) -> Result<()> {
    let (pidfile, _) = host_socat_paths(project, service, container_port);
    kill_if_alive(&pidfile);
    Ok(())
}

/// Per-project sweep: for every `ssg_services` row in `project`,
/// allocate a stable virtual port (or reuse the persisted one), then
/// `spawn_or_update` a host socat that forwards the virtual port to
/// the current dyn port. Called by SSG run/start/restart immediately
/// after `apply_to_state_and_response` writes the fresh dyn ports.
///
/// Returns the list of `"<project>/<service>:<container_port>"`
/// labels that were brought up successfully. Per-service spawn
/// errors are logged at `warn!` and swallowed — one bad service
/// must not block the rest of the project. Allocator errors
/// (band exhaustion) DO propagate, since they're a project-level
/// failure.
pub async fn reconcile_project(
    state: &AppState,
    project: &str,
    config: &AllocatorConfig,
) -> Result<ProjectReconcileReport> {
    let plans = collect_project_plans(state, project, config).await?;
    let mut report = ProjectReconcileReport::default();
    for plan in plans {
        let label = format!(
            "{project}/{service}:{port}",
            service = plan.service,
            port = plan.container_port,
        );
        match spawn_or_update_with_retry(state, project, &plan, config).await {
            SpawnOutcome::Spawned { rebound_to } => {
                report.reconciled.push(label.clone());
                if let Some(new_vport) = rebound_to {
                    report.rebound.push(RebindNotice {
                        label,
                        old_virtual_port: plan.virtual_port,
                        new_virtual_port: new_vport,
                    });
                }
            }
            SpawnOutcome::Failed(err) => warn!(
                project = %project,
                service = %plan.service,
                container_port = plan.container_port,
                error = %err,
                "host socat reconcile_project: spawn failed after retry; leaving previous state"
            ),
        }
    }
    Ok(report)
}

/// Per-call result of `reconcile_project`. Consumers append the
/// `reconciled` labels to their progress message and the `rebound`
/// list to a warning section so the user knows running coasts may
/// need to re-run.
#[derive(Debug, Default, Clone)]
pub struct ProjectReconcileReport {
    /// `"<project>/<service>:<container_port>"` for every service
    /// whose host socat is now live (either freshly spawned or
    /// idempotently reconfirmed).
    pub reconciled: Vec<String>,
    /// Services whose virtual port had to be re-allocated mid-flight
    /// because the persisted port had been claimed by something
    /// outside Coast. Empty in the common case. Populated only
    /// after the second spawn attempt succeeded.
    pub rebound: Vec<RebindNotice>,
}

/// Phase 28 collision-rebind: one service whose virtual port shifted
/// during reconciliation. The daemon surfaces these notices to the
/// user so they know to re-run any local consumer that was already
/// pointing at the old virtual port.
#[derive(Debug, Clone)]
pub struct RebindNotice {
    pub label: String,
    pub old_virtual_port: u16,
    pub new_virtual_port: u16,
}

/// Outcome of spawning the host socat for one service, including the
/// collision-retry path.
#[derive(Debug)]
enum SpawnOutcome {
    /// Spawn succeeded. `rebound_to` is `Some(new_port)` only if the
    /// retry path fired and chose a new virtual port.
    Spawned { rebound_to: Option<u16> },
    /// Spawn failed even after the rebind retry.
    Failed(CoastError),
}

/// Phase 28 collision-rebind: try `spawn_or_update`; if it fails
/// (typically `EADDRINUSE` on the listen port), drop the persisted
/// virtual-port row, reallocate, and retry once. If the retry also
/// fails, surface the error.
///
/// Single-retry on any failure (Option B from the plan) — we don't
/// distinguish EADDRINUSE from other startup faults. Band exhaustion
/// would have failed at `allocate_or_reuse` before we got here, so
/// in practice EADDRINUSE is the only realistic mid-flight failure.
async fn spawn_or_update_with_retry(
    state: &AppState,
    project: &str,
    plan: &ProjectSpawnPlan,
    config: &AllocatorConfig,
) -> SpawnOutcome {
    if let Err(first_err) = spawn_or_update(
        project,
        &plan.service,
        plan.container_port,
        plan.virtual_port,
        plan.dyn_port,
    )
    .await
    {
        warn!(
            project = %project,
            service = %plan.service,
            container_port = plan.container_port,
            virtual_port = plan.virtual_port,
            error = %first_err,
            "host socat spawn failed; trying once more after reallocating the virtual port"
        );
        // Drop the persisted row so `allocate_or_reuse` is forced to
        // pick a fresh port (it would otherwise hand back the same
        // value it just persisted).
        let new_vport = {
            let db = state.db.lock().await;
            if let Err(err) =
                db.clear_ssg_virtual_port_one(project, &plan.service, plan.container_port)
            {
                return SpawnOutcome::Failed(err);
            }
            match allocate_or_reuse(&*db, project, &plan.service, plan.container_port, config) {
                Ok(p) => p,
                Err(err) => return SpawnOutcome::Failed(err),
            }
        };
        return match spawn_or_update(
            project,
            &plan.service,
            plan.container_port,
            new_vport,
            plan.dyn_port,
        )
        .await
        {
            Ok(()) => SpawnOutcome::Spawned {
                rebound_to: (new_vport != plan.virtual_port).then_some(new_vport),
            },
            Err(retry_err) => SpawnOutcome::Failed(retry_err),
        };
    }
    SpawnOutcome::Spawned { rebound_to: None }
}

/// One service-port plan resolved under the DB lock by
/// `reconcile_project`. Decoupled from the spawn loop so the lock is
/// released before the (possibly slow) spawn calls.
#[derive(Debug, Clone)]
struct ProjectSpawnPlan {
    service: String,
    container_port: u16,
    virtual_port: u16,
    dyn_port: u16,
}

async fn collect_project_plans(
    state: &AppState,
    project: &str,
    config: &AllocatorConfig,
) -> Result<Vec<ProjectSpawnPlan>> {
    let db = state.db.lock().await;
    let services = db.list_ssg_services(project)?;
    let mut out = Vec::with_capacity(services.len());
    for svc in services {
        let virtual_port =
            allocate_or_reuse(&*db, project, &svc.service_name, svc.container_port, config)?;
        out.push(ProjectSpawnPlan {
            service: svc.service_name,
            container_port: svc.container_port,
            virtual_port,
            dyn_port: svc.dynamic_host_port,
        });
    }
    Ok(out)
}

/// Reconciliation sweep: for every project with a running SSG,
/// join `ssg_services` × `ssg_virtual_ports` on `(service_name,
/// container_port)` and ensure a host socat is running with the
/// correct argv. Used at daemon startup to repair state after a
/// daemon crash or host reboot.
///
/// Returns the list of `"<project>/<service>"` labels that were
/// successfully reconciled. Per-service errors are logged at `warn!`
/// and swallowed — one broken service must not block the rest.
pub async fn reconcile_all(state: &AppState) -> Result<Vec<String>> {
    let entries = collect_reconcile_entries(state).await?;
    let mut reconciled = Vec::with_capacity(entries.len());
    for entry in entries {
        let ReconcileEntry {
            project,
            service,
            container_port,
            virtual_port,
            dyn_port,
        } = entry;
        let label = format!("{project}/{service}:{container_port}");
        match spawn_or_update(&project, &service, container_port, virtual_port, dyn_port).await {
            Ok(()) => reconciled.push(label),
            Err(err) => warn!(
                project = %project,
                service = %service,
                container_port = container_port,
                error = %err,
                "host socat reconcile: failed; leaving old state in place"
            ),
        }
    }
    Ok(reconciled)
}

/// One row in the reconcile join. Joining `ssg` × `ssg_services` ×
/// `ssg_virtual_ports` produces this shape, and `reconcile_all`
/// turns each row into a `spawn_or_update` call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReconcileEntry {
    project: String,
    service: String,
    container_port: u16,
    virtual_port: u16,
    dyn_port: u16,
}

// --- Internal types -------------------------------------------------

/// Args for a single host socat spawn. Split out so tests can
/// substitute `/usr/bin/env sleep 3600` for `/usr/bin/env socat ...`
/// without touching the supervisor lifecycle.
#[derive(Debug, Clone)]
struct SpawnArgs {
    /// First token of the spawn (e.g. `"/usr/bin/env"`). Quoted as
    /// one unit when inserted into the shell script.
    binary_path: String,
    /// Remaining argv passed verbatim to `binary_path`. Quoted
    /// per-element.
    extra_args: Vec<String>,
    pidfile: PathBuf,
    logfile: PathBuf,
}

/// Build the production (socat) spawn args for a given forwarding
/// target. Tests build `SpawnArgs` by hand.
fn socat_spawn_args(
    _project: &str,
    _service: &str,
    virtual_port: u16,
    dyn_port: u16,
    pidfile: &Path,
    logfile: &Path,
) -> SpawnArgs {
    let listen = format!("TCP-LISTEN:{virtual_port},fork,reuseaddr");
    let upstream = format!("TCP:host.docker.internal:{dyn_port}");
    SpawnArgs {
        binary_path: "/usr/bin/env".to_string(),
        extra_args: vec!["socat".to_string(), listen, upstream],
        pidfile: pidfile.to_path_buf(),
        logfile: logfile.to_path_buf(),
    }
}

// --- Spawn script builder (pure) ------------------------------------

/// Generate an `sh -c`-executable script that spawns `args.binary_path`
/// with `args.extra_args` as a detached background process, writes
/// the pid to `args.pidfile`, and streams stdout+stderr to
/// `args.logfile`.
///
/// Also writes `args.pidfile.argv` as a sidecar with the exact
/// command line used, so `is_already_running` can detect upstream
/// changes without re-reading `ps`.
fn build_spawn_script(args: &SpawnArgs) -> String {
    use coast_core::compose::shell_quote;

    let mut argv = vec![args.binary_path.clone()];
    argv.extend(args.extra_args.iter().cloned());
    let argv_quoted: Vec<String> = argv.iter().map(|a| shell_quote(a)).collect();
    let cmd = argv_quoted.join(" ");

    let pidfile_q = shell_quote(&args.pidfile.to_string_lossy());
    let argvfile_q = shell_quote(&format!("{}.argv", args.pidfile.display()));
    let logfile_q = shell_quote(&args.logfile.to_string_lossy());

    // `nohup ... &` detaches; `echo $!` writes the spawned pid to
    // the pidfile; an adjacent `.argv` file records the exact
    // command for future idempotency checks.
    format!(
        "set -eu\n\
         nohup {cmd} > {logfile_q} 2>&1 < /dev/null &\n\
         echo $! > {pidfile_q}\n\
         printf %s {argv_record} > {argvfile_q}\n",
        argv_record = shell_quote(&cmd),
    )
}

// --- Idempotency checks ---------------------------------------------

fn argv_sidecar_path(pidfile: &Path) -> PathBuf {
    let mut p = pidfile.to_path_buf();
    let new_name = format!(
        "{}.argv",
        pidfile.file_name().unwrap_or_default().to_string_lossy()
    );
    p.set_file_name(new_name);
    p
}

fn read_argv_sidecar(pidfile: &Path) -> Option<String> {
    std::fs::read_to_string(argv_sidecar_path(pidfile)).ok()
}

fn is_already_running(args: &SpawnArgs) -> bool {
    use coast_core::compose::shell_quote;

    let Some(pid) = pidfile::read_pid(&args.pidfile) else {
        return false;
    };
    if !pidfile::is_alive(pid) {
        return false;
    }
    let Some(recorded_argv) = read_argv_sidecar(&args.pidfile) else {
        return false;
    };

    let mut argv = vec![args.binary_path.clone()];
    argv.extend(args.extra_args.iter().cloned());
    let expected = argv
        .iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ");

    recorded_argv.trim() == expected
}

// --- Spawn + kill ---------------------------------------------------

async fn spawn_with_args(args: &SpawnArgs) -> Result<()> {
    let script = build_spawn_script(args);
    let output = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!("failed to spawn host socat supervisor shell: {e}"))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::docker(format!(
            "host socat spawn script exited with {}: {}",
            output.status, stderr
        )));
    }
    Ok(())
}

fn kill_if_alive(pidfile: &Path) {
    if let Some(pid) = pidfile::read_pid(pidfile) {
        if pidfile::is_alive(pid) {
            // SIGTERM first; give the process up to 500ms to exit.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                if !pidfile::is_alive(pid) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            // If still alive, escalate to SIGKILL.
            if pidfile::is_alive(pid) {
                warn!(pid, "host socat did not exit on SIGTERM; sending SIGKILL");
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }
    let _ = std::fs::remove_file(pidfile);
    let _ = std::fs::remove_file(argv_sidecar_path(pidfile));
}

// --- Filesystem bootstrapping ---------------------------------------

fn ensure_socats_dir() -> Result<()> {
    let dir = host_socats_dir();
    std::fs::create_dir_all(&dir).map_err(|e| CoastError::Io {
        message: format!("failed to create host socats dir '{}': {e}", dir.display()),
        path: dir.clone(),
        source: Some(e),
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        // Best effort: ignore if the host's fs doesn't support mode bits.
        let _ = std::fs::set_permissions(&dir, perms);
    }
    Ok(())
}

fn tail_logfile(logfile: &Path) -> String {
    const MAX_TAIL_BYTES: usize = 2048;
    let Ok(content) = std::fs::read_to_string(logfile) else {
        return "<no log captured>".to_string();
    };
    if content.len() <= MAX_TAIL_BYTES {
        return content;
    }
    content[content.len() - MAX_TAIL_BYTES..].to_string()
}

// --- Reconciliation -------------------------------------------------

/// Join `ssg` × `ssg_services` × `ssg_virtual_ports` in-memory so
/// the caller's iteration below is allocation-free and DB-locked
/// only during the collection window.
///
/// The join key is `(project, service_name, container_port)` —
/// matching the per-port allocator and host socat keying.
async fn collect_reconcile_entries(state: &AppState) -> Result<Vec<ReconcileEntry>> {
    let db = state.db.lock().await;
    let ssgs = db.list_ssgs()?;
    let mut out = Vec::new();
    for ssg in ssgs {
        if ssg.status != "running" {
            continue;
        }
        let services = db.list_ssg_services(&ssg.project)?;
        let vports = db.list_ssg_virtual_ports(&ssg.project)?;
        // Build a (service, container_port) → port lookup for the
        // virtual ports so the O(N*M) loop collapses to O(N + M).
        let vport_by_key: std::collections::HashMap<(String, u16), u16> = vports
            .into_iter()
            .map(|rec| ((rec.service_name, rec.container_port), rec.port))
            .collect();
        for svc in services {
            let key = (svc.service_name.clone(), svc.container_port);
            let Some(&virtual_port) = vport_by_key.get(&key) else {
                continue;
            };
            out.push(ReconcileEntry {
                project: ssg.project.clone(),
                service: svc.service_name,
                container_port: svc.container_port,
                virtual_port,
                dyn_port: svc.dynamic_host_port,
            });
        }
    }
    Ok(out)
}

// --- Submodules -----------------------------------------------------

mod pidfile {
    use std::path::Path;

    pub(super) fn read_pid(pidfile: &Path) -> Option<i32> {
        let content = std::fs::read_to_string(pidfile).ok()?;
        content.trim().parse::<i32>().ok()
    }

    #[allow(dead_code)]
    pub(super) fn write_pid(pidfile: &Path, pid: i32) -> std::io::Result<()> {
        std::fs::write(pidfile, pid.to_string())
    }

    /// Send signal 0 to `pid`. If the result is 0, the process
    /// exists and we have permission to signal it. Anything else
    /// means dead/not-ours.
    pub(super) fn is_alive(pid: i32) -> bool {
        if pid <= 1 {
            // Never treat init as one of our supervisees.
            return false;
        }
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
}

mod preflight {
    use coast_core::error::{CoastError, Result};

    /// Return Ok when a `socat` binary is on the host's PATH.
    /// Returns a user-friendly install hint otherwise.
    pub(super) fn check_socat_available() -> Result<()> {
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg("command -v socat >/dev/null 2>&1")
            .status();
        match output {
            Ok(status) if status.success() => Ok(()),
            _ => Err(CoastError::docker(
                "socat is required but not found on PATH; \
                 install via `brew install socat` on macOS or \
                 `sudo apt install socat` on Ubuntu",
            )),
        }
    }
}

// --- Tests ----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Mutex, OnceLock};

    use tempfile::TempDir;

    use crate::state::StateDb;
    use coast_ssg::state::SsgServiceRecord;

    /// Serializes tests that mutate `PATH`. Only one test in this
    /// file touches `PATH`, but the lock is scoped for future-
    /// proofing.
    ///
    /// NOTE: `COAST_HOME` is DELIBERATELY not mutated by any test
    /// in this file. Other tests in the crate read `COAST_HOME`
    /// without serialization (`server::tests::test_default_*`,
    /// `handlers::tests::test_artifact_coastfile_path_*`). Touching
    /// `COAST_HOME` under concurrent test threads breaks those
    /// tests. The spawn / kill tests below build pidfile paths in
    /// a tempdir directly and exercise the non-path-reading APIs
    /// (`spawn_with_args`, `kill_if_alive`) rather than the public
    /// `spawn_or_update` / `kill` entry points that compute paths
    /// from `COAST_HOME`. End-to-end coverage of those entry points
    /// lives in Phase 28's integration test.
    fn path_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Build a `SpawnArgs` for a `sleep` stand-in rooted in `tmp`.
    /// Uses `/usr/bin/env` as the binary so `build_spawn_script` is
    /// structurally identical to production. Defaults to
    /// `container_port = 5432` (postgres canonical) — tests that
    /// want a different port use `sleep_args_in_for`.
    fn sleep_args_in(tmp: &TempDir, project: &str, service: &str, seconds: u32) -> SpawnArgs {
        sleep_args_in_for(tmp, project, service, 5432, seconds)
    }

    /// Same as `sleep_args_in` but lets the caller pick a
    /// `container_port` so the pidfile/logfile stem is unique even
    /// for multi-port services.
    fn sleep_args_in_for(
        tmp: &TempDir,
        project: &str,
        service: &str,
        container_port: u16,
        seconds: u32,
    ) -> SpawnArgs {
        let stem = format!("{project}--{service}--{container_port}");
        SpawnArgs {
            binary_path: "/usr/bin/env".to_string(),
            extra_args: vec!["sleep".to_string(), seconds.to_string()],
            pidfile: tmp.path().join(format!("{stem}.pid")),
            logfile: tmp.path().join(format!("{stem}.log")),
        }
    }

    /// Poll the pidfile for up to 2s waiting for the shell subshell
    /// to flush the pid. Under concurrent test load a fixed sleep
    /// is flaky; this tight-poll keeps happy-path fast and failure
    /// bounded. Returns the parsed pid or panics on timeout.
    async fn await_pidfile(pidfile: &Path) -> i32 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(pid) = pidfile::read_pid(pidfile) {
                return pid;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "pidfile '{}' was not populated within 2s",
                    pidfile.display()
                );
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Poll `is_alive(pid)` for up to `timeout`, returning true as
    /// soon as it goes false. Used after `kill_if_alive` to wait
    /// out the SIGKILL-to-zombie-reap window on loaded hosts
    /// without flaking on fast machines.
    async fn await_not_alive(pid: i32, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if !pidfile::is_alive(pid) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        !pidfile::is_alive(pid)
    }

    // --- 1. Pure-function script builder test ---

    #[test]
    fn build_spawn_script_includes_all_inputs() {
        let args = SpawnArgs {
            binary_path: "/usr/bin/env".to_string(),
            extra_args: vec![
                "socat".to_string(),
                "TCP-LISTEN:42001,fork,reuseaddr".to_string(),
                "TCP:host.docker.internal:61851".to_string(),
            ],
            pidfile: PathBuf::from("/tmp/coast/socats/cg--postgres--5432.pid"),
            logfile: PathBuf::from("/tmp/coast/socats/cg--postgres--5432.log"),
        };
        let script = build_spawn_script(&args);

        assert!(script.contains("'/usr/bin/env' 'socat'"));
        assert!(script.contains("TCP-LISTEN:42001,fork,reuseaddr"));
        assert!(script.contains("TCP:host.docker.internal:61851"));
        assert!(script.contains("'/tmp/coast/socats/cg--postgres--5432.pid'"));
        assert!(script.contains("'/tmp/coast/socats/cg--postgres--5432.log'"));
        assert!(script.contains("cg--postgres--5432.pid.argv"));
        assert!(script.contains("nohup"));
        assert!(script.contains("echo $!"));
    }

    // --- 2-5. spawn_with_args + pidfile lifecycle (no COAST_HOME mutation) ---

    #[tokio::test]
    async fn spawn_records_pid() {
        let tmp = TempDir::new().unwrap();
        let args = sleep_args_in(&tmp, "cg", "postgres", 3600);

        spawn_with_args(&args).await.unwrap();
        let pid = await_pidfile(&args.pidfile).await;
        assert!(pidfile::is_alive(pid), "child sleep must still be alive");

        kill_if_alive(&args.pidfile);
        assert!(await_not_alive(pid, std::time::Duration::from_secs(2)).await);
    }

    #[tokio::test]
    async fn spawn_twice_with_same_args_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let args = sleep_args_in(&tmp, "cg", "postgres", 3600);

        spawn_with_args(&args).await.unwrap();
        let first_pid = await_pidfile(&args.pidfile).await;

        // With a live pid + matching argv sidecar, `is_already_running`
        // returns true — callers of `spawn_or_update` short-circuit.
        assert!(is_already_running(&args));

        kill_if_alive(&args.pidfile);
        assert!(await_not_alive(first_pid, std::time::Duration::from_secs(2)).await);
    }

    #[tokio::test]
    async fn upstream_change_kills_old_pid_and_respawns() {
        let tmp = TempDir::new().unwrap();
        let first = sleep_args_in(&tmp, "cg", "postgres", 3600);

        spawn_with_args(&first).await.unwrap();
        let first_pid = await_pidfile(&first.pidfile).await;
        assert!(pidfile::is_alive(first_pid));

        // Different argv → not considered already-running.
        let second = sleep_args_in(&tmp, "cg", "postgres", 7200);
        assert!(!is_already_running(&second));

        // Kill-and-respawn is the production `spawn_or_update` flow.
        kill_if_alive(&first.pidfile);
        assert!(await_not_alive(first_pid, std::time::Duration::from_secs(2)).await);

        spawn_with_args(&second).await.unwrap();
        let second_pid = await_pidfile(&second.pidfile).await;
        assert!(pidfile::is_alive(second_pid));
        assert_ne!(first_pid, second_pid);

        kill_if_alive(&second.pidfile);
    }

    #[tokio::test]
    async fn stale_pidfile_with_dead_process_is_cleaned() {
        let tmp = TempDir::new().unwrap();
        let args = sleep_args_in(&tmp, "cg", "postgres", 3600);

        // Seed a pidfile pointing at init (pid 1). `is_alive` treats
        // pid <= 1 as never-ours, so we never try to signal it.
        std::fs::write(&args.pidfile, "1").unwrap();
        assert!(!pidfile::is_alive(1));

        // `kill_if_alive` on this pidfile must be a no-op that still
        // clears the stale file.
        kill_if_alive(&args.pidfile);
        assert!(!args.pidfile.exists());
    }

    // --- 6. low-level kill path ---

    #[tokio::test]
    async fn kill_if_alive_removes_pidfile_and_process() {
        let tmp = TempDir::new().unwrap();
        let args = sleep_args_in(&tmp, "cg", "postgres", 3600);

        spawn_with_args(&args).await.unwrap();
        let pid = await_pidfile(&args.pidfile).await;
        assert!(pidfile::is_alive(pid));

        kill_if_alive(&args.pidfile);

        assert!(!args.pidfile.exists());
        assert!(await_not_alive(pid, std::time::Duration::from_secs(2)).await);
    }

    #[test]
    fn kill_if_alive_is_idempotent_when_pidfile_absent() {
        let tmp = TempDir::new().unwrap();
        let pidfile = tmp.path().join("never-existed.pid");
        // Must not panic or error.
        kill_if_alive(&pidfile);
    }

    // --- 7-9. reconcile_all collector (DB-only, no spawn) ---

    fn svc(project: &str, name: &str, container_port: u16, dyn_port: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            project: project.to_string(),
            service_name: name.to_string(),
            container_port,
            dynamic_host_port: dyn_port,
            status: "running".to_string(),
        }
    }

    fn app_state_with(db: StateDb) -> AppState {
        AppState::new_for_testing(db)
    }

    /// Convenience: keys reconcile entries by `(service, container_port)`
    /// so tests can assert on them by name without poking at the
    /// vector's order.
    fn entries_by_key(
        entries: &[ReconcileEntry],
    ) -> std::collections::HashMap<(String, u16), (String, u16, u16)> {
        entries
            .iter()
            .map(|e| {
                (
                    (e.service.clone(), e.container_port),
                    (e.project.clone(), e.virtual_port, e.dyn_port),
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn collect_reconcile_entries_yields_one_per_service_port() {
        let db = StateDb::open_in_memory().unwrap();
        db.upsert_ssg("cg", "running", Some("cid"), Some("b1"))
            .unwrap();
        db.upsert_ssg_service(&svc("cg", "postgres", 5432, 61851))
            .unwrap();
        db.upsert_ssg_service(&svc("cg", "redis", 6379, 54827))
            .unwrap();
        db.upsert_ssg_virtual_port("cg", "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port("cg", "redis", 6379, 42002)
            .unwrap();
        let state = app_state_with(db);

        let entries = collect_reconcile_entries(&state).await.unwrap();
        assert_eq!(entries.len(), 2);

        let by_key = entries_by_key(&entries);
        assert_eq!(
            by_key.get(&("postgres".to_string(), 5432)),
            Some(&("cg".to_string(), 42001, 61851))
        );
        assert_eq!(
            by_key.get(&("redis".to_string(), 6379)),
            Some(&("cg".to_string(), 42002, 54827))
        );
    }

    // NOTE: A multi-port service (e.g. minio publishing 9000+9001)
    // cannot currently produce two `ssg_services` rows because that
    // table is keyed `(project, service_name)`. The per-port keying
    // on `ssg_virtual_ports` is forward-looking — once a future
    // phase amends `ssg_services` to be per-port, no schema change
    // is needed here. See DESIGN.md §24 for the multi-port note.

    #[tokio::test]
    async fn collect_reconcile_entries_skips_services_without_virtual_port() {
        let db = StateDb::open_in_memory().unwrap();
        db.upsert_ssg("cg", "running", Some("cid"), Some("b1"))
            .unwrap();
        db.upsert_ssg_service(&svc("cg", "postgres", 5432, 61851))
            .unwrap();
        // No upsert_ssg_virtual_port call → service skipped.
        let state = app_state_with(db);

        let entries = collect_reconcile_entries(&state).await.unwrap();
        assert!(entries.is_empty(), "no virtual port => skipped");
    }

    #[tokio::test]
    async fn collect_reconcile_entries_skips_ssgs_not_running() {
        let db = StateDb::open_in_memory().unwrap();
        db.upsert_ssg("cg", "stopped", Some("cid"), Some("b1"))
            .unwrap();
        db.upsert_ssg_service(&svc("cg", "postgres", 5432, 61851))
            .unwrap();
        db.upsert_ssg_virtual_port("cg", "postgres", 5432, 42001)
            .unwrap();
        let state = app_state_with(db);

        let entries = collect_reconcile_entries(&state).await.unwrap();
        assert!(entries.is_empty(), "stopped SSG => skipped");
    }

    #[tokio::test]
    async fn collect_reconcile_entries_isolates_projects() {
        let db = StateDb::open_in_memory().unwrap();
        db.upsert_ssg("cg", "running", Some("cid-cg"), Some("b1"))
            .unwrap();
        db.upsert_ssg("filemap", "running", Some("cid-fm"), Some("b1"))
            .unwrap();
        db.upsert_ssg_service(&svc("cg", "postgres", 5432, 61851))
            .unwrap();
        db.upsert_ssg_service(&svc("filemap", "postgres", 5432, 61900))
            .unwrap();
        db.upsert_ssg_virtual_port("cg", "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port("filemap", "postgres", 5432, 42002)
            .unwrap();
        let state = app_state_with(db);

        let entries = collect_reconcile_entries(&state).await.unwrap();
        assert_eq!(entries.len(), 2);

        let by_project: std::collections::HashMap<_, _> = entries
            .iter()
            .map(|e| (e.project.as_str(), (e.virtual_port, e.dyn_port)))
            .collect();
        assert_eq!(by_project.get("cg"), Some(&(42001, 61851)));
        assert_eq!(by_project.get("filemap"), Some(&(42002, 61900)));
    }

    // --- 9b. Phase 28 collision-rebind state mutations ---

    #[tokio::test]
    async fn collision_rebind_replaces_persisted_virtual_port_atomically() {
        // Phase 28: when `spawn_or_update_with_retry` decides to
        // rebind, it (a) drops the persisted row via
        // `clear_ssg_virtual_port_one`, then (b) calls
        // `allocate_or_reuse` again. We can exercise the state-level
        // contract directly without running socat.
        use crate::handlers::ssg::virtual_port_allocator::{allocate_or_reuse, AllocatorConfig};

        let db = StateDb::open_in_memory().unwrap();
        let cfg = AllocatorConfig {
            band_start: 42500,
            band_end: 42510,
        };

        // First allocation: persists row.
        let first = allocate_or_reuse(&db, "cg", "postgres", 5432, &cfg).unwrap();
        assert_eq!(
            db.get_ssg_virtual_port("cg", "postgres", 5432).unwrap(),
            Some(first)
        );

        // Simulate the rebind path:
        db.clear_ssg_virtual_port_one("cg", "postgres", 5432)
            .unwrap();
        assert!(db
            .get_ssg_virtual_port("cg", "postgres", 5432)
            .unwrap()
            .is_none());

        // Pre-bind the previously-allocated port so the allocator
        // skips it and chooses a different one.
        let _blocker = std::net::TcpListener::bind(("0.0.0.0", first)).unwrap();
        let second = allocate_or_reuse(&db, "cg", "postgres", 5432, &cfg).unwrap();
        assert_ne!(second, first, "rebind must pick a fresh port");
        assert_eq!(
            db.get_ssg_virtual_port("cg", "postgres", 5432).unwrap(),
            Some(second)
        );
    }

    #[tokio::test]
    async fn reconcile_project_returns_empty_report_when_no_services() {
        // Phase 28: a project with no `ssg_services` rows produces an
        // empty report — neither `reconciled` nor `rebound` is
        // populated, no spawn is attempted.
        use crate::handlers::ssg::virtual_port_allocator::AllocatorConfig;

        let db = StateDb::open_in_memory().unwrap();
        // No upsert_ssg_service calls — services list is empty.
        let state = app_state_with(db);

        let cfg = AllocatorConfig::default();
        let report = reconcile_project(&state, "cg", &cfg).await.unwrap();
        assert!(report.reconciled.is_empty());
        assert!(report.rebound.is_empty());
    }

    // --- 10. preflight ---

    #[test]
    fn preflight_errors_when_socat_missing() {
        let _guard = path_env_lock();
        let tmp = TempDir::new().unwrap();
        let saved = std::env::var_os("PATH");
        // Safety: serialized by `path_env_lock`; restored below.
        unsafe {
            std::env::set_var("PATH", tmp.path());
        }

        let err = preflight::check_socat_available()
            .expect_err("empty PATH must fail")
            .to_string();
        assert!(
            err.contains("install via") || err.contains("brew install socat"),
            "expected install hint: {err}"
        );

        unsafe {
            match saved {
                Some(v) => std::env::set_var("PATH", v),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}
