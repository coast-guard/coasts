// coastd — the coast daemon process.
//
// Runs as a background daemon (or in foreground with `--foreground`),
// listening on a Unix domain socket for CLI requests. Manages coast
// instances, port forwarding, shared services, and state.
rust_i18n::i18n!("../coast-i18n/locales", fallback = "en");

use std::sync::Arc;

use clap::Parser;
use nix::fcntl::{Flock, FlockArg};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use coast_core::error::Result;

mod analytics;
pub mod api;
mod bare_services;
mod dns;
mod docker_watcher;
mod docs_assets;
mod git_watcher;
mod handlers;
#[allow(dead_code)]
mod image_loader;
#[allow(dead_code)]
mod port_manager;
mod remote_stats;
pub mod server;
#[allow(dead_code)]
mod shared_services;
#[allow(dead_code)]
mod state;
#[cfg(test)]
mod test_support;

use server::AppState;
use state::StateDb;

/// Coast daemon — manages coast instances and services.
#[derive(Parser, Debug)]
#[command(name = "coastd", about = "Coast daemon process")]
struct Cli {
    /// Run in foreground instead of daemonizing.
    #[arg(long)]
    foreground: bool,

    /// Custom socket path (default: ~/.coast/coastd.sock).
    #[arg(long)]
    socket: Option<String>,

    /// HTTP API port (default: 31415, env: COAST_API_PORT).
    #[arg(long, env = "COAST_API_PORT")]
    api_port: Option<u16>,

    /// DNS server port for localcoast resolution (default: 5354, env: COAST_DNS_PORT).
    #[arg(long, env = "COAST_DNS_PORT")]
    dns_port: Option<u16>,
}

/// Main entry point for the coast daemon. Call this from your binary's main().
pub fn run() {
    ensure_host_tool_paths();
    let cli = Cli::parse();

    if cli.foreground {
        // Run directly in the foreground
        run_foreground(cli);
    } else {
        // Daemonize: fork, setsid, then run
        daemonize(cli);
    }
}

#[cfg(target_os = "macos")]
fn ensure_host_tool_paths() {
    let current_path = std::env::var_os("PATH");
    let existing_entries: Vec<std::path::PathBuf> = current_path
        .as_ref()
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();

    let updated_entries = extend_path_entries(
        existing_entries.clone(),
        macos_host_tool_candidates()
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_dir()),
    );

    if updated_entries == existing_entries {
        return;
    }

    match std::env::join_paths(&updated_entries) {
        Ok(path) => {
            unsafe {
                std::env::set_var("PATH", &path);
            }
            tracing::debug!(path = %path.to_string_lossy(), "updated PATH with macOS host tool directories");
        }
        Err(error) => {
            warn!(error = %error, "failed to join augmented PATH entries");
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_host_tool_paths() {}

#[cfg(any(target_os = "macos", test))]
fn extend_path_entries<I>(
    mut existing_entries: Vec<std::path::PathBuf>,
    candidates: I,
) -> Vec<std::path::PathBuf>
where
    I: IntoIterator<Item = std::path::PathBuf>,
{
    for candidate in candidates {
        if !existing_entries.iter().any(|entry| entry == &candidate) {
            existing_entries.push(candidate);
        }
    }

    existing_entries
}

#[cfg(target_os = "macos")]
fn macos_host_tool_candidates() -> &'static [&'static str] {
    &["/opt/homebrew/bin", "/usr/local/bin"]
}

/// Daemonize the process using fork + setsid.
fn daemonize(cli: Cli) {
    use nix::unistd::{fork, setsid, ForkResult};

    // Safety: we fork before starting any threads or async runtime
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Parent: print the child PID and exit
            println!("coastd started (pid: {child})");
            std::process::exit(0);
        }
        Ok(ForkResult::Child) => {
            // Child: create a new session
            if let Err(e) = setsid() {
                eprintln!("setsid failed: {e}");
                std::process::exit(1);
            }

            // Redirect stdin/stdout/stderr to /dev/null
            redirect_stdio();

            // Run the server
            run_foreground(cli);
        }
        Err(e) => {
            eprintln!("fork failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Redirect standard file descriptors to /dev/null for daemon mode.
fn redirect_stdio() {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    if let Ok(devnull) = OpenOptions::new().read(true).write(true).open("/dev/null") {
        let fd = devnull.as_raw_fd();
        // dup2 to stdin, stdout, stderr
        let _ = nix::unistd::dup2(fd, 0);
        let _ = nix::unistd::dup2(fd, 1);
        let _ = nix::unistd::dup2(fd, 2);
    }
}

/// Run the daemon in the foreground (also used after daemonize).
fn run_foreground(cli: Cli) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // When daemonized, stderr is /dev/null so write logs to $COAST_HOME/coastd.log.
    // In foreground mode, write to stderr as usual.
    let coast_dir =
        coast_core::artifact::coast_home().unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
    let log_path = coast_dir.join("coastd.log");

    if !cli.foreground {
        if let Ok(log_file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(false)
                .with_ansi(false)
                .with_writer(log_file)
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(false)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .init();
    }

    // Build the tokio runtime
    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    runtime.block_on(async move {
        if let Err(e) = run_daemon(cli).await {
            error!("coastd fatal error: {e}");
            std::process::exit(1);
        }
    });
}

/// Main daemon logic — initialize state, start server, handle shutdown.
async fn run_daemon(cli: Cli) -> Result<()> {
    // Ensure ~/.coast/ directory exists
    let coast_dir = server::ensure_coast_dir()?;
    info!(path = %coast_dir.display(), "coast directory ready");

    // Determine socket path
    let socket_path = match cli.socket {
        Some(ref p) => std::path::PathBuf::from(p),
        None => server::default_socket_path()?,
    };

    // Acquire exclusive flock to enforce single-instance.
    // The lock is held for the lifetime of the process -- the kernel releases
    // it automatically on exit (including SIGKILL/OOM).
    let lock_path = coast_dir.join("coastd.lock");
    let _lock_file = acquire_singleton_lock(&lock_path)?;

    // Determine PID file path
    let pid_path = server::default_pid_path()?;

    // Write PID file
    server::write_pid_file(&pid_path)?;

    // Clean up any orphaned socat/SSH processes from a previous daemon session
    port_manager::cleanup_orphaned_socat();
    handlers::remote::tunnel::cleanup_orphaned_ssh_tunnels();
    if port_manager::running_in_wsl() {
        port_manager::cleanup_orphaned_checkout_bridges();
    }

    // Open state database
    let db_path = coast_dir.join("state.db");
    let db = StateDb::open(&db_path)?;
    info!(path = %db_path.display(), "state database opened");

    // Create shared application state
    let state = Arc::new(AppState::new(db));

    // Start background git watcher (polls .git/HEAD for known projects)
    git_watcher::spawn_git_watcher(Arc::clone(&state));

    // Start background Docker connectivity watcher
    docker_watcher::spawn_docker_watcher(Arc::clone(&state));

    // Start background remote stats poller
    remote_stats::spawn_remote_stats_poller(Arc::clone(&state));

    // Set up shutdown signal handling
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    // Spawn signal handler
    let signal_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = wait_for_shutdown_signal().await {
            error!("signal handler error: {e}");
        }
        let _ = signal_tx.send(());
    });

    // Determine API port
    let api_port = cli.api_port.unwrap_or(api::DEFAULT_API_PORT);

    // Start the HTTP API server *before* kicking off restore work. This
    // decouples control-plane availability from the restore path: even if
    // restore_running_state is slow (e.g. stalled on an unreachable remote),
    // the API and Unix socket still bind promptly so `coast ls`, daemon
    // status probes, and local operations keep working.
    let api_state = Arc::clone(&state);
    let api_shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let app = api::api_router(api_state);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], api_port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("failed to bind HTTP API on port {api_port}: {e}");
                return;
            }
        };
        info!(port = api_port, "HTTP API server listening");

        let mut shutdown = api_shutdown_rx;
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown.recv().await;
            })
            .await
            .unwrap_or_else(|e| error!("HTTP API server error: {e}"));
    });

    // Start the embedded DNS server (resolves *.localcoast -> 127.0.0.1)
    let dns_port = cli.dns_port.unwrap_or(5354);
    tokio::spawn(async move {
        dns::run_dns_server(dns_port).await;
    });

    // Run restore in the background. Any subsystem that needs restored
    // state and can't survive its absence must wait for it explicitly —
    // but most operations either tolerate partial state (restart, rm)
    // or re-establish it lazily on first use (exec, logs).
    let restore_state = Arc::clone(&state);
    tokio::spawn(async move {
        restore_running_state(&restore_state).await;
        info!("restore_running_state complete");
    });

    // Run the Unix socket server (blocks until shutdown)
    let result = server::run_server(&socket_path, state, shutdown_rx).await;

    // Cleanup
    server::remove_pid_file(&pid_path)?;

    result
}

/// Acquire an exclusive flock on `coastd.lock` to enforce single-instance.
///
/// Returns the `Flock<File>` guard. The caller MUST keep it alive for the
/// entire daemon lifetime — dropping it releases the lock.
fn acquire_singleton_lock(lock_path: &std::path::Path) -> Result<Flock<std::fs::File>> {
    use coast_core::error::CoastError;

    let lock_file = std::fs::File::create(lock_path).map_err(|e| CoastError::Io {
        message: format!("failed to create lock file '{}': {e}", lock_path.display()),
        path: lock_path.to_path_buf(),
        source: Some(e),
    })?;

    let guard = Flock::lock(lock_file, FlockArg::LockExclusiveNonblock).map_err(|_| {
        CoastError::io_simple(
            "another coastd is already running. \
             Use `coast daemon kill` to stop it, or `coast daemon restart` to replace it.",
        )
    })?;

    info!(path = %lock_path.display(), "singleton lock acquired");
    Ok(guard)
}

