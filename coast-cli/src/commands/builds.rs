/// `coast builds` command — inspect build artifacts.
///
/// Provides subcommands to list, inspect, and explore coast build artifacts
/// including cached images, Docker images, secrets, and compose overrides.
/// Supports versioned builds with optional build_id arguments.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{
    BuildsContentResponse, BuildsDockerImagesResponse, BuildsImagesResponse, BuildsInspectResponse,
    BuildsLsResponse, BuildsRequest, BuildsResponse, DockerImageInfo, Request, Response,
};

/// Arguments for `coast builds`.
#[derive(Debug, Args)]
pub struct BuildsArgs {
    /// Builds subcommand.
    #[command(subcommand)]
    pub action: BuildsAction,
}

/// Subcommands for `coast builds`.
#[derive(Debug, Subcommand)]
pub enum BuildsAction {
    /// List all builds. If project is given, shows all versioned builds for it.
    Ls {
        /// Project name. If given, lists all builds for this project.
        project: Option<String>,
    },
    /// Show detailed build information.
    Inspect {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
    /// List cached image tarballs.
    Images {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
    /// List live Docker images on the host for a build.
    #[command(name = "docker-images")]
    DockerImages {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
    /// Inspect a specific Docker image on the host.
    #[command(name = "inspect-image")]
    InspectImage {
        /// Project name (auto-detected if omitted).
        #[arg(long)]
        project: Option<String>,
        /// Image reference or ID to inspect.
        image: String,
    },
    /// Show the rewritten compose.yml.
    Compose {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
    /// Show the raw manifest.json.
    Manifest {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
    /// Show the stored coastfile.toml.
    Coastfile {
        /// Project name (auto-detected if omitted).
        project: Option<String>,
        /// Build ID (defaults to latest).
        #[arg(long)]
        build_id: Option<String>,
    },
}

/// Execute the `coast builds` command.
pub async fn execute(args: &BuildsArgs, cli_project: &Option<String>) -> Result<()> {
    let request = match &args.action {
        BuildsAction::Ls { project } => {
            // If project given (via arg or --project), list all builds for it;
            // otherwise list latest build per project.
            let p = if project.is_some() {
                project.clone()
            } else {
                cli_project.clone()
            };
            Request::Builds(BuildsRequest::Ls { project: p })
        }
        BuildsAction::Inspect { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::Inspect {
                project: p,
                build_id: build_id.clone(),
            })
        }
        BuildsAction::Images { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::Images {
                project: p,
                build_id: build_id.clone(),
            })
        }
        BuildsAction::DockerImages { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::DockerImages {
                project: p,
                build_id: build_id.clone(),
            })
        }
        BuildsAction::InspectImage { project, image } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::InspectDockerImage {
                project: p,
                image: image.clone(),
            })
        }
        BuildsAction::Compose { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::Compose {
                project: p,
                build_id: build_id.clone(),
            })
        }
        BuildsAction::Manifest { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::Manifest {
                project: p,
                build_id: build_id.clone(),
            })
        }
        BuildsAction::Coastfile { project, build_id } => {
            let p = resolve(project, cli_project)?;
            Request::Builds(BuildsRequest::Coastfile {
                project: p,
                build_id: build_id.clone(),
            })
        }
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Builds(resp) => {
            display_response(&resp);
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("Unexpected response from daemon"),
    }
}

/// Resolve project name from subcommand arg, --project flag, or Coastfile.
fn resolve(sub_project: &Option<String>, cli_project: &Option<String>) -> Result<String> {
    if let Some(p) = sub_project {
        return Ok(p.clone());
    }
    if let Some(p) = cli_project {
        return Ok(p.clone());
    }
    let cwd = std::env::current_dir()?;
    let coastfile = cwd.join("Coastfile");
    if coastfile.exists() {
        let cf = coast_core::coastfile::Coastfile::from_file(&coastfile)?;
        return Ok(cf.name);
    }
    bail!(
        "Could not determine project name. Either:\n  \
         1. Pass the project name as an argument,\n  \
         2. Use --project <name>, or\n  \
         3. Run from a directory containing a Coastfile"
    )
}

