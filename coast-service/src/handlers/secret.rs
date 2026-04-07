use crate::state::ServiceState;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    RevealSecretRequest, RevealSecretResponse, SecretInfo, SecretRequest, SecretResponse,
};
use tracing::info;

pub async fn handle(req: SecretRequest, state: &ServiceState) -> Result<SecretResponse> {
    match req {
        SecretRequest::Set {
            instance,
            project,
            name,
            value,
        } => {
            info!(instance = %instance, name = %name, "remote secret set");

            let home = crate::state::service_home();
            let keystore_db = home.join("keystore.db");
            let keystore_key = home.join("keystore.key");

            match coast_secrets::keystore::Keystore::open(&keystore_db, &keystore_key) {
                Ok(keystore) => {
                    let _ = keystore.store_secret(&coast_secrets::keystore::StoreSecretParams {
                        coast_image: &project,
                        secret_name: &name,
                        value: value.as_bytes(),
                        inject_type: "env",
                        inject_target: &name,
                        extractor: "manual",
                        ttl_seconds: None,
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to open keystore for secret set");
                }
            }

            let db = state.db.lock().await;
            if let Ok(Some(inst)) = db.get_instance(&project, &instance) {
                if let (Some(ref cid), Some(ref docker)) = (&inst.container_id, &state.docker) {
                    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
                    use coast_docker::runtime::Runtime;
                    let cmd = format!(
                        "mkdir -p /run/secrets && echo -n '{}' > /run/secrets/{}",
                        value.replace('\'', "'\\''"),
                        name
                    );
                    let _ = rt.exec_in_coast(cid, &["sh", "-c", &cmd]).await;
                }
            }

            Ok(SecretResponse {
                message: format!("Secret '{name}' set for instance '{instance}'"),
                secrets: vec![],
            })
        }
        SecretRequest::List { instance, project } => {
            info!(instance = %instance, project = %project, "remote secret list");

            let home = crate::state::service_home();
            let keystore_db = home.join("keystore.db");
            let keystore_key = home.join("keystore.key");

            let secrets = match coast_secrets::keystore::Keystore::open(&keystore_db, &keystore_key)
            {
                Ok(keystore) => keystore
                    .get_all_secrets(&project)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| SecretInfo {
                        name: s.secret_name,
                        extractor: s.extractor,
                        inject: format!("{}:{}", s.inject_type, s.inject_target),
                        is_override: false,
                    })
                    .collect(),
                Err(_) => vec![],
            };

            Ok(SecretResponse {
                message: format!("{} secret(s)", secrets.len()),
                secrets,
            })
        }
    }
}

pub async fn handle_reveal(
    req: RevealSecretRequest,
    _state: &ServiceState,
) -> Result<RevealSecretResponse> {
    info!(project = %req.project, name = %req.name, secret = %req.secret, "remote secret reveal");

    let home = crate::state::service_home();
    let keystore_db = home.join("keystore.db");
    let keystore_key = home.join("keystore.key");

    let keystore = coast_secrets::keystore::Keystore::open(&keystore_db, &keystore_key)
        .map_err(|e| CoastError::state(format!("keystore error: {e}")))?;

    let instance_key = format!("{}/{}", req.project, req.name);
    let stored = keystore
        .get_secret(&instance_key, &req.secret)
        .ok()
        .flatten()
        .or_else(|| {
            keystore
                .get_secret(&req.project, &req.secret)
                .ok()
                .flatten()
        });

    match stored {
        Some(s) => Ok(RevealSecretResponse {
            name: req.secret,
            value: String::from_utf8_lossy(&s.value).to_string(),
        }),
        None => Err(CoastError::state(format!(
            "Secret '{}' not found",
            req.secret
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::instances::RemoteInstance;
    use crate::state::{ServiceDb, ServiceState};
    use std::sync::Mutex as StdMutex;

    static ENV_MUTEX: StdMutex<()> = StdMutex::new(());

    fn test_state() -> ServiceState {
        ServiceState::new_for_testing(ServiceDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str) -> RemoteInstance {
        RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "running".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_secret_set_stores_value() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("COAST_SERVICE_HOME", tmp.path());

        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("inst1", "proj1"))
                .unwrap();
        }

        let set_req = SecretRequest::Set {
            instance: "inst1".to_string(),
            project: "proj1".to_string(),
            name: "MY_KEY".to_string(),
            value: "my_value".to_string(),
        };
        let resp = handle(set_req, &state).await.unwrap();
        assert!(resp.message.contains("MY_KEY"));

        let list_req = SecretRequest::List {
            instance: "inst1".to_string(),
            project: "proj1".to_string(),
        };
        let resp = handle(list_req, &state).await.unwrap();
        assert!(resp.secrets.iter().any(|s| s.name == "MY_KEY"));

        std::env::remove_var("COAST_SERVICE_HOME");
    }

    #[tokio::test]
    async fn test_secret_list_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("COAST_SERVICE_HOME", tmp.path());

        let state = test_state();
        let req = SecretRequest::List {
            instance: "inst1".to_string(),
            project: "proj1".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.message, "0 secret(s)");
        assert!(resp.secrets.is_empty());

        std::env::remove_var("COAST_SERVICE_HOME");
    }

    #[tokio::test]
    async fn test_secret_set_no_docker() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("COAST_SERVICE_HOME", tmp.path());

        let state = test_state();

        let req = SecretRequest::Set {
            instance: "no-inst".to_string(),
            project: "proj".to_string(),
            name: "DB_PASS".to_string(),
            value: "s3cret".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert!(resp.message.contains("DB_PASS"));

        let keystore_db = tmp.path().join("keystore.db");
        assert!(keystore_db.exists());

        std::env::remove_var("COAST_SERVICE_HOME");
    }

    #[tokio::test]
    async fn test_secret_list_no_keystore() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let empty_dir = tmp.path().join("nonexistent");
        std::env::set_var("COAST_SERVICE_HOME", &empty_dir);

        let state = test_state();
        let req = SecretRequest::List {
            instance: "inst".to_string(),
            project: "proj".to_string(),
        };
        let resp = handle(req, &state).await.unwrap();
        assert_eq!(resp.message, "0 secret(s)");
        assert!(resp.secrets.is_empty());

        std::env::remove_var("COAST_SERVICE_HOME");
    }
}
