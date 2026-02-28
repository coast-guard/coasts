/// Custom extractor: invokes a `coast-extractor-{name}` executable on PATH.
///
/// The executable receives the extractor parameters as a JSON object on stdin
/// and should write the secret value to stdout. A non-zero exit code is
/// treated as an error, and stderr is included in the error message.
///
/// # Coastfile usage
///
/// ```toml
/// [secrets.vault_token]
/// extractor = "vault"        # looks for coast-extractor-vault on PATH
/// path = "secret/data/token"
/// inject = "env:VAULT_TOKEN"
/// ```
///
/// # Custom extractor protocol
///
/// 1. Coast invokes `coast-extractor-{name}` with JSON on stdin
/// 2. The executable reads params, fetches the secret, writes it to stdout
/// 3. Exit code 0 = success, non-zero = error (stderr is captured)
use crate::extractor::{Extractor, SecretValue};
use coast_core::error::{CoastError, Result};
use std::collections::HashMap;
use std::io::Write;
use tracing::debug;

/// Invokes a custom extractor executable found on PATH.
pub struct CustomExtractor {
    /// The name suffix used to locate the `coast-extractor-{name}` binary.
    extractor_name: String,
}

impl CustomExtractor {
    /// Creates a new `CustomExtractor` for the given name.
    ///
    /// The extractor will look for an executable named `coast-extractor-{name}`
    /// on the system PATH.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            extractor_name: name.into(),
        }
    }

    /// Returns the full executable name that this extractor looks for.
    pub fn executable_name(&self) -> String {
        format!("coast-extractor-{}", self.extractor_name)
    }
}

impl Extractor for CustomExtractor {
    fn name(&self) -> &str {
        &self.extractor_name
    }

    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
        let exe_name = self.executable_name();

        debug!(executable = %exe_name, "Invoking custom extractor");

        let params_json = serde_json::to_string(params).map_err(|e| {
            CoastError::secret(format!(
                "Failed to serialize parameters for custom extractor '{}': {}",
                exe_name, e
            ))
        })?;

        // Find the executable on PATH
        let exe_path = which(&exe_name).ok_or_else(|| {
            CoastError::secret(format!(
                "Custom extractor executable '{}' not found on PATH. \
                 Install it or add its directory to your PATH. \
                 Custom extractors must be named 'coast-extractor-{{name}}' \
                 and be executable.",
                exe_name
            ))
        })?;