fn display_response(resp: &BuildsResponse) {
    match resp {
        BuildsResponse::Ls(r) => display_ls(r),
        BuildsResponse::Inspect(r) => display_inspect(r),
        BuildsResponse::Images(r) => display_images(r),
        BuildsResponse::DockerImages(r) => display_docker_images(r),
        BuildsResponse::DockerImageInspect { data } => display_docker_image_inspect(data),
        BuildsResponse::Content(r) => display_content(r),
    }
}

fn display_ls(resp: &BuildsLsResponse) {
    if resp.builds.is_empty() {
        println!("No builds found.");
        println!("  Run `coast build` from a project directory to create one.");
        return;
    }

    // Detect if this is a per-project listing (all have build_ids) or cross-project
    let is_per_project = resp.builds.len() > 1
        && resp.builds.iter().all(|b| b.build_id.is_some())
        && resp.builds.windows(2).all(|w| w[0].project == w[1].project);

    if is_per_project {
        display_ls_per_project(resp);
    } else {
        display_ls_all_projects(resp);
    }
}

fn display_ls_all_projects(resp: &BuildsLsResponse) {
    let w_proj = resp
        .builds
        .iter()
        .map(|b| b.project.len())
        .max()
        .unwrap_or(0)
        .max(7);
    let w_bid = 10;
    let w_built = 13;
    let w_images = 10;
    let w_secrets = 7;
    let w_coast = resp
        .builds
        .iter()
        .map(|b| b.coast_image.as_deref().unwrap_or("(default dind)").len())
        .max()
        .unwrap_or(0)
        .max(11);
    let w_cache = 10;

    println!(
        "  {:<w_proj$}  {:<w_bid$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_coast$}  {:<w_cache$}  {}",
        "PROJECT".bold(),
        "BUILD".bold(),
        "BUILT".bold(),
        "IMAGES".bold(),
        "SECRETS".bold(),
        "COAST IMAGE".bold(),
        "CACHE".bold(),
        "INSTANCES".bold(),
    );

    for b in &resp.builds {
        let built = b
            .build_timestamp
            .as_deref()
            .map(relative_time)
            .unwrap_or_else(|| "unknown".to_string());

        let bid = b.build_id.as_deref().unwrap_or("—");
        let images = format!("{} + {}", b.images_cached, b.images_built);
        let coast_img = b.coast_image.as_deref().unwrap_or("(default dind)");
        let cache = format_bytes(b.cache_size_bytes);
        let instances = if b.running_count > 0 {
            format!("{} ({} running)", b.instance_count, b.running_count)
        } else {
            b.instance_count.to_string()
        };

        let project_str = if b.archived {
            format!("{} {}", b.project, "(archived)".dimmed())
        } else {
            b.project.clone()
        };

        println!(
            "  {:<w_proj$}  {:<w_bid$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_coast$}  {:<w_cache$}  {}",
            project_str,
            bid,
            built,
            images,
            b.secrets_count,
            coast_img,
            cache,
            instances,
        );
    }
}

fn display_ls_per_project(resp: &BuildsLsResponse) {
    let project = &resp.builds[0].project;
    println!("{} {}", "Builds for".bold(), project.bold());
    println!();

    let w_bid = 10;
    let w_built = 16;
    let w_images = 10;
    let w_secrets = 7;
    let w_cache = 10;

    let has_typed = resp.builds.iter().any(|b| b.coastfile_type.is_some());
    let w_type = 10;

    if has_typed {
        println!(
            "  {:<w_bid$}  {:<w_type$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_cache$}  {}",
            "BUILD ID".bold(),
            "TYPE".bold(),
            "BUILT".bold(),
            "IMAGES".bold(),
            "SECRETS".bold(),
            "CACHE".bold(),
            "".bold(),
        );
    } else {
        println!(
            "  {:<w_bid$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_cache$}  {}",
            "BUILD ID".bold(),
            "BUILT".bold(),
            "IMAGES".bold(),
            "SECRETS".bold(),
            "CACHE".bold(),
            "".bold(),
        );
    }

    for b in &resp.builds {
        let built = b
            .build_timestamp
            .as_deref()
            .map(relative_time)
            .unwrap_or_else(|| "unknown".to_string());

        let bid = b.build_id.as_deref().unwrap_or("—");
        let images = format!("{} + {}", b.images_cached, b.images_built);
        let cache = format_bytes(b.cache_size_bytes);
        let ct = b.coastfile_type.as_deref().unwrap_or("default");
        let marker = if b.is_latest {
            " (latest)".green().to_string()
        } else {
            String::new()
        };

        if has_typed {
            println!(
                "  {:<w_bid$}  {:<w_type$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_cache$}{}",
                bid, ct, built, images, b.secrets_count, cache, marker,
            );
        } else {
            println!(
                "  {:<w_bid$}  {:<w_built$}  {:<w_images$}  {:<w_secrets$}  {:<w_cache$}{}",
                bid, built, images, b.secrets_count, cache, marker,
            );
        }
    }
}

