/// `coast ls` command — list all coast instances.
///
/// Displays a table of all instances across projects (or filtered to one project)
/// with columns: NAME, PROJECT, STATUS, BRANCH, RUNTIME, CHECKED OUT.
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{InstanceSummary, LsRequest, Request, Response};
use coast_core::types::InstanceStatus;

/// Arguments for `coast ls`.
#[derive(Debug, Args)]
pub struct LsArgs {
    // No positional arguments; project filtering comes from the global --project flag
}

/// Execute the `coast ls` command.
///
/// The `project` parameter is `Option<&str>` because `ls` can list all projects
/// or be filtered to one.
pub async fn execute(args: &LsArgs, project: &Option<String>) -> Result<()> {
    let _ = args; // LsArgs has no fields, but we accept it for consistency

    let request = Request::Ls(LsRequest {
        project: project.clone(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Ls(resp) => {
            if resp.instances.is_empty() {
                println!("{}", t!("cli.info.no_instances_found"));
                if project.is_some() {
                    println!("  {}", t!("cli.info.run_hint"));
                } else {
                    println!("  {}", t!("cli.info.build_hint"));
                }
            } else {
                println!("{}", format_instance_table(&resp.instances));
            }
            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("{}", t!("error.unexpected_response"));
        }
    }
}

/// Format a table of instance summaries for display.
///
/// Column widths are computed dynamically from the data so that long branch
/// names, instance names, etc. never break alignment.
pub fn format_instance_table(instances: &[InstanceSummary]) -> String {
    if instances.is_empty() {
        return t!("cli.info.no_instances_found").to_string();
    }

    let projects: std::collections::HashSet<&str> =
        instances.iter().map(|i| i.project.as_str()).collect();
    let show_root = projects.len() > 1 || instances.iter().any(|i| i.project_root.is_some());

    let branches: Vec<&str> = instances
        .iter()
        .map(|i| i.branch.as_deref().unwrap_or("-"))
        .collect();
    let worktrees: Vec<&str> = instances
        .iter()
        .map(|i| i.worktree.as_deref().unwrap_or("-"))
        .collect();
    let roots: Vec<String> = instances
        .iter()
        .map(|i| {
            i.project_root
                .as_deref()
                .map(shorten_path)
                .unwrap_or_else(|| "-".to_string())
        })
        .collect();

    // Dynamic column widths: max(header_label_len, max_data_len)
    let w_name = instances
        .iter()
        .map(|i| i.name.len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let w_proj = instances
        .iter()
        .map(|i| i.project.len())
        .max()
        .unwrap_or(0)
        .max("PROJECT".len());
    let types: Vec<&str> = instances
        .iter()
        .map(|i| i.coastfile_type.as_deref().unwrap_or("default"))
        .collect();
    let has_non_default_type = types.iter().any(|t| *t != "default");
    let w_type = types
        .iter()
        .map(|t| t.len())
        .max()
        .unwrap_or(0)
        .max("TYPE".len());
    let w_stat = instances
        .iter()
        .map(|i| status_plain_len(&i.status))
        .max()
        .unwrap_or(0)
        .max("STATUS".len());
    let w_branch = branches
        .iter()
        .map(|b| b.len())
        .max()
        .unwrap_or(0)
        .max("BRANCH".len());
    let w_rt = instances
        .iter()
        .map(|i| i.runtime.as_str().len())
        .max()
        .unwrap_or(0)
        .max("RUNTIME".len());
    let w_wt = worktrees
        .iter()
        .map(|w| w.len())
        .max()
        .unwrap_or(0)
        .max("WORKTREE".len());
    let co_label = if show_root { "CO" } else { "CHECKED OUT" };
    let w_co = 1_usize.max(co_label.len());

    let sep = "  ";

    // Header
    let mut hdr: Vec<String> = vec![
        pad_colored(&"NAME".bold().to_string(), 4, w_name),
        pad_colored(&"PROJECT".bold().to_string(), 7, w_proj),
    ];
    if has_non_default_type {
        hdr.push(pad_colored(&"TYPE".bold().to_string(), 4, w_type));
    }
    hdr.extend([
        pad_colored(&"STATUS".bold().to_string(), 6, w_stat),
        pad_colored(&"BRANCH".bold().to_string(), 6, w_branch),
        pad_colored(&"RUNTIME".bold().to_string(), 7, w_rt),
        pad_colored(&"WORKTREE".bold().to_string(), 8, w_wt),
    ]);
    if show_root {
        hdr.push(pad_colored(&"CO".bold().to_string(), 2, w_co));
        hdr.push("ROOT".bold().to_string());
    } else {
        hdr.push(co_label.bold().to_string());
    }
    let mut lines = vec![hdr.join(sep)];

    // Data rows
    for (i, inst) in instances.iter().enumerate() {
        let status_colored = colorize_instance_status(&inst.status);
        let status_vis_len = status_plain_len(&inst.status);
        let checked = if inst.checked_out {
            pad_colored(&"*".green().bold().to_string(), 1, w_co)
        } else {
            " ".repeat(w_co)
        };

        let mut cols: Vec<String> =
            vec![pad_str(&inst.name, w_name), pad_str(&inst.project, w_proj)];
        if has_non_default_type {
            cols.push(pad_str(types[i], w_type));
        }
        cols.extend([
            pad_colored(&status_colored, status_vis_len, w_stat),
            pad_str(branches[i], w_branch),
            pad_str(inst.runtime.as_str(), w_rt),
            pad_str(worktrees[i], w_wt),
        ]);
        if show_root {
            cols.push(checked);
            cols.push(roots[i].clone());
        } else {
            cols.push(checked);
        }
        lines.push(cols.join(sep));
    }

    lines.join("\n")
}

/// Shorten a path by replacing the home directory with ~.
fn shorten_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Apply color to an instance status.
fn colorize_instance_status(status: &InstanceStatus) -> String {
    match status {
        InstanceStatus::Enqueued => "enqueued".cyan().to_string(),
        InstanceStatus::Provisioning => "provisioning".magenta().to_string(),
        InstanceStatus::Assigning => "assigning".magenta().to_string(),
        InstanceStatus::Unassigning => "unassigning".magenta().to_string(),
        InstanceStatus::Starting => "starting".blue().to_string(),
        InstanceStatus::Stopping => "stopping".blue().to_string(),
        InstanceStatus::Running => "running".green().to_string(),
        InstanceStatus::Stopped => "stopped".yellow().to_string(),
        InstanceStatus::CheckedOut => "checked_out".green().bold().to_string(),
        InstanceStatus::Idle => "idle".cyan().to_string(),
    }
}

/// Get the plain-text (no ANSI) length of an instance status.
fn status_plain_len(status: &InstanceStatus) -> usize {
    match status {
        InstanceStatus::Enqueued => "enqueued".len(),
        InstanceStatus::Provisioning => "provisioning".len(),
        InstanceStatus::Assigning => "assigning".len(),
        InstanceStatus::Unassigning => "unassigning".len(),
        InstanceStatus::Starting => "starting".len(),
        InstanceStatus::Stopping => "stopping".len(),
        InstanceStatus::Running => "running".len(),
        InstanceStatus::Stopped => "stopped".len(),
        InstanceStatus::CheckedOut => "checked_out".len(),
        InstanceStatus::Idle => "idle".len(),
    }
}

/// Pad a plain (no ANSI) string to a target width with trailing spaces.
fn pad_str(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - s.len()))
    }
}

