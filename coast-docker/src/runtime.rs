/// Runtime trait and container configuration types.
///
/// Defines the `Runtime` trait that all container runtimes (DinD, Sysbox, Podman)
/// implement. Also defines `ContainerConfig` for specifying how a coast container
/// should be created, and `ExecResult` for capturing command output.
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use coast_core::error::Result;

/// Configuration for creating a coast container.
///
/// Describes all the parameters needed to create and configure a coast
/// container on the host Docker daemon. Each runtime translates this
/// into runtime-specific Docker API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Project name (from the Coastfile).
    pub project: String,
    /// Instance name (e.g., "feature-oauth").
    pub instance_name: String,
    /// Docker image to use for the coast container (e.g., "docker:dind").
    pub image: String,
    /// Environment variables to set inside the coast container.
    pub env_vars: HashMap<String, String>,
    /// Bind mounts as (host_path, container_path) pairs.
    pub bind_mounts: Vec<BindMount>,
    /// Named volume mounts.
    pub volume_mounts: Vec<VolumeMount>,
    /// Tmpfs mounts (for file-based secrets).
    pub tmpfs_mounts: Vec<String>,
    /// Networks to connect this container to.
    pub networks: Vec<String>,
    /// Working directory inside the container.
    pub working_dir: Option<String>,
    /// Command/entrypoint override.
    pub entrypoint: Option<Vec<String>>,
    /// Command arguments.
    pub cmd: Option<Vec<String>>,
    /// Labels to attach to the container.
    pub labels: HashMap<String, String>,
    /// Ports to publish from the container to the host.
    /// Each entry maps a host port to a container port.
    pub published_ports: Vec<PortPublish>,
    /// Extra host entries ("hostname:ip") to add to the container's /etc/hosts.
    pub extra_hosts: Vec<String>,
}

/// A port to publish from the container to the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortPublish {
    /// Port on the host.
    pub host_port: u16,
    /// Port inside the container.
    pub container_port: u16,
}

/// A bind mount from host to container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindMount {
    /// Path on the host.
    pub host_path: PathBuf,
    /// Path inside the container.
    pub container_path: String,
    /// Whether the mount is read-only.
    pub read_only: bool,
    /// Mount propagation mode (e.g. `"rshared"`, `"rslave"`).
    /// When set, the mount uses bollard's structured `Mount` API
    /// instead of the `binds` string format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub propagation: Option<String>,
}

/// A named Docker volume mount.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    /// Docker volume name.
    pub volume_name: String,
    /// Path inside the container.
    pub container_path: String,
    /// Whether the mount is read-only.
    pub read_only: bool,
}