fn display_inspect(r: &BuildsInspectResponse) {
    println!("{} {}", "Build:".bold(), r.project.bold());
    if let Some(ref bid) = r.build_id {
        println!("  {:<18}{}", "Build ID:", bid);
    }
    {
        let ct = r.coastfile_type.as_deref().unwrap_or("default");
        println!("  {:<18}{}", "Type:", ct);
    }
    if let Some(ref root) = r.project_root {
        println!("  {:<18}{}", "Project Root:", root);
    }
    if let Some(ref ts) = r.build_timestamp {
        println!("  {:<18}{} ({})", "Built:", ts, relative_time(ts));
    }
    if let Some(ref hash) = r.coastfile_hash {
        println!("  {:<18}{}", "Coastfile Hash:", hash);
    }
    if let Some(ref img) = r.coast_image {
        println!("  {:<18}{}", "Coast Image:", img);
    }
    println!(
        "  {:<18}{} ({})",
        "Artifact:",
        r.artifact_path,
        format_bytes(r.artifact_size_bytes)
    );
    println!();

    println!(
        "{} {} cached + {} built ({} total cache)",
        "Images:".bold(),
        r.images_cached,
        r.images_built,
        format_bytes(r.cache_size_bytes)
    );

    if !r.secrets.is_empty() {
        println!("{} {}", "Secrets:".bold(), r.secrets.join(", "));
    } else {
        println!("{} (none)", "Secrets:".bold());
    }
    println!();

    if !r.omitted_services.is_empty() {
        println!(
            "{} {}",
            "Omitted Services:".bold(),
            r.omitted_services.join(", ")
        );
    }
    if !r.omitted_volumes.is_empty() {
        println!(
            "{} {}",
            "Omitted Volumes:".bold(),
            r.omitted_volumes.join(", ")
        );
    }
    if !r.omitted_services.is_empty() || !r.omitted_volumes.is_empty() {
        println!();
    }

    if !r.docker_images.is_empty() {
        println!("{} ({}):", "Docker Images".bold(), r.docker_images.len());
        print_docker_images_table(&r.docker_images, "  ");
        println!();
    }

    if !r.instances.is_empty() {
        println!("{} ({}):", "Instances".bold(), r.instances.len());
        let w_name = r
            .instances
            .iter()
            .map(|i| i.name.len())
            .max()
            .unwrap_or(0)
            .max(4);
        let w_status = 12;
        println!(
            "  {:<w_name$}  {:<w_status$}  {}",
            "NAME".bold(),
            "STATUS".bold(),
            "BRANCH".bold(),
        );
        for inst in &r.instances {
            let status = colorize_status(&inst.status);
            let branch = inst.branch.as_deref().unwrap_or("—");
            println!(
                "  {:<w_name$}  {:<w_status$}  {}",
                inst.name, status, branch,
            );
        }
    } else {
        println!("{} (none)", "Instances:".bold());
    }
}

