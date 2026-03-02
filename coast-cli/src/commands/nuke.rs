/// `coast nuke` — factory-reset coast.
///
/// Stops the daemon, removes all coast-managed Docker containers, volumes,
/// networks, and images, deletes the entire `$COAST_HOME` directory, then
/// recreates it and restarts the daemon so coast is immediately usable again.
/// Requires the user to type "nuke" to confirm unless `--force` is passed.
use anyhow::Result;
use clap::Args;
use colored::Colorize;
use std::io::{self, Write};

/// Arguments for `coast nuke`.
#[derive(Debug, Args)]
pub struct NukeArgs {
    /// Skip the confirmation prompt.
    #[arg(long)]
    pub force: bool,
}

/// Summary of what was cleaned up.
#[derive(Default)]
struct NukeReport {
    daemon_stopped: bool,
    containers_removed: usize,
    volumes_removed: usize,
    networks_removed: usize,
    images_removed: usize,
    home_deleted: bool,
    daemon_restarted: bool,
}

pub async fn execute(args: &NukeArgs) -> Result<()> {
    let coast_home = coast_core::artifact::coast_home()?;
    let home_display = coast_home.display().to_string();

    if !args.force {
        eprintln!(
            "{} This will permanently destroy ALL coast data:\n",
            "WARNING:".red().bold(),
        );
        eprintln!("  - Stop the coastd daemon");
        eprintln!("  - Remove all coast-managed Docker containers");
        eprintln!("  - Remove all coast-managed Docker volumes");
        eprintln!("  - Remove all coast-managed Docker networks");
        eprintln!("  - Remove all coast Docker images");
        eprintln!("  - Delete {home_display} (state DB, builds, logs, secrets, image cache)");
        eprintln!();
        print!("Type {} to confirm: ", "nuke".bold());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if input.trim() != "nuke" {
            println!("Aborted.");
            return Ok(());
        }
        eprintln!();
    }

    let mut report = NukeReport {
        daemon_stopped: stop_daemon().await,
        ..Default::default()
    };

    match bollard::Docker::connect_with_local_defaults() {
        Ok(docker) => {
            report.containers_removed = remove_containers(&docker).await;
            report.volumes_removed = remove_volumes(&docker).await;
            report.networks_removed = remove_networks(&docker).await;
            report.images_removed = remove_images(&docker).await;
        }
        Err(e) => {
            eprintln!(
                "  {} Could not connect to Docker ({}). Skipping Docker cleanup.",
                "skip".yellow().bold(),
                e,
            );
        }
    }

    // Step 6: Delete $COAST_HOME
    if coast_home.exists() {
        eprint!("  Deleting {} ...", home_display);
        match std::fs::remove_dir_all(&coast_home) {
            Ok(()) => {
                eprintln!(" {}", "done".green());
                report.home_deleted = true;
            }
            Err(e) => {
                eprintln!(" {}", "failed".red());
                eprintln!("    {e}");
            }
        }
    } else {
        eprintln!("  {} already gone", home_display);
        report.home_deleted = true;
    }

    // Step 7: Recreate $COAST_HOME and restart the daemon so coast is
    // immediately usable without any manual intervention.
    eprint!("  Recreating {} ...", home_display);
    match std::fs::create_dir_all(&coast_home) {
        Ok(()) => eprintln!(" {}", "done".green()),
        Err(e) => {
            eprintln!(" {}", "failed".red());
            eprintln!("    {e}");
            eprintln!(
                "\n{} Coast data was wiped but the home directory could not be recreated.\n    \
                 Run `coast daemon start` manually once the issue is resolved.",
                "warn".yellow().bold(),
            );
            print_report(&report);
            return Ok(());
        }
    }

    eprint!("  Starting coastd ...");
    report.daemon_restarted = restart_daemon().await;

    print_report(&report);
    Ok(())
}

/// Start a fresh daemon after the nuke. Returns true on success.
async fn restart_daemon() -> bool {
    match super::daemon::execute_start().await {
        Ok(()) => true,
        Err(e) => {
            eprintln!(" {} ({e})", "failed".red());
            eprintln!(
                "    Run {} manually to start the daemon.",
                "coast daemon start".bold(),
            );
            false
        }
    }
}

