use std::path::Path;

use tracing::info;

use coast_docker::runtime::Runtime;

/// Collected secret injection data returned by [`load_secrets_for_instance`].
pub(super) struct SecretInjectionPlan {
    /// Environment variables to inject into the coast container.
    pub env_vars: std::collections::HashMap<String, String>,
    /// Bind mounts for file-type secrets (host path -> container path).
    pub bind_mounts: Vec<coast_docker::runtime::BindMount>,
    /// Container paths for secret files (used in compose overrides).
    pub container_paths: Vec<String>,
    /// File contents to write via exec after the DinD container starts.
    /// Bind mounts from host don't propagate through DinD's overlay correctly,
    /// so we write file secrets via exec instead.
    pub files_for_exec: Vec<(String, Vec<u8>)>,
}

/// Load secrets from the keystore and build an injection plan.
///
/// Reads the encrypted keystore, resolves all secrets for the given coastfile
/// image name, and produces env vars, bind mounts, and file data for exec injection.
#[allow(clippy::cognitive_complexity)]
pub(super) fn load_secrets_for_instance(
    coastfile_path: &Path,
    instance_name: &str,
) -> SecretInjectionPlan {
    let mut env_vars = std::collections::HashMap::new();
    let mut bind_mounts: Vec<coast_docker::runtime::BindMount> = Vec::new();

    let home_s = dirs::home_dir().unwrap_or_default();
    let keystore_db = home_s.join(".coast").join("keystore.db");
    let keystore_key = home_s.join(".coast").join("keystore.key");

    if keystore_db.exists() && keystore_key.exists() {
        let coastfile_name = if coastfile_path.exists() {
            coast_core::coastfile::Coastfile::from_file(coastfile_path)
                .ok()
                .map(|cf| cf.name)
        } else {
            None
        };

        if let Some(ref image_name) = coastfile_name {
            match coast_secrets::keystore::Keystore::open(&keystore_db, &keystore_key) {
                Ok(keystore) => match keystore.get_all_secrets(image_name) {
                    Ok(secrets) if !secrets.is_empty() => {
                        let resolved: Vec<coast_secrets::inject::ResolvedSecret> = secrets
                            .iter()
                            .map(|s| coast_secrets::inject::ResolvedSecret {
                                name: s.secret_name.clone(),
                                inject_type: s.inject_type.clone(),
                                inject_target: s.inject_target.clone(),
                                value: s.value.clone(),
                            })
                            .collect();

                        let tmpfs_base = home_s
                            .join(".coast")
                            .join("secrets-tmpfs")
                            .join(instance_name);

                        match coast_secrets::inject::build_injection_plan(&resolved, &tmpfs_base) {
                            Ok(plan) => {
                                env_vars = plan.env_vars;

                                if !plan.file_mounts.is_empty() {
                                    if let Err(e) = std::fs::create_dir_all(&tmpfs_base) {
                                        tracing::warn!(
                                            error = %e,
                                            "failed to create secrets tmpfs dir"
                                        );
                                    } else {
                                        for fm in &plan.file_mounts {
                                            let secret_name = fm
                                                .host_path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("");
                                            if let Some(secret) =
                                                resolved.iter().find(|s| s.name == secret_name)
                                            {
                                                if let Err(e) =
                                                    std::fs::write(&fm.host_path, &secret.value)
                                                {
                                                    tracing::warn!(
                                                        error = %e,
                                                        path = %fm.host_path.display(),
                                                        "failed to write secret file"
                                                    );
                                                } else {
                                                    bind_mounts.push(
                                                        coast_docker::runtime::BindMount {
                                                            host_path: fm.host_path.clone(),
                                                            container_path: fm
                                                                .container_path
                                                                .to_string_lossy()
                                                                .to_string(),
                                                            read_only: false,
                                                            propagation: None,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                info!(
                                    env_count = env_vars.len(),
                                    file_count = bind_mounts.len(),
                                    "secrets loaded for injection"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "failed to build secret injection plan"
                                );
                            }
                        }
                    }
                    Ok(_) => {
                        info!("no secrets found in keystore for project");
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "failed to read secrets from keystore"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "failed to open keystore");
                }
            }
        }
    }

    let container_paths: Vec<String> = bind_mounts
        .iter()
        .map(|bm| bm.container_path.clone())
        .collect();

    let files_for_exec: Vec<(String, Vec<u8>)> = bind_mounts
        .iter()
        .filter_map(|bm| {
            std::fs::read(&bm.host_path)
                .ok()
                .map(|data| (bm.container_path.clone(), data))
        })
        .collect();

    SecretInjectionPlan {
        env_vars,
        bind_mounts,
        container_paths,
        files_for_exec,
    }
}

/// Write file-type secrets directly into the DinD container via exec.
///
/// Bind mounts from host don't propagate through DinD's overlay to the inner
/// Docker daemon, so inner compose services see a directory instead of a file.
/// Writing via exec creates the file on the DinD overlay where the inner daemon
/// can bind-mount it into service containers.
pub(super) async fn write_secret_files_via_exec(
    files_for_exec: &[(String, Vec<u8>)],
    container_id: &str,
    docker: &bollard::Docker,
) {
    if files_for_exec.is_empty() {
        return;
    }
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    for (container_path, data) in files_for_exec {
        let parent = std::path::Path::new(container_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());

        let b64 = base64_encode_bytes(data);
        let cmd = format!(
            "mkdir -p '{}' && echo '{}' | base64 -d > '{}'",
            parent, b64, container_path
        );
        if let Err(e) = runtime
            .exec_in_coast(container_id, &["sh", "-c", &cmd])
            .await
        {
            tracing::warn!(
                error = %e,
                path = %container_path,
                "failed to write secret file via exec"
            );
        }
    }
    info!(
        count = files_for_exec.len(),
        "secret files written into DinD via exec"
    );
}

/// Base64-encode a byte slice using the standard alphabet.
fn base64_encode_bytes(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 0x03) << 4 | b1 >> 4) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0x0f) << 2 | b2 >> 6) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- base64_encode_bytes ---

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode_bytes(b""), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode_bytes(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn test_base64_encode_padding_two_bytes() {
        assert_eq!(base64_encode_bytes(b"ab"), "YWI=");
    }

    #[test]
    fn test_base64_encode_no_padding() {
        assert_eq!(base64_encode_bytes(b"abc"), "YWJj");
    }

    #[test]
    fn test_base64_encode_single_byte() {
        assert_eq!(base64_encode_bytes(b"a"), "YQ==");
    }

    #[test]
    fn test_base64_encode_binary_data() {
        assert_eq!(base64_encode_bytes(&[0x00, 0xFF, 0x80]), "AP+A");
    }

    #[test]
    fn test_base64_encode_longer_string() {
        assert_eq!(
            base64_encode_bytes(b"Hello, World!"),
            "SGVsbG8sIFdvcmxkIQ=="
        );
    }

    // --- load_secrets_for_instance ---

    #[test]
    fn test_load_secrets_nonexistent_coastfile_returns_empty_plan() {
        let plan =
            load_secrets_for_instance(Path::new("/nonexistent/coastfile.toml"), "test-instance");
        assert!(plan.env_vars.is_empty());
        assert!(plan.bind_mounts.is_empty());
        assert!(plan.container_paths.is_empty());
        assert!(plan.files_for_exec.is_empty());
    }

    #[test]
    fn test_load_secrets_no_keystore_returns_empty_plan() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(&coastfile_path, "name = \"test\"\n").unwrap();

        let plan = load_secrets_for_instance(&coastfile_path, "test-instance");
        assert!(plan.env_vars.is_empty());
        assert!(plan.bind_mounts.is_empty());
        assert!(plan.container_paths.is_empty());
        assert!(plan.files_for_exec.is_empty());
    }

    #[test]
    fn test_secret_injection_plan_container_paths_match_bind_mounts() {
        let plan = SecretInjectionPlan {
            env_vars: std::collections::HashMap::new(),
            bind_mounts: vec![
                coast_docker::runtime::BindMount {
                    host_path: std::path::PathBuf::from("/tmp/secret1"),
                    container_path: "/run/secrets/db_pass".to_string(),
                    read_only: false,
                    propagation: None,
                },
                coast_docker::runtime::BindMount {
                    host_path: std::path::PathBuf::from("/tmp/secret2"),
                    container_path: "/run/secrets/api_key".to_string(),
                    read_only: false,
                    propagation: None,
                },
            ],
            container_paths: vec![
                "/run/secrets/db_pass".to_string(),
                "/run/secrets/api_key".to_string(),
            ],
            files_for_exec: vec![],
        };
        assert_eq!(plan.container_paths.len(), plan.bind_mounts.len());
        for (bm, cp) in plan.bind_mounts.iter().zip(plan.container_paths.iter()) {
            assert_eq!(&bm.container_path, cp);
        }
    }
}