        let mut child = std::process::Command::new(&exe_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                CoastError::secret(format!(
                    "Failed to start custom extractor '{}': {}. \
                     Verify the executable is valid and has execute permissions.",
                    exe_name, e
                ))
            })?;

        // Write params JSON to stdin
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(params_json.as_bytes()).map_err(|e| {
                CoastError::secret(format!(
                    "Failed to write to stdin of custom extractor '{}': {}",
                    exe_name, e
                ))
            })?;
            // stdin is dropped here, closing the pipe
        }

        let output = child.wait_with_output().map_err(|e| {
            CoastError::secret(format!(
                "Failed to wait for custom extractor '{}': {}",
                exe_name, e
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
                "Custom extractor '{}' failed with exit code {}: {}. \
                 Check the extractor's documentation for required parameters.",
                exe_name,
                exit_code,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8(output.stdout).map_err(|e| {
            CoastError::secret(format!(
                "Custom extractor '{}' produced non-UTF-8 output: {}",
                exe_name, e
            ))
        })?;

        Ok(SecretValue::Text(stdout.trim().to_string()))
    }
}

/// Simple `which`-like lookup: searches PATH for an executable with the given name.
fn which(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            // Check if executable (on unix, check the execute bit)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&candidate) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Some(candidate);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Mutex to serialize tests that modify the PATH environment variable.
    /// Required because `std::env::set_var` is process-global and tests run
    /// in parallel by default.
    static PATH_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to create a mock extractor script in a temp directory.
    /// Returns the temp dir (must be kept alive for the duration of the test).
    #[cfg(unix)]
    fn create_mock_extractor(dir: &TempDir, name: &str, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let exe_path = dir.path().join(name);
        let mut file = std::fs::File::create(&exe_path).unwrap();
        file.write_all(script.as_bytes()).unwrap();
        file.flush().unwrap();

        let mut perms = std::fs::metadata(&exe_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&exe_path, perms).unwrap();

        exe_path
    }

    /// Prepend a directory to PATH for the duration of a test.
    /// Returns the original PATH value for restoration.
    /// MUST be called while holding PATH_MUTEX.
    fn prepend_path(dir: &std::path::Path) -> String {
        let original = std::env::var("PATH").unwrap_or_default();
        let new_path = format!("{}:{}", dir.display(), original);
        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        original
    }

    /// Restore the PATH to its original value.
    /// MUST be called while holding PATH_MUTEX.
    fn restore_path(original: &str) {
        unsafe {
            std::env::set_var("PATH", original);
        }
    }

    #[test]
    fn test_custom_extractor_name() {
        let extractor = CustomExtractor::new("vault");
        assert_eq!(extractor.name(), "vault");
    }

    #[test]
    fn test_custom_extractor_executable_name() {
        let extractor = CustomExtractor::new("vault");
        assert_eq!(extractor.executable_name(), "coast-extractor-vault");
    }

    #[test]
    fn test_custom_extractor_not_found() {
        let extractor = CustomExtractor::new("nonexistent-extractor-12345");
        let params = HashMap::new();

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not found on PATH"),
            "Error should say executable not found: {msg}"
        );
        assert!(
            msg.contains("coast-extractor-nonexistent-extractor-12345"),
            "Error should include the full executable name"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_custom_extractor_success() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        create_mock_extractor(
            &dir,
            "coast-extractor-test-ok",
            "#!/bin/sh\necho secret-from-custom\n",
        );

        let original_path = prepend_path(dir.path());
        let extractor = CustomExtractor::new("test-ok");
        let params = HashMap::new();

        let result = extractor.extract(&params);
        restore_path(&original_path);

        let value = result.unwrap();
        assert_eq!(value, SecretValue::Text("secret-from-custom".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_custom_extractor_receives_params_as_json() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        // Script reads stdin and echoes it back so we can verify the JSON
        create_mock_extractor(&dir, "coast-extractor-test-echo", "#!/bin/sh\ncat\n");

        let original_path = prepend_path(dir.path());
        let extractor = CustomExtractor::new("test-echo");
        let mut params = HashMap::new();
        params.insert("key".to_string(), "value".to_string());

        let result = extractor.extract(&params);
        restore_path(&original_path);

        let value = result.unwrap();
        // The output should be the JSON of our params
        let parsed: HashMap<String, String> = serde_json::from_str(&match value {
            SecretValue::Text(t) => t,
            _ => panic!("Expected Text"),
        })
        .unwrap();
        assert_eq!(parsed.get("key").unwrap(), "value");
    }

    #[cfg(unix)]
    #[test]
    fn test_custom_extractor_failure() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        create_mock_extractor(
            &dir,
            "coast-extractor-test-fail",
            "#!/bin/sh\ncat > /dev/null\necho 'something went wrong' >&2\nexit 1\n",
        );

        let original_path = prepend_path(dir.path());
        let extractor = CustomExtractor::new("test-fail");
        let params = HashMap::new();

        let result = extractor.extract(&params);
        restore_path(&original_path);

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("failed"),
            "Error should indicate failure: {msg}"
        );
        assert!(
            msg.contains("something went wrong"),
            "Error should include stderr: {msg}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_custom_extractor_trims_output() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        create_mock_extractor(
            &dir,
            "coast-extractor-test-trim",
            "#!/bin/sh\necho '  trimmed  '\n",
        );

        let original_path = prepend_path(dir.path());
        let extractor = CustomExtractor::new("test-trim");
        let params = HashMap::new();

        let result = extractor.extract(&params);
        restore_path(&original_path);

        let value = result.unwrap();
        assert_eq!(value, SecretValue::Text("trimmed".to_string()));
    }

    #[test]
    fn test_which_finds_nothing_for_nonexistent() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let result = which("coast-extractor-definitely-not-here-12345");
        assert!(result.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_which_finds_executable_in_path() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        create_mock_extractor(&dir, "coast-extractor-test-which", "#!/bin/sh\necho ok\n");

        let original_path = prepend_path(dir.path());
        let result = which("coast-extractor-test-which");
        restore_path(&original_path);

        assert!(result.is_some());
        assert!(result.unwrap().ends_with("coast-extractor-test-which"));
    }

    #[cfg(unix)]
    #[test]
    fn test_which_ignores_non_executable() {
        let _guard = PATH_MUTEX.lock().unwrap();
        let dir = TempDir::new().unwrap();
        // Create a file but don't make it executable
        let file_path = dir.path().join("coast-extractor-test-noexec");
        std::fs::write(&file_path, "#!/bin/sh\necho ok\n").unwrap();
        // Permissions default to 0o644, not executable

        let original_path = prepend_path(dir.path());
        let result = which("coast-extractor-test-noexec");
        restore_path(&original_path);

        assert!(
            result.is_none(),
            "which should not find non-executable files"
        );
    }
}