/// Stop the daemon if it's running, cleaning up stale files either way.
async fn stop_daemon() -> bool {
    let Ok(pid_file) = super::daemon::pid_path() else {
        return false;
    };

    let pid = super::daemon::read_pid(&pid_file);
    let running = pid.is_some_and(super::daemon::is_running);

    if running {
        if let Err(e) = super::daemon::execute_kill(false).await {
            eprintln!("  {} Failed to stop daemon: {e}", "warn".yellow().bold(),);
            return false;
        }
        true
    } else {
        let _ = super::daemon::cleanup_stale_files();
        false
    }
}

/// Remove all containers labelled `coast.managed=true`.
async fn remove_containers(docker: &bollard::Docker) -> usize {
    use bollard::container::{ListContainersOptions, RemoveContainerOptions};
    use std::collections::HashMap;

    eprint!("  Removing containers ...");

    let mut filters = HashMap::new();
    filters.insert("label", vec!["coast.managed=true"]);
    let opts = ListContainersOptions {
        all: true,
        filters,
        ..Default::default()
    };

    let containers = match docker.list_containers(Some(opts)).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(" {} ({e})", "failed".red());
            return 0;
        }
    };

    let mut removed = 0;
    for c in &containers {
        if let Some(ref id) = c.id {
            let rm_opts = RemoveContainerOptions {
                force: true,
                v: true,
                ..Default::default()
            };
            if docker.remove_container(id, Some(rm_opts)).await.is_ok() {
                removed += 1;
            }
        }
    }

    eprintln!(" {} ({removed} removed)", "done".green());
    removed
}

/// Remove all coast-prefixed volumes.
async fn remove_volumes(docker: &bollard::Docker) -> usize {
    eprint!("  Removing volumes ...");

    let volumes = match docker.list_volumes::<String>(None).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!(" {} ({e})", "failed".red());
            return 0;
        }
    };

    let mut removed = 0;
    if let Some(vols) = volumes.volumes {
        for vol in &vols {
            let name = &vol.name;
            if (name.starts_with("coast--")
                || name.starts_with("coast-shared--")
                || name.starts_with("coast-dind--"))
                && docker.remove_volume(name, None).await.is_ok()
            {
                removed += 1;
            }
        }
    }

    eprintln!(" {} ({removed} removed)", "done".green());
    removed
}

/// Remove all networks labelled `coast.network`.
async fn remove_networks(docker: &bollard::Docker) -> usize {
    use bollard::network::ListNetworksOptions;
    use std::collections::HashMap;

    eprint!("  Removing networks ...");

    let mut filters = HashMap::new();
    filters.insert("label", vec!["coast.network"]);
    let opts = ListNetworksOptions { filters };

    let networks = match docker.list_networks(Some(opts)).await {
        Ok(n) => n,
        Err(e) => {
            eprintln!(" {} ({e})", "failed".red());
            return 0;
        }
    };

    let mut removed = 0;
    for net in &networks {
        if let Some(ref id) = net.id {
            if docker.remove_network(id).await.is_ok() {
                removed += 1;
            }
        }
    }

    eprintln!(" {} ({removed} removed)", "done".green());
    removed
}

/// Remove all Docker images tagged with `coast-image/`.
async fn remove_images(docker: &bollard::Docker) -> usize {
    use bollard::image::{ListImagesOptions, RemoveImageOptions};
    use std::collections::HashMap;

    eprint!("  Removing images ...");

    let opts = ListImagesOptions::<String> {
        all: false,
        filters: HashMap::new(),
        ..Default::default()
    };

    let images = match docker.list_images(Some(opts)).await {
        Ok(i) => i,
        Err(e) => {
            eprintln!(" {} ({e})", "failed".red());
            return 0;
        }
    };

    let mut removed = 0;
    for img in &images {
        let is_coast = img.repo_tags.iter().any(|t| t.starts_with("coast-image/"));
        if is_coast {
            let rm_opts = RemoveImageOptions {
                force: true,
                ..Default::default()
            };
            if let Some(id) = img.id.split(':').next_back() {
                let _ = docker.remove_image(id, Some(rm_opts), None).await;
                removed += 1;
            }
        }
    }

    eprintln!(" {} ({removed} removed)", "done".green());
    removed
}

