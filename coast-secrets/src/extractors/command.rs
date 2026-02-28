/// Command extractor: runs a shell command and captures its stdout as a secret.
///
/// The command is executed via `sh -c` so shell features like pipes,
/// redirects, and variable expansion are available.
///
/// # Coastfile usage
///
/// ```toml
/// [secrets.db_password]
/// extractor = "command"
/// run = "op read 'op://vault/db/password'"
/// inject = "env:DATABASE_PASSWORD"
/// ```
use crate::extractor::{Extractor, SecretValue};
use coast_core::error::{CoastError, Result};
use std::collections::HashMap;
use tracing::debug;

/// Runs a shell command and captures stdout as a secret value.
pub struct CommandExtractor;

impl CommandExtractor {
    /// Creates a new `CommandExtractor`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CommandExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for CommandExtractor {
    fn name(&self) -> &str {
        "command"
    }

    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
        let command = params.get("run").ok_or_else(|| {
            CoastError::secret(
                "Command extractor requires a 'run' parameter. \
                 Add `run = \"your-command\"` to the secret definition in your Coastfile.",
            )
        })?;

        debug!(command = %command, "Running command to extract secret");

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .map_err(|e| {
                CoastError::secret(format!(
                    "Failed to execute command '{}': {}. \
                     Verify the command is valid and `sh` is available.",
                    command, e
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let exit_code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            return Err(CoastError::secret(format!(
                "Command '{}' failed with exit code {}: {}. \
                 Fix the command and re-run `coast build`.",
                command,
                exit_code,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8(output.stdout).map_err(|e| {
            CoastError::secret(format!(
                "Command '{}' produced non-UTF-8 output: {}. \
                 If the secret is binary, consider using a file extractor instead.",
                command, e
            ))
        })?;

        Ok(SecretValue::Text(stdout.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_extractor_name() {
        let extractor = CommandExtractor::new();
        assert_eq!(extractor.name(), "command");
    }

    #[test]
    fn test_command_extractor_echo() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert("run".to_string(), "echo hello".to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("hello".to_string()));
    }

    #[test]
    fn test_command_extractor_trims_output() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert("run".to_string(), "echo '  padded  '".to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("padded".to_string()));
    }

    #[test]
    fn test_command_extractor_multiline_output() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert("run".to_string(), "printf 'line1\\nline2'".to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("line1\nline2".to_string()));
    }

    #[test]
    fn test_command_extractor_missing_run_param() {
        let extractor = CommandExtractor::new();
        let params = HashMap::new();

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("run"),
            "Error should mention missing 'run' param"
        );
        assert!(
            msg.contains("Coastfile"),
            "Error should mention Coastfile for guidance"
        );
    }

    #[test]
    fn test_command_extractor_failing_command() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert("run".to_string(), "false".to_string());

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed"),
            "Error should describe the command failure"
        );
        assert!(msg.contains("exit code"), "Error should include exit code");
    }

    #[test]
    fn test_command_extractor_nonexistent_command() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert(
            "run".to_string(),
            "coast_nonexistent_command_12345".to_string(),
        );

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        // The command runs via sh -c, so sh itself succeeds but the inner command fails
        assert!(
            msg.contains("failed") || msg.contains("not found"),
            "Error should indicate failure: {msg}"
        );
    }

    #[test]
    fn test_command_extractor_shell_pipe() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert(
            "run".to_string(),
            "echo 'hello world' | tr 'h' 'H'".to_string(),
        );

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("Hello world".to_string()));
    }

    #[test]
    fn test_command_extractor_stderr_in_error() {
        let extractor = CommandExtractor::new();
        let mut params = HashMap::new();
        params.insert("run".to_string(), "echo 'oops' >&2 && exit 1".to_string());

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("oops"), "Error should include stderr output");
    }

    #[test]
    fn test_command_extractor_default() {
        let extractor = CommandExtractor::default();
        assert_eq!(extractor.name(), "command");
    }
}
