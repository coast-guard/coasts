/// `coast ui` command — open the Coast dashboard in the default browser.
///
/// If run from within a known project directory, navigates directly to
/// that project's page. Detects the project by matching the cwd against
/// `project_root` paths from existing build manifests.
use anyhow::Result;
use clap::Args;
use colored::Colorize;

const DEFAULT_API_PORT: u16 = 31415;
const RESOLVER_PATH: &str = "/etc/resolver/localcoast";

/// Arguments for `coast ui`.
#[derive(Debug, Args)]
pub struct UiArgs {
    /// Override the port (default: 31415).
    #[arg(long)]
    port: Option<u16>,
}

/// Execute the `coast ui` command.
pub async fn execute(args: &UiArgs) -> Result<()> {
    let port = args.port.unwrap_or(DEFAULT_API_PORT);

    let host = if std::path::Path::new(RESOLVER_PATH).exists() {
        "localcoast"
    } else {
        "localhost"
    };

    let project = detect_project_from_cwd();

    let url = match project {
        Some(ref name) => format!("http://{host}:{port}/#/project/{name}"),
        None => format!("http://{host}:{port}"),
    };

    match &project {
        Some(name) => println!(
            "{} Opening {} (project: {})",
            "ok".green().bold(),
            url.bold(),
            name.bold(),
        ),
        None => println!("{} Opening {}", "ok".green().bold(), url.bold(),),
    }

    open_browser(&url)?;
    Ok(())
}

/// Scan ~/.coast/images/*/latest/manifest.json for project roots and find
/// which project (if any) the current working directory belongs to.
fn detect_project_from_cwd() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let home = dirs::home_dir()?;
    let images_dir = home.join(".coast").join("images");
    let entries = std::fs::read_dir(&images_dir).ok()?;

    let mut projects: Vec<(String, std::path::PathBuf)> = Vec::new();

    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let manifest_path = entry.path().join("latest").join("manifest.json");
        let content = std::fs::read_to_string(&manifest_path).ok();
        let root = content
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| v.get("project_root")?.as_str().map(ToString::to_string));

        if let Some(root) = root {
            projects.push((name, std::path::PathBuf::from(root)));
        }
    }

    // Sort by path length descending so deeper (more specific) roots match first
    projects.sort_by(|a, b| b.1.as_os_str().len().cmp(&a.1.as_os_str().len()));

    for (name, root) in &projects {
        if cwd.starts_with(root) {
            return Some(name.clone());
        }
    }
    None
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("Browser opening not supported on this platform. Visit: {url}");
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
        args: UiArgs,
    }

    #[test]
    fn test_ui_default_args() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(cli.args.port.is_none());
    }

    #[test]
    fn test_ui_custom_port() {
        let cli = TestCli::try_parse_from(["test", "--port", "8080"]).unwrap();
        assert_eq!(cli.args.port, Some(8080));
    }
}
