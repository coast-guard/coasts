/// `coast secret` command — manage per-instance secret overrides.
///
/// Provides `set` and `list` subcommands to manage secrets scoped to
/// a specific coast instance.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;

use coast_core::protocol::{Request, Response, SecretInfo, SecretRequest};

/// Arguments for `coast secret`.
#[derive(Debug, Args)]
pub struct SecretArgs {
    /// Name of the coast instance.
    pub instance: String,

    /// Secret subcommand.
    #[command(subcommand)]
    pub action: SecretAction,
}

/// Subcommands for `coast secret`.
#[derive(Debug, Subcommand)]
pub enum SecretAction {
    /// Set a per-instance secret override.
    Set {
        /// Secret name.
        name: String,
        /// Secret value.
        value: String,
    },
    /// List secrets for an instance.
    List,
}

/// Execute the `coast secret` command.
pub async fn execute(args: &SecretArgs, project: &str) -> Result<()> {
    let request = match &args.action {
        SecretAction::Set { name, value } => Request::Secret(SecretRequest::Set {
            instance: args.instance.clone(),
            project: project.to_string(),
            name: name.clone(),
            value: value.clone(),
        }),
        SecretAction::List => Request::Secret(SecretRequest::List {
            instance: args.instance.clone(),
            project: project.to_string(),
        }),
    };

    let response = super::send_request(request).await?;

    match response {
        Response::Secret(resp) => {
            if !resp.message.is_empty() {
                println!("{} {}", "ok".green().bold(), resp.message);
            }

            if !resp.secrets.is_empty() {
                println!("{}", format_secret_table(&resp.secrets));
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

/// Format a table of secrets for display.
pub fn format_secret_table(secrets: &[SecretInfo]) -> String {
    if secrets.is_empty() {
        return "  No secrets configured.".to_string();
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "  {:<25} {:<15} {:<25} {}",
        "NAME".bold(),
        "EXTRACTOR".bold(),
        "INJECT".bold(),
        "OVERRIDE".bold(),
    ));

    for secret in secrets {
        let override_marker = if secret.is_override {
            "yes".yellow().to_string()
        } else {
            "no".to_string()
        };

        lines.push(format!(
            "  {:<25} {:<15} {:<25} {}",
            secret.name, secret.extractor, secret.inject, override_marker,
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: SecretArgs,
    }

    #[test]
    fn test_secret_set_args() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "set", "API_KEY", "secret123"])
            .unwrap();
        assert_eq!(cli.args.instance, "feature-oauth");
        match cli.args.action {
            SecretAction::Set { name, value } => {
                assert_eq!(name, "API_KEY");
                assert_eq!(value, "secret123");
            }
            _ => panic!("Expected Set action"),
        }
    }

    #[test]
    fn test_secret_list_args() {
        let cli = TestCli::try_parse_from(["test", "feature-oauth", "list"]).unwrap();
        assert_eq!(cli.args.instance, "feature-oauth");
        assert!(matches!(cli.args.action, SecretAction::List));
    }

    #[test]
    fn test_secret_missing_instance() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_secret_set_missing_value() {
        let result = TestCli::try_parse_from(["test", "inst", "set", "NAME"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_format_secret_table_empty() {
        let output = format_secret_table(&[]);
        assert_eq!(output, "  No secrets configured.");
    }

    #[test]
    fn test_format_secret_table_with_secrets() {
        let secrets = vec![
            SecretInfo {
                name: "API_KEY".to_string(),
                extractor: "env".to_string(),
                inject: "env:API_KEY".to_string(),
                is_override: false,
            },
            SecretInfo {
                name: "DB_PASS".to_string(),
                extractor: "file".to_string(),
                inject: "file:/run/secrets/db".to_string(),
                is_override: true,
            },
        ];

        let output = format_secret_table(&secrets);
        assert!(output.contains("API_KEY"));
        assert!(output.contains("DB_PASS"));
        assert!(output.contains("env"));
        assert!(output.contains("file"));
        assert!(output.contains("NAME"));
        assert!(output.contains("EXTRACTOR"));
        assert!(output.contains("INJECT"));
        assert!(output.contains("OVERRIDE"));
    }
}
