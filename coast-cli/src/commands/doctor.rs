/// `coast doctor` — diagnose and repair orphaned state.
///
/// Scans the state DB for instances whose Docker containers no longer exist
/// (e.g. after a `docker system prune`) and removes the orphaned records.
/// Also detects dangling Docker containers that have no matching state DB record
/// (e.g. from a crashed provisioning run) and force-removes them.
use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Only report problems without fixing them.
    #[arg(long)]
    pub dry_run: bool,
}

#[allow(clippy::too_many_lines)]
pub async fn execute(args: &DoctorArgs) -> Result<()> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    let db_path = home.join(".coast").join("state.db");

    if !db_path.exists() {
        println!(
            "{} No state database found. Nothing to check.",
            "ok".green().bold()
        );
        return Ok(());
    }

    let db = rusqlite::Connection::open(&db_path).context("Failed to open state database")?;

    let docker = bollard::Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker. Is Docker running?")?;

    let mut fixes: Vec<String> = Vec::new();

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
                        println!(
                            "  {} Instance {} has missing container ({}), status: {}",
                            "!!".yellow().bold(),
                            label.bold(),
                            &cid[..12.min(cid.len())],
                            status,
                        );
                    } else {
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
                    println!(
                        "  {} Instance {} has no container ID but status is '{}'",
                        "!!".yellow().bold(),
                        label.bold(),
                        status,
                    );
                } else {
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

    // Report
    if args.dry_run {
        if fixes.is_empty() {
            println!("{} No issues found (dry run).", "ok".green().bold());
        }
    } else if fixes.is_empty() {
        println!(
            "{} Everything looks good. No orphaned state found.",
            "ok".green().bold()
        );
    } else {
        for fix in &fixes {
            println!("  {} {}", "fix".green().bold(), fix);
        }
        println!(
            "\n{} Fixed {} issue{}.",
            "ok".green().bold(),
            fixes.len(),
            if fixes.len() == 1 { "" } else { "s" },
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

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
}
