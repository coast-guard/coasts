/// Secret system integration tests.
///
/// Tests the ExtractorRegistry with built-in extractors, the encrypted
/// keystore lifecycle, TTL expiry detection, and injection plan building.
///
/// These tests do NOT require Docker but some exercise the filesystem.
use std::collections::HashMap;
use std::path::Path;

use coast_secrets::extractor::{ExtractorRegistry, SecretValue};
use coast_secrets::inject::{self, FileMount, ResolvedSecret};
use coast_secrets::keystore::Keystore;

// ---------------------------------------------------------------------------
// ExtractorRegistry with built-in extractors
// ---------------------------------------------------------------------------

#[test]
fn test_registry_builtins_present() {
    let registry = ExtractorRegistry::with_builtins();

    assert!(registry.contains("file"), "builtins must include 'file'");
    assert!(registry.contains("env"), "builtins must include 'env'");
    assert!(
        registry.contains("command"),
        "builtins must include 'command'"
    );
    assert!(
        registry.contains("macos-keychain"),
        "builtins must include 'macos-keychain'"
    );
    assert!(registry.len() >= 4);
}

#[test]
fn test_registry_env_extractor() {
    let registry = ExtractorRegistry::with_builtins();

    // Set a test env var
    let key = "COAST_TEST_SECRET_INTEGRATION_VAR";
    unsafe {
        std::env::set_var(key, "test_secret_value_12345");
    }

    let mut params = HashMap::new();
    params.insert("var".to_string(), key.to_string());

    let result = registry.extract("env", &params).unwrap();
    assert_eq!(
        result,
        SecretValue::Text("test_secret_value_12345".to_string())
    );

    unsafe {
        std::env::remove_var(key);
    }
}

#[test]
fn test_registry_env_extractor_missing_var() {
    let registry = ExtractorRegistry::with_builtins();

    let mut params = HashMap::new();
    params.insert(
        "var".to_string(),
        "COAST_NONEXISTENT_VAR_XXXYYY_12345".to_string(),
    );

    let result = registry.extract("env", &params);
    assert!(result.is_err());
}

#[test]
fn test_registry_file_extractor() {
    let dir = tempfile::tempdir().unwrap();
    let secret_file = dir.path().join("secret.txt");
    std::fs::write(&secret_file, "file_secret_content").unwrap();

    let registry = ExtractorRegistry::with_builtins();

    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        secret_file.to_string_lossy().to_string(),
    );

    let result = registry.extract("file", &params).unwrap();
    assert_eq!(result, SecretValue::Text("file_secret_content".to_string()));
}

#[test]
fn test_registry_file_extractor_missing_file() {
    let registry = ExtractorRegistry::with_builtins();

    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        "/tmp/coast-nonexistent-secret-file-xyz.txt".to_string(),
    );

    let result = registry.extract("file", &params);
    assert!(result.is_err());
}

#[test]
fn test_registry_command_extractor() {
    let registry = ExtractorRegistry::with_builtins();

    let mut params = HashMap::new();
    params.insert("run".to_string(), "printf hello_from_command".to_string());

    let result = registry.extract("command", &params).unwrap();
    assert_eq!(result, SecretValue::Text("hello_from_command".to_string()));
}

#[test]
fn test_registry_command_extractor_failing_command() {
    let registry = ExtractorRegistry::with_builtins();

    let mut params = HashMap::new();
    params.insert("run".to_string(), "false".to_string());

    let result = registry.extract("command", &params);
    assert!(result.is_err());
}

#[test]
fn test_registry_unknown_extractor() {
    let registry = ExtractorRegistry::with_builtins();

    let result = registry.extract("nonexistent-extractor", &HashMap::new());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    // Falls through to CustomExtractor which looks for coast-extractor-{name} on PATH
    assert!(
        msg.contains("not found on PATH"),
        "Expected 'not found on PATH' in error, got: {msg}"
    );
    assert!(msg.contains("nonexistent-extractor"));
}

