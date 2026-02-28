/// Extractor trait and registry for secret extraction.
///
/// This module defines the core abstraction for the Coast secret system:
///
/// - [`SecretValue`] — the value produced by an extractor (text or binary).
/// - [`Extractor`] — the trait that all secret extractors implement.
/// - [`ExtractorRegistry`] — a name-based lookup table mapping extractor names
///   to their implementations, used during `coast build` to resolve secrets.
///
/// Built-in extractors (file, env, command, macos-keychain, custom) are
/// registered automatically via [`ExtractorRegistry::with_builtins`]. Custom
/// extractors can be added at runtime via [`ExtractorRegistry::register`].
use std::collections::HashMap;

use tracing::debug;

use coast_core::error::{CoastError, Result};

/// A secret value returned by an extractor.
///
/// Secrets can be either UTF-8 text or arbitrary binary data. The injection
/// layer uses this distinction when deciding how to deliver the secret into
/// a coast container — binary secrets are always file-mounted, while text
/// secrets can be injected as either environment variables or files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretValue {
    /// A UTF-8 text secret (e.g., API keys, passwords, connection strings).
    Text(String),
    /// A binary secret (e.g., certificate files, keystore blobs).
    Binary(Vec<u8>),
}

impl SecretValue {
    /// Returns the secret value as a byte slice, regardless of variant.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            SecretValue::Text(s) => s.as_bytes(),
            SecretValue::Binary(b) => b,
        }
    }

    /// Returns the secret value as a string slice if it is a text secret.
    ///
    /// Returns `None` for binary secrets.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            SecretValue::Text(s) => Some(s),
            SecretValue::Binary(_) => None,
        }
    }

    /// Returns `true` if this is a text secret.
    pub fn is_text(&self) -> bool {
        matches!(self, SecretValue::Text(_))
    }

    /// Returns `true` if this is a binary secret.
    pub fn is_binary(&self) -> bool {
        matches!(self, SecretValue::Binary(_))
    }
}

/// Trait for secret extractors.
///
/// Each extractor is identified by a unique name and can extract a secret
/// from an external source given a set of parameters from the Coastfile.
///
/// # Implementing a custom extractor
///
/// ```rust,ignore
/// use coast_secrets::extractor::{Extractor, SecretValue};
/// use coast_core::error::{CoastError, Result};
/// use std::collections::HashMap;
///
/// struct VaultExtractor;
///
/// impl Extractor for VaultExtractor {
///     fn name(&self) -> &str { "vault" }
///
///     fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
///         let path = params.get("path").ok_or_else(|| {
///             CoastError::secret("vault extractor requires a 'path' parameter")
///         })?;
///         // ... fetch from Vault ...
///         Ok(SecretValue::Text("secret-from-vault".to_string()))
///     }
/// }
/// ```
pub trait Extractor: Send + Sync {
    /// Returns the unique name of this extractor (e.g., "file", "env", "macos-keychain").
    fn name(&self) -> &str;

