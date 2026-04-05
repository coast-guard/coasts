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
mod remote;
pub mod server;
#[allow(dead_code)]
mod shared_services;
#[allow(dead_code)]
mod state;

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

    // Clean up any orphaned socat processes from a previous daemon session
    port_manager::cleanup_orphaned_socat();
    if port_manager::running_in_wsl() {
        port_manager::cleanup_orphaned_checkout_bridges();
    }

    // Open state database
    let db_path = coast_dir.join("state.db");
    let db = StateDb::open(&db_path)?;
    info!(path = %db_path.display(), "state database opened");

    // Create shared application state
    let state = Arc::new(AppState::new(db));

    restore_running_state(&state).await;

    // Start background git watcher (polls .git/HEAD for known projects)
    git_watcher::spawn_git_watcher(Arc::clone(&state));

    // Start background Docker connectivity watcher
    docker_watcher::spawn_docker_watcher(Arc::clone(&state));

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

    // Start the HTTP API server
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
                        .filter(|s| s.status != "running")
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
    let cf_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    coast_core::coastfile::Coastfile::from_file(&cf_path)
        .map(|cf| cf.healthcheck)
        .unwrap_or_default()
}

/// Background loop that probes each port's dynamic_port every 5 seconds.
/// Uses HTTP GET for ports with a `[healthcheck]` path configured, falls back
/// to TCP connect for ports without one. Any HTTP response = healthy.
async fn port_health_cache_loop(state: Arc<server::AppState>) {
    use coast_core::types::PortHealthStatus;
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_default();

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
            let healthcheck_paths = load_healthcheck_paths(project);
            let allocs = {
                let db = state.db.lock().await;
                db.get_port_allocations(project, name).unwrap_or_default()
            };
            let key = format!("{project}:{name}");
            let mut statuses: Vec<PortHealthStatus> = Vec::new();
            for alloc in &allocs {
                let port = alloc.dynamic_port;
                let mapping: coast_core::types::PortMapping = alloc.into();

                let healthy = if let Some(path) = healthcheck_paths.get(&mapping.logical_name) {
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
            state.port_health_cache.lock().await.insert(key, statuses);
            if changed {
                state.emit_event(coast_core::protocol::CoastEvent::PortHealthChanged {
                    name: name.clone(),
                    project: project.clone(),
                });
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
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
        let cid = inst.container_id.as_ref().unwrap();
        let coast_ip = match rt.get_container_ip(cid).await {
            Ok(ip) => ip.to_string(),
            Err(e) => {
                warn!(
                    instance = %inst.name, project = %inst.project, error = %e,
                    "could not resolve container IP, skipping port restore"
                );
                continue;
            }
        };
        restore_socat_for_instance(state, inst, &coast_ip).await;
    }
}

/// Spawn socat forwarders for a single instance.
#[allow(clippy::cognitive_complexity)]
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
    let use_wsl_bridge = state.docker.is_some() && port_manager::running_in_wsl();

    let mut dynamic_ok = 0u32;
    let mut canonical_ok = 0u32;
    for entry in &cmds {
        match port_manager::spawn_socat(&entry.cmd) {
            Ok(pid) => {
                let _ = pid;
                dynamic_ok += 1;
            }
            Err(e) => {
                warn!(
                    instance = %inst.name, port = %entry.logical_name,
                    error = %e, "failed to restore socat"
                );
            }
        }
    }

    if is_checked_out {
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
                Ok(()) => {
                    canonical_ok += allocs.len() as u32;
                }
                Err(e) => {
                    warn!(
                        instance = %inst.name,
                        error = %e,
                        "failed to restore WSL checkout bridge"
                    );
                }
            }
        } else {
            for alloc in &allocs {
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
                        canonical_ok += 1;
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
    }

    // If the instance was checked out but none of its canonical forwarders
    // could be restored (ports occupied by another process, socat missing, etc.),
    // downgrade to Running so the UI doesn't show a stale "checked out" badge
    // with no working canonical ports.
    if is_checked_out && canonical_ok == 0 {
        let expected_canonical = allocs.len();
        if expected_canonical > 0 {
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
        CoastEvent::InstanceCreated { name, project }
        | CoastEvent::InstanceStarted { name, project } => {
            let key = api::ws_stats::stats_key(project, name);
            let db = state.db.lock().await;
            if let Ok(Some(inst)) = db.get_instance(project, name) {
                if let Some(ref cid) = inst.container_id {
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
        let cid = inst.container_id.as_ref().unwrap().clone();
        let key = api::ws_stats::stats_key(&inst.project, &inst.name);
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

    // Restore socat port forwarding (dynamic + canonical for checked-out).
    if state.docker.is_some() {
        restore_socat_forwarding(state, &active_instances).await;
    }

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
            remote_name: None,
        }
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
}