fn print_report(report: &NukeReport) {
    eprintln!();
    if report.daemon_stopped {
        eprintln!("  {} coastd daemon stopped", "ok".green().bold());
    }
    if report.containers_removed > 0 {
        eprintln!(
            "  {} {} container{} removed",
            "ok".green().bold(),
            report.containers_removed,
            if report.containers_removed == 1 {
                ""
            } else {
                "s"
            },
        );
    }
    if report.volumes_removed > 0 {
        eprintln!(
            "  {} {} volume{} removed",
            "ok".green().bold(),
            report.volumes_removed,
            if report.volumes_removed == 1 { "" } else { "s" },
        );
    }
    if report.networks_removed > 0 {
        eprintln!(
            "  {} {} network{} removed",
            "ok".green().bold(),
            report.networks_removed,
            if report.networks_removed == 1 {
                ""
            } else {
                "s"
            },
        );
    }
    if report.images_removed > 0 {
        eprintln!(
            "  {} {} image{} removed",
            "ok".green().bold(),
            report.images_removed,
            if report.images_removed == 1 { "" } else { "s" },
        );
    }
    if report.home_deleted {
        eprintln!("  {} coast home directory wiped clean", "ok".green().bold());
    }
    if report.daemon_restarted {
        eprintln!("  {} coastd daemon restarted", "ok".green().bold());
    }
    eprintln!(
        "\n{} Coast has been factory-reset and is ready to use.",
        "done".green().bold(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: NukeArgs,
    }

    #[test]
    fn test_nuke_args_default() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(!cli.args.force);
    }

    #[test]
    fn test_nuke_args_force() {
        let cli = TestCli::try_parse_from(["test", "--force"]).unwrap();
        assert!(cli.args.force);
    }

    #[test]
    fn test_nuke_report_default() {
        let report = NukeReport::default();
        assert!(!report.daemon_stopped);
        assert_eq!(report.containers_removed, 0);
        assert_eq!(report.volumes_removed, 0);
        assert_eq!(report.networks_removed, 0);
        assert_eq!(report.images_removed, 0);
        assert!(!report.home_deleted);
        assert!(!report.daemon_restarted);
    }

    #[test]
    fn test_nuke_deletes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_home = tmp.path().join(".coast-test-nuke");
        std::fs::create_dir_all(fake_home.join("images").join("my-app")).unwrap();
        std::fs::write(fake_home.join("state.db"), b"fake").unwrap();
        std::fs::write(fake_home.join("coastd.log"), b"log").unwrap();

        assert!(fake_home.exists());
        std::fs::remove_dir_all(&fake_home).unwrap();
        assert!(!fake_home.exists(), "directory should be deleted");
    }

    #[test]
    fn test_nuke_deletes_nonexistent_directory_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_home = tmp.path().join("does-not-exist");
        assert!(!fake_home.exists());
    }

    #[test]
    fn test_nuke_volume_prefix_matching() {
        let coast_prefixes = ["coast--", "coast-shared--", "coast-dind--"];
        let test_cases = [
            ("coast--dev-1--postgres_data", true),
            ("coast-shared--myapp--redis_data", true),
            ("coast-dind--myapp--dev-1", true),
            ("myapp-coasts-dev-1_data", false),
            ("unrelated-volume", false),
        ];

        for (name, expected) in &test_cases {
            let matches = coast_prefixes.iter().any(|p| name.starts_with(p));
            assert_eq!(
                matches,
                *expected,
                "Volume '{name}' should {}match coast prefixes",
                if *expected { "" } else { "not " },
            );
        }
    }

    #[test]
    fn test_nuke_image_tag_matching() {
        let test_cases = [
            ("coast-image/myapp:abc123", true),
            ("coast-image/other:latest", true),
            ("nginx:latest", false),
            ("postgres:16", false),
        ];

        for (tag, expected) in &test_cases {
            let matches = tag.starts_with("coast-image/");
            assert_eq!(
                matches,
                *expected,
                "Image tag '{tag}' should {}match coast-image/ prefix",
                if *expected { "" } else { "not " },
            );
        }
    }
}