    /// Extracts a secret value using the provided parameters.
    ///
    /// Parameters are key-value pairs from the Coastfile's `[secrets.<name>]` section
    /// (excluding reserved keys like `extractor`, `inject`, and `ttl`).
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Secret` with an actionable message if extraction
    /// fails (e.g., file not found, env var not set, command returned non-zero).
    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue>;
}

/// Registry mapping extractor names to their implementations.
///
/// Used during `coast build` to resolve the extractor referenced by each secret
/// definition in the Coastfile. The registry supports both built-in extractors
/// and user-registered custom extractors.
///
/// # Example
///
/// ```rust,ignore
/// use coast_secrets::extractor::ExtractorRegistry;
///
/// // Start with all built-in extractors
/// let mut registry = ExtractorRegistry::with_builtins();
///
/// // Look up and use an extractor
/// let extractor = registry.get("env")?;
/// let secret = extractor.extract(&params)?;
///
/// // Or use the convenience method
/// let secret = registry.extract("env", &params)?;
/// ```
pub struct ExtractorRegistry {
    extractors: HashMap<String, Box<dyn Extractor>>,
    aliases: HashMap<String, String>,
}

impl ExtractorRegistry {
    /// Creates a new empty registry with no extractors registered.
    pub fn new() -> Self {
        Self {
            extractors: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Resolves an alias to the canonical extractor name.
    ///
    /// If `name` is a registered alias, returns the canonical name it points to.
    /// Otherwise returns `name` unchanged.
    fn resolve_name<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases.get(name).map(String::as_str).unwrap_or(name)
    }

    /// Registers an alias for an extractor name.
    ///
    /// After calling `add_alias("keychain", "macos-keychain")`, lookups for
    /// `"keychain"` will resolve to the `"macos-keychain"` extractor.
    pub fn add_alias(&mut self, alias: &str, target: &str) {
        debug!(alias = %alias, target = %target, "registering extractor alias");
        self.aliases.insert(alias.to_string(), target.to_string());
    }

    /// Creates a registry pre-populated with all built-in extractors.
    ///
    /// Built-in extractors include:
    /// - `file` — reads a secret from a file on the host filesystem
    /// - `env` — reads a secret from a host environment variable
    /// - `command` — runs a shell command and captures stdout
    /// - `macos-keychain` — reads from macOS Keychain (available on all
    ///   platforms; returns a clear error on non-macOS)
    /// - `custom` — invokes a `coast-extractor-{name}` binary found on PATH
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        registry.register(Box::new(crate::extractors::file::FileExtractor));
        registry.register(Box::new(crate::extractors::env::EnvExtractor));
        registry.register(Box::new(crate::extractors::command::CommandExtractor));
        registry.register(Box::new(crate::extractors::keychain::KeychainExtractor));
        // "keychain" is a convenience alias for "macos-keychain"
        registry.add_alias("keychain", "macos-keychain");
        // Note: custom extractors are resolved dynamically by name
        // (coast-extractor-{name} on PATH) and are not static builtins.

        debug!(
            count = registry.len(),
            names = ?registry.available_names(),
            "initialized extractor registry with builtins"
        );

        registry
    }

    /// Registers an extractor in the registry.
    ///
    /// If an extractor with the same name already exists, it is replaced.
    /// This allows overriding built-in extractors with custom implementations.
    pub fn register(&mut self, extractor: Box<dyn Extractor>) {
        let name = extractor.name().to_string();
        debug!(extractor = %name, "registering extractor");
        self.extractors.insert(name, extractor);
    }

    /// Looks up an extractor by name, resolving aliases.
    ///
    /// Returns `None` if no extractor with the given name (or alias) is registered.
    /// For a version that returns an actionable error, use [`Self::get_or_err`].
    pub fn get(&self, name: &str) -> Option<&dyn Extractor> {
        let resolved = self.resolve_name(name);
        self.extractors
            .get(resolved)
            .map(std::convert::AsRef::as_ref)
    }

    /// Looks up an extractor by name, returning an actionable error if not found.
    ///
    /// The error message lists all available extractors and suggests installing
    /// a custom extractor binary.
    pub fn get_or_err(&self, name: &str) -> Result<&dyn Extractor> {
        self.get(name).ok_or_else(|| {
            CoastError::secret(format!(
                "Unknown extractor '{}'. Available extractors: [{}]. \
                 Check the extractor name in your Coastfile or install a custom extractor \
                 named 'coast-extractor-{}'.",
                name,
                self.available_names().join(", "),
                name
            ))
        })
    }

