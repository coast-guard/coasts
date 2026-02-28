/// `coast logs` command — stream logs from a coast instance.
///
/// Executes `docker compose logs` inside the coast container and
/// streams the output to stdout. Optionally filters by service name
/// and supports `--follow` for real-time tailing.
use anyhow::{bail, Result};
use clap::Args;
use std::io::Write;

use coast_core::protocol::{LogsRequest, Request, Response};

/// Arguments for `coast logs`.
#[derive(Debug, Args)]
pub struct LogsArgs {
    /// Name of the coast instance.
    pub name: String,

    /// Legacy positional compose service name to filter logs.
    #[arg(value_name = "SERVICE")]
    pub legacy_service: Option<String>,

    /// Compose service name to filter logs.
    #[arg(
        long = "service",
        value_name = "SERVICE",
        conflicts_with = "legacy_service"
    )]
    pub service: Option<String>,

    /// Tail logs and keep streaming new entries (tail -f behavior).
    /// Pass an optional number of lines (e.g. `--tail 50`), or use bare
    /// `--tail` to stream all available logs.
    #[arg(long, value_name = "LINES", num_args = 0..=1)]
    pub tail: Option<Option<u32>>,

    /// Follow log output (tail -f style).
    #[arg(short, long)]
    pub follow: bool,
}

/// Execute the `coast logs` command.
pub async fn execute(args: &LogsArgs, project: &str) -> Result<()> {
    let service = args.service.clone().or_else(|| args.legacy_service.clone());
    let (tail, tail_all, follow_from_tail) = resolve_tail_and_follow(args.tail);
    let follow = args.follow || follow_from_tail;
    let request = Request::Logs(LogsRequest {
        name: args.name.clone(),
        project: project.to_string(),
        service,
        tail,
        tail_all,
        follow,
    });

    let response = if follow {
        super::send_logs_request(request, |chunk| {
            print!("{chunk}");
            let _ = std::io::stdout().flush();
        })
        .await?
    } else {
        super::send_request(request).await?
    };

    match response {
        Response::Logs(resp) => {
            if !resp.output.is_empty() {
                print!("{}", resp.output);
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

fn resolve_tail_and_follow(tail_arg: Option<Option<u32>>) -> (Option<u32>, bool, bool) {
    match tail_arg {
        Some(Some(lines)) => (Some(lines), false, true),
        Some(None) => (None, true, true),
        None => (None, false, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: LogsArgs,
    }

    #[test]
    fn test_logs_args_name_only() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.legacy_service.is_none());
        assert!(cli.args.service.is_none());
        assert!(cli.args.tail.is_none());
        assert!(!cli.args.follow);
    }

    #[test]
    fn test_logs_args_with_legacy_positional_service() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "web"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert_eq!(cli.args.legacy_service, Some("web".to_string()));
        assert!(cli.args.service.is_none());
    }

    #[test]
    fn test_logs_args_with_service_flag() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "--service", "web"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.legacy_service.is_none());
        assert_eq!(cli.args.service, Some("web".to_string()));
    }

    #[test]
    fn test_logs_args_with_follow() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "--follow"]).unwrap();
        assert!(cli.args.follow);
    }

    #[test]
    fn test_logs_args_short_follow() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "-f"]).unwrap();
        assert!(cli.args.follow);
    }

    #[test]
    fn test_logs_args_with_tail() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "--tail", "50"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert_eq!(cli.args.tail, Some(Some(50)));
    }

    #[test]
    fn test_logs_args_with_bare_tail() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "--tail"]).unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert_eq!(cli.args.tail, Some(None));
    }

    #[test]
    fn test_logs_args_all_options() {
        let cli = TestCli::try_parse_from([
            "test",
            "feature-oauth",
            "--service",
            "web",
            "--tail",
            "50",
            "--follow",
        ])
        .unwrap();
        assert_eq!(cli.args.name, "feature-oauth");
        assert!(cli.args.legacy_service.is_none());
        assert_eq!(cli.args.service, Some("web".to_string()));
        assert_eq!(cli.args.tail, Some(Some(50)));
        assert!(cli.args.follow);
    }

    #[test]
    fn test_logs_args_missing_name() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_logs_args_conflict_flag_and_positional_service() {
        let result = TestCli::try_parse_from(["test", "feature-oauth", "web", "--service", "api"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_tail_and_follow_absent() {
        let (tail, tail_all, follow) = resolve_tail_and_follow(None);
        assert_eq!(tail, None);
        assert!(!tail_all);
        assert!(!follow);
    }

    #[test]
    fn test_resolve_tail_and_follow_lines() {
        let (tail, tail_all, follow) = resolve_tail_and_follow(Some(Some(42)));
        assert_eq!(tail, Some(42));
        assert!(!tail_all);
        assert!(follow);
    }

    #[test]
    fn test_resolve_tail_and_follow_bare_tail() {
        let (tail, tail_all, follow) = resolve_tail_and_follow(Some(None));
        assert_eq!(tail, None);
        assert!(tail_all);
        assert!(follow);
    }
}