fn display_images(r: &BuildsImagesResponse) {
    if r.images.is_empty() {
        println!("No cached images found.");
        return;
    }

    let w_type = 6;
    let w_ref = r
        .images
        .iter()
        .map(|i| i.reference.len())
        .max()
        .unwrap_or(0)
        .max(9);
    let w_size = 10;

    println!(
        "  {:<w_type$}  {:<w_ref$}  {:<w_size$}  {}",
        "TYPE".bold(),
        "REFERENCE".bold(),
        "SIZE".bold(),
        "CACHED".bold(),
    );

    for img in &r.images {
        let size = format_bytes(img.size_bytes);
        let cached = img
            .modified
            .as_deref()
            .map(relative_time)
            .unwrap_or_else(|| "—".to_string());
        let type_colored = match img.image_type.as_str() {
            "built" => "built".cyan().to_string(),
            "pulled" => "pulled".to_string(),
            "base" => "base".dimmed().to_string(),
            other => other.to_string(),
        };
        println!(
            "  {:<w_type$}  {:<w_ref$}  {:<w_size$}  {}",
            type_colored, img.reference, size, cached,
        );
    }

    println!();
    println!(
        "Total: {} images, {} cache",
        r.images.len(),
        format_bytes(r.total_size_bytes)
    );
}

fn display_docker_images(r: &BuildsDockerImagesResponse) {
    if r.images.is_empty() {
        println!("No Docker images found for this project.");
        return;
    }
    print_docker_images_table(&r.images, "");
}

fn print_docker_images_table(images: &[DockerImageInfo], prefix: &str) {
    let w_repo = images
        .iter()
        .map(|i| i.repository.len())
        .max()
        .unwrap_or(0)
        .max(10);
    let w_tag = images.iter().map(|i| i.tag.len()).max().unwrap_or(0).max(3);

    println!(
        "{prefix}{:<w_repo$}  {:<w_tag$}  {:<14}  {:<14}  {}",
        "REPOSITORY".bold(),
        "TAG".bold(),
        "IMAGE ID".bold(),
        "CREATED".bold(),
        "SIZE".bold(),
    );

    for img in images {
        let created = if img.created.is_empty() {
            "—".to_string()
        } else {
            relative_time(&img.created)
        };
        println!(
            "{prefix}{:<w_repo$}  {:<w_tag$}  {:<14}  {:<14}  {}",
            img.repository, img.tag, img.id, created, img.size,
        );
    }
}

fn display_docker_image_inspect(data: &serde_json::Value) {
    match serde_json::to_string_pretty(&data) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("Failed to format inspect data: {e}"),
    }
}

fn display_content(r: &BuildsContentResponse) {
    print!("{}", r.content);
}

fn colorize_status(status: &coast_core::types::InstanceStatus) -> String {
    use coast_core::types::InstanceStatus;
    match status {
        InstanceStatus::Enqueued => "enqueued".cyan().to_string(),
        InstanceStatus::Running => "running".green().to_string(),
        InstanceStatus::Stopped => "stopped".yellow().to_string(),
        InstanceStatus::CheckedOut => "checked_out".green().bold().to_string(),
        InstanceStatus::Provisioning => "provisioning".blue().to_string(),
        InstanceStatus::Assigning => "assigning".blue().to_string(),
        InstanceStatus::Unassigning => "unassigning".blue().to_string(),
        InstanceStatus::Starting => "starting".blue().to_string(),
        InstanceStatus::Stopping => "stopping".blue().to_string(),
        InstanceStatus::Idle => "idle".dimmed().to_string(),
    }
}

/// Format bytes into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1_073_741_824.0 {
        format!("{:.1} GB", b / 1_073_741_824.0)
    } else if b >= 1_048_576.0 {
        format!("{:.0} MB", b / 1_048_576.0)
    } else if b >= 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Convert an RFC3339 timestamp to a relative time string (e.g. "2 hours ago").