#[test]
fn test_registry_custom_extractor_override() {
    let mut registry = ExtractorRegistry::with_builtins();
    let initial_count = registry.len();

    // Add a custom extractor
    struct CustomExtractor;
    impl coast_secrets::extractor::Extractor for CustomExtractor {
        fn name(&self) -> &str {
            "custom-vault"
        }
        fn extract(
            &self,
            _params: &HashMap<String, String>,
        ) -> coast_core::error::Result<SecretValue> {
            Ok(SecretValue::Text("from-custom-vault".to_string()))
        }
    }

    registry.register(Box::new(CustomExtractor));
    assert_eq!(registry.len(), initial_count + 1);
    assert!(registry.contains("custom-vault"));

    let result = registry.extract("custom-vault", &HashMap::new()).unwrap();
    assert_eq!(result, SecretValue::Text("from-custom-vault".to_string()));

    // Builtins should still be present
    assert!(registry.contains("file"));
    assert!(registry.contains("env"));
    assert!(registry.contains("command"));
}

// ---------------------------------------------------------------------------
// Keystore integration tests (using temp files on disk)
// ---------------------------------------------------------------------------

#[test]
fn test_keystore_create_store_retrieve_delete() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();

    // Store secrets
    ks.store_secret(
        "my-app",
        "api_key",
        b"secret-api-key-123",
        "env",
        "API_KEY",
        "env",
        None,
    )
    .unwrap();

    ks.store_secret(
        "my-app",
        "gcp_creds",
        b"{\"type\": \"service_account\"}",
        "file",
        "/run/secrets/gcp.json",
        "file",
        None,
    )
    .unwrap();

    // Retrieve single
    let secret = ks.get_secret("my-app", "api_key").unwrap().unwrap();
    assert_eq!(secret.coast_image, "my-app");
    assert_eq!(secret.secret_name, "api_key");
    assert_eq!(secret.value, b"secret-api-key-123");
    assert_eq!(secret.inject_type, "env");
    assert_eq!(secret.inject_target, "API_KEY");
    assert_eq!(secret.extractor, "env");
    assert!(secret.ttl_seconds.is_none());

    // Retrieve all
    let all = ks.get_all_secrets("my-app").unwrap();
    assert_eq!(all.len(), 2);

    // Get non-existent
    let missing = ks.get_secret("my-app", "nonexistent").unwrap();
    assert!(missing.is_none());

    // Delete
    let count = ks.delete_secrets_for_image("my-app").unwrap();
    assert_eq!(count, 2);

    let all = ks.get_all_secrets("my-app").unwrap();
    assert!(all.is_empty());
}

#[test]
fn test_keystore_encryption_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();

    // Store binary data with all byte values
    let binary_data: Vec<u8> = (0..=255).collect();
    ks.store_secret(
        "img",
        "binary_secret",
        &binary_data,
        "file",
        "/secret",
        "file",
        None,
    )
    .unwrap();

    let secret = ks.get_secret("img", "binary_secret").unwrap().unwrap();
    assert_eq!(secret.value, binary_data);
}

#[test]
fn test_keystore_per_image_scoping() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();

    ks.store_secret("app1", "key", b"value1", "env", "K", "file", None)
        .unwrap();
    ks.store_secret("app2", "key", b"value2", "env", "K", "file", None)
        .unwrap();

    let s1 = ks.get_secret("app1", "key").unwrap().unwrap();
    let s2 = ks.get_secret("app2", "key").unwrap().unwrap();
    assert_eq!(s1.value, b"value1");
    assert_eq!(s2.value, b"value2");

    // Deleting one image's secrets should not affect the other
    ks.delete_secrets_for_image("app1").unwrap();
    assert!(ks.get_secret("app1", "key").unwrap().is_none());
    assert!(ks.get_secret("app2", "key").unwrap().is_some());
}