/// Background loop that keeps the shared services response cache warm.
async fn shared_services_cache_loop(state: Arc<server::AppState>) {
    loop {
        let projects: Vec<String> = {
            let db = state.db.lock().await;
            db.list_shared_services(None)
                .unwrap_or_default()
                .into_iter()
                .filter(|s| s.status == "running")
                .map(|s| s.project)
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        };
        for project in &projects {
            if let Ok(resp) = handlers::shared::fetch_shared_services(project, &state).await {
                let mut cache = state.shared_services_cache.lock().await;
                cache.insert(project.clone(), (tokio::time::Instant::now(), resp));
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

/// Background loop that keeps the per-instance service health cache warm.
async fn service_health_cache_loop(state: Arc<server::AppState>) {
    loop {
        let running: Vec<(String, String)> = {
            let db = state.db.lock().await;
            db.list_instances()
                .unwrap_or_default()
                .into_iter()
                .filter(|i| {
                    matches!(
                        i.status,
                        coast_core::types::InstanceStatus::Running
                            | coast_core::types::InstanceStatus::CheckedOut
                            | coast_core::types::InstanceStatus::Idle
                    )
                })
                .map(|i| (i.project, i.name))
                .collect()
        };
        for (project, name) in &running {
            let req = coast_core::protocol::PsRequest {
                project: project.clone(),
                name: name.clone(),
            };
            let key = format!("{project}:{name}");
            match handlers::ps::handle(req, &state).await {
                Ok(resp) => {
                    let down = resp
                        .services
                        .iter()
                        .filter(|s| !s.status.starts_with("running"))
                        .count() as u32;
                    state.service_health_cache.lock().await.insert(key, down);
                }
                Err(_) => {
                    state.service_health_cache.lock().await.remove(&key);
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }
}

/// Load healthcheck paths from the build artifact coastfile for a project.
fn load_healthcheck_paths(project: &str) -> std::collections::HashMap<String, String> {
    let home = dirs::home_dir().unwrap_or_default();
    let images_dir = home.join(".coast").join("images").join(project);
    for link_name in &["latest", "latest-remote"] {
        let cf_path = images_dir.join(link_name).join("coastfile.toml");
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&cf_path) {
            if !cf.healthcheck.is_empty() {
                return cf.healthcheck;
            }
        }
    }
    std::collections::HashMap::new()
}

/// Background loop that probes each port's dynamic_port every 5 seconds.
/// Uses HTTP GET for ports with a `[healthcheck]` path configured, falls back
/// to TCP connect for ports without one. Any HTTP response = healthy.
///
/// For remote instances, when all ports go unhealthy, automatically kills
/// stale SSH tunnels and re-establishes them (auto-heal).
async fn port_health_cache_loop(state: Arc<server::AppState>) {
    use coast_core::types::PortHealthStatus;
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();

    let mut tunnel_heal_cooldown: std::collections::HashMap<String, tokio::time::Instant> =
        std::collections::HashMap::new();

    loop {
        let running: Vec<(String, String, Option<String>)> = {
            let db = state.db.lock().await;
            db.list_instances()
                .unwrap_or_default()
                .into_iter()
                .filter(|i| {
                    matches!(
                        i.status,
                        coast_core::types::InstanceStatus::Running
                            | coast_core::types::InstanceStatus::CheckedOut
                            | coast_core::types::InstanceStatus::Idle
                    )
                })
                .map(|i| (i.project, i.name, i.remote_host))
                .collect()
        };
        for (project, name, remote_host) in &running {
            let healthcheck_paths = load_healthcheck_paths(project);
            let allocs = {
                let db = state.db.lock().await;
                db.get_port_allocations(project, name).unwrap_or_default()
            };
            let key = format!("{project}:{name}");
            let mut statuses: Vec<PortHealthStatus> = Vec::new();

            let remote_tunnels_dead = if remote_host.is_some() && !allocs.is_empty() {
                let pattern = format!("ssh -N -L {}:", allocs[0].dynamic_port);
                let result = tokio::process::Command::new("pgrep")
                    .args(["-f", &pattern])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await;
                match result {
                    Ok(status) => !status.success(),
                    Err(_) => false,
                }
            } else {
                false
            };

            for alloc in &allocs {
                let port = alloc.dynamic_port;
                let mapping: coast_core::types::PortMapping = alloc.into();

                let healthy = if remote_tunnels_dead {
                    false
                } else if let Some(path) = healthcheck_paths.get(&mapping.logical_name) {
                    let url = format!("http://127.0.0.1:{}{}", port, path);
                    http_client.get(&url).send().await.is_ok()
                } else {
                    tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        tokio::net::TcpStream::connect(("127.0.0.1", port)),
                    )
                    .await
                    .map(|r| r.is_ok())
                    .unwrap_or(false)
                };

                statuses.push(PortHealthStatus {
                    logical_name: mapping.logical_name,
                    canonical_port: mapping.canonical_port,
                    dynamic_port: mapping.dynamic_port,
                    is_primary: mapping.is_primary,
                    healthy,
                });
            }
            let changed = {
                let cache = state.port_health_cache.lock().await;
                match cache.get(&key) {
                    Some(prev) => {
                        prev.len() != statuses.len()
                            || prev
                                .iter()
                                .zip(statuses.iter())
                                .any(|(a, b)| a.healthy != b.healthy)
                    }
                    None => true,
                }
            };

            let port_count = statuses.len();
            let healthy_count = statuses.iter().filter(|s| s.healthy).count();

            let dead_dynamic_ports: Vec<u16> = statuses
                .iter()
                .filter(|s| !s.healthy)
                .map(|s| s.dynamic_port)
                .collect();

            state
                .port_health_cache
                .lock()
                .await
                .insert(key.clone(), statuses);
            if changed {
                state.emit_event(coast_core::protocol::CoastEvent::PortHealthChanged {
                    name: name.clone(),
                    project: project.clone(),
                });
            }

            let unhealthy_count = port_count - healthy_count;
            if remote_host.is_some() && unhealthy_count > 0 {
                if healthy_count == 0 {
                    heal_unhealthy_instance(
                        &state,
                        project,
                        name,
                        &key,
                        &dead_dynamic_ports,
                        &mut tunnel_heal_cooldown,
                    )
                    .await;
                } else {
                    heal_partial_tunnels(
                        &state,
                        project,
                        name,
                        &key,
                        &allocs,
                        &dead_dynamic_ports,
                        &mut tunnel_heal_cooldown,
                    )
                    .await;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

/// Re-establish SSH tunnels for specific unhealthy ports on a remote instance
/// where some other ports are still healthy (partial failure).
async fn heal_partial_tunnels(
    state: &Arc<server::AppState>,
    project: &str,
    name: &str,
    key: &str,
    allocs: &[state::PortAllocationRecord],
    dead_ports: &[u16],
    cooldown: &mut std::collections::HashMap<String, tokio::time::Instant>,
) {
    let now = tokio::time::Instant::now();
    let cooldown_ok = cooldown
        .get(key)
        .map(|last| now.duration_since(*last).as_secs() >= 30)
        .unwrap_or(true);

    if !cooldown_ok {
        return;
    }

    let tunnel_pairs: Vec<(u16, u16)> = allocs
        .iter()
        .filter(|a| dead_ports.contains(&a.dynamic_port))
        .filter_map(|a| a.remote_dynamic_port.map(|rdp| (a.dynamic_port, rdp)))
        .collect();

    if tunnel_pairs.is_empty() {
        return;
    }

    warn!(
        instance = %name,
        project = %project,
        dead = dead_ports.len(),
        "some remote ports unhealthy — re-establishing missing tunnels"
    );
    cooldown.insert(key.to_owned(), now);

    handlers::remote::tunnel::kill_tunnels_for_ports(dead_ports);

    if let Ok(remote_config) =
        handlers::remote::resolve_remote_for_instance(project, name, state).await
    {
        forward_and_log_tunnels(&remote_config, &tunnel_pairs, name).await;
    }
}

/// Heal a single remote instance whose ports are all unhealthy.
///
/// Kills only the SSH tunnel processes for this instance's dynamic ports,
/// invalidates the tunnel cache for the specific remote host, and
/// re-establishes both forward and reverse tunnels.
async fn heal_unhealthy_instance(
    state: &Arc<server::AppState>,
    project: &str,
    name: &str,
    key: &str,
    dynamic_ports: &[u16],
    cooldown: &mut std::collections::HashMap<String, tokio::time::Instant>,
) {
    let now = tokio::time::Instant::now();
    let cooldown_ok = cooldown
        .get(key)
        .map(|last| now.duration_since(*last).as_secs() >= 30)
        .unwrap_or(true);

    if !cooldown_ok {
        return;
    }

    warn!(
        instance = %name,
        project = %project,
        ports = dynamic_ports.len(),
        "all remote ports unhealthy — re-establishing SSH tunnels"
    );
    cooldown.insert(key.to_owned(), now);

    handlers::remote::tunnel::kill_tunnels_for_ports(dynamic_ports);

    if let Ok(remote_config) =
        handlers::remote::resolve_remote_for_instance(project, name, state).await
    {
        handlers::remote::tunnel::invalidate_cache_for_host(&remote_config).await;
    }

    heal_remote_tunnels(state, project, name).await;
    heal_shared_service_tunnels(state, project).await;
}

/// Re-establish shared service reverse tunnels for a specific remote instance.
async fn heal_shared_service_tunnels(state: &Arc<server::AppState>, project: &str) {
    let inline_pairs = shared_service_reverse_pairs(project);

    let (remotes, instances) = {
        let db = state.db.lock().await;
        (
            db.list_remotes().unwrap_or_default(),
            db.list_instances().unwrap_or_default(),
        )
    };
    let inst = instances
        .iter()
        .find(|i| i.project == project && i.remote_host.is_some());
    let Some(inst) = inst else {
        return;
    };
    let remote_host = inst.remote_host.as_deref().unwrap();
    let entry = remotes
        .iter()
        .find(|r| r.name == remote_host || r.host == remote_host);
    let Some(entry) = entry else {
        return;
    };

    let connection = coast_core::types::RemoteConnection::from_entry(
        entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );

    if !inline_pairs.is_empty() {
        match handlers::remote::tunnel::reverse_forward_ports(&connection, &inline_pairs).await {
            Ok(pids) => {
                tracing::info!(
                    project = %project,
                    tunnels = inline_pairs.len(),
                    pids = ?pids,
                    "healed inline shared service reverse tunnels"
                );
            }
            Err(e) => {
                tracing::warn!(
                    project = %project,
                    error = %e,
                    "failed to heal inline shared service reverse tunnels"
                );
            }
        }
    }

    // Phase 30: also heal SSG-shared reverse tunnels (one per
    // `(project, remote_host, service, container_port)` quad). For
    // each persisted row, probe the recorded ssh pid; respawn and
    // update the pid when the previous child has died.
    heal_ssg_shared_tunnels(state, project, &connection, &entry.host).await;
}

/// Phase 30 heal pass for SSG shared reverse tunnels. Reads
/// `ssg_shared_tunnels` for `(project, remote_host)`, probes each
/// row's recorded `ssh_pid`, and respawns any that have died. The
/// virtual port is preserved across the respawn so consumers never
/// see the local upstream change.
async fn heal_ssg_shared_tunnels(
    state: &Arc<server::AppState>,
    project: &str,
    connection: &coast_core::types::RemoteConnection,
    remote_host: &str,
) {
    respawn_dead_ssg_shared_tunnels(state, project, connection, remote_host, "phase 30 heal").await;
}

/// Phase 30 shared respawn loop used by both `heal_ssg_shared_tunnels`
/// (steady-state recovery) and `restore_ssg_shared_tunnels` (daemon
/// startup). Reads the persisted `ssg_shared_tunnels` rows for
/// `(project, remote_host)`, identifies any whose ssh pid is dead,
/// respawns them via `reverse_forward_ports`, and updates the pid
/// column. Errors are logged with the `caller_tag` prefix and
/// swallowed — one bad row must not block the rest.
async fn respawn_dead_ssg_shared_tunnels(
    state: &Arc<server::AppState>,
    project: &str,
    connection: &coast_core::types::RemoteConnection,
    remote_host: &str,
    caller_tag: &'static str,
) {
    let to_respawn = match collect_dead_ssg_shared_tunnels(state, project, remote_host).await {
        Ok(items) => items,
        Err(err) => {
            tracing::warn!(
                project = %project,
                remote = %remote_host,
                error = %err,
                caller = %caller_tag,
                "failed to list ssg_shared_tunnels"
            );
            return;
        }
    };
    if to_respawn.is_empty() {
        return;
    }

    let pairs: Vec<(u16, u16)> = to_respawn.iter().map(|(_, _, vp)| (*vp, *vp)).collect();
    let pids = match handlers::remote::tunnel::reverse_forward_ports(connection, &pairs).await {
        Ok(pids) => pids,
        Err(err) => {
            tracing::warn!(
                project = %project,
                remote = %remote_host,
                error = %err,
                caller = %caller_tag,
                "failed to respawn SSG shared tunnels"
            );
            return;
        }
    };

    persist_respawned_ssg_pids(state, project, remote_host, &to_respawn, &pids, caller_tag).await;
    tracing::info!(
        project = %project,
        remote = %remote_host,
        respawned = pids.len(),
        caller = %caller_tag,
        "respawned SSG shared tunnels"
    );
}

/// Phase 30: write the freshly-spawned ssh PIDs back into
/// `ssg_shared_tunnels`. Errors per row are logged and swallowed so
/// one bad write doesn't lose the rest.
async fn persist_respawned_ssg_pids(
    state: &Arc<server::AppState>,
    project: &str,
    remote_host: &str,
    to_respawn: &[(String, u16, u16)],
    pids: &[u32],
    caller_tag: &'static str,
) {
    use coast_ssg::state::SsgStateExt;
    let db = state.db.lock().await;
    for ((service, container_port, _vport), pid) in to_respawn.iter().zip(pids.iter()) {
        if let Err(err) = db.update_ssg_shared_tunnel_pid(
            project,
            remote_host,
            service,
            *container_port,
            Some(*pid as i32),
        ) {
            tracing::warn!(
                project = %project,
                remote = %remote_host,
                service = %service,
                container_port,
                error = %err,
                caller = %caller_tag,
                "failed to update ssg_shared_tunnel pid"
            );
        }
    }
}

/// Phase 30: snapshot every `ssg_shared_tunnels` row for
/// `(project, remote_host)` whose recorded `ssh_pid` is dead (or
/// `None`). Returns a vec of `(service_name, container_port,
/// virtual_port)` tuples ready to feed into `reverse_forward_ports`.
async fn collect_dead_ssg_shared_tunnels(
    state: &Arc<server::AppState>,
    project: &str,
    remote_host: &str,
) -> coast_core::error::Result<Vec<(String, u16, u16)>> {
    use coast_ssg::state::SsgStateExt;
    let db = state.db.lock().await;
    let rows = db.list_ssg_shared_tunnels_for_remote(project, remote_host)?;
    let mut out = Vec::new();
    for row in rows {
        let alive = row.ssh_pid.is_some_and(handlers::run::is_pid_alive);
        if !alive {
            out.push((row.service_name, row.container_port, row.virtual_port));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// In-DinD runtime state restore
// ---------------------------------------------------------------------------
//
// When the outer DinD container restarts (for example, after Docker Desktop
// restarts containers on host boot), the mount namespace and network
// plumbing inside the container are reset. The bind mounts, alias IPs, and
// socat proxies that `coast run` / `coast start` set up via `docker exec`
// live only in the container's live runtime and are not persisted. Two
// consequences matter at startup:
//
//   1. `/workspace` is no longer bind-mounted to `/host-project` (or the
//      current worktree), so compose services under /workspace cannot find
//      their files. The symptom is
//        "env file /workspace/... not found" during compose up.
//   2. The alias IPs on the DinD's inner `docker0` and the socat proxies
//      that forward to shared services on the host are gone, so compose
//      services cannot reach `postgres`/`redis`/etc. The symptom is
//        "connect: connection timed out" trying to reach 172.18.255.254.
//
// The helpers below re-apply those settings during daemon startup. They
// are best-effort (per-instance timeouts, log-and-continue on failure) so
// one pathological instance cannot block daemon startup for everyone else.

/// Max time we spend trying to re-apply in-DinD runtime state for a single
/// instance before giving up and moving on. Generous enough to cover the
/// inner-dockerd startup wait plus a few seconds for actual work.
const RESTORE_PER_INSTANCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// How long we wait for the inner `dockerd` (and with it `docker0`) to
/// come up after a DinD container restart. Polling is cheap (2s ticks).
const INNER_DOCKER_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Poll `ip -o -4 addr show docker0` inside the DinD until it succeeds
/// (meaning the inner `dockerd` has started and created the `docker0`
/// bridge) or the timeout elapses. Returns true iff docker0 appeared.
async fn wait_for_inner_docker0(docker: &bollard::Docker, container_id: &str) -> bool {
    use coast_docker::runtime::Runtime;
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let deadline = std::time::Instant::now() + INNER_DOCKER_WAIT;
    loop {
        match rt
            .exec_in_coast(container_id, &["sh", "-lc", "ip -o -4 addr show docker0"])
            .await
        {
            Ok(r) if r.success() && r.stdout.contains("inet ") => return true,
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Look up the canonical coastfile on disk for an instance (if any).
///
/// This is `~/.coast/images/<project>/<build_id_or_latest>/coastfile.toml`.
/// Returns `None` when the artifact is missing (e.g. builds pruned).
fn artifact_coastfile_path(project: &str, build_id: Option<&str>) -> Option<std::path::PathBuf> {
    let project_dir = handlers::run::paths::project_images_dir(project);
    if let Some(bid) = build_id {
        let resolved = project_dir.join(bid);
        if resolved.exists() {
            let p = resolved.join("coastfile.toml");
            if p.exists() {
                return Some(p);
            }
        }
    }
    let p = project_dir.join("latest").join("coastfile.toml");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Re-apply the `/workspace` bind mount inside each local DinD.
///
/// Iterates local (non-remote) active instances. For each, re-runs the
/// bind script that `provision::bind_workspace` uses at run/start time.
/// The script is effectively idempotent: `findmnt` short-circuits the
/// bind when /workspace is already mounted, and the private/cache mount
/// commands handle existing mounts gracefully.
///
/// Instances in `checked_out` state with a worktree are currently
/// restored to the project root; re-entering a worktree requires an
/// explicit `coast assign` after recovery.
async fn restore_workspace_mounts(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    for inst in instances {
        if inst.remote_host.is_some() {
            continue;
        }
        let Some(cid) = inst.container_id.as_ref() else {
            continue;
        };
        let cid = cid.clone();
        let project = inst.project.clone();
        let name = inst.name.clone();
        let build_id = inst.build_id.clone();
        let docker = docker.clone();

        let work = async move {
            restore_single_workspace_mount(docker, project, name, cid, build_id).await
        };
        if (tokio::time::timeout(RESTORE_PER_INSTANCE_TIMEOUT, work).await).is_err() {
            warn!(instance = %inst.name, project = %inst.project, "workspace mount restore timed out");
        }
    }
}

/// Read `private_paths` + `bare_services` from the build's coastfile.
/// Returns empty vectors if the coastfile is missing or fails to parse;
/// missing coastfile data never stops the mount restore.
fn load_private_and_bare(
    project: &str,
    name: &str,
    build_id: Option<&str>,
) -> (Vec<String>, Vec<coast_core::types::BareServiceConfig>) {
    let Some(path) = artifact_coastfile_path(project, build_id) else {
        return (Vec::new(), Vec::new());
    };
    match coast_core::coastfile::Coastfile::from_file(&path) {
        Ok(cf) => (cf.private_paths, cf.services),
        Err(e) => {
            warn!(instance = %name, project = %project, error = %e, "workspace restore: failed to parse coastfile; skipping private/cache mounts");
            (Vec::new(), Vec::new())
        }
    }
}

/// Build the idempotent `sh -c` command that re-applies /workspace + the
/// associated private-path and cache-mount overlays.
///
/// The script short-circuits if /workspace is already a distinct mount
/// (findmnt check), so it is safe to call after a daemon restart that
/// did not actually lose the mount.
fn build_workspace_mount_script(
    private_paths: &[String],
    bare_services: &[coast_core::types::BareServiceConfig],
) -> String {
    let private_cmds =
        coast_core::coastfile::Coastfile::build_private_paths_mount_commands(private_paths);
    let cache_cmds = coast_core::coastfile::Coastfile::build_cache_mount_commands(bare_services);
    format!(
        "set -e; mkdir -p /workspace; \
         if findmnt -T /workspace >/dev/null 2>&1 && [ \"$(findmnt -n -o TARGET -T /workspace)\" = \"/workspace\" ]; then \
           exit 0; \
         fi; \
         mount --bind /host-project /workspace && mount --make-rshared /workspace{private_cmds}{cache_cmds}"
    )
}

async fn restore_single_workspace_mount(
    docker: bollard::Docker,
    project: String,
    name: String,
    container_id: String,
    build_id: Option<String>,
) {
    let (private_paths, bare_services) =
        load_private_and_bare(&project, &name, build_id.as_deref());
    let cmd = build_workspace_mount_script(&private_paths, &bare_services);

    use coast_docker::runtime::Runtime;
    let rt = coast_docker::dind::DindRuntime::with_client(docker);
    let result = rt.exec_in_coast(&container_id, &["sh", "-c", &cmd]).await;
    log_workspace_restore_result(&project, &name, result);
}

fn log_workspace_restore_result(
    project: &str,
    name: &str,
    result: coast_core::error::Result<coast_docker::runtime::ExecResult>,
) {
    match result {
        Ok(r) if r.success() => {
            info!(instance = %name, project = %project, "restored /workspace bind mount");
        }
        Ok(r) => {
            warn!(instance = %name, project = %project, stderr = %r.stderr, "workspace mount restore: exec reported failure");
        }
        Err(e) => {
            warn!(instance = %name, project = %project, error = %e, "workspace mount restore: exec error");
        }
    }
}

/// Re-apply the shared-service routing (docker0 alias IPs + socat proxies)
/// inside each local DinD that has `shared_services` configured.
///
/// For each local active instance:
///   - Load the build's coastfile. Skip if absent or no shared services.
///   - Reconnect the DinD to `coast-shared-<project>` on the host Docker
///     daemon (best effort; already-connected is a no-op warn).
///   - Call `plan_shared_service_routing` + `ensure_shared_service_proxies`.
///     Both are idempotent: `ip addr add ... || true` in the script and
///     socat pid files are replaced if stale.
async fn restore_shared_service_proxies(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    for inst in instances {
        if inst.remote_host.is_some() {
            continue;
        }
        let Some(cid) = inst.container_id.as_ref() else {
            continue;
        };
        let cid = cid.clone();
        let project = inst.project.clone();
        let name = inst.name.clone();
        let build_id = inst.build_id.clone();
        let docker = docker.clone();

        let work = async move {
            restore_single_shared_proxies(docker, project, name, cid, build_id).await
        };
        if (tokio::time::timeout(RESTORE_PER_INSTANCE_TIMEOUT, work).await).is_err() {
            warn!(instance = %inst.name, project = %inst.project, "shared-service proxy restore timed out");
        }
    }
}

/// Load a coastfile that declares at least one shared service.
/// Returns None on any "nothing to do" condition (no artifact, parse fail,
/// empty shared_services). Emits a warn log for the parse-fail case.
fn load_coastfile_with_shared_services(
    project: &str,
    name: &str,
    build_id: Option<&str>,
) -> Option<coast_core::coastfile::Coastfile> {
    let path = artifact_coastfile_path(project, build_id)?;
    let coastfile = match coast_core::coastfile::Coastfile::from_file(&path) {
        Ok(cf) => cf,
        Err(e) => {
            warn!(instance = %name, project = %project, error = %e, "shared-service restore: failed to parse coastfile");
            return None;
        }
    };
    if coastfile.shared_services.is_empty() {
        return None;
    }
    Some(coastfile)
}

/// Build the `service name -> shared container name` map in the same way
/// that `setup_shared_services` does at run/start time.
fn build_shared_target_map(
    project: &str,
    coastfile: &coast_core::coastfile::Coastfile,
) -> std::collections::HashMap<String, String> {
    coastfile
        .shared_services
        .iter()
        .map(|svc| {
            (
                svc.name.clone(),
                shared_services::shared_container_name(project, &svc.name),
            )
        })
        .collect()
}

/// Plan routing and ensure proxies inside the DinD; log results.
async fn apply_shared_service_plan(
    docker: &bollard::Docker,
    project: &str,
    name: &str,
    container_id: &str,
    shared_services: &[coast_core::types::SharedServiceConfig],
    target_containers: &std::collections::HashMap<String, String>,
) {
    let plan = match coast_docker::shared_service_routing::plan_shared_service_routing(
        docker,
        container_id,
        shared_services,
        target_containers,
    )
    .await
    {
        Ok(plan) => plan,
        Err(e) => {
            warn!(instance = %name, project = %project, error = %e, "shared-service restore: failed to plan routing");
            return;
        }
    };

    match coast_docker::shared_service_routing::ensure_shared_service_proxies(
        docker,
        container_id,
        &plan,
    )
    .await
    {
        Ok(()) => {
            info!(instance = %name, project = %project, routes = plan.routes.len(), "restored shared-service proxies");
        }
        Err(e) => {
            warn!(instance = %name, project = %project, error = %e, "shared-service restore: failed to ensure proxies");
        }
    }
}

async fn restore_single_shared_proxies(
    docker: bollard::Docker,
    project: String,
    name: String,
    container_id: String,
    build_id: Option<String>,
) {
    let Some(coastfile) = load_coastfile_with_shared_services(&project, &name, build_id.as_deref())
    else {
        return;
    };

    // After a DinD restart (e.g. Docker Desktop bringing containers back
    // on reboot) the inner `dockerd` needs a moment to start and create
    // the `docker0` bridge. `plan_shared_service_routing` reads
    // `ip -o -4 addr show docker0` and fails if docker0 is not yet
    // present; wait for it before proceeding.
    if !wait_for_inner_docker0(&docker, &container_id).await {
        warn!(instance = %name, project = %project, "shared-service restore: inner docker0 never appeared; skipping");
        return;
    }

    // Reconnect DinD to the shared-services network (may already be connected).
    let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
    let net_name = coast_docker::network::shared_network_name(&project);
    if let Err(e) = nm.connect_container(&net_name, &container_id).await {
        // "already exists in network" / similar is normal; demote to debug.
        tracing::debug!(instance = %name, project = %project, error = %e, "shared-service restore: reconnect to shared network (likely already connected)");
    }

    let target_containers = build_shared_target_map(&project, &coastfile);
    apply_shared_service_plan(
        &docker,
        &project,
        &name,
        &container_id,
        &coastfile.shared_services,
        &target_containers,
    )
    .await;
}

/// Restore socat port forwarding for all running instances after daemon restart.
async fn restore_socat_forwarding(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let docker = state.docker.as_ref().unwrap();
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;

    for inst in instances {
        let coast_ip = if inst.remote_host.is_some() {
            "127.0.0.1".to_string()
        } else {
            let cid = inst.container_id.as_ref().unwrap();
            match rt.get_container_ip(cid).await {
                Ok(ip) => ip.to_string(),
                Err(e) => {
                    warn!(
                        instance = %inst.name, project = %inst.project, error = %e,
                        "could not resolve container IP, skipping port restore"
                    );
                    continue;
                }
            }
        };
        restore_socat_for_instance(state, inst, &coast_ip).await;
    }
}

/// Parse a remote published port from a Docker ports string like
/// "0.0.0.0:35969->3000/tcp, 0.0.0.0:39325->4000/tcp" given a target
/// canonical (container) port.
fn parse_remote_port_from_docker_ports(ports_str: &str, canonical_port: u16) -> Option<u16> {
    let target = format!("->{canonical_port}/");
    for segment in ports_str.split(',') {
        let segment = segment.trim();
        if segment.contains(&target) {
            let colon_pos = segment.find(':')?;
            let arrow_pos = segment.find("->")?;
            let host_port_str = &segment[colon_pos + 1..arrow_pos];
            return host_port_str.parse().ok();
        }
    }
    None
}

async fn connect_remote_for_heal(
    state: &Arc<server::AppState>,
    project: &str,
    name: &str,
) -> Option<(
    coast_core::types::RemoteConnection,
    coast_core::protocol::PsResponse,
)> {
    let remote_config =
        match handlers::remote::resolve_remote_for_instance(project, name, state).await {
            Ok(c) => c,
            Err(e) => {
                warn!(instance = %name, error = %e, "cannot resolve remote for tunnel heal");
                return None;
            }
        };

    let client = match handlers::remote::RemoteClient::connect(&remote_config).await {
        Ok(c) => c,
        Err(e) => {
            warn!(instance = %name, error = %e, "cannot connect to remote for tunnel heal");
            return None;
        }
    };

    let ps_req = coast_core::protocol::PsRequest {
        name: name.to_string(),
        project: project.to_string(),
    };
    let ps_resp = match handlers::remote::forward::forward_ps(&client, &ps_req).await {
        Ok(r) => r,
        Err(e) => {
            warn!(instance = %name, error = %e, "failed to query remote ps for tunnel heal");
            return None;
        }
    };

    Some((remote_config, ps_resp))
}

fn build_heal_tunnel_pairs(
    allocs: &[state::PortAllocationRecord],
    ps_resp: &coast_core::protocol::PsResponse,
) -> Vec<(u16, u16)> {
    allocs
        .iter()
        .filter_map(|a| {
            if let Some(rdp) = a.remote_dynamic_port {
                return Some((a.dynamic_port, rdp));
            }
            for svc in &ps_resp.services {
                if let Some(rdp) = parse_remote_port_from_docker_ports(&svc.ports, a.canonical_port)
                {
                    return Some((a.dynamic_port, rdp));
                }
            }
            None
        })
        .collect()
}

fn build_restore_tunnel_pairs(allocs: &[state::PortAllocationRecord]) -> Vec<(u16, u16)> {
    allocs
        .iter()
        .filter_map(|a| a.remote_dynamic_port.map(|rdp| (a.dynamic_port, rdp)))
        .collect()
}

/// Re-establish SSH tunnels for a single remote instance by querying
/// coast-service for the current port mappings. Does not depend on the
/// `remote_dynamic_port` column being populated in the local DB.
async fn heal_remote_tunnels(state: &Arc<server::AppState>, project: &str, name: &str) {
    let Some((remote_config, ps_resp)) = connect_remote_for_heal(state, project, name).await else {
        return;
    };

    let allocs = {
        let db = state.db.lock().await;
        db.get_port_allocations(project, name).unwrap_or_default()
    };

    let tunnel_pairs = build_heal_tunnel_pairs(&allocs, &ps_resp);

    if tunnel_pairs.is_empty() {
        warn!(instance = %name, "no port mappings found for tunnel heal");
        return;
    }

    match handlers::remote::tunnel::forward_ports(&remote_config, &tunnel_pairs).await {
        Ok(pids) => {
            info!(
                instance = %name,
                tunnels = tunnel_pairs.len(),
                pids = ?pids,
                "SSH tunnels re-established (auto-heal)"
            );
        }
        Err(e) => {
            warn!(instance = %name, error = %e, "failed to re-establish SSH tunnels");
        }
    }
}

async fn forward_and_log_tunnels(
    connection: &coast_core::types::RemoteConnection,
    tunnel_pairs: &[(u16, u16)],
    instance_name: &str,
) {
    match handlers::remote::tunnel::forward_ports(connection, tunnel_pairs).await {
        Ok(pids) => {
            tracing::info!(
                instance = %instance_name,
                tunnels = tunnel_pairs.len(),
                pids = ?pids,
                "restored SSH port tunnels"
            );
        }
        Err(e) => {
            tracing::warn!(
                instance = %instance_name,
                error = %e,
                "failed to restore SSH port tunnels"
            );
        }
    }
}

async fn restore_instance_tunnels(
    state: &Arc<server::AppState>,
    inst: &coast_core::types::CoastInstance,
    remotes: &[coast_core::types::RemoteEntry],
) {
    let remote_host = inst.remote_host.as_deref().unwrap();
    let entry = remotes
        .iter()
        .find(|r| r.name == remote_host || r.host == remote_host);
    let Some(entry) = entry else {
        tracing::warn!(
            instance = %inst.name,
            remote = %remote_host,
            "remote entry not found, skipping tunnel restore"
        );
        return;
    };

    let allocs = {
        let db = state.db.lock().await;
        db.get_port_allocations(&inst.project, &inst.name)
            .unwrap_or_default()
    };

    let tunnel_pairs = build_restore_tunnel_pairs(&allocs);

    if tunnel_pairs.is_empty() {
        tracing::debug!(
            instance = %inst.name,
            "no remote port mappings stored, skipping tunnel restore"
        );
        return;
    }

    let connection = coast_core::types::RemoteConnection::from_entry(
        entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );

    forward_and_log_tunnels(&connection, &tunnel_pairs, &inst.name).await;
}

/// Phase 28 daemon-startup hook for the host socat supervisor.
/// Reconciles every persisted SSG service against its host socat,
/// logging errors instead of failing daemon boot. See DESIGN.md
/// section 24.4 for the rationale.
async fn restore_host_socats(state: &Arc<server::AppState>) {
    match handlers::ssg::host_socat::reconcile_all(state).await {
        Ok(reconciled) if !reconciled.is_empty() => {
            tracing::info!(
                count = reconciled.len(),
                services = ?reconciled,
                "restore: host socat supervisor reconciled SSG services"
            );
        }
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(
                error = %err,
                "restore: host socat reconcile_all failed; consumers may see \
                 ECONNREFUSED on shared services until the next ssg run/start"
            );
        }
    }
}

/// Re-establish SSH port tunnels for remote instances after daemon restart.
async fn restore_remote_tunnels(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let remote_instances: Vec<_> = instances
        .iter()
        .filter(|inst| inst.remote_host.is_some())
        .collect();

    if remote_instances.is_empty() {
        return;
    }

    let remotes = {
        let db = state.db.lock().await;
        db.list_remotes().unwrap_or_default()
    };

    for inst in remote_instances {
        restore_instance_tunnels(state, inst, &remotes).await;
    }
}

/// Extract shared service reverse tunnel port pairs from a project's Coastfile.
pub fn shared_service_reverse_pairs(project: &str) -> Vec<(u16, u16)> {
    let Ok(images_dir) = coast_core::artifact::artifact_dir(project) else {
        return Vec::new();
    };
    let candidates = ["latest-remote", "latest"];
    let coastfile_path = candidates.iter().find_map(|name| {
        let p = images_dir.join(name).join("coastfile.toml");
        if p.exists() {
            Some(p)
        } else {
            None
        }
    });
    let Some(cf_path) = coastfile_path else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&cf_path) else {
        return Vec::new();
    };
    let Ok(cf) = coast_core::coastfile::Coastfile::parse(&content, &images_dir) else {
        return Vec::new();
    };
    cf.shared_services
        .iter()
        .flat_map(|svc| {
            svc.ports
                .iter()
                .map(|p| (p.container_port, p.container_port))
        })
        .collect()
}

/// Re-establish SSH reverse tunnels for shared services after daemon restart.
///
/// Phase 24: every instance gets its own `ssh -R` process for INLINE
/// forwards because every instance owns distinct `remote_port`s
/// (allocated once in `setup_shared_service_tunnels` per §18).
///
/// Phase 30: SSG forwards are now coalesced per
/// `(project, remote_host, service, container_port)` — only ONE
/// `ssh -R` exists across all instances of that project on that
/// remote VM. Daemon-restart restore therefore splits in two:
///
///   1. `restore_ssg_shared_tunnels` runs first and rebuilds shared
///      tunnels from `ssg_shared_tunnels` — at-most-once per quad,
///      regardless of how many instances reference each.
///   2. `restore_tunnels_for_instance` runs per-instance for INLINE
///      forwards only (SSG rows in `shared_service_forwards` are
///      filtered out, since the shared restore above already handled
///      them).
async fn restore_shared_service_tunnels(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let remote_instances: Vec<_> = instances
        .iter()
        .filter(|inst| inst.remote_host.is_some())
        .collect();

    if remote_instances.is_empty() {
        return;
    }

    let remotes = {
        let db = state.db.lock().await;
        db.list_remotes().unwrap_or_default()
    };

    // Phase 30: dedupe (project, remote_host) pairs that have at
    // least one live remote instance. Restore SSG shared tunnels
    // once per pair before per-instance inline restore.
    let mut shared_keys: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for inst in &remote_instances {
        if let Some(host) = inst.remote_host.as_deref() {
            shared_keys.insert((inst.project.clone(), host.to_string()));
        }
    }
    for (project, remote_host) in &shared_keys {
        restore_ssg_shared_tunnels(state, project, remote_host, &remotes).await;
    }

    for inst in &remote_instances {
        restore_tunnels_for_instance(state, inst, &remotes).await;
    }
}

/// Phase 30 daemon-startup hook for SSG shared tunnels. Reads every
/// `ssg_shared_tunnels` row for `(project, remote_host)`, probes the
/// recorded `ssh_pid`, and respawns any whose pid is dead (or was
/// never written because the daemon crashed mid-spawn last run). The
/// virtual port is preserved across the respawn so consumers never
/// see the local upstream change.
async fn restore_ssg_shared_tunnels(
    state: &Arc<server::AppState>,
    project: &str,
    remote_host: &str,
    remotes: &[coast_core::types::RemoteEntry],
) {
    let Some(entry) = remotes
        .iter()
        .find(|r| r.name == remote_host || r.host == remote_host)
    else {
        tracing::warn!(
            project = %project,
            remote = %remote_host,
            "phase 30 restore: no remote entry for shared-tunnel rows; skipping"
        );
        return;
    };
    let connection = coast_core::types::RemoteConnection::from_entry(
        entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );
    respawn_dead_ssg_shared_tunnels(state, project, &connection, remote_host, "phase 30 restore")
        .await;
}

/// Read the persisted reverse-tunnel pairs for an instance from the
/// daemon state DB.
///
/// Phase 18: the pairs are allocated once in
/// `setup_shared_service_tunnels` and stored in `shared_service_forwards`
/// so daemon-restart recovery (`restore_tunnels_for_instance`) and
/// `reestablish_shared_service_tunnels` (on `coast start`) can replay
/// the exact same tunnels without having to reallocate `remote_port`s
/// or re-evaluate SSG state. Returns an empty vec when the instance has
/// no recorded forwards.
///
/// Phase 30: SSG-backed forwards are FILTERED OUT here — the shared
/// SSG tunnels are restored once per `(project, remote_host)` by
/// `restore_ssg_shared_tunnels`, not once per instance. Per-instance
/// replay only handles inline shared services. The optional
/// `remote_host` lets the caller scope the SSG-shared filter.
pub(crate) async fn shared_service_reverse_pairs_with_ssg(
    state: &server::AppState,
    project: &str,
    instance: &str,
) -> Vec<(u16, u16)> {
    shared_service_reverse_pairs_filtered(state, project, instance, None).await
}

/// Phase 30: variant of [`shared_service_reverse_pairs_with_ssg`]
/// that filters out forwards already covered by an
/// `ssg_shared_tunnels` row for `remote_host`. Use this from the
/// per-instance restore path so SSG shared tunnels are not respawned
/// once-per-instance.
pub(crate) async fn shared_service_reverse_pairs_filtered(
    state: &server::AppState,
    project: &str,
    instance: &str,
    remote_host: Option<&str>,
) -> Vec<(u16, u16)> {
    use coast_ssg::state::SsgStateExt;
    let db = state.db.lock().await;
    let rows = match db.list_shared_service_forwards_for_instance(project, instance) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(
                project = %project,
                instance = %instance,
                error = %e,
                "failed to read shared_service_forwards; skipping reverse tunnels"
            );
            return Vec::new();
        }
    };
    let ssg_keys: std::collections::HashSet<(String, u16)> = match remote_host {
        Some(host) => match db.list_ssg_shared_tunnels_for_remote(project, host) {
            Ok(rows) => rows
                .into_iter()
                .map(|r| (r.service_name, r.container_port))
                .collect(),
            Err(e) => {
                tracing::warn!(
                    project = %project,
                    remote = %host,
                    error = %e,
                    "failed to read ssg_shared_tunnels; replaying every forward as inline"
                );
                std::collections::HashSet::new()
            }
        },
        None => std::collections::HashSet::new(),
    };
    rows.into_iter()
        .filter(|r| !ssg_keys.contains(&(r.service_name.clone(), r.port)))
        .map(|r| (r.remote_port, r.local_port))
        .collect()
}

async fn restore_tunnels_for_instance(
    state: &Arc<server::AppState>,
    inst: &coast_core::types::CoastInstance,
    remotes: &[coast_core::types::RemoteEntry],
) {
    // Phase 18: read the persisted (remote_port, local_port) pairs from
    // the state DB. Allocation happened once in
    // `setup_shared_service_tunnels`; we replay without re-evaluating
    // SSG state or reallocating remote ports.
    //
    // Phase 24: no dedup by host. Every `(project, instance)` on the
    // same remote VM owns distinct `remote_port`s for INLINE forwards,
    // so each instance needs its own `ssh -R` process. Two projects
    // sharing a remote now both get their tunnels restored.
    //
    // Phase 30: SSG-backed forwards are filtered out here — they're
    // restored once per `(project, remote_host)` by
    // `restore_ssg_shared_tunnels` rather than once per instance. The
    // filter compares each `shared_service_forwards` row against the
    // `ssg_shared_tunnels` rows for this instance's remote.
    let Some(remote_host) = inst.remote_host.as_deref() else {
        return;
    };
    let reverse_pairs = shared_service_reverse_pairs_filtered(
        state.as_ref(),
        &inst.project,
        &inst.name,
        Some(remote_host),
    )
    .await;
    if reverse_pairs.is_empty() {
        return;
    }
    let Some(entry) = remotes
        .iter()
        .find(|r| r.name == remote_host || r.host == remote_host)
    else {
        tracing::warn!(
            instance = %inst.name,
            remote = %remote_host,
            "remote entry not found, skipping shared service tunnel restore"
        );
        return;
    };

    let connection = coast_core::types::RemoteConnection::from_entry(
        entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );

    let _ = create_reverse_tunnels(
        state,
        &connection,
        &reverse_pairs,
        &inst.project,
        &inst.name,
    )
    .await;
}

async fn create_reverse_tunnels(
    state: &Arc<server::AppState>,
    connection: &coast_core::types::RemoteConnection,
    reverse_pairs: &[(u16, u16)],
    project: &str,
    instance_name: &str,
) -> bool {
    match handlers::remote::tunnel::reverse_forward_ports(connection, reverse_pairs).await {
        Ok(pids) => {
            tracing::info!(
                instance = %instance_name,
                tunnels = reverse_pairs.len(),
                pids = ?pids,
                "restored shared service reverse tunnels"
            );
            if !pids.is_empty() {
                // Phase 4.5: repopulate the in-memory PID map so
                // `coast ssg stop/rm --force` can tear these ssh
                // children down later if the consumer references the
                // SSG. See `coast-ssg/DESIGN.md §17-19`.
                let mut map = state.shared_service_tunnel_pids.lock().await;
                map.insert((project.to_string(), instance_name.to_string()), pids);
            }
            true
        }
        Err(e) => {
            tracing::warn!(
                instance = %instance_name,
                error = %e,
                "failed to restore shared service reverse tunnels"
            );
            false
        }
    }
}

/// Resolve worktree path and remount /workspace in the shell container.
/// Returns the host path to the workspace source for rsync.
async fn remount_worktree_in_shell(
    docker: &bollard::Docker,
    inst: &coast_core::types::CoastInstance,
) -> Option<std::path::PathBuf> {
    let shell_container = format!("{}-coasts-{}-shell", inst.project, inst.name);
    let cf_data = handlers::assign::load_coastfile_data(&inst.project);
    let project_root = handlers::assign::read_project_root(&inst.project);

    if let Some(ref wt_name) = inst.worktree_name {
        let wt_path = handlers::assign::services::detect_worktree_path(
            &project_root,
            &cf_data.worktree_dirs,
            &cf_data.default_worktree_dir,
            wt_name,
        )
        .await;
        let Some(loc) = wt_path.filter(|l| l.host_path.exists()) else {
            tracing::warn!(instance = %inst.name, wt = %wt_name, "worktree not found, skipping restore");
            return None;
        };
        let mount_cmd = format!(
            "umount -l /workspace 2>/dev/null; mount --bind {} /workspace && mount --make-rshared /workspace",
            loc.container_mount_src
        );
        let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
        use coast_docker::runtime::Runtime;
        if let Err(e) = rt
            .exec_in_coast(&shell_container, &["sh", "-c", &mount_cmd])
            .await
        {
            tracing::warn!(instance = %inst.name, error = %e, "failed to remount worktree in shell");
        }
        Some(loc.host_path)
    } else {
        project_root
    }
}

/// Restore worktree mounts and mutagen sessions for remote instances
/// after daemon restart.
async fn restore_remote_worktrees(
    state: &Arc<server::AppState>,
    instances: &[coast_core::types::CoastInstance],
) {
    let Some(docker) = state.docker.as_ref() else {
        return;
    };

    let remotes = {
        let db = state.db.lock().await;
        db.list_remotes().unwrap_or_default()
    };

    for inst in instances {
        let Some(remote_host) = inst.remote_host.as_deref() else {
            continue;
        };
        let Some(entry) = remotes
            .iter()
            .find(|r| r.name == remote_host || r.host == remote_host)
        else {
            continue;
        };

        let connection = coast_core::types::RemoteConnection::from_entry(
            entry,
            &coast_core::types::RemoteConfig {
                workspace_sync: coast_core::types::SyncStrategy::default(),
            },
        );

        let Some(workspace_source) = remount_worktree_in_shell(&docker, inst).await else {
            continue;
        };

        let Ok(client) = handlers::remote::RemoteClient::connect(&connection).await else {
            tracing::warn!(instance = %inst.name, "failed to connect to remote for worktree restore");
            continue;
        };

        let service_home = client.query_service_home().await;
        let remote_workspace =
            handlers::remote::remote_workspace_path(&service_home, &inst.project, &inst.name);

        if workspace_source.exists() {
            let _ = client
                .sync_workspace_no_delete(&workspace_source, &remote_workspace)
                .await;
        }

        let shell_container = format!("{}-coasts-{}-shell", inst.project, inst.name);
        handlers::run::start_mutagen_in_shell(
            &docker,
            &shell_container,
            &inst.project,
            &inst.name,
            &remote_workspace,
            &connection,
        )
        .await;

        tracing::info!(
            instance = %inst.name,
            worktree = ?inst.worktree_name,
            "restored worktree mount and mutagen for remote instance"
        );
    }
}

/// Downgrade a CheckedOut instance to Running when all canonical port
/// forwarders failed to restore. This prevents the UI from showing a
/// stale "checked out" badge with no working canonical ports.
async fn downgrade_checked_out_if_all_canonical_failed(
    state: &Arc<server::AppState>,
    inst: &coast_core::types::CoastInstance,
    canonical_ok: u32,
    expected_canonical: usize,
) {
    if canonical_ok > 0 || expected_canonical == 0 {
        return;
    }
    warn!(
        instance = %inst.name, project = %inst.project,
        "canonical port forwarding failed for all {} port(s); \
         reverting to Running status. Re-run `coast checkout {}` \
         once the ports are free.",
        expected_canonical, inst.name,
    );
    let db = state.db.lock().await;
    let _ = db.update_instance_status(
        &inst.project,
        &inst.name,
        &coast_core::types::InstanceStatus::Running,
    );
    drop(db);
    state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
        name: inst.name.clone(),
        project: inst.project.clone(),
        status: "running".to_string(),
    });
}

/// Spawn dynamic socat forwarders from restoration commands. Returns the
/// number of successfully spawned forwarders.
fn restore_dynamic_socats(cmds: &[port_manager::RestoreSocatCmd], instance_name: &str) -> u32 {
    let mut ok = 0u32;
    for entry in cmds {
        match port_manager::spawn_socat(&entry.cmd) {
            Ok(_) => ok += 1,
            Err(e) => {
                warn!(
                    instance = %instance_name, port = %entry.logical_name,
                    error = %e, "failed to restore socat"
                );
            }
        }
    }
    ok
}

/// Restore canonical port forwarders for a checked-out instance.
/// Uses a WSL bridge container when running under WSL, or spawns
/// individual socat processes otherwise. Returns the number of
/// successfully restored canonical ports.
async fn restore_canonical_forwarders(
    state: &Arc<server::AppState>,
    inst: &coast_core::types::CoastInstance,
    allocs: &[state::PortAllocationRecord],
    use_wsl_bridge: bool,
) -> u32 {
    let mut ok = 0u32;
    if use_wsl_bridge {
        let bridge_ports = allocs
            .iter()
            .map(|alloc| port_manager::CheckoutBridgePort {
                _logical_name: &alloc.logical_name,
                canonical_port: alloc.canonical_port,
                dynamic_port: alloc.dynamic_port,
            })
            .collect::<Vec<_>>();

        match port_manager::start_checkout_bridge(&inst.project, &inst.name, &bridge_ports) {
            Ok(()) => ok += allocs.len() as u32,
            Err(e) => {
                warn!(
                    instance = %inst.name,
                    error = %e,
                    "failed to restore WSL checkout bridge"
                );
            }
        }
    } else {
        for alloc in allocs {
            if !port_manager::is_port_available(alloc.canonical_port) {
                warn!(
                    instance = %inst.name,
                    port = %alloc.logical_name,
                    "canonical port already in use, skipping"
                );
                continue;
            }

            let cmd = port_manager::socat_command_canonical(
                alloc.canonical_port,
                "127.0.0.1",
                alloc.dynamic_port,
            );

            match port_manager::spawn_socat(&cmd) {
                Ok(pid) => {
                    let db = state.db.lock().await;
                    let _ = db.update_socat_pid(
                        &inst.project,
                        &inst.name,
                        &alloc.logical_name,
                        Some(pid as i32),
                    );
                    ok += 1;
                }
                Err(e) => {
                    warn!(
                        instance = %inst.name,
                        port = %alloc.logical_name,
                        error = %e,
                        "failed to restore canonical socat"
                    );
                }
            }
        }
    }
    ok
}

/// Spawn socat forwarders for a single instance.
async fn restore_socat_for_instance(
    state: &Arc<server::AppState>,
    inst: &coast_core::types::CoastInstance,
    coast_ip: &str,
) {
    let allocs = {
        let db = state.db.lock().await;
        db.get_port_allocations(&inst.project, &inst.name)
            .unwrap_or_default()
    };

    let is_checked_out = inst.status == coast_core::types::InstanceStatus::CheckedOut;
    let ports: Vec<_> = allocs
        .iter()
        .map(|a| port_manager::PortToRestore {
            logical_name: &a.logical_name,
            canonical_port: a.canonical_port,
            dynamic_port: a.dynamic_port,
        })
        .collect();
    let cmds = port_manager::restoration_commands(&ports, coast_ip, false);

    let dynamic_ok = restore_dynamic_socats(&cmds, &inst.name);

    let mut canonical_ok = 0u32;
    if is_checked_out {
        let use_wsl_bridge = state.docker.is_some() && port_manager::running_in_wsl();
        canonical_ok = restore_canonical_forwarders(state, inst, &allocs, use_wsl_bridge).await;
        downgrade_checked_out_if_all_canonical_failed(state, inst, canonical_ok, allocs.len())
            .await;
    }

    info!(
        instance = %inst.name, project = %inst.project,
        dynamic_ports = dynamic_ok, canonical_ports = canonical_ok,
        checked_out = is_checked_out, "restored port forwarding"
    );
}

/// React to instance lifecycle events by starting/stopping background stats collectors.
async fn handle_stats_lifecycle_event(
    state: &Arc<AppState>,
    event: &coast_core::protocol::CoastEvent,
) {
    use coast_core::protocol::CoastEvent;

    match event {
        CoastEvent::InstanceCreated { name, project, .. }
        | CoastEvent::InstanceStarted { name, project, .. } => {
            let key = api::ws_stats::stats_key(project, name);
            let db = state.db.lock().await;
            if let Ok(Some(inst)) = db.get_instance(project, name) {
                if inst.remote_host.is_some() {
                    let project = project.clone();
                    let name = name.clone();
                    drop(db);
                    if !state.stats_collectors.lock().await.contains_key(&key) {
                        api::ws_stats::start_remote_dind_stats_collector(
                            Arc::clone(state),
                            key,
                            project,
                            name,
                        )
                        .await;
                    }
                } else if let Some(ref cid) = inst.container_id {
                    let cid = cid.clone();
                    let project = project.clone();
                    let name = name.clone();
                    drop(db);

                    if !state.stats_collectors.lock().await.contains_key(&key) {
                        api::ws_stats::start_stats_collector(Arc::clone(state), cid.clone(), key)
                            .await;
                    }

                    api::ws_service_stats::discover_and_start_service_collectors(
                        Arc::clone(state),
                        cid,
                        project,
                        name,
                    )
                    .await;
                }
            }
        }
        CoastEvent::InstanceStopped { name, project }
        | CoastEvent::InstanceRemoved { name, project } => {
            let key = api::ws_stats::stats_key(project, name);
            api::ws_stats::stop_stats_collector(state, &key).await;
            api::ws_service_stats::stop_all_service_collectors_for_instance(state, project, name)
                .await;
        }
        _ => {}
    }
}

/// Wait for SIGTERM or SIGINT (ctrl-c).
async fn wait_for_shutdown_signal() -> Result<()> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).map_err(|e| {
        coast_core::error::CoastError::io_simple(format!("failed to register SIGTERM handler: {e}"))
    })?;
    let mut sigint = signal(SignalKind::interrupt()).map_err(|e| {
        coast_core::error::CoastError::io_simple(format!("failed to register SIGINT handler: {e}"))
    })?;

    tokio::select! {
        _ = sigterm.recv() => {
            info!("received SIGTERM");
        }
        _ = sigint.recv() => {
            info!("received SIGINT");
        }
    }

    Ok(())
}

/// Restore all running-state resources after daemon startup: stats collectors,
/// socat port forwarding, agent shells, shared service collectors, and caches.
async fn restore_running_state(state: &Arc<server::AppState>) {
    let active_instances: Vec<_> = {
        let db = state.db.lock().await;
        db.list_instances()
            .unwrap_or_default()
            .into_iter()
            .filter(|inst| {
                let active = inst.status == coast_core::types::InstanceStatus::Running
                    || inst.status == coast_core::types::InstanceStatus::CheckedOut;
                active && inst.container_id.is_some()
            })
            .collect()
    };

    // Start background stats collectors for all running instances.
    for inst in &active_instances {
        let key = api::ws_stats::stats_key(&inst.project, &inst.name);
        if inst.remote_host.is_some() {
            api::ws_stats::start_remote_dind_stats_collector(
                Arc::clone(state),
                key,
                inst.project.clone(),
                inst.name.clone(),
            )
            .await;
        } else {
            let cid = inst.container_id.as_ref().unwrap().clone();
            api::ws_stats::start_stats_collector(Arc::clone(state), cid.clone(), key).await;

            let state_clone = Arc::clone(state);
            let project = inst.project.clone();
            let name = inst.name.clone();
            tokio::spawn(async move {
                api::ws_service_stats::discover_and_start_service_collectors(
                    state_clone,
                    cid,
                    project,
                    name,
                )
                .await;
            });
        }
    }

    // Restore socat port forwarding (dynamic + canonical for checked-out).
    if state.docker.is_some() {
        restore_socat_forwarding(state, &active_instances).await;
    }

    // Restore in-DinD runtime state that is lost when the outer container
    // restarts (e.g. Docker Desktop auto-restart after a host reboot):
    //   - /workspace bind mount
    //   - inner docker0 alias IPs + socat proxies for shared services
    //   - Phase 28: per-(project, service, container_port) host
    //     socats so consumer in-DinD socats keep resolving
    //     `host.docker.internal:<virtual_port>` to the SSG's current
    //     dyn port. We reconcile BEFORE the in-DinD proxies replay so
    //     the host endpoint is already listening when the consumer's
    //     socat reconnects.
    // These are independent of remote restore and must run even when
    // remote restoration is slow / unreachable.
    if state.docker.is_some() {
        restore_workspace_mounts(state, &active_instances).await;
        restore_host_socats(state).await;
        restore_shared_service_proxies(state, &active_instances).await;
    }

    // Restore SSH port tunnels for remote instances.
    restore_remote_tunnels(state, &active_instances).await;

    // Restore SSH reverse tunnels for shared services.
    restore_shared_service_tunnels(state, &active_instances).await;

    // Phase 6: re-spawn SSG canonical-port checkouts (socats die
    // when the daemon exits; rows in `ssg_port_checkouts` survive).
    // If the SSG itself is stopped, respawn is a no-op — rows' PIDs
    // will be null and the next `ssg run/start` kicks the respawn
    // off via the lifecycle hook. Per-project SSG (§23): iterate
    // every known project's SSG row and respawn its own checkouts.
    let ssg_projects: Vec<String> = {
        use coast_ssg::state::SsgStateExt;
        let db = state.db.lock().await;
        match db.list_ssgs() {
            Ok(rows) => rows.into_iter().map(|r| r.project).collect(),
            Err(err) => {
                tracing::warn!(error = %err, "restore: failed to list SSG rows; skipping checkout respawn");
                Vec::new()
            }
        }
    };
    for project in ssg_projects {
        let _messages =
            handlers::ssg::checkout::respawn_checkouts_after_lifecycle(&project, state).await;
    }

    // Restore worktree mounts and mutagen sessions for remote instances.
    restore_remote_worktrees(state, &active_instances).await;

    // Restore agent shells (background tasks -- Docker exec is slow).
    for inst in active_instances {
        let state_clone = Arc::clone(state);
        let cid = inst.container_id.unwrap();
        let project = inst.project;
        let name = inst.name;
        let ct = inst.coastfile_type;
        tokio::spawn(async move {
            api::streaming::spawn_agent_shell_if_configured(
                &state_clone,
                &project,
                &name,
                &cid,
                ct.as_deref(),
            )
            .await;
        });
    }

    // Start host-service stats collectors for all running shared services.
    let running_shared: Vec<(String, String)> = {
        let db = state.db.lock().await;
        db.list_shared_services(None)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.status == "running")
            .map(|s| (s.project, s.service_name))
            .collect()
    };
    for (project, service) in running_shared {
        let container_name = shared_services::shared_container_name(&project, &service);
        let key = api::ws_host_service_stats::stats_key(&project, &service);
        let state_clone = Arc::clone(state);
        tokio::spawn(async move {
            api::ws_host_service_stats::start_host_service_collector(
                state_clone,
                container_name,
                key,
            )
            .await;
        });
    }

    tokio::spawn(shared_services_cache_loop(Arc::clone(state)));
    tokio::spawn(service_health_cache_loop(Arc::clone(state)));
    tokio::spawn(port_health_cache_loop(Arc::clone(state)));

    // Event bus listener for stats collector lifecycle.
    {
        let state_for_events = Arc::clone(state);
        let mut event_rx = state.event_bus.subscribe();
        tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        handle_stats_lifecycle_event(&state_for_events, &event).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("stats lifecycle listener lagged, skipped {n} events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // Reconcile worktrees deleted while the daemon was down.
    git_watcher::reconcile_orphaned_worktrees(state).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_parse_foreground() {
        let cli = Cli::parse_from(["coastd", "--foreground"]);
        assert!(cli.foreground);
        assert!(cli.socket.is_none());
    }

    #[test]
    fn test_cli_parse_custom_socket() {
        let cli = Cli::parse_from(["coastd", "--socket", "/tmp/test.sock"]);
        assert!(!cli.foreground);
        assert_eq!(cli.socket.as_deref(), Some("/tmp/test.sock"));
    }

    #[test]
    fn test_cli_parse_both_flags() {
        let cli = Cli::parse_from(["coastd", "--foreground", "--socket", "/tmp/test.sock"]);
        assert!(cli.foreground);
        assert_eq!(cli.socket.as_deref(), Some("/tmp/test.sock"));
    }

    #[test]
    fn test_cli_parse_default() {
        let cli = Cli::parse_from(["coastd"]);
        assert!(!cli.foreground);
        assert!(cli.socket.is_none());
    }

    #[test]
    fn test_extend_path_entries_appends_only_missing_candidates() {
        let existing = vec![
            std::path::PathBuf::from("/usr/bin"),
            std::path::PathBuf::from("/bin"),
        ];
        let updated = extend_path_entries(
            existing,
            [
                std::path::PathBuf::from("/opt/homebrew/bin"),
                std::path::PathBuf::from("/usr/bin"),
            ],
        );

        assert_eq!(
            updated,
            vec![
                std::path::PathBuf::from("/usr/bin"),
                std::path::PathBuf::from("/bin"),
                std::path::PathBuf::from("/opt/homebrew/bin"),
            ]
        );
    }

    fn make_test_instance(
        name: &str,
        project: &str,
        status: coast_core::types::InstanceStatus,
    ) -> coast_core::types::CoastInstance {
        coast_core::types::CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("container-{name}")),
            runtime: coast_core::types::RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    // --- downgrade_checked_out_if_all_canonical_failed unit tests ---

    #[tokio::test]
    async fn test_downgrade_when_all_canonical_failed() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));
        let inst = make_test_instance(
            "co-fail",
            "proj",
            coast_core::types::InstanceStatus::CheckedOut,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
        }
        let mut rx = state.event_bus.subscribe();

        downgrade_checked_out_if_all_canonical_failed(&state, &inst, 0, 2).await;

        let db = state.db.lock().await;
        let updated = db.get_instance("proj", "co-fail").unwrap().unwrap();
        assert_eq!(updated.status, coast_core::types::InstanceStatus::Running);
        drop(db);
        let event = rx.try_recv().unwrap();
        match event {
            coast_core::protocol::CoastEvent::InstanceStatusChanged { status, .. } => {
                assert_eq!(status, "running");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_no_downgrade_when_some_canonical_succeeded() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));
        let inst = make_test_instance(
            "co-ok",
            "proj",
            coast_core::types::InstanceStatus::CheckedOut,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
        }

        downgrade_checked_out_if_all_canonical_failed(&state, &inst, 1, 2).await;

        let db = state.db.lock().await;
        let updated = db.get_instance("proj", "co-ok").unwrap().unwrap();
        assert_eq!(
            updated.status,
            coast_core::types::InstanceStatus::CheckedOut
        );
    }

    #[tokio::test]
    async fn test_no_downgrade_when_no_expected_canonical() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));
        let inst = make_test_instance(
            "co-empty",
            "proj",
            coast_core::types::InstanceStatus::CheckedOut,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
        }

        downgrade_checked_out_if_all_canonical_failed(&state, &inst, 0, 0).await;

        let db = state.db.lock().await;
        let updated = db.get_instance("proj", "co-empty").unwrap().unwrap();
        assert_eq!(
            updated.status,
            coast_core::types::InstanceStatus::CheckedOut
        );
    }

    /// When canonical port forwarding fails for all ports during daemon
    /// startup restoration, the instance should be downgraded from
    /// CheckedOut to Running so the UI doesn't show a stale badge.
    #[tokio::test]
    async fn test_restore_downgrades_checked_out_when_canonical_ports_occupied() {
        use std::net::TcpListener;

        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));

        // Occupy a port so the canonical socat pre-check in the restore
        // function skips it.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port = listener.local_addr().unwrap().port();

        // Pick an ephemeral dynamic port (unused).
        let dyn_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let dynamic_port = dyn_listener.local_addr().unwrap().port();
        drop(dyn_listener);

        let inst = make_test_instance(
            "restore-co",
            "proj-a",
            coast_core::types::InstanceStatus::CheckedOut,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
            db.insert_port_allocation(
                "proj-a",
                "restore-co",
                &coast_core::types::PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: occupied_port,
                    dynamic_port,
                    is_primary: false,
                },
            )
            .unwrap();
        }

        // Subscribe to events before the restore so we can check for the
        // status change event.
        let mut event_rx = state.event_bus.subscribe();

        // Run the restore function. Canonical socat will be skipped because
        // the port is occupied. The function should downgrade to Running.
        restore_socat_for_instance(&state, &inst, "127.0.0.1").await;

        // Verify: instance status is now Running, not CheckedOut.
        let db = state.db.lock().await;
        let updated = db.get_instance("proj-a", "restore-co").unwrap().unwrap();
        assert_eq!(
            updated.status,
            coast_core::types::InstanceStatus::Running,
            "instance should be downgraded to Running after canonical restore failure"
        );
        drop(db);

        // Verify: an InstanceStatusChanged event was emitted.
        let event = event_rx.try_recv();
        assert!(event.is_ok(), "expected an InstanceStatusChanged event");
        match event.unwrap() {
            coast_core::protocol::CoastEvent::InstanceStatusChanged {
                ref name,
                ref project,
                ref status,
            } => {
                assert_eq!(name, "restore-co");
                assert_eq!(project, "proj-a");
                assert_eq!(status, "running");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // Clean up any socat processes that may have been spawned for
        // the dynamic port.
        let db = state.db.lock().await;
        let allocs = db.get_port_allocations("proj-a", "restore-co").unwrap();
        for alloc in &allocs {
            if let Some(pid) = alloc.socat_pid {
                let _ = port_manager::kill_socat(pid as u32);
            }
        }

        drop(listener);
    }

    /// When the instance is CheckedOut and canonical ports restore
    /// successfully, the instance should remain CheckedOut.
    #[tokio::test]
    async fn test_restore_keeps_checked_out_when_canonical_ports_succeed() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));

        // Find a free canonical port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let canonical_port = listener.local_addr().unwrap().port();
        drop(listener);
        // Find a free dynamic port.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dynamic_port = listener.local_addr().unwrap().port();
        drop(listener);

        let inst = make_test_instance(
            "restore-ok",
            "proj-b",
            coast_core::types::InstanceStatus::CheckedOut,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
            db.insert_port_allocation(
                "proj-b",
                "restore-ok",
                &coast_core::types::PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port,
                    dynamic_port,
                    is_primary: false,
                },
            )
            .unwrap();
        }

        restore_socat_for_instance(&state, &inst, "127.0.0.1").await;

        let db = state.db.lock().await;
        let updated = db.get_instance("proj-b", "restore-ok").unwrap().unwrap();
        // If socat is installed, canonical spawned and status stays CheckedOut.
        // If socat is NOT installed, canonical_ok=0 and it gets downgraded.
        // We test the behavior appropriate to the environment.
        let socat_available = std::process::Command::new("socat")
            .arg("-V")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok();
        if socat_available {
            assert_eq!(
                updated.status,
                coast_core::types::InstanceStatus::CheckedOut,
                "with socat available, status should remain CheckedOut"
            );
        } else {
            assert_eq!(
                updated.status,
                coast_core::types::InstanceStatus::Running,
                "without socat, status should be downgraded to Running"
            );
        }
        drop(db);

        // Cleanup any spawned socat processes.
        let db = state.db.lock().await;
        let allocs = db.get_port_allocations("proj-b", "restore-ok").unwrap();
        for alloc in &allocs {
            if let Some(pid) = alloc.socat_pid {
                let _ = port_manager::kill_socat(pid as u32);
            }
        }
    }

    #[tokio::test]
    async fn test_restore_ignores_stopped_instance_with_stale_checkout_pid() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));

        let inst = make_test_instance(
            "stopped-co",
            "proj-c",
            coast_core::types::InstanceStatus::Stopped,
        );
        {
            let db = state.db.lock().await;
            db.insert_instance(&inst).unwrap();
            db.insert_port_allocation(
                "proj-c",
                "stopped-co",
                &coast_core::types::PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 50000,
                    is_primary: false,
                },
            )
            .unwrap();
            db.update_socat_pid("proj-c", "stopped-co", "web", Some(4_194_304))
                .unwrap();
        }

        restore_running_state(&state).await;

        let db = state.db.lock().await;
        let updated = db.get_instance("proj-c", "stopped-co").unwrap().unwrap();
        assert_eq!(updated.status, coast_core::types::InstanceStatus::Stopped);
        let allocs = db.get_port_allocations("proj-c", "stopped-co").unwrap();
        assert_eq!(allocs[0].socat_pid, Some(4_194_304));
    }

    #[test]
    fn test_singleton_lock_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("coastd.lock");
        let _lock = acquire_singleton_lock(&lock_path).unwrap();
    }

    #[test]
    fn test_singleton_lock_rejects_second() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("coastd.lock");
        let _lock = acquire_singleton_lock(&lock_path).unwrap();
        let result = acquire_singleton_lock(&lock_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already running"),
            "error should mention already running, got: {err}"
        );
    }

    #[test]
    fn test_singleton_lock_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let lock_path = tmp.path().join("coastd.lock");
        {
            let _lock = acquire_singleton_lock(&lock_path).unwrap();
        } // Flock<File> dropped here, releasing the lock
        let _lock2 = acquire_singleton_lock(&lock_path).unwrap();
    }

    // -----------------------------------------------------------------------
    // In-DinD restore helpers
    // -----------------------------------------------------------------------

    /// Serializes tests that mutate COAST_HOME so they don't race.
    /// Delegates to the crate-wide shared lock in `test_support` so
    /// every site across the crate uses the SAME mutex.
    fn coast_home_env_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::coast_home_env_lock()
    }

    fn with_coast_home<F, R>(home: &std::path::Path, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = coast_home_env_lock();
        let prev = std::env::var_os("COAST_HOME");
        // Safety: guarded by mutex; restored before unlock.
        unsafe { std::env::set_var("COAST_HOME", home) };
        let r = f();
        match prev {
            Some(v) => unsafe { std::env::set_var("COAST_HOME", v) },
            None => unsafe { std::env::remove_var("COAST_HOME") },
        }
        r
    }

    #[test]
    fn test_artifact_coastfile_path_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let proj_dir = tmp.path().join("images").join("demo").join("latest");
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::write(proj_dir.join("coastfile.toml"), "").unwrap();
        let found = with_coast_home(tmp.path(), || artifact_coastfile_path("demo", None));
        assert_eq!(found, Some(proj_dir.join("coastfile.toml")));
    }

    #[test]
    fn test_artifact_coastfile_path_build_id() {
        let tmp = tempfile::tempdir().unwrap();
        let bid = "abc_20260101";
        let proj_dir = tmp.path().join("images").join("demo").join(bid);
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::write(proj_dir.join("coastfile.toml"), "").unwrap();
        let found = with_coast_home(tmp.path(), || artifact_coastfile_path("demo", Some(bid)));
        assert_eq!(found, Some(proj_dir.join("coastfile.toml")));
    }

    #[test]
    fn test_artifact_coastfile_path_falls_back_to_latest_when_build_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let latest_dir = tmp.path().join("images").join("demo").join("latest");
        std::fs::create_dir_all(&latest_dir).unwrap();
        std::fs::write(latest_dir.join("coastfile.toml"), "").unwrap();
        // build_id does not exist on disk -> falls through to latest
        let found = with_coast_home(tmp.path(), || {
            artifact_coastfile_path("demo", Some("missing_build"))
        });
        assert_eq!(found, Some(latest_dir.join("coastfile.toml")));
    }

    #[test]
    fn test_artifact_coastfile_path_none_when_no_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let found = with_coast_home(tmp.path(), || artifact_coastfile_path("demo", None));
        assert!(found.is_none());
    }

    /// The workspace restore helper is a no-op when there is no Docker
    /// client (which matches the behaviour of other restore helpers).
    /// It should complete immediately without errors.
    #[tokio::test]
    async fn test_restore_workspace_mounts_noop_without_docker() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));
        let inst = make_test_instance(
            "inst-1",
            "proj-x",
            coast_core::types::InstanceStatus::Running,
        );
        // Should complete without touching docker (state.docker is None for test).
        restore_workspace_mounts(&state, &[inst]).await;
    }

    /// Shared-service restore is also a no-op without a Docker client.
    #[tokio::test]
    async fn test_restore_shared_service_proxies_noop_without_docker() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing(db));
        let inst = make_test_instance(
            "inst-1",
            "proj-y",
            coast_core::types::InstanceStatus::Running,
        );
        restore_shared_service_proxies(&state, &[inst]).await;
    }

    /// Remote instances must be skipped by both restore helpers: their
    /// runtime state lives on the remote host, not in a local DinD.
    #[tokio::test]
    async fn test_restore_helpers_skip_remote_instances() {
        let db = state::StateDb::open_in_memory().unwrap();
        // A stub Docker client lets `state.docker.as_ref()` succeed, so we
        // exercise the `remote_host.is_some()` skip branch, not the
        // no-docker early return.
        let state = Arc::new(server::AppState::new_for_testing_with_docker(db));
        let mut inst = make_test_instance(
            "dev-remote",
            "proj-r",
            coast_core::types::InstanceStatus::Running,
        );
        inst.remote_host = Some("test-remote".to_string());

        // Both helpers must return quickly (<5s) without trying to
        // exec into the (nonexistent) stub container.
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            restore_workspace_mounts(&state, &[inst.clone()]),
        )
        .await;
        assert!(
            res.is_ok(),
            "workspace restore should skip remote instances"
        );

        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            restore_shared_service_proxies(&state, &[inst]),
        )
        .await;
        assert!(
            res.is_ok(),
            "shared-service restore should skip remote instances"
        );
    }

    /// An instance without a container_id must be skipped (it means the
    /// instance is enqueued/provisioning but not yet created).
    #[tokio::test]
    async fn test_restore_helpers_skip_instances_without_container_id() {
        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing_with_docker(db));
        let mut inst = make_test_instance(
            "inst-unprovisioned",
            "proj-z",
            coast_core::types::InstanceStatus::Running,
        );
        inst.container_id = None;
        // Should complete without calling exec_in_coast.
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            restore_workspace_mounts(&state, &[inst.clone()]),
        )
        .await;
        assert!(res.is_ok());
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            restore_shared_service_proxies(&state, &[inst]),
        )
        .await;
        assert!(res.is_ok());
    }

    /// Shared-service restore must be a no-op (early return after parsing
    /// the coastfile) when the coastfile declares no shared_services.
    #[tokio::test]
    async fn test_restore_shared_service_proxies_skips_when_no_shared_services() {
        let tmp = tempfile::tempdir().unwrap();
        let artifact_dir = tmp.path().join("images").join("proj-empty").join("latest");
        std::fs::create_dir_all(&artifact_dir).unwrap();
        std::fs::write(
            artifact_dir.join("coastfile.toml"),
            "[coast]\nname = \"proj-empty\"\nruntime = \"dind\"\n",
        )
        .unwrap();

        let db = state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(server::AppState::new_for_testing_with_docker(db));
        let inst = make_test_instance(
            "inst-e",
            "proj-empty",
            coast_core::types::InstanceStatus::Running,
        );

        let _guard = coast_home_env_lock();
        let prev = std::env::var_os("COAST_HOME");
        // Safety: serialized by coast_home_env_lock.
        unsafe { std::env::set_var("COAST_HOME", tmp.path()) };
        // Since there are no shared services, this returns before attempting
        // any docker exec -- and is well under the per-instance timeout.
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            restore_shared_service_proxies(&state, &[inst]),
        )
        .await;
        match prev {
            Some(v) => unsafe { std::env::set_var("COAST_HOME", v) },
            None => unsafe { std::env::remove_var("COAST_HOME") },
        }
        assert!(
            res.is_ok(),
            "shared-service restore should return promptly when the coastfile has no shared_services"
        );
    }
}
