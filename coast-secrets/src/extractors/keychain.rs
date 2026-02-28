/// macOS Keychain extractor: reads secrets from the macOS Keychain.
///
/// This extractor is gated on `#[cfg(target_os = "macos")]`. On non-macOS
/// platforms, it provides a stub that returns a clear error message.
///
/// # Coastfile usage (macOS only)
///
/// ```toml
/// [secrets.api_token]
/// extractor = "keychain"           # or "macos-keychain"
/// service = "com.example.myapp"
/// account = "api-token"            # optional, defaults to $USER
/// inject = "env:API_TOKEN"
/// ```
use crate::extractor::{Extractor, SecretValue};
use coast_core::error::{CoastError, Result};
use std::collections::HashMap;

/// macOS Keychain extractor (macOS implementation).
///
/// Uses the `security-framework` crate to read generic password items
/// from the user's default keychain.
#[cfg(target_os = "macos")]
pub struct KeychainExtractor;

#[cfg(target_os = "macos")]
impl KeychainExtractor {
    /// Creates a new `KeychainExtractor`.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl Default for KeychainExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl Extractor for KeychainExtractor {
    fn name(&self) -> &str {
        "macos-keychain"
    }

    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
        let service = params.get("service").ok_or_else(|| {
            CoastError::secret(
                "macos-keychain extractor requires a 'service' parameter. \
                 Add `service = \"com.example.myapp\"` to the secret definition in your Coastfile.",
            )
        })?;

        // Account is optional — defaults to the current macOS username ($USER).
        let default_account = std::env::var("USER").unwrap_or_default();
        let account = params
            .get("account")
            .map(String::as_str)
            .unwrap_or(&default_account);

        tracing::debug!(
            service = %service,
            account = %account,
            "Reading secret from macOS Keychain"
        );

        let password_bytes = security_framework::passwords::get_generic_password(service, account)
            .map_err(|e| {
                CoastError::secret(format!(
                    "Failed to read keychain item (service='{}', account='{}'): {}. \
                     Add the item with: security add-generic-password -s '{}' -a '{}' -w '<value>'",
                    service, account, e, service, account
                ))
            })?;

        let value = String::from_utf8(password_bytes).map_err(|e| {
            CoastError::secret(format!(
                "Keychain item (service='{}', account='{}') contains non-UTF-8 data: {}",
                service, account, e
            ))
        })?;

        Ok(SecretValue::Text(value))
    }
}

/// macOS Keychain extractor stub (non-macOS platforms).
///
/// Always returns an error indicating that this extractor is only
/// available on macOS.
#[cfg(not(target_os = "macos"))]
pub struct KeychainExtractor;

#[cfg(not(target_os = "macos"))]
impl KeychainExtractor {
    /// Creates a new `KeychainExtractor`.
    ///
    /// Note: On non-macOS platforms, this extractor will always return an error
    /// when `extract` is called.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(target_os = "macos"))]
impl Default for KeychainExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "macos"))]
impl Extractor for KeychainExtractor {
    fn name(&self) -> &str {
        "macos-keychain"
    }

    fn extract(&self, _params: &HashMap<String, String>) -> Result<SecretValue> {
        Err(CoastError::secret(
            "macos-keychain extractor is only available on macOS. \
             Use a different extractor (e.g., 'file', 'env', or 'command') \
             on this platform.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keychain_extractor_name() {
        let extractor = KeychainExtractor::new();
        assert_eq!(extractor.name(), "macos-keychain");
    }

    #[test]
    fn test_keychain_extractor_default() {
        let extractor = KeychainExtractor::default();
        assert_eq!(extractor.name(), "macos-keychain");
    }

    /// On non-macOS platforms, the extractor should always return an error.
    /// On macOS, this test exercises the error path for a missing keychain item.
    #[test]
    fn test_keychain_extractor_error_path() {
        let extractor = KeychainExtractor::new();
        let mut params = HashMap::new();
        params.insert(
            "service".to_string(),
            "com.coast.test.nonexistent".to_string(),
        );
        params.insert("account".to_string(), "nonexistent-account".to_string());

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();

        #[cfg(not(target_os = "macos"))]
        {
            assert!(
                msg.contains("only available on macOS"),
                "Error should say it's macOS-only: {msg}"
            );
        }

        #[cfg(target_os = "macos")]
        {
            assert!(
                msg.contains("Failed to read keychain item")
                    || msg.contains("only available on macOS"),
                "Error should describe the failure: {msg}"
            );
        }
    }

    /// On macOS, verify that missing params produce clear errors.
    #[cfg(target_os = "macos")]
    #[test]
    fn test_keychain_extractor_missing_service_param() {
        let extractor = KeychainExtractor::new();
        let mut params = HashMap::new();
        params.insert("account".to_string(), "test".to_string());

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("service"),
            "Error should mention missing 'service' param"
        );
    }

    /// On macOS, verify that missing account param defaults to $USER
    /// and attempts a keychain lookup (which will fail for our test service,
    /// but the error should mention the defaulted account, not a missing param).
    #[cfg(target_os = "macos")]
    #[test]
    fn test_keychain_extractor_missing_account_defaults_to_user() {
        let extractor = KeychainExtractor::new();
        let mut params = HashMap::new();
        params.insert(
            "service".to_string(),
            "com.coast.test.nonexistent".to_string(),
        );
        // No "account" param — should default to $USER

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        // Should fail with a keychain lookup error (not a missing param error)
        assert!(
            msg.contains("Failed to read keychain item"),
            "Error should be a keychain lookup failure, got: {msg}"
        );
        // The defaulted account ($USER) should appear in the error
        let user = std::env::var("USER").unwrap_or_default();
        if !user.is_empty() {
            assert!(
                msg.contains(&user),
                "Error should mention the defaulted account '{}', got: {msg}",
                user
            );
        }
    }

    /// On non-macOS, verify that even with valid-looking params, it errors.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn test_keychain_extractor_always_errors_on_non_macos() {
        let extractor = KeychainExtractor::new();
        let params = HashMap::new(); // No params needed -- should error before checking them

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("only available on macOS"),
            "Should reject on non-macOS regardless of params"
        );
    }
}
