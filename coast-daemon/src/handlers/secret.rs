/// Handler for the `coast secret` command.
///
/// Manages per-instance secret overrides. Supports setting a secret
/// value for a specific instance and listing secrets.
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SecretInfo, SecretRequest, SecretResponse};

use crate::server::AppState;

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
#[allow(clippy::cognitive_complexity)]
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
        if let Some(ref home) = dirs::home_dir() {
            let keystore_db_path = home.join(".coast").join("keystore.db");
            let keystore_key_path = home.join(".coast").join("keystore.key");

            match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
                Ok(keystore) => {
                    keystore.store_secret(
                        &format!("{project}/{instance}"),
                        &name,
                        value.as_bytes(),
                        "env",
                        &name,
                        "manual",
                        None,
                    )?;
                    info!(
                        instance = %instance,
                        secret_name = %name,
                        "secret override stored in keystore"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "keystore not available, secret stored in response only");
                }
            }
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
/// Returns both the base secrets from the coast image and any per-instance overrides.
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
    // Query secrets from the keystore:
    // 1. Get base secrets for the coast image (project-level)
    // 2. Get per-instance overrides
    // 3. Merge: overrides take precedence
    // Only interact with the keystore when a Docker client is available (i.e., not in tests).
    let mut secrets: Vec<SecretInfo> = Vec::new();
    if state.docker.is_some() {
        if let Some(ref home) = dirs::home_dir() {
            let keystore_db_path = home.join(".coast").join("keystore.db");
            let keystore_key_path = home.join(".coast").join("keystore.key");

            if keystore_db_path.exists() {
                if let Ok(keystore) =
                    coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path)
                {
                    // Get base secrets for the project
                    if let Ok(base_secrets) = keystore.get_all_secrets(&project) {
                        for s in &base_secrets {
                            secrets.push(SecretInfo {
                                name: s.secret_name.clone(),
                                extractor: s.extractor.clone(),
                                inject: format!("{}:{}", s.inject_type, s.inject_target),
                                is_override: false,
                            });
                        }
                    }
                    // Get per-instance overrides and merge
                    if let Ok(instance_secrets) =
                        keystore.get_all_secrets(&format!("{project}/{instance}"))
                    {
                        for s in &instance_secrets {
                            // Remove any base secret with the same name, then add the override
                            secrets.retain(|existing| existing.name != s.secret_name);
                            secrets.push(SecretInfo {
                                name: s.secret_name.clone(),
                                extractor: s.extractor.clone(),
                                inject: format!("{}:{}", s.inject_type, s.inject_target),
                                is_override: true,
                            });
                        }
                    }
                }
            }
        }
    }

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
}