    /// Extracts a secret using the named extractor.
    ///
    /// First checks for a registered (built-in) extractor. If none is found,
    /// falls back to a custom extractor (`coast-extractor-{name}` on PATH).
    ///
    /// # Errors
    ///
    /// Returns `CoastError::Secret` if neither a built-in nor custom extractor
    /// can be found, or if the extraction itself fails.
    pub fn extract(
        &self,
        extractor_name: &str,
        params: &HashMap<String, String>,
    ) -> Result<SecretValue> {
        // Resolve aliases (e.g., "keychain" → "macos-keychain")
        let resolved = self.resolve_name(extractor_name);

        // Try built-in extractors first
        if let Some(extractor) = self
            .extractors
            .get(resolved)
            .map(std::convert::AsRef::as_ref)
        {
            return extractor.extract(params);
        }

        // Fall back to custom extractor (coast-extractor-{name} on PATH)
        debug!(
            extractor = %extractor_name,
            "no built-in extractor found, trying custom extractor on PATH"
        );
        let custom = crate::extractors::custom::CustomExtractor::new(extractor_name);
        custom.extract(params)
    }

    /// Returns the names of all registered extractors, sorted alphabetically.
    pub fn available_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.extractors.keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns the number of registered extractors.
    pub fn len(&self) -> usize {
        self.extractors.len()
    }

    /// Returns `true` if the registry has no registered extractors.
    pub fn is_empty(&self) -> bool {
        self.extractors.is_empty()
    }

    /// Returns `true` if an extractor with the given name (or alias) is registered.
    pub fn contains(&self, name: &str) -> bool {
        let resolved = self.resolve_name(name);
        self.extractors.contains_key(resolved)
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ExtractorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractorRegistry")
            .field("extractors", &self.available_names())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Test helpers: mock extractors
    // -----------------------------------------------------------------------

    /// A test extractor that always returns a fixed value.
    struct MockExtractor {
        mock_name: String,
        mock_value: SecretValue,
    }

    impl MockExtractor {
        fn new(name: &str, value: SecretValue) -> Self {
            Self {
                mock_name: name.to_string(),
                mock_value: value,
            }
        }

        fn text(name: &str, text: &str) -> Self {
            Self::new(name, SecretValue::Text(text.to_string()))
        }
    }

    impl Extractor for MockExtractor {
        fn name(&self) -> &str {
            &self.mock_name
        }

        fn extract(&self, _params: &HashMap<String, String>) -> Result<SecretValue> {
            Ok(self.mock_value.clone())
        }
    }

    /// A test extractor that always returns an error.
    struct FailingExtractor {
        mock_name: String,
        error_message: String,
    }

    impl FailingExtractor {
        fn new(name: &str, error_message: &str) -> Self {
            Self {
                mock_name: name.to_string(),
                error_message: error_message.to_string(),
            }
        }
    }

    impl Extractor for FailingExtractor {
        fn name(&self) -> &str {
            &self.mock_name
        }

        fn extract(&self, _params: &HashMap<String, String>) -> Result<SecretValue> {
            Err(CoastError::secret(&self.error_message))
        }
    }

    /// A test extractor that echoes back the "value" param.
    struct EchoExtractor;

    impl Extractor for EchoExtractor {
        fn name(&self) -> &str {
            "echo"
        }

        fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
            let value = params.get("value").ok_or_else(|| {
                CoastError::secret(
                    "echo extractor requires a 'value' parameter. \
                     Add `value = \"...\"` to your params.",
                )
            })?;
            Ok(SecretValue::Text(value.clone()))
        }
    }

    // -----------------------------------------------------------------------
    // SecretValue tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_secret_value_text_as_bytes() {
        let value = SecretValue::Text("hello".to_string());
        assert_eq!(value.as_bytes(), b"hello");
    }

    #[test]
    fn test_secret_value_binary_as_bytes() {
        let data = vec![0x00, 0xFF, 0x42];
        let value = SecretValue::Binary(data.clone());
        assert_eq!(value.as_bytes(), &data);
    }

    #[test]
    fn test_secret_value_text_as_text() {
        let value = SecretValue::Text("api-key-123".to_string());
        assert_eq!(value.as_text(), Some("api-key-123"));
    }

