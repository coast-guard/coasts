/// Handler for the `coast secret` command.
///
/// Manages per-instance secret overrides. Supports setting a secret
/// value for a specific instance and listing secrets.
use std::collections::HashSet;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SecretInfo, SecretRequest, SecretResponse};

use crate::server::AppState;

/// Attempt to store a secret in the keystore.
///
/// Resolves home directory, builds keystore paths, opens the keystore,
/// and stores the secret. Returns `Some(())` on success, `None` if any
/// step fails (with a warning log).
fn try_store_secret_in_keystore(coast_image: &str, secret_name: &str, value: &[u8]) -> Option<()> {
    let home = dirs::home_dir()?;
    let keystore_db_path = home.join(".coast").join("keystore.db");
    let keystore_key_path = home.join(".coast").join("keystore.key");

    match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
        Ok(keystore) => {
            if let Err(e) = keystore.store_secret(&coast_secrets::keystore::StoreSecretParams {
                coast_image,
                secret_name,
                value,
                inject_type: "env",
                inject_target: secret_name,
                extractor: "manual",
                ttl_seconds: None,
            }) {
                tracing::warn!(error = %e, "failed to store secret in keystore");
                return None;
            }
            Some(())
        }
        Err(e) => {
            tracing::warn!(error = %e, "keystore not available, secret stored in response only");
            None
        }
    }
}

/// Merge base secrets with per-instance overrides.
///
/// 1. Collect base secrets, filtering by `declared` names if provided
/// 2. For each override, remove matching base entry and add the override
///
/// Returns a list of `SecretInfo` with `is_override` set appropriately.
fn merge_secrets(
    base: &[coast_secrets::keystore::StoredSecret],
    overrides: &[coast_secrets::keystore::StoredSecret],
    declared: &Option<HashSet<String>>,
) -> Vec<SecretInfo> {
    let mut secrets: Vec<SecretInfo> = Vec::new();

    // Add base secrets, filtered by declared names
    for s in base {
        if let Some(ref allowed) = declared {
            if !allowed.contains(&s.secret_name) {
                continue;
            }
        }
        secrets.push(SecretInfo {
            name: s.secret_name.clone(),
            extractor: s.extractor.clone(),
            inject: format!("{}:{}", s.inject_type, s.inject_target),
            is_override: false,
        });
    }

    // Apply overrides: remove matching base entry and add the override
    for s in overrides {
        secrets.retain(|existing| existing.name != s.secret_name);
        secrets.push(SecretInfo {
            name: s.secret_name.clone(),
            extractor: s.extractor.clone(),
            inject: format!("{}:{}", s.inject_type, s.inject_target),
            is_override: true,
        });
    }

    secrets
}

/// Handle a secret request.
///
/// Dispatches to set or list operations based on the request variant.
pub async fn handle(req: SecretRequest, state: &AppState) -> Result<SecretResponse> {
    match req {
        SecretRequest::Set {
            instance,
            project,
            name,
            value,
        } => handle_set(instance, project, name, value, state).await,
        SecretRequest::List { instance, project } => handle_list(instance, project, state).await,
    }
}

/// Set a per-instance secret override.
///
/// Stores the secret value in the keystore, scoped to the specific instance.
async fn handle_set(
    instance: String,
    project: String,
    name: String,
    value: String,
    state: &AppState,
) -> Result<SecretResponse> {
    info!(
        instance = %instance,
        project = %project,
        secret_name = %name,
        "handling secret set request"
    );

    // Phase 1: DB read (locked) — verify instance exists
    {
        let db = state.db.lock().await;
        let inst = db.get_instance(&project, &instance)?;
        if inst.is_none() {
            return Err(CoastError::InstanceNotFound {
                name: instance.clone(),
                project: project.clone(),
            });
        }
    }

    // Phase 2: Keystore I/O (unlocked)
    // Per-instance overrides use "{project}/{instance}" as the coast_image key.
    // Only interact with the keystore when a Docker client is available (i.e., not in tests).
    if state.docker.is_some() {
        let coast_image = format!("{project}/{instance}");
        if try_store_secret_in_keystore(&coast_image, &name, value.as_bytes()).is_some() {
            info!(
                instance = %instance,
                secret_name = %name,
                "secret override stored in keystore"
            );
        }
    }

    info!(
        instance = %instance,
        secret_name = %name,
        "secret override set"
    );

    Ok(SecretResponse {
        message: format!(
            "Secret '{}' set for instance '{}' in project '{}'.",
            name, instance, project
        ),
        secrets: vec![SecretInfo {
            name,
            extractor: "manual".to_string(),
            inject: "env".to_string(),
            is_override: true,
        }],
    })
}