fn relative_time(ts: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return ts.to_string();
    };
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(dt);

    if duration.num_days() > 30 {
        format!("{} months ago", duration.num_days() / 30)
    } else if duration.num_days() > 0 {
        let d = duration.num_days();
        if d == 1 {
            "1 day ago".to_string()
        } else {
            format!("{d} days ago")
        }
    } else if duration.num_hours() > 0 {
        let h = duration.num_hours();
        if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{h} hours ago")
        }
    } else if duration.num_minutes() > 0 {
        let m = duration.num_minutes();
        if m == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{m} minutes ago")
        }
    } else {
        "just now".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use coast_core::protocol::query::InstanceSummary;
    use coast_core::protocol::{
        BuildSummary, BuildsContentResponse, BuildsDockerImagesResponse, BuildsImagesResponse,
        BuildsInspectResponse, BuildsLsResponse, CachedImageInfo, DockerImageInfo,
    };
    use coast_core::types::{InstanceStatus, RuntimeType};

    // ---- colorize_status ----

    #[test]
    fn test_colorize_status_all_variants() {
        let cases = [
            (InstanceStatus::Provisioning, "provisioning"),
            (InstanceStatus::Assigning, "assigning"),
            (InstanceStatus::Unassigning, "unassigning"),
            (InstanceStatus::Starting, "starting"),
            (InstanceStatus::Stopping, "stopping"),
            (InstanceStatus::Running, "running"),
            (InstanceStatus::Stopped, "stopped"),
            (InstanceStatus::CheckedOut, "checked_out"),
            (InstanceStatus::Idle, "idle"),
        ];
        for (status, expected_text) in &cases {
            let result = colorize_status(status);
            assert!(
                !result.is_empty(),
                "colorize_status({expected_text}) returned empty"
            );
            assert!(
                result.contains(expected_text),
                "colorize_status({expected_text}) = {result:?} does not contain {expected_text:?}"
            );
        }
    }

    // ---- relative_time ----

    fn ts_ago(dur: Duration) -> String {
        (Utc::now() - dur).to_rfc3339()
    }

    #[test]
    fn test_relative_time_just_now() {
        let ts = Utc::now().to_rfc3339();
        assert_eq!(relative_time(&ts), "just now");
    }

    #[test]
    fn test_relative_time_30_seconds_ago() {
        let ts = ts_ago(Duration::seconds(30));
        assert_eq!(relative_time(&ts), "just now");
    }

    #[test]
    fn test_relative_time_1_minute_ago() {
        let ts = ts_ago(Duration::minutes(1));
        assert_eq!(relative_time(&ts), "1 minute ago");
    }

    #[test]
    fn test_relative_time_minutes_ago() {
        let ts = ts_ago(Duration::minutes(45));
        assert_eq!(relative_time(&ts), "45 minutes ago");
    }

    #[test]
    fn test_relative_time_1_hour_ago() {
        let ts = ts_ago(Duration::hours(1));
        assert_eq!(relative_time(&ts), "1 hour ago");
    }

    #[test]
    fn test_relative_time_hours_ago() {
        let ts = ts_ago(Duration::hours(5));
        assert_eq!(relative_time(&ts), "5 hours ago");
    }

    #[test]
    fn test_relative_time_1_day_ago() {
        let ts = ts_ago(Duration::days(1));
        assert_eq!(relative_time(&ts), "1 day ago");
    }

    #[test]
    fn test_relative_time_days_ago() {
        let ts = ts_ago(Duration::days(15));
        assert_eq!(relative_time(&ts), "15 days ago");
    }

    #[test]
    fn test_relative_time_weeks_boundary() {
        let ts = ts_ago(Duration::days(7));
        assert_eq!(relative_time(&ts), "7 days ago");
    }

    #[test]
    fn test_relative_time_months_ago() {
        let ts = ts_ago(Duration::days(60));
        assert_eq!(relative_time(&ts), "2 months ago");
    }

    #[test]
    fn test_relative_time_years_as_months() {
        let ts = ts_ago(Duration::days(400));
        assert_eq!(relative_time(&ts), "13 months ago");
    }

    #[test]
    fn test_relative_time_invalid() {
        assert_eq!(relative_time("not-a-date"), "not-a-date");
    }

    #[test]
    fn test_relative_time_empty() {
        assert_eq!(relative_time(""), "");
    }

    // ---- format_bytes ----

    #[test]
    fn test_format_bytes_zero() {
        assert_eq!(format_bytes(0), "0 B");
    }

    #[test]
    fn test_format_bytes_small() {
        assert_eq!(format_bytes(500), "500 B");
    }

    #[test]
    fn test_format_bytes_one_kb() {
        assert_eq!(format_bytes(1024), "1 KB");
    }

    #[test]
    fn test_format_bytes_one_mb() {
        assert_eq!(format_bytes(1_048_576), "1 MB");
    }

    #[test]
    fn test_format_bytes_one_gb() {
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn test_format_bytes_one_tb() {
        assert_eq!(format_bytes(1_099_511_627_776), "1024.0 GB");
    }

    #[test]
    fn test_format_bytes_boundary_below_kb() {
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_boundary_below_mb() {
        assert_eq!(format_bytes(1_048_575), "1024 KB");
    }

    #[test]
    fn test_format_bytes_boundary_below_gb() {
        assert_eq!(format_bytes(1_073_741_823), "1024 MB");
    }

    // ---- helpers to build test data ----

    fn make_build_summary(project: &str, build_id: Option<&str>) -> BuildSummary {
        BuildSummary {
            project: project.to_string(),
            build_id: build_id.map(|s| s.to_string()),
            is_latest: build_id.is_none(),
            project_root: Some("/home/dev/proj".to_string()),
            build_timestamp: Some(Utc::now().to_rfc3339()),
            images_cached: 3,
            images_built: 1,
            secrets_count: 2,
            coast_image: None,
            cache_size_bytes: 52_428_800,
            instance_count: 2,
            running_count: 1,
            archived: false,
            instances_using: 0,
            coastfile_type: None,
        }
    }

    fn make_instance_summary(name: &str, status: InstanceStatus) -> InstanceSummary {
        InstanceSummary {
            name: name.to_string(),
            project: "test-proj".to_string(),
            status,
            branch: Some("main".to_string()),
            runtime: RuntimeType::Dind,
            checked_out: false,
            project_root: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            port_count: 0,
            primary_port_service: None,
            primary_port_canonical: None,
            primary_port_dynamic: None,
            primary_port_url: None,
            down_service_count: 0,
            remote_name: None,
        }
    }

    fn make_docker_image(repo: &str) -> DockerImageInfo {
        DockerImageInfo {
            id: "sha256:abc123".to_string(),
            repository: repo.to_string(),
            tag: "latest".to_string(),
            created: Utc::now().to_rfc3339(),
            size: "150 MB".to_string(),
            size_bytes: 157_286_400,
        }
    }

    fn make_cached_image(reference: &str) -> CachedImageInfo {
        CachedImageInfo {
            reference: reference.to_string(),
            filename: format!("{reference}.tar"),
            size_bytes: 50_000_000,
            image_type: "pulled".to_string(),
            modified: Some(Utc::now().to_rfc3339()),
        }
    }

    // ---- display_ls_all_projects (smoke) ----

    #[test]
    fn test_display_ls_all_projects_no_panic() {
        let resp = BuildsLsResponse {
            builds: vec![
                make_build_summary("project-alpha", None),
                make_build_summary("project-beta", None),
            ],
        };
        display_ls_all_projects(&resp);
    }

    #[test]
    fn test_display_ls_all_projects_empty() {
        let resp = BuildsLsResponse { builds: vec![] };
        display_ls_all_projects(&resp);
    }

    #[test]
    fn test_display_ls_all_projects_archived() {
        let mut b = make_build_summary("old-proj", None);
        b.archived = true;
        let resp = BuildsLsResponse { builds: vec![b] };
        display_ls_all_projects(&resp);
    }

    // ---- display_ls_per_project (smoke) ----

    #[test]
    fn test_display_ls_per_project_no_panic() {
        let resp = BuildsLsResponse {
            builds: vec![
                make_build_summary("my-app", Some("build-001")),
                make_build_summary("my-app", Some("build-002")),
            ],
        };
        display_ls_per_project(&resp);
    }

    #[test]
    fn test_display_ls_per_project_with_coastfile_type() {
        let mut b1 = make_build_summary("my-app", Some("build-001"));
        b1.coastfile_type = Some("light".to_string());
        b1.is_latest = false;
        let mut b2 = make_build_summary("my-app", Some("build-002"));
        b2.coastfile_type = Some("default".to_string());
        b2.is_latest = true;
        let resp = BuildsLsResponse {
            builds: vec![b1, b2],
        };
        display_ls_per_project(&resp);
    }

    // ---- display_inspect (smoke) ----

    #[test]
    fn test_display_inspect_no_panic() {
        let resp = BuildsInspectResponse {
            project: "test-proj".to_string(),
            build_id: Some("build-001".to_string()),
            project_root: Some("/home/dev/test-proj".to_string()),
            build_timestamp: Some(Utc::now().to_rfc3339()),
            coastfile_hash: Some("abc123def".to_string()),
            coast_image: Some("coast-custom:latest".to_string()),
            artifact_path: "/home/.coast/images/test-proj".to_string(),
            artifact_size_bytes: 104_857_600,
            images_cached: 4,
            images_built: 2,
            cache_size_bytes: 209_715_200,
            secrets: vec!["DB_PASSWORD".to_string(), "API_KEY".to_string()],
            built_services: vec!["web".to_string()],
            pulled_images: vec!["postgres:16".to_string()],
            base_images: vec!["node:20".to_string()],
            omitted_services: vec!["redis".to_string()],
            omitted_volumes: vec!["tmp-data".to_string()],
            mcp_servers: vec![],
            mcp_clients: vec![],
            shared_services: vec![],
            volumes: vec![],
            instances: vec![
                make_instance_summary("dev-1", InstanceStatus::Running),
                make_instance_summary("dev-2", InstanceStatus::Stopped),
            ],
            docker_images: vec![make_docker_image("test-proj-web")],
            coastfile_type: None,
        };
        display_inspect(&resp);
    }

    #[test]
    fn test_display_inspect_minimal() {
        let resp = BuildsInspectResponse {
            project: "minimal".to_string(),
            build_id: None,
            project_root: None,
            build_timestamp: None,
            coastfile_hash: None,
            coast_image: None,
            artifact_path: "/tmp/art".to_string(),
            artifact_size_bytes: 0,
            images_cached: 0,
            images_built: 0,
            cache_size_bytes: 0,
            secrets: vec![],
            built_services: vec![],
            pulled_images: vec![],
            base_images: vec![],
            omitted_services: vec![],
            omitted_volumes: vec![],
            mcp_servers: vec![],
            mcp_clients: vec![],
            shared_services: vec![],
            volumes: vec![],
            instances: vec![],
            docker_images: vec![],
            coastfile_type: None,
        };
        display_inspect(&resp);
    }

    // ---- display_images (smoke) ----

    #[test]
    fn test_display_images_no_panic() {
        let resp = BuildsImagesResponse {
            images: vec![
                make_cached_image("postgres:16"),
                make_cached_image("node:20"),
            ],
            total_size_bytes: 100_000_000,
        };
        display_images(&resp);
    }

    #[test]
    fn test_display_images_empty() {
        let resp = BuildsImagesResponse {
            images: vec![],
            total_size_bytes: 0,
        };
        display_images(&resp);
    }

    #[test]
    fn test_display_images_built_type() {
        let mut img = make_cached_image("myapp-web");
        img.image_type = "built".to_string();
        let resp = BuildsImagesResponse {
            images: vec![img],
            total_size_bytes: 50_000_000,
        };
        display_images(&resp);
    }

    // ---- display_docker_images (smoke) ----

    #[test]
    fn test_display_docker_images_no_panic() {
        let resp = BuildsDockerImagesResponse {
            images: vec![
                make_docker_image("myapp-web"),
                make_docker_image("myapp-worker"),
            ],
        };
        display_docker_images(&resp);
    }

    #[test]
    fn test_display_docker_images_empty() {
        let resp = BuildsDockerImagesResponse { images: vec![] };
        display_docker_images(&resp);
    }

    #[test]
    fn test_display_docker_images_empty_created() {
        let mut img = make_docker_image("test");
        img.created = String::new();
        let resp = BuildsDockerImagesResponse { images: vec![img] };
        display_docker_images(&resp);
    }

    // ---- display_content (smoke) ----

    #[test]
    fn test_display_content_no_panic() {
        let resp = BuildsContentResponse {
            content: "version: '3'\nservices:\n  web:\n    image: node:20\n".to_string(),
            file_type: "compose".to_string(),
        };
        display_content(&resp);
    }

    #[test]
    fn test_display_content_empty() {
        let resp = BuildsContentResponse {
            content: String::new(),
            file_type: "manifest".to_string(),
        };
        display_content(&resp);
    }
}