    #[test]
    fn test_secret_value_binary_as_text_returns_none() {
        let value = SecretValue::Binary(vec![0x00, 0xFF]);
        assert_eq!(value.as_text(), None);
    }

    #[test]
    fn test_secret_value_is_text() {
        assert!(SecretValue::Text("x".to_string()).is_text());
        assert!(!SecretValue::Binary(vec![]).is_text());
    }

    #[test]
    fn test_secret_value_is_binary() {
        assert!(SecretValue::Binary(vec![]).is_binary());
        assert!(!SecretValue::Text("x".to_string()).is_binary());
    }

    #[test]
    fn test_secret_value_equality() {
        let a = SecretValue::Text("same".to_string());
        let b = SecretValue::Text("same".to_string());
        let c = SecretValue::Text("different".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_secret_value_binary_equality() {
        let a = SecretValue::Binary(vec![1, 2, 3]);
        let b = SecretValue::Binary(vec![1, 2, 3]);
        let c = SecretValue::Binary(vec![4, 5, 6]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_secret_value_text_not_equal_to_binary() {
        let text = SecretValue::Text("hello".to_string());
        let binary = SecretValue::Binary(b"hello".to_vec());
        assert_ne!(text, binary);
    }

    #[test]
    fn test_secret_value_clone() {
        let original = SecretValue::Binary(vec![1, 2, 3]);
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_secret_value_debug_text() {
        let text = SecretValue::Text("key".to_string());
        let debug_str = format!("{:?}", text);
        assert!(debug_str.contains("Text"));
        assert!(debug_str.contains("key"));
    }

    #[test]
    fn test_secret_value_debug_binary() {
        let binary = SecretValue::Binary(vec![42]);
        let debug_str = format!("{:?}", binary);
        assert!(debug_str.contains("Binary"));
    }

    #[test]
    fn test_secret_value_empty_text() {
        let value = SecretValue::Text(String::new());
        assert_eq!(value.as_bytes(), b"");
        assert_eq!(value.as_text(), Some(""));
        assert!(value.is_text());
        assert!(!value.is_binary());
    }

    #[test]
    fn test_secret_value_empty_binary() {
        let value = SecretValue::Binary(vec![]);
        assert!(value.as_bytes().is_empty());
        assert_eq!(value.as_text(), None);
        assert!(value.is_binary());
        assert!(!value.is_text());
    }

    // -----------------------------------------------------------------------
    // ExtractorRegistry::new / default tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_new_registry_is_empty() {
        let registry = ExtractorRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_default_registry_is_empty() {
        let registry = ExtractorRegistry::default();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.available_names().is_empty());
    }

    // -----------------------------------------------------------------------
    // register and get tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_register_and_get() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("test", "secret")));

        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        let extractor = registry.get("test");
        assert!(extractor.is_some());
        assert_eq!(extractor.unwrap().name(), "test");
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let registry = ExtractorRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_register_overwrites_existing() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("test", "first")));
        registry.register(Box::new(MockExtractor::text("test", "second")));

        assert_eq!(registry.len(), 1);

        let result = registry.extract("test", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Text("second".to_string()));
    }

    #[test]
    fn test_register_multiple_extractors() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("alpha", "a")));
        registry.register(Box::new(MockExtractor::text("beta", "b")));
        registry.register(Box::new(MockExtractor::text("gamma", "c")));

        assert_eq!(registry.len(), 3);
        assert!(registry.contains("alpha"));
        assert!(registry.contains("beta"));
        assert!(registry.contains("gamma"));
        assert!(!registry.contains("delta"));
    }

    // -----------------------------------------------------------------------
    // contains tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_contains_registered_extractor() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("present", "v")));

        assert!(registry.contains("present"));
        assert!(!registry.contains("absent"));
    }

    // -----------------------------------------------------------------------
    // available_names tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_available_names_empty_registry() {
        let registry = ExtractorRegistry::new();
        assert!(registry.available_names().is_empty());
    }

    #[test]
    fn test_available_names_sorted_alphabetically() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("zulu", "z")));
        registry.register(Box::new(MockExtractor::text("alpha", "a")));
        registry.register(Box::new(MockExtractor::text("mike", "m")));

        let names = registry.available_names();
        assert_eq!(names, vec!["alpha", "mike", "zulu"]);
    }

    // -----------------------------------------------------------------------
    // get_or_err tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_or_err_found() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("env", "val")));

        let extractor = registry.get_or_err("env").unwrap();
        assert_eq!(extractor.name(), "env");
    }

    #[test]
    fn test_get_or_err_not_found_actionable_error() {
        let registry = ExtractorRegistry::new();
        let result = registry.get_or_err("vault");
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();

        assert!(
            msg.contains("Unknown extractor 'vault'"),
            "Error should name the unknown extractor, got: {msg}"
        );
        assert!(
            msg.contains("coast-extractor-vault"),
            "Error should suggest custom extractor binary, got: {msg}"
        );
        assert!(
            msg.contains("Coastfile"),
            "Error should reference the Coastfile, got: {msg}"
        );
    }

    #[test]
    fn test_get_or_err_lists_available_extractors() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("env", "v")));
        registry.register(Box::new(MockExtractor::text("file", "v")));

        let result = registry.get_or_err("nonexistent");
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();

        assert!(
            msg.contains("env") && msg.contains("file"),
            "Error should list available extractors, got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // extract convenience method tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_success() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("mock", "the-secret")));

        let result = registry.extract("mock", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Text("the-secret".to_string()));
    }

    #[test]
    fn test_extract_unknown_extractor_error() {
        let registry = ExtractorRegistry::new();
        let result = registry.extract("nonexistent", &HashMap::new());

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // Falls through to CustomExtractor which looks for coast-extractor-{name} on PATH
        assert!(
            msg.contains("not found on PATH"),
            "Should report custom extractor not found, got: {msg}"
        );
        assert!(
            msg.contains("coast-extractor-nonexistent"),
            "Should include the full executable name, got: {msg}"
        );
    }

    #[test]
    fn test_extract_with_failing_extractor() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(FailingExtractor::new(
            "broken",
            "something went wrong",
        )));

        let result = registry.extract("broken", &HashMap::new());
        assert!(result.is_err());

        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("something went wrong"),
            "Should propagate extractor error, got: {msg}"
        );
    }

    #[test]
    fn test_extract_passes_params_to_extractor() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(EchoExtractor));

        let mut params = HashMap::new();
        params.insert("value".to_string(), "my-secret-value".to_string());

        let result = registry.extract("echo", &params).unwrap();
        assert_eq!(result, SecretValue::Text("my-secret-value".to_string()));
    }

    #[test]
    fn test_extract_echo_missing_param_error() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(EchoExtractor));

        let result = registry.extract("echo", &HashMap::new());
        assert!(result.is_err());

        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("'value' parameter"),
            "Should describe the missing param, got: {msg}"
        );
    }

    #[test]
    fn test_extract_binary_value() {
        let mut registry = ExtractorRegistry::new();
        let binary_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        registry.register(Box::new(MockExtractor::new(
            "binary-source",
            SecretValue::Binary(binary_data.clone()),
        )));

        let result = registry.extract("binary-source", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Binary(binary_data));
        assert!(result.is_binary());
    }

    // -----------------------------------------------------------------------
    // with_builtins tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_with_builtins_has_file_extractor() {
        let registry = ExtractorRegistry::with_builtins();
        assert!(
            registry.contains("file"),
            "builtins should include 'file' extractor"
        );
    }

    #[test]
    fn test_with_builtins_has_env_extractor() {
        let registry = ExtractorRegistry::with_builtins();
        assert!(
            registry.contains("env"),
            "builtins should include 'env' extractor"
        );
    }

    #[test]
    fn test_with_builtins_has_command_extractor() {
        let registry = ExtractorRegistry::with_builtins();
        assert!(
            registry.contains("command"),
            "builtins should include 'command' extractor"
        );
    }

    #[test]
    fn test_with_builtins_is_not_empty() {
        let registry = ExtractorRegistry::with_builtins();
        assert!(!registry.is_empty());
        assert!(registry.len() >= 3);
    }

    #[test]
    fn test_with_builtins_extractor_names_match() {
        let registry = ExtractorRegistry::with_builtins();

        let file_ext = registry.get("file").unwrap();
        assert_eq!(file_ext.name(), "file");

        let env_ext = registry.get("env").unwrap();
        assert_eq!(env_ext.name(), "env");

        let cmd_ext = registry.get("command").unwrap();
        assert_eq!(cmd_ext.name(), "command");
    }

    #[test]
    fn test_with_builtins_can_register_additional() {
        let mut registry = ExtractorRegistry::with_builtins();
        let initial_count = registry.len();

        registry.register(Box::new(MockExtractor::text("vault", "from-vault")));

        assert_eq!(registry.len(), initial_count + 1);
        assert!(registry.contains("vault"));
        // Builtins still present
        assert!(registry.contains("file"));
        assert!(registry.contains("env"));
        assert!(registry.contains("command"));
    }

    #[test]
    fn test_with_builtins_can_override_builtin() {
        let mut registry = ExtractorRegistry::with_builtins();
        let initial_count = registry.len();

        // Override the "env" extractor with a custom implementation
        registry.register(Box::new(MockExtractor::text("env", "overridden")));

        // Count should remain the same (replaced, not added)
        assert_eq!(registry.len(), initial_count);

        let result = registry.extract("env", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Text("overridden".to_string()));
    }

    // -----------------------------------------------------------------------
    // Debug impl tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_debug_output_includes_names() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("alpha", "a")));
        registry.register(Box::new(MockExtractor::text("beta", "b")));

        let debug = format!("{:?}", registry);
        assert!(debug.contains("ExtractorRegistry"));
        assert!(debug.contains("alpha"));
        assert!(debug.contains("beta"));
    }

    #[test]
    fn test_debug_output_empty_registry() {
        let registry = ExtractorRegistry::new();
        let debug = format!("{:?}", registry);
        assert!(debug.contains("ExtractorRegistry"));
    }

    // -----------------------------------------------------------------------
    // Trait object safety and Send + Sync tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extractor_is_object_safe() {
        // Verifies Extractor can be used as a trait object.
        let extractors: Vec<Box<dyn Extractor>> = vec![
            Box::new(MockExtractor::text("a", "va")),
            Box::new(EchoExtractor),
            Box::new(FailingExtractor::new("f", "fail")),
        ];

        assert_eq!(extractors.len(), 3);
        assert_eq!(extractors[0].name(), "a");
        assert_eq!(extractors[1].name(), "echo");
        assert_eq!(extractors[2].name(), "f");
    }

    #[test]
    fn test_extractor_send_sync() {
        // Verify that Box<dyn Extractor> satisfies Send + Sync bounds.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Box<dyn Extractor>>();
    }

    // -----------------------------------------------------------------------
    // Edge case and error path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_empty_string_name() {
        let registry = ExtractorRegistry::new();
        assert!(registry.get("").is_none());
    }

    #[test]
    fn test_register_empty_string_name() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("", "val")));

        assert_eq!(registry.len(), 1);
        assert!(registry.contains(""));
        assert!(registry.get("").is_some());
    }

    #[test]
    fn test_extract_with_empty_params() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("simple", "no-params-needed")));

        let result = registry.extract("simple", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Text("no-params-needed".to_string()));
    }

    #[test]
    fn test_extract_with_many_params() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(EchoExtractor));

        let mut params = HashMap::new();
        params.insert("value".to_string(), "target".to_string());
        params.insert("extra1".to_string(), "ignored".to_string());
        params.insert("extra2".to_string(), "also-ignored".to_string());

        let result = registry.extract("echo", &params).unwrap();
        assert_eq!(result, SecretValue::Text("target".to_string()));
    }

    #[test]
    fn test_len_and_is_empty_consistency() {
        let mut registry = ExtractorRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        registry.register(Box::new(MockExtractor::text("one", "1")));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);

        registry.register(Box::new(MockExtractor::text("two", "2")));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 2);

        // Overwrite should not change count
        registry.register(Box::new(MockExtractor::text("one", "1-replaced")));
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn test_available_names_after_overwrite() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("alpha", "a")));
        registry.register(Box::new(MockExtractor::text("alpha", "a2")));

        let names = registry.available_names();
        assert_eq!(names, vec!["alpha"]);
    }

    #[test]
    fn test_extract_unknown_with_populated_registry_lists_available() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("file", "f")));
        registry.register(Box::new(MockExtractor::text("env", "e")));
        registry.register(Box::new(MockExtractor::text("command", "c")));

        let err = registry.extract("vault", &HashMap::new()).unwrap_err();
        let msg = err.to_string();

        // Falls through to CustomExtractor; error mentions the executable name
        assert!(
            msg.contains("coast-extractor-vault"),
            "Should include the executable name, got: {msg}"
        );
        assert!(
            msg.contains("not found on PATH"),
            "Should report not found on PATH, got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Alias tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_alias_resolves_to_canonical() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("macos-keychain", "kc-secret")));
        registry.add_alias("keychain", "macos-keychain");

        let result = registry.extract("keychain", &HashMap::new()).unwrap();
        assert_eq!(result, SecretValue::Text("kc-secret".to_string()));
    }

    #[test]
    fn test_alias_get_returns_extractor() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("macos-keychain", "kc")));
        registry.add_alias("keychain", "macos-keychain");

        let ext = registry.get("keychain");
        assert!(ext.is_some(), "alias should resolve via get()");
        assert_eq!(ext.unwrap().name(), "macos-keychain");
    }

    #[test]
    fn test_alias_contains_returns_true() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("macos-keychain", "kc")));
        registry.add_alias("keychain", "macos-keychain");

        assert!(registry.contains("keychain"));
        assert!(registry.contains("macos-keychain"));
    }

    #[test]
    fn test_alias_does_not_affect_len() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("macos-keychain", "kc")));
        registry.add_alias("keychain", "macos-keychain");

        assert_eq!(registry.len(), 1, "aliases should not increase len()");
    }

    #[test]
    fn test_alias_not_in_available_names() {
        let mut registry = ExtractorRegistry::new();
        registry.register(Box::new(MockExtractor::text("macos-keychain", "kc")));
        registry.add_alias("keychain", "macos-keychain");

        let names = registry.available_names();
        assert!(names.contains(&"macos-keychain".to_string()));
        assert!(
            !names.contains(&"keychain".to_string()),
            "aliases should not appear in available_names()"
        );
    }

    #[test]
    fn test_alias_to_nonexistent_falls_through_to_custom() {
        let mut registry = ExtractorRegistry::new();
        registry.add_alias("shortcut", "nonexistent");

        let err = registry.extract("shortcut", &HashMap::new()).unwrap_err();
        let msg = err.to_string();
        // Alias resolves to "nonexistent", which isn't registered, so it falls
        // through to custom extractor lookup for the ORIGINAL name "shortcut"
        assert!(
            msg.contains("not found on PATH"),
            "Should fall through to custom extractor, got: {msg}"
        );
    }

    #[test]
    fn test_with_builtins_has_keychain_alias() {
        let registry = ExtractorRegistry::with_builtins();

        assert!(
            registry.contains("keychain"),
            "builtins should include 'keychain' alias"
        );
        assert!(
            registry.contains("macos-keychain"),
            "builtins should include 'macos-keychain' extractor"
        );
    }
}
