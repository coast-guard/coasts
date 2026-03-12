/// `coast doctor` — diagnose and repair orphaned state.
///
/// Scans the state DB for instances whose Docker containers no longer exist
/// (e.g. after a `docker system prune`) and removes the orphaned records.
/// Also detects dangling Docker containers that have no matching state DB record
/// (e.g. from a crashed provisioning run) and force-removes them.
use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StaleCheckoutRow {
    project: String,
    instance_name: String,
    logical_name: String,
    status: Option<String>,
    socat_pid: i32,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Only report problems without fixing them.
    #[arg(long)]
    pub dry_run: bool,
}

fn active_coast_home() -> Result<PathBuf> {
    coast_core::artifact::coast_home().context("Could not determine Coast home directory")
}

fn active_state_db_path() -> Result<PathBuf> {
    Ok(active_coast_home()?.join("state.db"))
}

fn stale_docker_dry_run_message(pid: u32, port: u16) -> String {
    format!(
        "Docker Desktop (pid {pid}) is holding port {port} with no matching container port mapping.\n\
         This stale binding blocks `coast checkout` from forwarding this port via socat."
    )
}

fn stale_docker_manual_action_message(port: u16) -> String {
    format!(
        "Port {port} is held by Docker Desktop but no Coast containers exist to restart.\n\
         Run killall com.docker.backend && open -a Docker to release the stale port binding."
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessStatus {
    state: char,
    command: String,
}

fn parse_ps_process_status(output: &str) -> Option<ProcessStatus> {
    let line = output.lines().find(|line| !line.trim().is_empty())?;
    let trimmed = line.trim();
    let mut parts = trimmed.split_whitespace();
    let state = parts.next()?.chars().next()?;
    let command = parts.collect::<Vec<_>>().join(" ");
    Some(ProcessStatus { state, command })
}

fn inspect_process_status(pid: u32) -> Option<ProcessStatus> {
    let output = std::process::Command::new("ps")
        .args(["-o", "state=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    parse_ps_process_status(&String::from_utf8_lossy(&output.stdout))
}

fn process_looks_stale(pid: u32) -> bool {
    match inspect_process_status(pid) {
        None => true,
        Some(status) => status.state == 'Z' || status.command != "socat",
    }
}

#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn execute(args: &DoctorArgs) -> Result<()> {
    let coast_home = active_coast_home()?;
    let db_path = active_state_db_path()?;

    if !db_path.exists() {
        println!(
            "{} No state database found in {}. Nothing to check.",
            "ok".green().bold(),
            coast_home.display()
        );
        return Ok(());
    }

    let db = rusqlite::Connection::open(&db_path).context("Failed to open state database")?;

    let docker = coast_docker::host::connect_to_host_docker()
        .context("Failed to connect to Docker. Is Docker running and is your active Docker context reachable?")?;

    let mut fixes: Vec<String> = Vec::new();
    let mut findings: Vec<String> = Vec::new();

    // Check instances
    {
        let mut stmt = db.prepare("SELECT name, project, status, container_id FROM instances")?;

        let rows: Vec<(String, String, String, Option<String>)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(Result::ok)
            .collect();

        for (name, project, status, container_id) in &rows {
            if let Some(cid) = container_id {
                let exists = docker.inspect_container(cid, None).await.is_ok();

                if !exists {
                    let label = format!("{project}/{name}");
                    if args.dry_run {
                        findings.push(format!(
                            "Instance {label} has missing container ({}) with status {status}",
                            &cid[..12.min(cid.len())]
                        ));
                        println!(
                            "  {} Instance {} has missing container ({}), status: {}",
                            "!!".yellow().bold(),
                            label.bold(),
                            &cid[..12.min(cid.len())],
                            status,
                        );
                    } else {
                        clear_instance_socat_pids(&db, project, name, &mut fixes, kill_socat_pid)?;
                        db.execute(
                            "DELETE FROM port_allocations WHERE project = ?1 AND instance_name = ?2",
                            rusqlite::params![project, name],
                        )?;
                        db.execute(
                            "DELETE FROM instances WHERE project = ?1 AND name = ?2",
                            rusqlite::params![project, name],
                        )?;
                        fixes.push(format!("Removed orphaned instance {}", label));
                    }
                }
            } else if status != "stopped" {
                let label = format!("{project}/{name}");
                if args.dry_run {
                    findings.push(format!(
                        "Instance {label} has no container ID but status is '{status}'"
                    ));
                    println!(
                        "  {} Instance {} has no container ID but status is '{}'",
                        "!!".yellow().bold(),
                        label.bold(),
                        status,
                    );
                } else {
                    clear_instance_socat_pids(&db, project, name, &mut fixes, kill_socat_pid)?;
                    db.execute(
                        "DELETE FROM port_allocations WHERE project = ?1 AND instance_name = ?2",
                        rusqlite::params![project, name],
                    )?;
                    db.execute(
                        "DELETE FROM instances WHERE project = ?1 AND name = ?2",
                        rusqlite::params![project, name],
                    )?;
                    fixes.push(format!("Removed orphaned instance {}", label));
                }
            }
        }
    }

    repair_stale_checkout_rows(&db, args.dry_run, &mut fixes, &mut findings, kill_socat_pid)?;

    // Check shared services
    {
        let mut stmt =
            db.prepare("SELECT project, service_name, container_id, status FROM shared_services")?;

        let rows: Vec<(String, String, Option<String>, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(Result::ok)
            .collect();

        for (project, service_name, container_id, status) in &rows {
            if status == "running" {
                let exists = if let Some(cid) = container_id {
                    docker.inspect_container(cid, None).await.is_ok()
                } else {
                    false
                };

                if !exists {
                    let label = format!("{project}/{service_name}");
                    if args.dry_run {
                        findings.push(format!(
                            "Shared service {label} marked running but container is gone"
                        ));
                        println!(
                            "  {} Shared service {} marked running but container is gone",
                            "!!".yellow().bold(),
                            label.bold(),
                        );
                    } else {
                        db.execute(
                            "UPDATE shared_services SET status = 'stopped', container_id = NULL WHERE project = ?1 AND service_name = ?2",
                            rusqlite::params![project, service_name],
                        )?;
                        fixes.push(format!("Marked shared service {} as stopped", label));
                    }
                }
            }
        }
    }

    // Check for dangling containers (exist in Docker but not in state DB)
    {
        use bollard::container::ListContainersOptions;
        use std::collections::HashMap;

        let mut filters = HashMap::new();
        filters.insert("label".to_string(), vec!["coast.managed=true".to_string()]);
        let opts = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        if let Ok(containers) = docker.list_containers(Some(opts)).await {
            for container in &containers {
                let labels = container.labels.as_ref();
                let project = labels.and_then(|l| l.get("coast.project")).cloned();
                let instance = labels.and_then(|l| l.get("coast.instance")).cloned();

                if let (Some(proj), Some(inst)) = (project, instance) {
                    let exists: bool = db
                        .prepare("SELECT 1 FROM instances WHERE project = ?1 AND name = ?2")
                        .and_then(|mut s| s.exists(rusqlite::params![&proj, &inst]))
                        .unwrap_or(false);

                    if !exists {
                        let container_name = container
                            .names
                            .as_ref()
                            .and_then(|n| n.first())
                            .map(|n| n.trim_start_matches('/').to_string())
                            .unwrap_or_else(|| container.id.clone().unwrap_or_default());
                        let label = format!("{proj}/{inst}");

                        if args.dry_run {
                            findings.push(format!(
                                "Dangling container '{container_name}' for {label} has no state DB record"
                            ));
                            println!(
                                "  {} Dangling container '{}' for {} has no state DB record",
                                "!!".yellow().bold(),
                                container_name.bold(),
                                label.bold(),
                            );
                        } else {
                            if let Some(ref cid) = container.id {
                                let rm_opts = bollard::container::RemoveContainerOptions {
                                    force: true,
                                    v: true,
                                    ..Default::default()
                                };
                                let _ = docker.remove_container(cid, Some(rm_opts)).await;
                                let cache_vol = format!("coast-dind--{proj}--{inst}");
                                let _ = docker.remove_volume(&cache_vol, None).await;
                            }
                            fixes.push(format!(
                                "Removed dangling container '{}' for {}",
                                container_name, label,
                            ));
                        }
                    }
                }
            }
        }
    }

    handle_stale_docker_port_bindings(args, &docker, &mut fixes, &mut findings).await;

    // Report
    if args.dry_run {
        if findings.is_empty() {
            println!("{} No issues found (dry run).", "ok".green().bold());
        } else {
            println!(
                "\n{} Found {} issue{} (dry run).",
                "ok".green().bold(),
                findings.len(),
                if findings.len() == 1 { "" } else { "s" },
            );
        }
    } else if fixes.is_empty() && findings.is_empty() {
        println!(
            "{} Everything looks good. No orphaned state found.",
            "ok".green().bold()
        );
    } else {
        for fix in &fixes {
            println!("  {} {}", "fix".green().bold(), fix);
        }
        if !fixes.is_empty() {
            println!(
                "\n{} Fixed {} issue{}.",
                "ok".green().bold(),
                fixes.len(),
                if fixes.len() == 1 { "" } else { "s" },
            );
        }
        if !findings.is_empty() {
            println!(
                "\n{} {} issue{} still require attention.",
                "!!".yellow().bold(),
                findings.len(),
                if findings.len() == 1 { "" } else { "s" },
            );
        }
    }

    Ok(())
}

fn find_stale_checkout_rows_with(
    db: &rusqlite::Connection,
    is_stale_pid: impl Fn(u32) -> bool,
) -> Result<Vec<StaleCheckoutRow>> {
    let mut stmt = db.prepare(
        "SELECT p.project, p.instance_name, p.logical_name, p.socat_pid, i.status
         FROM port_allocations p
         LEFT JOIN instances i
           ON i.project = p.project AND i.name = p.instance_name
         WHERE p.socat_pid IS NOT NULL
         ORDER BY p.project, p.instance_name, p.logical_name",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(StaleCheckoutRow {
            project: row.get(0)?,
            instance_name: row.get(1)?,
            logical_name: row.get(2)?,
            socat_pid: row.get(3)?,
            status: row.get(4)?,
        })
    })?;

    let mut stale = Vec::new();
    for row in rows {
        let row = row?;
        let is_stale =
            row.status.as_deref() != Some("checked_out") || is_stale_pid(row.socat_pid as u32);
        if is_stale {
            stale.push(row);
        }
    }

    Ok(stale)
}

fn find_stale_checkout_rows(db: &rusqlite::Connection) -> Result<Vec<StaleCheckoutRow>> {
    find_stale_checkout_rows_with(db, process_looks_stale)
}

fn repair_stale_checkout_rows(
    db: &rusqlite::Connection,
    dry_run: bool,
    fixes: &mut Vec<String>,
    findings: &mut Vec<String>,
    killer: impl Fn(u32) -> Result<()>,
) -> Result<()> {
    let stale_rows = find_stale_checkout_rows(db)?;

    for row in stale_rows {
        let label = format!("{}/{}", row.project, row.instance_name);
        let reason = match row.status.as_deref() {
            Some(status) => format!("instance status is '{status}'"),
            None => "instance record is missing".to_string(),
        };

        if dry_run {
            findings.push(format!(
                "Stale checkout pid {} for {} ({}, service '{}') would be cleared",
                row.socat_pid, label, reason, row.logical_name
            ));
            println!(
                "  {} Stale checkout pid {} for {} ({}, service '{}') would be cleared",
                "!!".yellow().bold(),
                row.socat_pid,
                label.bold(),
                reason,
                row.logical_name,
            );
            continue;
        }

        let _ = killer(row.socat_pid as u32);
        if row.status.is_some() {
            db.execute(
                "UPDATE port_allocations SET socat_pid = NULL
                 WHERE project = ?1 AND instance_name = ?2 AND logical_name = ?3",
                rusqlite::params![row.project, row.instance_name, row.logical_name],
            )?;
            if row.status.as_deref() == Some("checked_out") {
                db.execute(
                    "UPDATE instances SET status = 'running' WHERE project = ?1 AND name = ?2",
                    rusqlite::params![row.project, row.instance_name],
                )?;
            }
        } else {
            db.execute(
                "DELETE FROM port_allocations
                 WHERE project = ?1 AND instance_name = ?2 AND logical_name = ?3",
                rusqlite::params![row.project, row.instance_name, row.logical_name],
            )?;
        }
        fixes.push(format!(
            "Cleared stale checkout pid {} for {} ({})",
            row.socat_pid, label, reason,
        ));
    }

    Ok(())
}

fn clear_instance_socat_pids(
    db: &rusqlite::Connection,
    project: &str,
    instance_name: &str,
    fixes: &mut Vec<String>,
    killer: impl Fn(u32) -> Result<()>,
) -> Result<usize> {
    let mut stmt = db.prepare(
        "SELECT logical_name, socat_pid
         FROM port_allocations
         WHERE project = ?1 AND instance_name = ?2 AND socat_pid IS NOT NULL
         ORDER BY logical_name",
    )?;
    let rows = stmt.query_map(rusqlite::params![project, instance_name], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
    })?;

    let mut cleared = 0usize;
    for row in rows {
        let (logical_name, pid) = row?;
        let _ = killer(pid as u32);
        db.execute(
            "UPDATE port_allocations SET socat_pid = NULL
             WHERE project = ?1 AND instance_name = ?2 AND logical_name = ?3",
            rusqlite::params![project, instance_name, logical_name],
        )?;
        fixes.push(format!(
            "Killed stale checkout pid {} for {}/{} ({})",
            pid, project, instance_name, logical_name,
        ));
        cleared += 1;
    }

    Ok(cleared)
}