/// Result of executing a command inside a coast container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    /// Exit code of the command (0 = success).
    pub exit_code: i64,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl ExecResult {
    /// Returns true if the command exited successfully (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

impl ContainerConfig {
    /// Generate the canonical container name for this coast instance.
    ///
    /// Naming convention: `{project}-coasts-{instance_name}`
    /// Follows Docker Compose conventions so Docker Desktop displays
    /// just the instance name within the compose project group.
    pub fn container_name(&self) -> String {
        format!("{}-coasts-{}", self.project, self.instance_name)
    }

    /// Create a new ContainerConfig with required fields and sensible defaults.
    pub fn new(project: &str, instance_name: &str, image: &str) -> Self {
        let mut labels = HashMap::new();
        labels.insert("coast.project".to_string(), project.to_string());
        labels.insert("coast.instance".to_string(), instance_name.to_string());
        labels.insert("coast.managed".to_string(), "true".to_string());
        labels.insert(
            "com.docker.compose.project".to_string(),
            format!("{}-coasts", project),
        );
        labels.insert(
            "com.docker.compose.service".to_string(),
            instance_name.to_string(),
        );
        labels.insert(
            "com.docker.compose.container-number".to_string(),
            "1".to_string(),
        );
        labels.insert("com.docker.compose.oneoff".to_string(), "False".to_string());

        Self {
            project: project.to_string(),
            instance_name: instance_name.to_string(),
            image: image.to_string(),
            env_vars: HashMap::new(),
            bind_mounts: Vec::new(),
            volume_mounts: Vec::new(),
            tmpfs_mounts: Vec::new(),
            networks: Vec::new(),
            working_dir: None,
            entrypoint: None,
            cmd: None,
            labels,
            published_ports: Vec::new(),
            extra_hosts: Vec::new(),
        }
    }
}

/// Trait for container runtimes that can manage coast containers.
///
/// Each runtime (DinD, Sysbox, Podman) implements this trait to provide
/// the specific Docker API calls needed to create and manage coast
/// containers with an inner daemon.
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Return the name of this runtime (e.g., "dind", "sysbox", "podman").
    fn name(&self) -> &str;

    /// Create a coast container with the given configuration.
    ///
    /// Returns the container ID on success.
    async fn create_coast_container(&self, config: &ContainerConfig) -> Result<String>;

    /// Start a previously created coast container.
    async fn start_coast_container(&self, container_id: &str) -> Result<()>;

    /// Stop a running coast container.
    async fn stop_coast_container(&self, container_id: &str) -> Result<()>;

    /// Remove a coast container. The container must be stopped first.
    async fn remove_coast_container(&self, container_id: &str) -> Result<()>;

    /// Execute a command inside a running coast container.
    ///
    /// Returns the exit code, stdout, and stderr.
    async fn exec_in_coast(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult>;

    /// Get the IP address of a running coast container on the Docker bridge network.
    async fn get_container_ip(&self, container_id: &str) -> Result<IpAddr>;

    /// Whether this runtime requires the `--privileged` flag.
    fn requires_privileged(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_name_generation() {
        let config = ContainerConfig::new("my-app", "feature-oauth", "docker:dind");
        assert_eq!(config.container_name(), "my-app-coasts-feature-oauth");
    }

    #[test]
    fn test_container_name_with_special_chars() {
        let config = ContainerConfig::new("my-app", "main", "docker:dind");
        assert_eq!(config.container_name(), "my-app-coasts-main");
    }

    #[test]
    fn test_container_config_new_defaults() {
        let config = ContainerConfig::new("proj", "inst", "docker:dind");
        assert_eq!(config.project, "proj");
        assert_eq!(config.instance_name, "inst");
        assert_eq!(config.image, "docker:dind");
        assert!(config.env_vars.is_empty());
        assert!(config.bind_mounts.is_empty());
        assert!(config.volume_mounts.is_empty());
        assert!(config.tmpfs_mounts.is_empty());
        assert!(config.networks.is_empty());
        assert!(config.working_dir.is_none());
        assert!(config.entrypoint.is_none());
        assert!(config.cmd.is_none());
    }

    #[test]
    fn test_container_config_labels() {
        let config = ContainerConfig::new("my-app", "feature-x", "docker:dind");
        assert_eq!(
            config.labels.get("coast.project"),
            Some(&"my-app".to_string())
        );
        assert_eq!(
            config.labels.get("coast.instance"),
            Some(&"feature-x".to_string())
        );
        assert_eq!(
            config.labels.get("coast.managed"),
            Some(&"true".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.project"),
            Some(&"my-app-coasts".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.service"),
            Some(&"feature-x".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.container-number"),
            Some(&"1".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.oneoff"),
            Some(&"False".to_string())
        );
    }

    #[test]
    fn test_exec_result_success() {
        let result = ExecResult {
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: String::new(),
        };
        assert!(result.success());
    }

    #[test]
    fn test_exec_result_failure() {
        let result = ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error".to_string(),
        };
        assert!(!result.success());
    }

    #[test]
    fn test_exec_result_negative_exit_code() {
        let result = ExecResult {
            exit_code: -1,
            stdout: String::new(),
            stderr: "killed".to_string(),
        };
        assert!(!result.success());
    }

    #[test]
    fn test_container_config_serialization() {
        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ContainerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.project, "my-app");
        assert_eq!(deserialized.instance_name, "test");
        assert_eq!(deserialized.image, "docker:dind");
    }

    #[test]
    fn test_exec_result_serialization() {
        let result = ExecResult {
            exit_code: 0,
            stdout: "hello".to_string(),
            stderr: "warn".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ExecResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.exit_code, 0);
        assert_eq!(deserialized.stdout, "hello");
        assert_eq!(deserialized.stderr, "warn");
    }

    #[test]
    fn test_bind_mount_serialization() {
        let mount = BindMount {
            host_path: PathBuf::from("/home/user/project"),
            container_path: "/workspace".to_string(),
            read_only: false,
            propagation: None,
        };
        let json = serde_json::to_string(&mount).unwrap();
        let deserialized: BindMount = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.host_path, PathBuf::from("/home/user/project"));
        assert_eq!(deserialized.container_path, "/workspace");
        assert!(!deserialized.read_only);
    }

    #[test]
    fn test_volume_mount_serialization() {
        let mount = VolumeMount {
            volume_name: "coast--test--pg_data".to_string(),
            container_path: "/var/lib/postgresql/data".to_string(),
            read_only: false,
        };
        let json = serde_json::to_string(&mount).unwrap();
        let deserialized: VolumeMount = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.volume_name, "coast--test--pg_data");
        assert!(!deserialized.read_only);
    }

    #[test]
    fn test_bind_mount_read_only() {
        let mount = BindMount {
            host_path: PathBuf::from("/cache"),
            container_path: "/mnt/cache".to_string(),
            read_only: true,
            propagation: None,
        };
        assert!(mount.read_only);
    }

    #[test]
    fn test_port_publish_serialization() {
        let pp = PortPublish {
            host_port: 59000,
            container_port: 3000,
        };
        let json = serde_json::to_string(&pp).unwrap();
        let deserialized: PortPublish = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.host_port, 59000);
        assert_eq!(deserialized.container_port, 3000);
    }

    #[test]
    fn test_container_config_published_ports_default_empty() {
        let config = ContainerConfig::new("proj", "inst", "docker:dind");
        assert!(config.published_ports.is_empty());
    }

    #[test]
    fn test_container_config_with_published_ports_serialization() {
        let mut config = ContainerConfig::new("proj", "inst", "docker:dind");
        config.published_ports.push(PortPublish {
            host_port: 59000,
            container_port: 3000,
        });
        config.published_ports.push(PortPublish {
            host_port: 60000,
            container_port: 5432,
        });

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ContainerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.published_ports.len(), 2);
        assert_eq!(deserialized.published_ports[0].host_port, 59000);
        assert_eq!(deserialized.published_ports[1].container_port, 5432);
    }
}
