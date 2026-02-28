/// `coast installation-prompt` — print the Coastfile installation prompt for AI coding agents.
///
/// This command is standalone and does not require the daemon to be running.
/// The prompt text is compiled into the binary via `include_str!()`.
use anyhow::Result;
use clap::Args;

const PROMPT: &str = include_str!("../../../docs/installation_prompt.txt");

/// Arguments for `coast installation-prompt`.
#[derive(Debug, Args)]
pub struct InstallationPromptArgs {}

/// Print the installation prompt to stdout.
pub async fn execute(_args: &InstallationPromptArgs) -> Result<()> {
    print!("{PROMPT}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: InstallationPromptArgs,
    }

    #[test]
    fn test_installation_prompt_args_parse() {
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        let _ = cli.args;
    }

    #[test]
    fn test_prompt_text_is_non_empty() {
        assert!(
            !PROMPT.is_empty(),
            "installation_prompt.txt should be compiled into the binary"
        );
    }

    #[test]
    fn test_prompt_text_contains_coastfile_schema() {
        assert!(PROMPT.contains("[coast]"));
        assert!(PROMPT.contains("[ports]"));
        assert!(PROMPT.contains("coast build"));
    }
}