/// Pad a colored string to a target visible width.
///
/// ANSI escape codes are invisible but count in `format!("{:<width}")`,
/// which breaks column alignment. This pads based on visible character count.
fn pad_colored(colored: &str, visible_len: usize, width: usize) -> String {
    if visible_len >= width {
        colored.to_string()
    } else {
        format!("{}{}", colored, " ".repeat(width - visible_len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use coast_core::types::RuntimeType;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: LsArgs,
    }

    #[test]
    fn test_ls_args_parse() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        // LsArgs has no fields to check, just verify it parses
        let _ = cli.args;
    }

    #[test]
    fn test_format_instance_table_empty() {
        let output = format_instance_table(&[]);
        assert_eq!(output, "No coast instances found.");
    }

    #[test]
    fn test_format_instance_table_single() {
        let instances = vec![InstanceSummary {
            name: "main".to_string(),
            project: "my-app".to_string(),
            status: InstanceStatus::CheckedOut,
            branch: Some("main".to_string()),
            runtime: RuntimeType::Dind,
            checked_out: true,
            project_root: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            port_count: 3,
            primary_port_service: None,
            primary_port_canonical: None,
            primary_port_dynamic: None,
            primary_port_url: None,
            down_service_count: 0,
            remote_name: None,
        }];

        let output = format_instance_table(&instances);
        assert!(output.contains("main"));
        assert!(output.contains("my-app"));
        assert!(output.contains("dind"));
        // Header check
        assert!(output.contains("NAME"));
        assert!(output.contains("PROJECT"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("BRANCH"));
        assert!(output.contains("RUNTIME"));
        assert!(output.contains("WORKTREE"));
        assert!(output.contains("CHECKED OUT"));
    }

    #[test]
    fn test_format_instance_table_multiple() {
        let instances = vec![
            InstanceSummary {
                name: "main".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::Running,
                branch: Some("main".to_string()),
                runtime: RuntimeType::Dind,
                checked_out: false,
                project_root: None,
                worktree: None,
                build_id: None,
                coastfile_type: None,
                port_count: 2,
                primary_port_service: None,
                primary_port_canonical: None,
                primary_port_dynamic: None,
                primary_port_url: None,
                down_service_count: 0,
                remote_name: None,
            },
            InstanceSummary {
                name: "feature-x".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::CheckedOut,
                branch: Some("feature/x".to_string()),
                runtime: RuntimeType::Dind,
                checked_out: true,
                project_root: None,
                worktree: Some("feature-x".to_string()),
                build_id: None,
                coastfile_type: None,
                port_count: 2,
                primary_port_service: None,
                primary_port_canonical: None,
                primary_port_dynamic: None,
                primary_port_url: None,
                down_service_count: 0,
                remote_name: None,
            },
            InstanceSummary {
                name: "snapshot-1".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::Stopped,
                branch: None,
                runtime: RuntimeType::Sysbox,
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
            },
        ];

        let output = format_instance_table(&instances);
        assert!(output.contains("main"));
        assert!(output.contains("feature-x"));
        assert!(output.contains("snapshot-1"));
        assert!(output.contains("sysbox"));
        // The None branch should show as "-"
        assert!(output.contains("-"));
    }

    #[test]
    fn test_format_instance_table_long_branch_aligns() {
        let instances = vec![
            InstanceSummary {
                name: "dev-1".to_string(),
                project: "app".to_string(),
                status: InstanceStatus::Running,
                branch: Some("testing-if-branches-change".to_string()),
                runtime: RuntimeType::Dind,
                checked_out: false,
                project_root: None,
                worktree: Some("dev-1".to_string()),
                build_id: None,
                coastfile_type: None,
                port_count: 3,
                primary_port_service: None,
                primary_port_canonical: None,
                primary_port_dynamic: None,
                primary_port_url: None,
                down_service_count: 0,
                remote_name: None,
            },
            InstanceSummary {
                name: "dev-2".to_string(),
                project: "app".to_string(),
                status: InstanceStatus::Running,
                branch: Some("main".to_string()),
                runtime: RuntimeType::Dind,
                checked_out: false,
                project_root: None,
                worktree: None,
                build_id: None,
                coastfile_type: None,
                port_count: 3,
                primary_port_service: None,
                primary_port_canonical: None,
                primary_port_dynamic: None,
                primary_port_url: None,
                down_service_count: 0,
                remote_name: None,
            },
        ];

        let output = format_instance_table(&instances);
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 3);

        // Strip ANSI codes to check visible alignment
        let strip = |s: &str| -> String {
            let mut out = String::new();
            let mut in_escape = false;
            for c in s.chars() {
                if c == '\x1b' {
                    in_escape = true;
                } else if in_escape {
                    if c.is_ascii_alphabetic() {
                        in_escape = false;
                    }
                } else {
                    out.push(c);
                }
            }
            out
        };

        let hdr = strip(lines[0]);
        let row1 = strip(lines[1]);
        let row2 = strip(lines[2]);

        // RUNTIME column start should be at the same position in all rows
        let rt_pos_hdr = hdr.find("RUNTIME").expect("header has RUNTIME");
        let rt_pos_r1 = row1.find("dind").expect("row1 has dind");
        let rt_pos_r2 = row2.find("dind").expect("row2 has dind");
        assert_eq!(
            rt_pos_hdr, rt_pos_r1,
            "RUNTIME column aligned between header and row1"
        );
        assert_eq!(rt_pos_r1, rt_pos_r2, "RUNTIME column aligned between rows");
    }

    #[test]
    fn test_colorize_instance_status_running() {
        let s = colorize_instance_status(&InstanceStatus::Running);
        assert!(s.contains("running"));
    }

    #[test]
    fn test_colorize_instance_status_stopped() {
        let s = colorize_instance_status(&InstanceStatus::Stopped);
        assert!(s.contains("stopped"));
    }

    #[test]
    fn test_colorize_instance_status_checked_out() {
        let s = colorize_instance_status(&InstanceStatus::CheckedOut);
        assert!(s.contains("checked_out"));
    }
}
