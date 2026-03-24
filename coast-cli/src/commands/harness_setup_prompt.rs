/// `coast harness-setup-prompt` — print harness-specific setup prompts for AI coding agents.
///
/// This command is standalone and does not require the daemon to be running.
/// The prompt texts are compiled into the binary via `include_str!()`.
///
/// Accepts one or more `--harness` flags. When called with multiple harnesses,
/// the output includes section headers and a preamble telling the agent which
/// steps to skip (CLI check and worktree_dir update are already handled by the
/// installation flow).
use anyhow::{bail, Result};
use clap::Args;

const CLAUDE_CODE: &str = include_str!("../../../docs/harnesses/claude_code_setup_prompt.txt");
const CODEX: &str = include_str!("../../../docs/harnesses/codex_setup_prompt.txt");
const CURSOR: &str = include_str!("../../../docs/harnesses/cursor_setup_prompt.txt");
const CONDUCTOR: &str = include_str!("../../../docs/harnesses/conductor_setup_prompt.txt");
const T3_CODE: &str = include_str!("../../../docs/harnesses/t3_code_setup_prompt.txt");
const SHEP: &str = include_str!("../../../docs/harnesses/shep_setup_prompt.txt");

/// Known harness names and their display labels.
const KNOWN_HARNESSES: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("codex", "OpenAI Codex"),
    ("cursor", "Cursor"),
    ("conductor", "Conductor"),
    ("t3-code", "T3 Code"),
    ("shep", "Shep"),
];

/// Arguments for `coast harness-setup-prompt`.
#[derive(Debug, Args)]
pub struct HarnessSetupPromptArgs {
    /// Harness(es) to print setup prompts for. Can be specified multiple times.
    /// Valid values: claude-code, codex, cursor, conductor, t3-code, shep
    #[arg(long = "harness", required = true)]
    harnesses: Vec<String>,
}

fn resolve_prompt(name: &str) -> Result<(&str, &str)> {
    match name {
        "claude-code" => Ok(("Claude Code", CLAUDE_CODE)),
        "codex" => Ok(("OpenAI Codex", CODEX)),
        "cursor" => Ok(("Cursor", CURSOR)),
        "conductor" => Ok(("Conductor", CONDUCTOR)),
        "t3-code" => Ok(("T3 Code", T3_CODE)),
        "shep" => Ok(("Shep", SHEP)),
        _ => {
            let valid: Vec<&str> = KNOWN_HARNESSES.iter().map(|(k, _)| *k).collect();
            bail!(
                "Unknown harness: {name}\nValid harnesses: {}",
                valid.join(", ")
            );
        }
    }
}

/// Print the harness setup prompt(s) to stdout.
pub async fn execute(args: &HarnessSetupPromptArgs) -> Result<()> {
    // Validate all harness names up front before printing anything.
    let resolved: Vec<(&str, &str)> = args
        .harnesses
        .iter()
        .map(|h| resolve_prompt(h))
        .collect::<Result<Vec<_>>>()?;

    let multiple = resolved.len() > 1;

    if multiple {
        print!(
            "\
=== HARNESS SKILLS SETUP ===

You are setting up Coast skills for multiple harnesses. Process each section
below in order, completing one harness before moving to the next.

For EVERY harness below, skip these steps (they are already done):
- Step 1 (Check for Coast CLI) — already verified.
- The \"Update the Coastfile\" / worktree_dir step — already configured.

"
        );
    }

    for (i, (label, prompt)) in resolved.iter().enumerate() {
        if multiple {
            print!("=== HARNESS {}: {} ===\n\n", i + 1, label);
        }
        print!("{prompt}");
        if !prompt.ends_with('\n') {
            println!();
        }
        if multiple && i + 1 < resolved.len() {
            println!();
        }
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
        args: HarnessSetupPromptArgs,
    }

    #[test]
    fn test_parse_single_harness() {
        let cli = TestCli::try_parse_from(["test", "--harness", "claude-code"]).unwrap();
        assert_eq!(cli.args.harnesses, vec!["claude-code"]);
    }

    #[test]
    fn test_parse_multiple_harnesses() {
        let cli = TestCli::try_parse_from([
            "test",
            "--harness",
            "claude-code",
            "--harness",
            "codex",
            "--harness",
            "cursor",
        ])
        .unwrap();
        assert_eq!(cli.args.harnesses, vec!["claude-code", "codex", "cursor"]);
    }

    #[test]
    fn test_requires_at_least_one_harness() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_all_known_harnesses() {
        for (key, expected_label) in KNOWN_HARNESSES {
            let (label, prompt) = resolve_prompt(key).unwrap();
            assert_eq!(label, *expected_label);
            assert!(!prompt.is_empty(), "prompt for {key} should not be empty");
        }
    }

    #[test]
    fn test_resolve_unknown_harness() {
        let result = resolve_prompt("vim");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown harness: vim"));
        assert!(err.contains("claude-code"));
    }

    #[test]
    fn test_prompts_contain_expected_content() {
        assert!(CLAUDE_CODE.contains("Claude Code"));
        assert!(CODEX.contains("Codex"));
        assert!(CURSOR.contains("Cursor"));
        assert!(CONDUCTOR.contains("Conductor"));
        assert!(T3_CODE.contains("T3 Code"));
        assert!(SHEP.contains("Shep"));
    }
}