fn kill_socat_pid(pid: u32) -> Result<()> {
    let nix_pid = Pid::from_raw(pid as i32);
    match signal::killpg(nix_pid, Signal::SIGKILL) {
        Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
        Err(nix::errno::Errno::EPERM) if process_looks_stale(pid) => Ok(()),
        Err(err) => Err(anyhow::anyhow!(
            "failed to kill socat process group (PGID {}): {}",
            pid,
            err
        )),
    }
}

/// Returns stale Docker Desktop port bindings as `(port, pid)` pairs.
async fn find_stale_docker_ports(docker: &bollard::Docker) -> Vec<(u16, u32)> {
    use std::collections::HashSet;

    let mut container_ports: HashSet<u16> = HashSet::new();
    if let Ok(containers) = docker
        .list_containers(Some(bollard::container::ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        }))
        .await
    {
        for c in &containers {
            if let Some(ports) = &c.ports {
                for p in ports {
                    if let Some(public_port) = p.public_port {
                        container_ports.insert(public_port);
                    }
                }
            }
        }
    }

    let output = match tokio::process::Command::new("lsof")
        .args(["-iTCP", "-sTCP:LISTEN", "-nP", "-F", "pcn"])
        .output()
        .await
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    parse_stale_ports_from_lsof(&output, &container_ports)
}

