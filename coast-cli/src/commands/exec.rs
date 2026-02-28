/// `coast exec` command — execute a command inside a coast container.
///
/// Runs an arbitrary command inside the specified coast container.
/// When stdin is a TTY, spawns `docker exec -it` directly for full interactive
/// support. Otherwise, uses the daemon path to capture stdout/stderr as strings.
/// Defaults to `sh` if no command is given (Alpine-based coast containers may
/// not have `bash` installed).
use std::io::IsTerminal;

use anyhow::{bail, Result};
use clap::Args;

use coast_core::protocol::{ExecRequest, Request, Response};

/// Arguments for `coast exec`.
#[derive(Debug, Args)]
pub struct ExecArgs {
    /// Name of the coast instance.
    pub name: String,

    /// Command to run inside the coast container (default: sh).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// Build the container name from project and instance name.
pub fn container_name(project: &str, name: &str) -> String {
    format!("{}-coasts-{}", project, name)
}

/// Resolve host uid:gid for docker exec user mapping.
fn host_uid_gid() -> Option<String> {
    #[cfg(unix)]
    {
        let uid = unsafe { nix::libc::getuid() };
        let gid = unsafe { nix::libc::getgid() };
        Some(format!("{uid}:{gid}"))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Build arguments for interactive `docker exec`.
fn build_interactive_docker_exec_args(
    container: &str,
    command: &[String],
    user_spec: Option<&str>,
) -> Vec<String> {
    let mut args = vec!["exec".to_string(), "-it".to_string()];
    if let Some(user) = user_spec {
        args.push("-u".to_string());
        args.push(user.to_string());
    }
    args.push(container.to_string());
    args.extend(command.iter().cloned());
    args
}

/// Resolve the command to run, defaulting to sh.
pub fn resolve_command(command: &[String]) -> Vec<String> {
    if command.is_empty() {
        vec!["sh".to_string()]
    } else {
        command.to_vec()
    }
}

/// Execute the `coast exec` command.
pub async fn execute(args: &ExecArgs, project: &str) -> Result<()> {
    let command = resolve_command(&args.command);

    // Interactive mode: stdin is a TTY → spawn docker exec -it directly
    // for full TTY passthrough without going through the daemon.
    if std::io::stdin().is_terminal() {
        let container = container_name(project, &args.name);
        let user_spec = host_uid_gid();
        let docker_args =
            build_interactive_docker_exec_args(&container, &command, user_spec.as_deref());
        let mut cmd = std::process::Command::new("docker");
        cmd.args(&docker_args);
        let status = cmd.status()?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        return Ok(());
    }

    // Non-interactive: use daemon path (captures stdout/stderr as strings)
    let request = Request::Exec(ExecRequest {
        name: args.name.clone(),
        project: project.to_string(),
        command,
    });

    let response = super::send_request(request).await?;

    match response {
        Response::Exec(resp) => {
            if !resp.stdout.is_empty() {
                print!("{}", resp.stdout);
            }
            if !resp.stderr.is_empty() {
                eprint!("{}", resp.stderr);
            }

            if resp.exit_code != 0 {
                std::process::exit(resp.exit_code);
            }

            Ok(())
        }
        Response::Error(e) => {
            bail!("{}", e.error);
        }
        _ => {
            bail!("Unexpected response from daemon");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: ExecArgs,
    }

    #[test]
    fn test_exec_args_name_only() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.command.is_empty());
    }

    #[test]
    fn test_exec_args_with_command() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "ls", "-la"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert_eq!(cli.args.command, vec!["ls", "-la"]);
    }

    #[test]
    fn test_exec_args_with_complex_command() {
        let cli =
            TestCli::try_parse_from(["test", "my-instance", "docker", "compose", "ps"]).unwrap();
        assert_eq!(cli.args.command, vec!["docker", "compose", "ps"]);
    }

    #[test]
    fn test_exec_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_command_empty_defaults_to_sh() {
        let cmd = resolve_command(&[]);
        assert_eq!(cmd, vec!["sh"]);
    }

    #[test]
    fn test_resolve_command_provided() {
        let cmd = resolve_command(&["ls".to_string(), "-la".to_string()]);
        assert_eq!(cmd, vec!["ls", "-la"]);
    }

    #[test]
    fn test_container_name_construction() {
        assert_eq!(container_name("my-app", "main"), "my-app-coasts-main");
        assert_eq!(
            container_name("my-app", "feature-oauth"),
            "my-app-coasts-feature-oauth"
        );
    }

    #[test]
    fn test_build_interactive_docker_exec_args_with_user() {
        let args = build_interactive_docker_exec_args(
            "my-app-coasts-main",
            &["sh".to_string(), "-c".to_string(), "echo hi".to_string()],
            Some("501:20"),
        );
        assert_eq!(
            args,
            vec![
                "exec",
                "-it",
                "-u",
                "501:20",
                "my-app-coasts-main",
                "sh",
                "-c",
                "echo hi",
            ]
        );
    }

    #[test]
    fn test_build_interactive_docker_exec_args_without_user() {
        let args =
            build_interactive_docker_exec_args("my-app-coasts-main", &["sh".to_string()], None);
        assert_eq!(args, vec!["exec", "-it", "my-app-coasts-main", "sh"]);
    }
}
