/// File extractor: reads a secret from a file on the host filesystem.
///
/// Supports tilde (`~`) expansion via `shellexpand` so users can reference
/// files relative to their home directory (e.g., `~/.ssh/id_rsa`).
///
/// # Coastfile usage
///
/// ```toml
/// [secrets.my_cert]
/// extractor = "file"
/// path = "~/.config/my-app/cert.pem"
/// inject = "file:/run/secrets/cert.pem"
/// ```
use crate::extractor::{Extractor, SecretValue};
use coast_core::error::{CoastError, Result};
use std::collections::HashMap;
use tracing::debug;

/// Reads a secret from a file on the host filesystem.
pub struct FileExtractor;

impl FileExtractor {
    /// Creates a new `FileExtractor`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for FileExtractor {
    fn name(&self) -> &str {
        "file"
    }

    fn extract(&self, params: &HashMap<String, String>) -> Result<SecretValue> {
        let path = params.get("path").ok_or_else(|| {
            CoastError::secret(
                "File extractor requires a 'path' parameter. \
                 Add `path = \"/path/to/secret\"` to the secret definition in your Coastfile.",
            )
        })?;

        let expanded = shellexpand::tilde(path);
        let file_path = std::path::Path::new(expanded.as_ref());

        debug!(path = %file_path.display(), "Reading secret from file");

        let contents = std::fs::read_to_string(file_path).map_err(|e| {
            CoastError::secret(format!(
                "Failed to read secret file '{}': {}. \
                 Verify the file exists and is readable.",
                file_path.display(),
                e
            ))
        })?;

        Ok(SecretValue::Text(contents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_extractor_name() {
        let extractor = FileExtractor::new();
        assert_eq!(extractor.name(), "file");
    }

    #[test]
    fn test_file_extractor_reads_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "super-secret-value").unwrap();
        tmp.flush().unwrap();

        let extractor = FileExtractor::new();
        let mut params = HashMap::new();
        params.insert("path".to_string(), tmp.path().to_string_lossy().to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("super-secret-value".to_string()));
    }

    #[test]
    fn test_file_extractor_reads_multiline() {
        let mut tmp = NamedTempFile::new().unwrap();
        writeln!(tmp, "line1").unwrap();
        writeln!(tmp, "line2").unwrap();
        tmp.flush().unwrap();

        let extractor = FileExtractor::new();
        let mut params = HashMap::new();
        params.insert("path".to_string(), tmp.path().to_string_lossy().to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("line1\nline2\n".to_string()));
    }

    #[test]
    fn test_file_extractor_missing_path_param() {
        let extractor = FileExtractor::new();
        let params = HashMap::new();

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("path"),
            "Error should mention missing 'path' param"
        );
        assert!(
            msg.contains("Coastfile"),
            "Error should mention Coastfile for guidance"
        );
    }

    #[test]
    fn test_file_extractor_nonexistent_file() {
        let extractor = FileExtractor::new();
        let mut params = HashMap::new();
        params.insert(
            "path".to_string(),
            "/tmp/coast-test-nonexistent-file-12345".to_string(),
        );

        let err = extractor.extract(&params).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Failed to read"),
            "Error should describe the failure"
        );
        assert!(
            msg.contains("coast-test-nonexistent-file-12345"),
            "Error should include the file path"
        );
    }

    #[test]
    fn test_file_extractor_tilde_expansion() {
        // We can't easily test that ~ actually resolves correctly in a portable way,
        // but we can test that a path without ~ still works (no regression).
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "tilde-test").unwrap();
        tmp.flush().unwrap();

        let extractor = FileExtractor::new();
        let mut params = HashMap::new();
        // Use the absolute path (no tilde) -- shellexpand::tilde is a no-op here
        params.insert("path".to_string(), tmp.path().to_string_lossy().to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text("tilde-test".to_string()));
    }

    #[test]
    fn test_file_extractor_empty_file() {
        let tmp = NamedTempFile::new().unwrap();
        // File is created but empty

        let extractor = FileExtractor::new();
        let mut params = HashMap::new();
        params.insert("path".to_string(), tmp.path().to_string_lossy().to_string());

        let result = extractor.extract(&params).unwrap();
        assert_eq!(result, SecretValue::Text(String::new()));
    }

    #[test]
    fn test_file_extractor_default() {
        let extractor = FileExtractor::default();
        assert_eq!(extractor.name(), "file");
    }
}
