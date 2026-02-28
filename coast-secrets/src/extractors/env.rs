/// Env extractor: reads a secret from a host environment variable.
///
/// # Coastfile usage
///
/// ```toml
/// [secrets.api_key]
/// extractor = "env"
/// var = "MY_API_KEY"
/// inject = "env:API_KEY"
/// ```
use crate::extractor::{Extractor, SecretValue};
use coast_core::error::{CoastError, Result};
use std::collections::HashMap;
use tracing::debug;

/// Reads a secret from a host environment variable.
pub struct EnvExtractor;

impl EnvExtractor {
    /// Creates a new `EnvExtractor`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for EnvExtractor {
    fn name(&self) -> &str {
        "env"
    }

    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
        let var_name = params.get("var").ok_or_else(|| {
            CoastError::secret(
                "Env extractor requires a 'var' parameter. \
                 Add `var = \"MY_ENV_VAR\"` to the secret definition in your Coastfile.",
            )
        })?;

        debug!(var = %var_name, "Reading secret from environment variable");

        let value = std::env::var(var_name).map_err(|_| {
            CoastError::secret(format!(
                "Environment variable '{}' is not set. \
                 Set it with `export {}=<value>` before running `coast build`.",
                var_name, var_name
            ))
        })?;

        Ok(SecretValue::Text(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use unique env var names per test to avoid races in parallel test execution.

    #[test]
    fn test_env_extractor_name() {
        let extractor = EnvExtractor::new();
        assert_eq!(extractor.name(), "env");
    }

    #[test]
    fn test_env_extractor_reads_var() {
        let var_name = "COAST_TEST_ENV_READS_VAR";
        unsafe {
            std::env::set_var(var_name, "my-secret-value");
        }

        let extractor = EnvExtractor::new();
        let mut params = HashMap::new();
        params.insert("var".to_string(), var_name.to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("my-secret-value".to_string()));

        // Clean up
        unsafe {
            std::env::remove_var(var_name);
        }
    }

    #[test]
    fn test_env_extractor_missing_var_param() {
        let extractor = EnvExtractor::new();
        let params = HashMap::new();

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("var"),
            "Error should mention missing 'var' param"
        );
        assert!(
            msg.contains("Coastfile"),
            "Error should mention Coastfile for guidance"
        );
    }

    #[test]
    fn test_env_extractor_unset_var() {
        let var_name = "COAST_TEST_DEFINITELY_NOT_SET_12345";
        // Make sure it's not set
        unsafe {
            std::env::remove_var(var_name);
        }

        let extractor = EnvExtractor::new();
        let mut params = HashMap::new();
        params.insert("var".to_string(), var_name.to_string());

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(var_name),
            "Error should include the variable name"
        );
        assert!(
            msg.contains("export"),
            "Error should suggest how to set the variable"
        );
    }

    #[test]
    fn test_env_extractor_empty_value() {
        let var_name = "COAST_TEST_ENV_EMPTY_VALUE";
        unsafe {
            std::env::set_var(var_name, "");
        }

        let extractor = EnvExtractor::new();
        let mut params = HashMap::new();
        params.insert("var".to_string(), var_name.to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text(String::new()));

        unsafe {
            std::env::remove_var(var_name);
        }
    }

    #[test]
    fn test_env_extractor_default() {
        let extractor = EnvExtractor::default();
        assert_eq!(extractor.name(), "env");
    }
}
