/// `coast sync` command — manage file synchronization with remote VMs.
///
/// Provides subcommands to create, terminate, pause, resume, flush, and status sync sessions.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{
    Request, Response, SyncCreateRequest, SyncFlushRequest, SyncPauseRequest, SyncRequest,
    SyncResumeRequest, SyncResponse, SyncSessionInfo, SyncStatusRequest, SyncTerminateRequest,
};

/// Arguments for `coast sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Sync subcommand.
    #[command(subcommand)]
    pub action: SyncAction,
}

/// Subcommands for `coast sync`.
#[derive(Debug, Subcommand)]
pub enum SyncAction {
    /// Show sync status for all sessions or a specific project.
    Status {
        /// Project name (optional, if not specified shows all).
        project: Option<String>,
    },
    /// Create a new sync session for a project.
    Create {
        /// Project name.
        project: String,
        /// Branch/worktree name.
        #[arg(long, short = 'b', default_value = "main")]
        branch: String,
        /// Remote name to sync to.
        #[arg(long, short = 'r')]
        remote: String,
        /// Local path to sync (optional, defaults to project directory).
        #[arg(long, short = 'p')]
        path: Option<String>,
    },
    /// Terminate a sync session for a project.
    Terminate {
        /// Project name.
        project: String,
    },
    /// Pause sync for a project.
    Pause {
        /// Project name.
        project: String,
    },
    /// Resume sync for a project.
    Resume {
        /// Project name.
        project: String,
    },
    /// Flush pending sync changes for a project.
    Flush {
        /// Project name.
        project: String,
    },
}

/// Execute the `coast sync` command.
pub async fn execute(args: &SyncArgs) -> Result<()> {
    let request = match &args.action {
        SyncAction::Status { project } => {
            Request::Sync(SyncRequest::Status(SyncStatusRequest {
                project: project.clone(),
            }))
        }
        SyncAction::Create {
            project,
            branch,
            remote,
            path,
        } => Request::Sync(SyncRequest::Create(SyncCreateRequest {
            project: project.clone(),
            branch: branch.clone(),
            remote_name: remote.clone(),
            local_path: path.clone(),
        })),
        SyncAction::Terminate { project } => {
            Request::Sync(SyncRequest::Terminate(SyncTerminateRequest {
                project: project.clone(),
            }))
        }
        SyncAction::Pause { project } => Request::Sync(SyncRequest::Pause(SyncPauseRequest {
            project: project.clone(),
        })),
        SyncAction::Resume { project } => {
            Request::Sync(SyncRequest::Resume(SyncResumeRequest {
                project: project.clone(),
            }))
        }
        SyncAction::Flush { project } => Request::Sync(SyncRequest::Flush(SyncFlushRequest {
            project: project.clone(),
        })),
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Sync(resp) => handle_sync_response(resp),
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("Unexpected response from daemon");
        }
    }
}

/// Handle a sync response and print appropriate output.
fn handle_sync_response(resp: SyncResponse) -> Result<()> {
    match resp {
        SyncResponse::Status(r) => {
            if r.sessions.is_empty() {
                println!("No sync sessions active.");
                println!();
                println!(
                    "Create a sync session with: {} sync create <project> --remote <remote>",
                    "coast".cyan()
                );
            } else {
                println!("{}", format_sessions_table(&r.sessions));
            }
        }
        SyncResponse::Create(r) => {
            if r.created {
                let session_info = r
                    .session_name
                    .map(|s| format!(" (session: {})", s))
                    .unwrap_or_default();
                println!(
                    "{} {}{}",
                    "✓".green().bold(),
                    r.message,
                    session_info.dimmed()
                );
            } else {
                bail!("{}", r.message);
            }
        }
        SyncResponse::Terminate(r) => {
            if r.terminated {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                bail!("{}", r.message);
            }
        }
        SyncResponse::Pause(r) => {
            if r.paused {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                bail!("{}", r.message);
            }
        }
        SyncResponse::Resume(r) => {
            if r.resumed {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                bail!("{}", r.message);
            }
        }
        SyncResponse::Flush(r) => {
            if r.flushed {
                println!("{} {}", "✓".green().bold(), r.message);
            } else {
                bail!("{}", r.message);
            }
        }
    }
    Ok(())
}

/// Format a table of sync sessions for display.
fn format_sessions_table(sessions: &[SyncSessionInfo]) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "  {:<20} {:<15} {:<15} {:<40}",
        "PROJECT".bold(),
        "REMOTE".bold(),
        "STATUS".bold(),
        "PATH".bold(),
    ));

    for session in sessions {
        let status_colored = match session.status.as_str() {
            "Watching for changes" | "watching" => "watching".green().to_string(),
            s if s.contains("Scanning") || s.contains("scanning") => {
                "scanning".yellow().to_string()
            }
            s if s.contains("Staging") || s.contains("staging") => "staging".yellow().to_string(),
            s if s.contains("Connecting") || s.contains("connecting") => {
                "connecting".yellow().to_string()
            }
            s if s.contains("Paused") || s.eq("Paused") => "paused".dimmed().to_string(),
            s if s.contains("error") || s.contains("Error") => s.red().to_string(),
            s => s.to_string(),
        };

        // Truncate path for display
        let path_display = if session.local_path.len() > 38 {
            format!("...{}", &session.local_path[session.local_path.len() - 35..])
        } else {
            session.local_path.clone()
        };

        lines.push(format!(
            "  {:<20} {:<15} {:<15} {:<40}",
            truncate_str(&session.project, 20),
            truncate_str(&session.remote_name, 15),
            status_colored,
            path_display,
        ));
    }

    lines.join("\n")
}

