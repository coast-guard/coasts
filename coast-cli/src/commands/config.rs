/// `coast config` command — manage Coast configuration.
///
/// Currently supports setting the display language for the CLI and API.
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use rust_i18n::t;

use coast_core::protocol::{
    AnalyticsAction, Request, Response, SetAnalyticsRequest, SetLanguageRequest,
};

/// Arguments for `coast config`.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

/// Build the "after help" block that lists every supported language.
fn languages_help() -> String {
    let mut lines = String::from("Supported languages:\n");
    for (&code, &name) in coast_i18n::SUPPORTED_LANGUAGES
        .iter()
        .zip(coast_i18n::LANGUAGE_NAMES.iter())
    {
        lines.push_str(&format!("  {code:<4} {name}\n"));
    }
    lines
}

/// Subcommands for `coast config`.
#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Set the display language for Coast CLI and API.
    #[command(
        name = "set-language",
        after_long_help = "Supported languages:\n  en   English\n  zh   中文\n  ja   日本語\n  ko   한국어\n  ru   Русский\n  pt   Português\n  es   Español"
    )]
    SetLanguage {
        /// Language code.
        #[arg(
            value_parser = parse_locale,
            long_help = "Language code — one of: en, zh, ja, ko, ru, pt, es",
        )]
        locale: String,
    },
    /// Manage anonymous usage analytics.
    #[command(name = "analytics")]
    Analytics {
        /// Enable analytics.
        #[arg(long, group = "analytics_action")]
        enable: bool,
        /// Disable analytics.
        #[arg(long, group = "analytics_action")]
        disable: bool,
        /// Show current analytics status (default when no flags given).
        #[arg(long, group = "analytics_action")]
        status: bool,
    },
}

/// Clap value-parser that validates the locale and produces a friendly error listing the choices.
fn parse_locale(s: &str) -> Result<String, String> {
    if coast_i18n::is_valid_language(s) {
        Ok(s.to_string())
    } else {
        Err(format!(
            "unknown language '{}'\n\n{}\nExample: coast config set-language zh",
            s,
            languages_help(),
        ))
    }
}

/// Execute the `coast config` command.
pub async fn execute(args: &ConfigArgs) -> Result<()> {
    match &args.action {
        ConfigAction::SetLanguage { locale } => execute_set_language(locale).await,
        ConfigAction::Analytics {
            enable,
            disable,
            status: _,
        } => {
            let action = if *enable {
                AnalyticsAction::Enable
            } else if *disable {
                AnalyticsAction::Disable
            } else {
                AnalyticsAction::Status
            };
            execute_analytics(action).await
        }
    }
}

async fn execute_set_language(locale: &str) -> Result<()> {
    // clap's value_parser already validated the locale; this is a belt-and-suspenders check.
    if !coast_i18n::is_valid_language(locale) {
        bail!(
            "{}",
            t!("error.invalid_language", locale = locale, code = locale)
        );
    }

    let request = Request::SetLanguage(SetLanguageRequest {
        language: locale.to_string(),
    });

    let response = super::send_request(request).await?;

    match response {
        Response::SetLanguage(resp) => {
            rust_i18n::set_locale(&resp.language);
            let name = coast_i18n::language_name(&resp.language).unwrap_or(&resp.language);
            println!(
                "{} {}",
                "ok".green().bold(),
                t!(
                    "cli.ok.language_set",
                    code = &resp.language,
                    language = name
                ),
            );
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

async fn execute_analytics(action: AnalyticsAction) -> Result<()> {
    let is_status = matches!(action, AnalyticsAction::Status);

    let request = Request::SetAnalytics(SetAnalyticsRequest { action });
    let response = super::send_request(request).await?;

    match response {
        Response::SetAnalytics(resp) => {
            if is_status {
                let state = if resp.enabled {
                    "enabled".green().bold().to_string()
                } else {
                    "disabled".yellow().bold().to_string()
                };
                println!("Analytics: {state}");
            } else if resp.enabled {
                println!("{} Analytics enabled", "ok".green().bold());
            } else {
                println!("{} Analytics disabled", "ok".green().bold());
            }
            Ok(())
        }
        Response::Error(e) => bail!("{}", e.error),
        _ => bail!("unexpected response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: ConfigArgs,
    }

    #[test]
    fn test_config_set_language_parse() {
        let cli = TestCli::try_parse_from(["test", "set-language", "zh"]).unwrap();
        match cli.args.action {
            ConfigAction::SetLanguage { ref locale } => {
                assert_eq!(locale, "zh");
            }
            _ => panic!("expected SetLanguage"),
        }
    }

    #[test]
    fn test_config_set_language_parse_all_codes() {
        for code in coast_i18n::SUPPORTED_LANGUAGES {
            let cli = TestCli::try_parse_from(["test", "set-language", code]).unwrap();
            match cli.args.action {
                ConfigAction::SetLanguage { ref locale } => {
                    assert_eq!(locale, *code);
                }
                _ => panic!("expected SetLanguage"),
            }
        }
    }

    #[test]
    fn test_config_missing_subcommand() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_set_language_missing_locale() {
        let result = TestCli::try_parse_from(["test", "set-language"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_analytics_enable() {
        let cli = TestCli::try_parse_from(["test", "analytics", "--enable"]).unwrap();
        match cli.args.action {
            ConfigAction::Analytics {
                enable,
                disable,
                status,
            } => {
                assert!(enable);
                assert!(!disable);
                assert!(!status);
            }
            _ => panic!("expected Analytics"),
        }
    }

    #[test]
    fn test_config_analytics_disable() {
        let cli = TestCli::try_parse_from(["test", "analytics", "--disable"]).unwrap();
        match cli.args.action {
            ConfigAction::Analytics {
                enable,
                disable,
                status,
            } => {
                assert!(!enable);
                assert!(disable);
                assert!(!status);
            }
            _ => panic!("expected Analytics"),
        }
    }

    #[test]
    fn test_config_analytics_status() {
        let cli = TestCli::try_parse_from(["test", "analytics", "--status"]).unwrap();
        match cli.args.action {
            ConfigAction::Analytics {
                enable,
                disable,
                status,
            } => {
                assert!(!enable);
                assert!(!disable);
                assert!(status);
            }
            _ => panic!("expected Analytics"),
        }
    }

    #[test]
    fn test_config_analytics_no_flags_defaults_to_status() {
        let cli = TestCli::try_parse_from(["test", "analytics"]).unwrap();
        match cli.args.action {
            ConfigAction::Analytics {
                enable,
                disable,
                status,
            } => {
                // No flags → all false, which execute() treats as status
                assert!(!enable);
                assert!(!disable);
                assert!(!status);
            }
            _ => panic!("expected Analytics"),
        }
    }

    #[test]
    fn test_config_analytics_conflicting_flags() {
        let result = TestCli::try_parse_from(["test", "analytics", "--enable", "--disable"]);
        assert!(result.is_err());
    }
}
