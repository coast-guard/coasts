/// Secret injection logic.
///
/// Given a list of resolved secrets, produces environment variable mappings
/// and file mount specifications for injection into coast containers.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use coast_core::error::Result;

/// A resolved secret ready for injection.
#[derive(Debug, Clone)]
pub struct ResolvedSecret {
    /// Secret name.
    pub name: String,
    /// Injection type: "env" or "file".
    pub inject_type: String,
    /// Injection target: env var name or container file path.
    pub inject_target: String,
    /// Decrypted secret value.
    pub value: Vec<u8>,
}

/// A file mount specification for injecting secrets as files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMount {
    /// Path on the host where the secret is written (tmpfs).
    pub host_path: PathBuf,
    /// Path inside the coast container where the file appears.
    pub container_path: PathBuf,
}

/// The result of processing secrets for injection.
#[derive(Debug, Clone)]
pub struct InjectionPlan {
    /// Environment variables to set in the coast container.
    pub env_vars: HashMap<String, String>,
    /// File mounts to create in the coast container.
    pub file_mounts: Vec<FileMount>,
}

/// Build an injection plan from a list of resolved secrets.
///
/// Secrets with `inject_type == "env"` become environment variables.
/// Secrets with `inject_type == "file"` become tmpfs file mounts.
///
/// The `tmpfs_base` is the host directory where secret files are written
/// before being mounted into the container (e.g., `/tmp/coast-secrets/{instance}/`).
pub fn build_injection_plan(
    secrets: &[ResolvedSecret],
    tmpfs_base: &Path,
) -> Result<InjectionPlan> {
    let mut env_vars = HashMap::new();
    let mut file_mounts = Vec::new();

    for secret in secrets {
        match secret.inject_type.as_str() {
            "env" => {
                let value = String::from_utf8(secret.value.clone()).map_err(|_| {
                    coast_core::error::CoastError::secret(format!(
                        "Secret '{}' contains non-UTF-8 data and cannot be injected as env var '{}'",
                        secret.name, secret.inject_target
                    ))
                })?;
                env_vars.insert(secret.inject_target.clone(), value);
            }
            "file" => {
                let host_path = tmpfs_base.join(&secret.name);
                let container_path = PathBuf::from(&secret.inject_target);
                file_mounts.push(FileMount {
                    host_path,
                    container_path,
                });
            }
            other => {
                return Err(coast_core::error::CoastError::secret(format!(
                    "Unknown inject type '{}' for secret '{}'. Expected 'env' or 'file'.",
                    other, secret.name
                )));
            }
        }
    }

    Ok(InjectionPlan {
        env_vars,
        file_mounts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_injection() {
        let secrets = vec![ResolvedSecret {
            name: "api_key".to_string(),
            inject_type: "env".to_string(),
            inject_target: "API_KEY".to_string(),
            value: b"secret123".to_vec(),
        }];

        let plan = build_injection_plan(&secrets, Path::new("/tmp/secrets")).unwrap();
        assert_eq!(plan.env_vars.get("API_KEY").unwrap(), "secret123");
        assert!(plan.file_mounts.is_empty());
    }

    #[test]
    fn test_file_injection() {
        let secrets = vec![ResolvedSecret {
            name: "gcp_creds".to_string(),
            inject_type: "file".to_string(),
            inject_target: "/run/secrets/gcp.json".to_string(),
            value: b"{\"key\": \"value\"}".to_vec(),
        }];

        let plan = build_injection_plan(&secrets, Path::new("/tmp/secrets")).unwrap();
        assert!(plan.env_vars.is_empty());
        assert_eq!(plan.file_mounts.len(), 1);
        assert_eq!(
            plan.file_mounts[0],
            FileMount {
                host_path: PathBuf::from("/tmp/secrets/gcp_creds"),
                container_path: PathBuf::from("/run/secrets/gcp.json"),
            }
        );
    }

    #[test]
    fn test_mixed_injection() {
        let secrets = vec![
            ResolvedSecret {
                name: "api_key".to_string(),
                inject_type: "env".to_string(),
                inject_target: "API_KEY".to_string(),
                value: b"key123".to_vec(),
            },
            ResolvedSecret {
                name: "db_pass".to_string(),
                inject_type: "env".to_string(),
                inject_target: "PGPASSWORD".to_string(),
                value: b"dbpass".to_vec(),
            },
            ResolvedSecret {
                name: "cert".to_string(),
                inject_type: "file".to_string(),
                inject_target: "/etc/ssl/cert.pem".to_string(),
                value: b"cert-data".to_vec(),
            },
        ];

        let plan = build_injection_plan(&secrets, Path::new("/tmp/s")).unwrap();
        assert_eq!(plan.env_vars.len(), 2);
        assert_eq!(plan.env_vars.get("API_KEY").unwrap(), "key123");
        assert_eq!(plan.env_vars.get("PGPASSWORD").unwrap(), "dbpass");
        assert_eq!(plan.file_mounts.len(), 1);
        assert_eq!(
            plan.file_mounts[0].container_path,
            PathBuf::from("/etc/ssl/cert.pem")
        );
    }

    #[test]
    fn test_unknown_inject_type() {
        let secrets = vec![ResolvedSecret {
            name: "bad".to_string(),
            inject_type: "unknown".to_string(),
            inject_target: "whatever".to_string(),
            value: b"val".to_vec(),
        }];

        let result = build_injection_plan(&secrets, Path::new("/tmp"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown inject type"));
    }

    #[test]
    fn test_non_utf8_env_injection() {
        let secrets = vec![ResolvedSecret {
            name: "binary".to_string(),
            inject_type: "env".to_string(),
            inject_target: "VAR".to_string(),
            value: vec![0xFF, 0xFE], // Invalid UTF-8
        }];

        let result = build_injection_plan(&secrets, Path::new("/tmp"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("non-UTF-8"));
    }

    #[test]
    fn test_empty_secrets() {
        let plan = build_injection_plan(&[], Path::new("/tmp")).unwrap();
        assert!(plan.env_vars.is_empty());
        assert!(plan.file_mounts.is_empty());
    }

    #[test]
    fn test_file_mount_host_path_construction() {
        let secrets = vec![
            ResolvedSecret {
                name: "secret_a".to_string(),
                inject_type: "file".to_string(),
                inject_target: "/a".to_string(),
                value: b"a".to_vec(),
            },
            ResolvedSecret {
                name: "secret_b".to_string(),
                inject_type: "file".to_string(),
                inject_target: "/b".to_string(),
                value: b"b".to_vec(),
            },
        ];

        let plan =
            build_injection_plan(&secrets, Path::new("/tmp/coast-secrets/my-instance")).unwrap();
        assert_eq!(
            plan.file_mounts[0].host_path,
            PathBuf::from("/tmp/coast-secrets/my-instance/secret_a")
        );
        assert_eq!(
            plan.file_mounts[1].host_path,
            PathBuf::from("/tmp/coast-secrets/my-instance/secret_b")
        );
    }
}