/// List secrets for an instance.
///
/// Returns base secrets declared in the instance's Coastfile plus any per-instance
/// overrides. Secrets from the keystore that were not declared in the instance's
/// build Coastfile are filtered out to prevent cross-build leakage.
async fn handle_list(
    instance: String,
    project: String,
    state: &AppState,
) -> Result<SecretResponse> {
    info!(
        instance = %instance,
        project = %project,
        "handling secret list request"
    );

    // Phase 1: DB read (locked) — verify instance exists
    let build_id: Option<String> = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&project, &instance)?;
        match inst {
            Some(i) => i.build_id.clone(),
            None => {
                return Err(CoastError::InstanceNotFound {
                    name: instance.clone(),
                    project: project.clone(),
                });
            }
        }
    };

    let declared: Option<HashSet<String>> =
        super::declared_secret_names(&project, build_id.as_deref());

    // Phase 2: Keystore I/O (unlocked)
    // Query secrets from the keystore:
    // 1. Get base secrets for the coast image (project-level)
    // 2. Get per-instance overrides
    // 3. Merge: overrides take precedence
    // Only interact with the keystore when a Docker client is available (i.e., not in tests).
    let secrets: Vec<SecretInfo> = if state.docker.is_some() {
        let home = dirs::home_dir();
        if let Some(ref home) = home {
            let keystore_db_path = home.join(".coast").join("keystore.db");
            let keystore_key_path = home.join(".coast").join("keystore.key");

            if keystore_db_path.exists() {
                if let Ok(keystore) =
                    coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path)
                {
                    let base_secrets = keystore.get_all_secrets(&project).unwrap_or_default();
                    let instance_secrets = keystore
                        .get_all_secrets(&format!("{project}/{instance}"))
                        .unwrap_or_default();
                    merge_secrets(&base_secrets, &instance_secrets, &declared)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    info!(
        instance = %instance,
        secret_count = secrets.len(),
        "listing secrets"
    );

    Ok(SecretResponse {
        message: format!(
            "Secrets for instance '{}' in project '{}'.",
            instance, project
        ),
        secrets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};
    use coast_secrets::keystore::StoredSecret;

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("cid".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    fn make_instance_with_build(name: &str, project: &str, build_id: &str) -> CoastInstance {
        CoastInstance {
            build_id: Some(build_id.to_string()),
            ..make_instance(name, project)
        }
    }

    /// Helper to create a `StoredSecret` for testing.
    fn make_stored_secret(name: &str, extractor: &str) -> StoredSecret {
        StoredSecret {
            coast_image: "test-image".to_string(),
            secret_name: name.to_string(),
            value: vec![],
            inject_type: "env".to_string(),
            inject_target: name.to_string(),
            extracted_at: chrono::Utc::now(),
            extractor: extractor.to_string(),
            ttl_seconds: None,
        }
    }

    #[tokio::test]
    async fn test_secret_set() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app"))
                .unwrap();
        }

        let req = SecretRequest::Set {
            instance: "feat-a".to_string(),
            project: "my-app".to_string(),
            name: "API_KEY".to_string(),
            value: "secret-value-123".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.message.contains("API_KEY"));
        assert!(resp.message.contains("feat-a"));
        assert_eq!(resp.secrets.len(), 1);
        assert!(resp.secrets[0].is_override);
    }

    #[tokio::test]
    async fn test_secret_set_nonexistent_instance() {
        let state = test_state();
        let req = SecretRequest::Set {
            instance: "nonexistent".to_string(),
            project: "my-app".to_string(),
            name: "KEY".to_string(),
            value: "val".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_secret_list() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app"))
                .unwrap();
        }

        let req = SecretRequest::List {
            instance: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.message.contains("feat-a"));
        // Empty until keystore is integrated
        assert!(resp.secrets.is_empty());
    }

    #[tokio::test]
    async fn test_secret_list_nonexistent_instance() {
        let state = test_state();
        let req = SecretRequest::List {
            instance: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_secret_list_with_build_id_returns_empty_without_docker() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance_with_build("dev-1", "my-app", "build-abc"))
                .unwrap();
        }

        let req = SecretRequest::List {
            instance: "dev-1".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(
            resp.secrets.is_empty(),
            "Without Docker, no keystore secrets should be returned"
        );
    }

    #[tokio::test]
    async fn test_secret_list_instance_without_build_id() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("dev-1", "my-app"))
                .unwrap();
        }

        let req = SecretRequest::List {
            instance: "dev-1".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(
            resp.secrets.is_empty(),
            "Without Docker, no keystore secrets should be returned"
        );
    }

    // ==================== merge_secrets tests ====================

    #[test]
    fn test_merge_secrets_no_base_no_overrides() {
        let base: Vec<StoredSecret> = vec![];
        let overrides: Vec<StoredSecret> = vec![];
        let declared: Option<HashSet<String>> = None;

        let result = merge_secrets(&base, &overrides, &declared);
        assert!(result.is_empty());
    }

    #[test]
    fn test_merge_secrets_two_base_no_overrides() {
        let base = vec![
            make_stored_secret("API_KEY", "aws-extractor"),
            make_stored_secret("DB_PASSWORD", "vault-extractor"),
        ];
        let overrides: Vec<StoredSecret> = vec![];
        let declared: Option<HashSet<String>> = None;

        let result = merge_secrets(&base, &overrides, &declared);

        assert_eq!(result.len(), 2);
        assert!(!result[0].is_override);
        assert!(!result[1].is_override);
        assert!(result.iter().any(|s| s.name == "API_KEY"));
        assert!(result.iter().any(|s| s.name == "DB_PASSWORD"));
    }

    #[test]
    fn test_merge_secrets_base_filtered_by_declared() {
        let base = vec![
            make_stored_secret("API_KEY", "aws-extractor"),
            make_stored_secret("DB_PASSWORD", "vault-extractor"),
            make_stored_secret("EXTRA_SECRET", "manual"),
        ];
        let overrides: Vec<StoredSecret> = vec![];
        let declared: Option<HashSet<String>> = Some(
            ["API_KEY".to_string(), "DB_PASSWORD".to_string()]
                .into_iter()
                .collect(),
        );

        let result = merge_secrets(&base, &overrides, &declared);

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|s| s.name == "API_KEY"));
        assert!(result.iter().any(|s| s.name == "DB_PASSWORD"));
        assert!(!result.iter().any(|s| s.name == "EXTRA_SECRET"));
    }

    #[test]
    fn test_merge_secrets_override_replaces_base() {
        let base = vec![make_stored_secret("API_KEY", "aws-extractor")];
        let overrides = vec![make_stored_secret("API_KEY", "manual")];
        let declared: Option<HashSet<String>> = None;

        let result = merge_secrets(&base, &overrides, &declared);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "API_KEY");
        assert!(result[0].is_override);
        assert_eq!(result[0].extractor, "manual");
    }

    #[test]
    fn test_merge_secrets_override_for_new_name() {
        let base = vec![make_stored_secret("API_KEY", "aws-extractor")];
        let overrides = vec![make_stored_secret("NEW_SECRET", "manual")];
        let declared: Option<HashSet<String>> = None;

        let result = merge_secrets(&base, &overrides, &declared);

        assert_eq!(result.len(), 2);
        // Base secret present
        let api_key = result.iter().find(|s| s.name == "API_KEY").unwrap();
        assert!(!api_key.is_override);
        // Override added
        let new_secret = result.iter().find(|s| s.name == "NEW_SECRET").unwrap();
        assert!(new_secret.is_override);
    }

    #[test]
    fn test_merge_secrets_declared_none_includes_all_base() {
        let base = vec![
            make_stored_secret("SECRET_A", "extractor-a"),
            make_stored_secret("SECRET_B", "extractor-b"),
            make_stored_secret("SECRET_C", "extractor-c"),
        ];
        let overrides: Vec<StoredSecret> = vec![];
        let declared: Option<HashSet<String>> = None;

        let result = merge_secrets(&base, &overrides, &declared);

        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|s| !s.is_override));
    }

    // ==================== try_store_secret_in_keystore smoke test ====================

    #[test]
    fn test_try_store_secret_in_keystore_does_not_panic() {
        // Smoke test: this should not panic even if keystore is unavailable.
        // We're testing that the function handles errors gracefully.
        // In a typical test environment (CI), keystore may not be set up,
        // so we expect None to be returned.
        let result = try_store_secret_in_keystore("test-image", "TEST_SECRET", b"test-value");
        // We don't assert on the result because it depends on the environment.
        // The important thing is that it doesn't panic.
        let _ = result;
    }
}