async fn handle_stale_docker_port_bindings(
    args: &DoctorArgs,
    docker: &bollard::Docker,
    fixes: &mut Vec<String>,
    findings: &mut Vec<String>,
) {
    let stale_ports = find_stale_docker_ports(docker).await;
    for (port, pid) in &stale_ports {
        if args.dry_run {
            findings.push(format!(
                "Docker Desktop (pid {}) is holding stale port {} with no matching container port mapping",
                pid, port
            ));
            println!(
                "  {} {}",
                "!!".yellow().bold(),
                stale_docker_dry_run_message(*pid, *port).replace(
                    &format!("port {port}"),
                    &format!("port {}", port.to_string().bold())
                ),
            );
            continue;
        }

        let restarted = restart_coast_containers(docker).await;
        if restarted > 0 {
            fixes.push(format!(
                "Restarted {} Coast container(s) to clear stale Docker port binding on port {}",
                restarted, port,
            ));
        } else {
            findings.push(format!(
                "Port {} is held by Docker Desktop but no Coast containers exist to restart",
                port
            ));
            println!(
                "  {} {}",
                "!!".yellow().bold(),
                stale_docker_manual_action_message(*port)
                    .replace(
                        &format!("Port {port}"),
                        &format!("Port {}", port.to_string().bold())
                    )
                    .replace(
                        "killall com.docker.backend && open -a Docker",
                        &"killall com.docker.backend && open -a Docker"
                            .bold()
                            .to_string(),
                    ),
            );
        }
        break;
    }
}