#[test]
fn test_keystore_replace_secret() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();

    ks.store_secret("img", "key", b"old-value", "env", "K", "file", None)
        .unwrap();
    ks.store_secret("img", "key", b"new-value", "env", "K", "file", None)
        .unwrap();

    let secret = ks.get_secret("img", "key").unwrap().unwrap();
    assert_eq!(secret.value, b"new-value");

    // Should still only have one secret
    let all = ks.get_all_secrets("img").unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn test_keystore_ttl_expiry_detection() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();

    // Store a secret with 1-second TTL
    ks.store_secret("img", "short-lived", b"val", "env", "V", "cmd", Some(1))
        .unwrap();

    // Store a secret with no TTL (should never expire)
    ks.store_secret("img", "forever", b"val", "env", "V", "cmd", None)
        .unwrap();

    // Store a secret with long TTL
    ks.store_secret("img", "long-lived", b"val", "env", "V", "cmd", Some(999999))
        .unwrap();

    // Wait for short TTL to expire
    std::thread::sleep(std::time::Duration::from_secs(2));

    let expired = ks.get_expired_secrets("img").unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].secret_name, "short-lived");
}

#[test]
fn test_keystore_key_file_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let _ks = Keystore::open(&db_path, &key_path).unwrap();
    assert!(key_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&key_path).unwrap().permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "key file must have 0600 permissions"
        );
    }
}

#[test]
fn test_keystore_persistence_across_opens() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    // Store a secret
    {
        let ks = Keystore::open(&db_path, &key_path).unwrap();
        ks.store_secret(
            "img",
            "persistent",
            b"persisted-value",
            "env",
            "V",
            "file",
            None,
        )
        .unwrap();
    }

    // Reopen and retrieve
    {
        let ks = Keystore::open(&db_path, &key_path).unwrap();
        let secret = ks.get_secret("img", "persistent").unwrap().unwrap();
        assert_eq!(secret.value, b"persisted-value");
    }
}

#[test]
fn test_keystore_delete_nonexistent() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("keystore.db");
    let key_path = dir.path().join("keystore.key");

    let ks = Keystore::open(&db_path, &key_path).unwrap();
    let count = ks.delete_secrets_for_image("nonexistent-image").unwrap();
    assert_eq!(count, 0);
}

// ---------------------------------------------------------------------------
// Injection plan building
// ---------------------------------------------------------------------------

#[test]
fn test_injection_plan_env_secrets() {
    let secrets = vec![
        ResolvedSecret {
            name: "api_key".to_string(),
            inject_type: "env".to_string(),
            inject_target: "API_KEY".to_string(),
            value: b"secret-key-123".to_vec(),
        },
        ResolvedSecret {
            name: "db_pass".to_string(),
            inject_type: "env".to_string(),
            inject_target: "PGPASSWORD".to_string(),
            value: b"db-password".to_vec(),
        },
    ];

    let plan = inject::build_injection_plan(&secrets, Path::new("/tmp/secrets")).unwrap();
    assert_eq!(plan.env_vars.len(), 2);
    assert_eq!(plan.env_vars.get("API_KEY").unwrap(), "secret-key-123");
    assert_eq!(plan.env_vars.get("PGPASSWORD").unwrap(), "db-password");
    assert!(plan.file_mounts.is_empty());
}

#[test]
fn test_injection_plan_file_secrets() {
    let secrets = vec![
        ResolvedSecret {
            name: "gcp_creds".to_string(),
            inject_type: "file".to_string(),
            inject_target: "/run/secrets/gcp.json".to_string(),
            value: b"{\"type\": \"service_account\"}".to_vec(),
        },
        ResolvedSecret {
            name: "tls_cert".to_string(),
            inject_type: "file".to_string(),
            inject_target: "/etc/ssl/cert.pem".to_string(),
            value: b"-----BEGIN CERTIFICATE-----".to_vec(),
        },
    ];

    let plan = inject::build_injection_plan(&secrets, Path::new("/tmp/coast-secrets/my-instance"))
        .unwrap();

    assert!(plan.env_vars.is_empty());
    assert_eq!(plan.file_mounts.len(), 2);

    // Verify host path construction
    assert_eq!(
        plan.file_mounts[0],
        FileMount {
            host_path: std::path::PathBuf::from("/tmp/coast-secrets/my-instance/gcp_creds"),
            container_path: std::path::PathBuf::from("/run/secrets/gcp.json"),
        }
    );
    assert_eq!(
        plan.file_mounts[1],
        FileMount {
            host_path: std::path::PathBuf::from("/tmp/coast-secrets/my-instance/tls_cert"),
            container_path: std::path::PathBuf::from("/etc/ssl/cert.pem"),
        }
    );
}