/// Truncate a string to a maximum length, adding ellipsis if needed.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: SyncArgs,
    }

    #[test]
    fn test_sync_status_args() {
        let cli = TestCli::try_parse_from(["test", "status"]).unwrap();
        match cli.args.action {
            SyncAction::Status { project } => {
                assert!(project.is_none());
            }
            _ => panic!("Expected Status action"),
        }
    }

    #[test]
    fn test_sync_status_with_project() {
        let cli = TestCli::try_parse_from(["test", "status", "my-project"]).unwrap();
        match cli.args.action {
            SyncAction::Status { project } => {
                assert_eq!(project, Some("my-project".to_string()));
            }
            _ => panic!("Expected Status action"),
        }
    }

    #[test]
    fn test_sync_create_args() {
        let cli =
            TestCli::try_parse_from(["test", "create", "my-project", "--remote", "dev-vm"]).unwrap();
        match cli.args.action {
            SyncAction::Create {
                project,
                branch,
                remote,
                path,
            } => {
                assert_eq!(project, "my-project");
                assert_eq!(branch, "main");
                assert_eq!(remote, "dev-vm");
                assert!(path.is_none());
            }
            _ => panic!("Expected Create action"),
        }
    }

    #[test]
    fn test_sync_create_with_all_options() {
        let cli = TestCli::try_parse_from([
            "test",
            "create",
            "my-project",
            "-r",
            "staging",
            "-b",
            "feature/auth",
            "-p",
            "/home/user/code/my-project",
        ])
        .unwrap();
        match cli.args.action {
            SyncAction::Create {
                project,
                branch,
                remote,
                path,
            } => {
                assert_eq!(project, "my-project");
                assert_eq!(branch, "feature/auth");
                assert_eq!(remote, "staging");
                assert_eq!(path, Some("/home/user/code/my-project".to_string()));
            }
            _ => panic!("Expected Create action"),
        }
    }

    #[test]
    fn test_sync_terminate_args() {
        let cli = TestCli::try_parse_from(["test", "terminate", "my-project"]).unwrap();
        match cli.args.action {
            SyncAction::Terminate { project } => {
                assert_eq!(project, "my-project");
            }
            _ => panic!("Expected Terminate action"),
        }
    }

    #[test]
    fn test_sync_pause_args() {
        let cli = TestCli::try_parse_from(["test", "pause", "my-project"]).unwrap();
        match cli.args.action {
            SyncAction::Pause { project } => {
                assert_eq!(project, "my-project");
            }
            _ => panic!("Expected Pause action"),
        }
    }

    #[test]
    fn test_sync_resume_args() {
        let cli = TestCli::try_parse_from(["test", "resume", "my-project"]).unwrap();
        match cli.args.action {
            SyncAction::Resume { project } => {
                assert_eq!(project, "my-project");
            }
            _ => panic!("Expected Resume action"),
        }
    }

    #[test]
    fn test_sync_flush_args() {
        let cli = TestCli::try_parse_from(["test", "flush", "my-project"]).unwrap();
        match cli.args.action {
            SyncAction::Flush { project } => {
                assert_eq!(project, "my-project");
            }
            _ => panic!("Expected Flush action"),
        }
    }

    #[test]
    fn test_truncate_str_no_truncation() {
        assert_eq!(truncate_str("short", 10), "short");
    }

    #[test]
    fn test_truncate_str_with_truncation() {
        assert_eq!(truncate_str("this-is-a-very-long-string", 15), "this-is-a-ve...");
    }

    #[test]
    fn test_format_sessions_table_empty() {
        let output = format_sessions_table(&[]);
        // Should only have header
        assert!(output.contains("PROJECT"));
        assert!(output.contains("REMOTE"));
        assert!(output.contains("STATUS"));
    }

    #[test]
    fn test_format_sessions_table_with_sessions() {
        let sessions = vec![
            SyncSessionInfo {
                project: "my-app".to_string(),
                remote_name: "dev-vm".to_string(),
                local_path: "/home/user/code/my-app".to_string(),
                remote_path: "~/coast-workspaces/my-app/main".to_string(),
                status: "Watching for changes".to_string(),
                last_sync_at: None,
            },
            SyncSessionInfo {
                project: "other-project".to_string(),
                remote_name: "staging".to_string(),
                local_path: "/home/user/code/other".to_string(),
                remote_path: "~/coast-workspaces/other/feature".to_string(),
                status: "Paused".to_string(),
                last_sync_at: Some("2024-01-15T10:30:00Z".to_string()),
            },
        ];

        let output = format_sessions_table(&sessions);
        assert!(output.contains("my-app"));
        assert!(output.contains("dev-vm"));
        assert!(output.contains("other-project"));
        assert!(output.contains("staging"));
    }
}
