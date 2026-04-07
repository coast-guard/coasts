use std::path::Path;

use tracing::{info, warn};

use coast_core::coastfile::Coastfile;
use coast_core::types::InjectType;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SecretExtractionOutput {
    pub secrets_extracted: usize,
    pub warnings: Vec<String>,
}

/// Extract and store a single secret, returning true on success.
fn extract_single_secret(
    secret_config: &coast_core::types::SecretConfig,
    coastfile: &Coastfile,
    registry: &coast_secrets::extractor::ExtractorRegistry,
    keystore: &coast_secrets::keystore::Keystore,
    output: &mut SecretExtractionOutput,
) {
    let mut resolved_params = secret_config.params.clone();
    if let Some(path) = resolved_params.get("path") {
        let path = std::path::Path::new(path);
        if path.is_relative() {
            let abs = coastfile.project_root.join(path);
            resolved_params.insert("path".to_string(), abs.to_string_lossy().to_string());
        }
    }

    let inject_target = match &secret_config.inject {
        InjectType::Env(name) => name.clone(),
        InjectType::File(path) => path.display().to_string(),
    };

    let value = match registry.extract(&secret_config.extractor, &resolved_params) {
        Ok(v) => v,
        Err(error) => {
            warn!(
                secret = %secret_config.name,
                extractor = %secret_config.extractor,
                error = %error,
                "secret extraction failed, skipping (use `coast secret set` to provide manually)"
            );
            output.warnings.push(format!(
                "Failed to extract secret '{}' using extractor '{}': {error}",
                secret_config.name, secret_config.extractor
            ));
            return;
        }
    };

    let value_bytes = value.as_bytes().to_vec();
    let (inject_type_str, inject_target_str) = match &secret_config.inject {
        InjectType::Env(name) => ("env", name.as_str()),
        InjectType::File(path) => ("file", path.to_str().unwrap_or("")),
    };
    let ttl_seconds = secret_config.ttl.as_deref().and_then(parse_ttl_to_seconds);

    if let Err(error) = keystore.store_secret(&coast_secrets::keystore::StoreSecretParams {
        coast_image: &coastfile.name,
        secret_name: &secret_config.name,
        value: &value_bytes,
        inject_type: inject_type_str,
        inject_target: inject_target_str,
        extractor: &secret_config.extractor,
        ttl_seconds,
    }) {
        warn!(secret = %secret_config.name, error = %error, "failed to store secret");
        output.warnings.push(format!(
            "Failed to store secret '{}': {error}",
            secret_config.name
        ));
    } else {
        info!(
            secret = %secret_config.name,
            extractor = %secret_config.extractor,
            target = %inject_target,
            "secret extracted"
        );
        output.secrets_extracted += 1;
    }
}

pub fn extract_secrets(coastfile: &Coastfile, home: &Path) -> SecretExtractionOutput {
    if coastfile.secrets.is_empty() {
        return SecretExtractionOutput::default();
    }

    info!(count = coastfile.secrets.len(), "extracting secrets");

    let mut output = SecretExtractionOutput::default();
    let keystore_db_path = home.join("keystore.db");
    let keystore_key_path = home.join("keystore.key");

    let keystore =
        match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
            Ok(ks) => ks,
            Err(error) => {
                output.warnings.push(format!(
                    "Failed to open keystore: {error}. Secrets will not be stored."
                ));
                return output;
            }
        };

    if let Err(error) = keystore.delete_secrets_for_image(&coastfile.name) {
        output.warnings.push(format!(
            "Failed to clear old secrets for '{}': {error}",
            coastfile.name
        ));
    }

    let registry = coast_secrets::extractor::ExtractorRegistry::with_builtins();

    for secret_config in &coastfile.secrets {
        extract_single_secret(secret_config, coastfile, &registry, &keystore, &mut output);
    }

    output
}

fn parse_ttl_to_seconds(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix('s') {
        num.trim().parse::<i64>().ok()
    } else if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<i64>().ok().map(|n| n * 60)
    } else if let Some(num) = s.strip_suffix('h') {
        num.trim().parse::<i64>().ok().map(|n| n * 3600)
    } else if let Some(num) = s.strip_suffix('d') {
        num.trim().parse::<i64>().ok().map(|n| n * 86400)
    } else {
        s.parse::<i64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_secrets_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let toml = "[coast]\nname = \"test-proj\"\n";
        let cf = Coastfile::parse(toml, tmp.path()).unwrap();

        let result = extract_secrets(&cf, tmp.path());
        assert_eq!(result, SecretExtractionOutput::default());
        assert_eq!(result.secrets_extracted, 0);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_result_default_values() {
        let result = SecretExtractionOutput::default();
        assert_eq!(result.secrets_extracted, 0);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_extraction_with_file_secret() {
        let tmp = tempfile::tempdir().unwrap();
        let secret_file = tmp.path().join("secret.txt");
        std::fs::write(&secret_file, "my-secret-value").unwrap();

        let toml = r#"
[coast]
name = "test-proj"

[secrets.my_key]
extractor = "file"
inject = "env:MY_KEY"
path = "secret.txt"
"#;
        let cf = Coastfile::parse(toml, tmp.path()).unwrap();
        let result = extract_secrets(&cf, tmp.path());

        assert_eq!(result.secrets_extracted, 1);
        assert!(result.warnings.is_empty());

        let keystore = coast_secrets::keystore::Keystore::open(
            &tmp.path().join("keystore.db"),
            &tmp.path().join("keystore.key"),
        )
        .unwrap();
        let stored = keystore.get_secret("test-proj", "my_key").unwrap().unwrap();
        assert_eq!(stored.value, b"my-secret-value");
        assert_eq!(stored.inject_type, "env");
        assert_eq!(stored.inject_target, "MY_KEY");
    }

    #[test]
    fn test_extraction_failure_produces_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let toml = r#"
[coast]
name = "test-proj"

[secrets.missing]
extractor = "file"
inject = "env:MISSING_VAR"
path = "/nonexistent/coast-test-secret-xyz-123.txt"
"#;
        let cf = Coastfile::parse(toml, tmp.path()).unwrap();
        let result = extract_secrets(&cf, tmp.path());

        assert_eq!(result.secrets_extracted, 0);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("Failed to extract secret 'missing'"));
    }

    #[test]
    fn test_parse_ttl_to_seconds_values() {
        assert_eq!(parse_ttl_to_seconds(""), None);
        assert_eq!(parse_ttl_to_seconds("3600"), Some(3600));
        assert_eq!(parse_ttl_to_seconds("30s"), Some(30));
        assert_eq!(parse_ttl_to_seconds("2m"), Some(120));
        assert_eq!(parse_ttl_to_seconds("1h"), Some(3600));
        assert_eq!(parse_ttl_to_seconds("1d"), Some(86400));
        assert_eq!(parse_ttl_to_seconds("bad"), None);
    }
}