#[test]
fn test_injection_plan_mixed() {
    let secrets = vec![
        ResolvedSecret {
            name: "api_key".to_string(),
            inject_type: "env".to_string(),
            inject_target: "API_KEY".to_string(),
            value: b"key-123".to_vec(),
        },
        ResolvedSecret {
            name: "cert".to_string(),
            inject_type: "file".to_string(),
            inject_target: "/etc/ssl/cert.pem".to_string(),
            value: b"cert-data".to_vec(),
        },
    ];

    let plan = inject::build_injection_plan(&secrets, Path::new("/tmp/s")).unwrap();
    assert_eq!(plan.env_vars.len(), 1);
    assert_eq!(plan.file_mounts.len(), 1);
}

#[test]
fn test_injection_plan_empty() {
    let plan = inject::build_injection_plan(&[], Path::new("/tmp")).unwrap();
    assert!(plan.env_vars.is_empty());
    assert!(plan.file_mounts.is_empty());
}

#[test]
fn test_injection_plan_non_utf8_env_error() {
    let secrets = vec![ResolvedSecret {
        name: "binary".to_string(),
        inject_type: "env".to_string(),
        inject_target: "VAR".to_string(),
        value: vec![0xFF, 0xFE], // Invalid UTF-8
    }];

    let result = inject::build_injection_plan(&secrets, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("non-UTF-8"));
}

#[test]
fn test_injection_plan_unknown_inject_type_error() {
    let secrets = vec![ResolvedSecret {
        name: "bad".to_string(),
        inject_type: "unknown".to_string(),
        inject_target: "whatever".to_string(),
        value: b"val".to_vec(),
    }];

    let result = inject::build_injection_plan(&secrets, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unknown inject type"));
}

// ---------------------------------------------------------------------------
// End-to-end: extract with registry, then build injection plan
// ---------------------------------------------------------------------------

#[test]
fn test_extract_then_inject_env() {
    let key = "COAST_E2E_SECRET_TEST_VAR";
    unsafe {
        std::env::set_var(key, "e2e-secret-value");
    }

    let registry = ExtractorRegistry::with_builtins();
    let mut params = HashMap::new();
    params.insert("var".to_string(), key.to_string());

    let secret_value = registry.extract("env", &params).unwrap();
    let text = secret_value.as_text().unwrap().to_string();

    let resolved = vec![ResolvedSecret {
        name: "test_secret".to_string(),
        inject_type: "env".to_string(),
        inject_target: "MY_SECRET".to_string(),
        value: text.as_bytes().to_vec(),
    }];

    let plan = inject::build_injection_plan(&resolved, Path::new("/tmp")).unwrap();
    assert_eq!(plan.env_vars.get("MY_SECRET").unwrap(), "e2e-secret-value");

    unsafe {
        std::env::remove_var(key);
    }
}

#[test]
fn test_extract_then_inject_file() {
    let dir = tempfile::tempdir().unwrap();
    let secret_file = dir.path().join("credentials.json");
    std::fs::write(&secret_file, r#"{"key":"value"}"#).unwrap();

    let registry = ExtractorRegistry::with_builtins();
    let mut params = HashMap::new();
    params.insert(
        "path".to_string(),
        secret_file.to_string_lossy().to_string(),
    );

    let secret_value = registry.extract("file", &params).unwrap();
    let text = secret_value.as_text().unwrap().to_string();

    let resolved = vec![ResolvedSecret {
        name: "gcp_creds".to_string(),
        inject_type: "file".to_string(),
        inject_target: "/run/secrets/gcp.json".to_string(),
        value: text.as_bytes().to_vec(),
    }];

    let plan = inject::build_injection_plan(&resolved, Path::new("/tmp/secrets")).unwrap();
    assert_eq!(plan.file_mounts.len(), 1);
    assert_eq!(
        plan.file_mounts[0].container_path,
        std::path::PathBuf::from("/run/secrets/gcp.json")
    );
    assert_eq!(
        plan.file_mounts[0].host_path,
        std::path::PathBuf::from("/tmp/secrets/gcp_creds")
    );
}
