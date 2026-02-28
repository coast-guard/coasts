/// Built-in secret extractors and registry initialization.
///
/// This module re-exports all built-in extractor types and provides
/// [`default_registry`] to create an [`ExtractorRegistry`] pre-populated
/// with all built-in extractors.
pub mod command;
pub mod custom;
pub mod env;
pub mod file;
pub mod keychain;

pub use command::CommandExtractor;
pub use custom::CustomExtractor;
pub use env::EnvExtractor;
pub use file::FileExtractor;
pub use keychain::KeychainExtractor;

use crate::extractor::ExtractorRegistry;

/// Creates an [`ExtractorRegistry`] populated with all built-in extractors.
///
/// This is a convenience wrapper around [`ExtractorRegistry::with_builtins`].
/// The following extractors are registered:
/// - `"file"` — reads a file from the host filesystem
/// - `"env"` — reads a host environment variable
/// - `"command"` — runs a shell command and captures stdout
/// - `"macos-keychain"` — reads from the macOS Keychain (macOS only; returns
///   an error on other platforms)
///
/// Custom extractors (those using `coast-extractor-{name}` executables on PATH)
/// are not included here. They are resolved dynamically when a Coastfile
/// references an extractor name that is not in the registry.
pub fn default_registry() -> ExtractorRegistry {
    ExtractorRegistry::with_builtins()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_has_file_extractor() {
        let registry = default_registry();
        let extractor = registry.get("file").unwrap();
        assert_eq!(extractor.name(), "file");
    }

    #[test]
    fn test_default_registry_has_env_extractor() {
        let registry = default_registry();
        let extractor = registry.get("env").unwrap();
        assert_eq!(extractor.name(), "env");
    }

    #[test]
    fn test_default_registry_has_command_extractor() {
        let registry = default_registry();
        let extractor = registry.get("command").unwrap();
        assert_eq!(extractor.name(), "command");
    }

    #[test]
    fn test_default_registry_has_keychain_extractor() {
        let registry = default_registry();
        let extractor = registry.get("macos-keychain").unwrap();
        assert_eq!(extractor.name(), "macos-keychain");
    }

    #[test]
    fn test_default_registry_has_expected_count() {
        let registry = default_registry();
        let available = registry.available_names();
        assert_eq!(available.len(), 4);
    }

    #[test]
    fn test_default_registry_unknown_extractor_returns_none() {
        let registry = default_registry();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_default_registry_unknown_via_get_or_err() {
        let registry = default_registry();
        let result = registry.get_or_err("nonexistent");
        assert!(result.is_err());
        let msg = result.err().unwrap().to_string();
        assert!(msg.contains("nonexistent"), "Error should mention the name");
    }
}