fn parse_stale_ports_from_lsof(
    output: &str,
    container_ports: &std::collections::HashSet<u16>,
) -> Vec<(u16, u32)> {
    let mut stale: Vec<(u16, u32)> = Vec::new();
    let mut current_pid: u32 = 0;
    let mut is_docker = false;

    for line in output.lines() {
        if let Some(pid_str) = line.strip_prefix('p') {
            current_pid = pid_str.parse().unwrap_or(0);
            is_docker = false;
        } else if let Some(cmd) = line.strip_prefix('c') {
            is_docker = cmd.contains("com.docker") || cmd.contains("vpnkit");
        } else if let Some(name) = line.strip_prefix('n') {
            if !is_docker {
                continue;
            }
            if let Some(port_str) = name.rsplit(':').next() {
                if let Ok(port) = port_str.parse::<u16>() {
                    if !container_ports.contains(&port) && !stale.iter().any(|(p, _)| *p == port) {
                        stale.push((port, current_pid));
                    }
                }
            }
        }
    }

    stale
}

/// Restart all running Coast-managed containers.
async fn restart_coast_containers(docker: &bollard::Docker) -> usize {
    use std::collections::HashMap;

    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec!["coast.managed=true".to_string()]);
    filters.insert("status".to_string(), vec!["running".to_string()]);

    let opts = bollard::container::ListContainersOptions {
        all: false,
        filters,
        ..Default::default()
    };

    let Ok(containers) = docker.list_containers(Some(opts)).await else {
        return 0;
    };

    let mut count = 0;
    for container in &containers {
        if let Some(ref id) = container.id {
            if docker
                .restart_container(
                    id,
                    Some(bollard::container::RestartContainerOptions { t: 10 }),
                )
                .await
                .is_ok()
            {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::sync::{Arc, Mutex, OnceLock};

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: DoctorArgs,
    }

    #[test]
    fn test_doctor_args_default() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(!cli.args.dry_run);
    }

    #[test]
    fn test_doctor_args_dry_run() {
        let cli = TestCli::try_parse_from(["test", "--dry-run"]).unwrap();
        assert!(cli.args.dry_run);
    }

    #[test]
    fn test_dangling_cache_volume_name_construction() {
        let project = "my-app";
        let instance = "dev-1";
        let vol = format!("coast-dind--{project}--{instance}");
        assert_eq!(vol, "coast-dind--my-app--dev-1");
    }

    #[test]
    fn test_dangling_container_name_trim() {
        let docker_name = "/my-app-coasts-dev-1";
        let trimmed = docker_name.trim_start_matches('/');
        assert_eq!(trimmed, "my-app-coasts-dev-1");
    }

    #[test]
    fn test_parse_lsof_output_extracts_docker_ports() {
        let output = "p1234\nccom.docker.backend\nn*:4000\nn[::]:4000\np5678\ncnode\nn*:3000\n";
        let container_ports = std::collections::HashSet::new();
        let stale = parse_stale_ports_from_lsof(output, &container_ports);
        assert_eq!(stale, vec![(4000, 1234)]);
    }

    #[test]
    fn test_parse_lsof_skips_container_published_ports() {
        let output = "p1234\nccom.docker.backend\nn*:4000\nn*:5432\n";
        let mut container_ports: std::collections::HashSet<u16> = std::collections::HashSet::new();
        container_ports.insert(4000);
        let stale = parse_stale_ports_from_lsof(output, &container_ports);
        assert_eq!(stale, vec![(5432, 1234)]);
    }

    #[test]
    fn test_parse_ps_process_status_parses_state_and_command() {
        let parsed = parse_ps_process_status("Z socat\n").unwrap();
        assert_eq!(parsed.state, 'Z');
        assert_eq!(parsed.command, "socat");
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_active_state_db_path_uses_coast_home_env() {
        let _guard = env_lock().lock().unwrap();
        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", "/tmp/coast-dev-test-home");
        }

        let path = active_state_db_path().unwrap();
        assert_eq!(path, PathBuf::from("/tmp/coast-dev-test-home/state.db"));

        match prev {
            Some(value) => unsafe { std::env::set_var("COAST_HOME", value) },
            None => unsafe { std::env::remove_var("COAST_HOME") },
        }
    }

    #[test]
    fn test_stale_docker_dry_run_message_mentions_port_and_pid() {
        let message = stale_docker_dry_run_message(1234, 3000);
        assert!(message.contains("1234"));
        assert!(message.contains("3000"));
        assert!(message.contains("stale binding"));
    }

    #[test]
    fn test_stale_docker_manual_action_message_mentions_restart_command() {
        let message = stale_docker_manual_action_message(3000);
        assert!(message.contains("3000"));
        assert!(message.contains("killall com.docker.backend && open -a Docker"));
    }

    fn setup_test_db() -> rusqlite::Connection {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(
            "
            CREATE TABLE instances (
                name TEXT NOT NULL,
                project TEXT NOT NULL,
                status TEXT NOT NULL,
                container_id TEXT,
                PRIMARY KEY (project, name)
            );
            CREATE TABLE port_allocations (
                project TEXT NOT NULL,
                instance_name TEXT NOT NULL,
                logical_name TEXT NOT NULL,
                canonical_port INTEGER NOT NULL,
                dynamic_port INTEGER NOT NULL,
                socat_pid INTEGER,
                PRIMARY KEY (project, instance_name, logical_name)
            );
            ",
        )
        .unwrap();
        db
    }

    #[test]
    fn test_find_stale_checkout_rows_includes_stopped_and_running_instances() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["dev-1", "proj", "stopped"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["dev-2", "proj", "running"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params!["dev-3", "proj", "checked_out"],
        )
        .unwrap();
        for (instance, logical_name, pid) in [
            ("dev-1", "web", 1111),
            ("dev-2", "api", 2222),
            ("dev-3", "db", 3333),
        ] {
            db.execute(
                "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
                 VALUES ('proj', ?1, ?2, 3000, 50000, ?3)",
                rusqlite::params![instance, logical_name, pid],
            )
            .unwrap();
        }

        let stale = find_stale_checkout_rows_with(&db, |_| false).unwrap();
        assert_eq!(stale.len(), 2);
        assert!(stale.iter().any(|row| row.instance_name == "dev-1"));
        assert!(stale.iter().any(|row| row.instance_name == "dev-2"));
        assert!(!stale.iter().any(|row| row.instance_name == "dev-3"));
    }

    #[test]
    fn test_find_stale_checkout_rows_includes_missing_instance_rows() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'ghost', 'web', 3000, 50000, 4444)",
            [],
        )
        .unwrap();

        let stale = find_stale_checkout_rows_with(&db, |_| false).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].instance_name, "ghost");
        assert!(stale[0].status.is_none());
    }

    #[test]
    fn test_find_stale_checkout_rows_includes_checked_out_rows_with_stale_pid() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'checked_out', 'cid-1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 1234)",
            [],
        )
        .unwrap();

        let stale = find_stale_checkout_rows_with(&db, |pid| pid == 1234).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].instance_name, "dev-1");
        assert_eq!(stale[0].status.as_deref(), Some("checked_out"));
    }

    #[test]
    fn test_repair_stale_checkout_rows_clears_pid_for_stopped_instance() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'stopped', NULL)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 5555)",
            [],
        )
        .unwrap();

        let killed: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let killed_clone = Arc::clone(&killed);
        let mut fixes = Vec::new();
        let mut findings = Vec::new();
        repair_stale_checkout_rows(&db, false, &mut fixes, &mut findings, move |pid| {
            killed_clone.lock().unwrap().push(pid);
            Ok(())
        })
        .unwrap();

        let pid: Option<i32> = db
            .query_row(
                "SELECT socat_pid FROM port_allocations WHERE project = 'proj' AND instance_name = 'dev-1' AND logical_name = 'web'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(pid.is_none());
        assert_eq!(*killed.lock().unwrap(), vec![5555]);
        assert_eq!(fixes.len(), 1);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_repair_stale_checkout_rows_deletes_orphaned_port_allocation() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'ghost', 'web', 3000, 50000, 6666)",
            [],
        )
        .unwrap();

        let mut fixes = Vec::new();
        let mut findings = Vec::new();
        repair_stale_checkout_rows(&db, false, &mut fixes, &mut findings, |_pid| Ok(())).unwrap();

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM port_allocations WHERE project = 'proj' AND instance_name = 'ghost'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(fixes.len(), 1);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_repair_stale_checkout_rows_dry_run_records_findings_without_mutation() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'stopped', NULL)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 5555)",
            [],
        )
        .unwrap();

        let mut fixes = Vec::new();
        let mut findings = Vec::new();
        repair_stale_checkout_rows(&db, true, &mut fixes, &mut findings, |_pid| {
            panic!("dry-run should not kill pids")
        })
        .unwrap();

        let pid: Option<i32> = db
            .query_row(
                "SELECT socat_pid FROM port_allocations WHERE project = 'proj' AND instance_name = 'dev-1' AND logical_name = 'web'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pid, Some(5555));
        assert!(fixes.is_empty());
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn test_repair_stale_checkout_rows_demotes_checked_out_instance_with_stale_pid() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'checked_out', 'cid-1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 5555)",
            [],
        )
        .unwrap();

        let mut fixes = Vec::new();
        let mut findings = Vec::new();
        repair_stale_checkout_rows(&db, false, &mut fixes, &mut findings, |_pid| {
            Err(anyhow::anyhow!("stale pid"))
        })
        .unwrap();

        let status: String = db
            .query_row(
                "SELECT status FROM instances WHERE project = 'proj' AND name = 'dev-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "running");
        let pid: Option<i32> = db
            .query_row(
                "SELECT socat_pid FROM port_allocations WHERE project = 'proj' AND instance_name = 'dev-1' AND logical_name = 'web'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(pid.is_none());
    }

    #[test]
    fn test_clear_instance_socat_pids_clears_all_rows_for_instance() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'checked_out', 'cid-1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 7777)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'api', 8080, 50001, 8888)",
            [],
        )
        .unwrap();

        let killed: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let killed_clone = Arc::clone(&killed);
        let mut fixes = Vec::new();
        let cleared = clear_instance_socat_pids(&db, "proj", "dev-1", &mut fixes, move |pid| {
            killed_clone.lock().unwrap().push(pid);
            Ok(())
        })
        .unwrap();

        assert_eq!(cleared, 2);
        let pids: Vec<Option<i32>> = db
            .prepare(
                "SELECT socat_pid FROM port_allocations
                 WHERE project = 'proj' AND instance_name = 'dev-1'
                 ORDER BY logical_name",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(pids, vec![None, None]);
        assert_eq!(*killed.lock().unwrap(), vec![8888, 7777]);
    }

    #[test]
    fn test_clear_instance_socat_pids_clears_rows_even_if_kill_fails() {
        let db = setup_test_db();
        db.execute(
            "INSERT INTO instances (name, project, status, container_id) VALUES ('dev-1', 'proj', 'checked_out', 'cid-1')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
             VALUES ('proj', 'dev-1', 'web', 3000, 50000, 7777)",
            [],
        )
        .unwrap();

        let mut fixes = Vec::new();
        let cleared = clear_instance_socat_pids(&db, "proj", "dev-1", &mut fixes, |_pid| {
            Err(anyhow::anyhow!("stale pid"))
        })
        .unwrap();

        assert_eq!(cleared, 1);
        let pid: Option<i32> = db
            .query_row(
                "SELECT socat_pid FROM port_allocations WHERE project = 'proj' AND instance_name = 'dev-1' AND logical_name = 'web'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(pid.is_none());
    }
}
