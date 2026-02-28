/// Podman runtime implementation.
///
/// Uses Podman instead of Docker for the inner container runtime.
/// Coast containers run the Podman system service inside, and compose
/// interaction uses `podman-compose` instead of `docker compose`.
use std::collections::HashMap;
use std::net::IpAddr;

use async_trait::async_trait;
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::Docker;
use tracing::{debug, info};

use coast_core::error::{CoastError, Result};

use crate::runtime::{ContainerConfig, ExecResult, Runtime};

/// The default image used for Podman coast containers.
///
/// This image needs Podman pre-installed. In practice a custom coast
/// base image would be built with Podman and podman-compose installed.
pub const PODMAN_IMAGE: &str = "quay.io/podman/stable:latest";

/// Podman container runtime.
///
/// Runs coast containers with Podman as the inner runtime instead of Docker.
/// The host Docker daemon (accessed via bollard) still manages the outer
/// coast container, but inside the container, Podman runs as the container
/// engine with `podman system service` providing the API.
///
/// Compose interaction uses `podman-compose` instead of `docker compose`.
pub struct PodmanRuntime {
    /// Bollard Docker client connected to the host daemon.
    ///
    /// Note: We still use Docker on the host to manage the outer coast
    /// container. Only the inner runtime uses Podman.
    docker: Docker,
}

impl PodmanRuntime {
    /// Create a new Podman runtime connected to the default Docker socket.
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults().map_err(|e| CoastError::Docker {
            message: format!(
                "Failed to connect to Docker daemon. Is Docker running? \
                 The Podman runtime still requires Docker on the host to \
                 manage coast containers. Error: {e}"
            ),
            source: Some(Box::new(e)),
        })?;
        Ok(Self { docker })
    }

    /// Create a new Podman runtime with an existing Docker client.
    pub fn with_client(docker: Docker) -> Self {
        Self { docker }
    }

    /// Build the bollard container configuration from a `ContainerConfig`.
    ///
    /// Similar to DinD but configures the container to run Podman internally.
    /// This is a pure function suitable for unit testing.
    pub fn build_container_config(config: &ContainerConfig) -> PodmanCreateParams {
        let container_name = config.container_name();

        // Build environment variables
        let env: Vec<String> = config
            .env_vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        // Build bind mount strings
        let mut binds: Vec<String> = Vec::new();
        for mount in &config.bind_mounts {
            let mode = if mount.read_only { "ro" } else { "rw" };
            binds.push(format!(
                "{}:{}:{mode}",
                mount.host_path.display(),
                mount.container_path
            ));
        }

        // Build volume mount strings
        for mount in &config.volume_mounts {
            let mode = if mount.read_only { "ro" } else { "rw" };
            binds.push(format!(
                "{}:{}:{mode}",
                mount.volume_name, mount.container_path
            ));
        }

        // Build tmpfs mounts
        let mut tmpfs: HashMap<String, String> = HashMap::new();
        for path in &config.tmpfs_mounts {
            tmpfs.insert(path.clone(), "rw,noexec,nosuid,size=64m".to_string());
        }

        let labels = config.labels.clone();

        PodmanCreateParams {
            name: container_name,
            image: config.image.clone(),
            env,
            binds,
            tmpfs,
            labels,
            privileged: true,
            working_dir: config.working_dir.clone(),
            entrypoint: config.entrypoint.clone(),
            cmd: config.cmd.clone(),
            networks: config.networks.clone(),
        }
    }
}

/// Parameters for creating a Podman coast container, extracted for testability.
#[derive(Debug, Clone)]
pub struct PodmanCreateParams {
    /// Container name.
    pub name: String,
    /// Docker image.
    pub image: String,
    /// Environment variables in "KEY=VALUE" format.
    pub env: Vec<String>,
    /// Bind mounts in "host:container:mode" format.
    pub binds: Vec<String>,
    /// Tmpfs mounts as path -> options.
    pub tmpfs: HashMap<String, String>,
    /// Container labels.
    pub labels: HashMap<String, String>,
    /// Whether the container runs in privileged mode.
    ///
    /// Podman containers need privileged mode to run the inner Podman daemon.
    pub privileged: bool,
    /// Working directory override.
    pub working_dir: Option<String>,
    /// Entrypoint override.
    pub entrypoint: Option<Vec<String>>,
    /// Command arguments.
    pub cmd: Option<Vec<String>>,
    /// Networks to connect to.
    pub networks: Vec<String>,
}

#[async_trait]
impl Runtime for PodmanRuntime {
    fn name(&self) -> &str {
        "podman"
    }

    async fn create_coast_container(&self, config: &ContainerConfig) -> Result<String> {
        let params = Self::build_container_config(config);

        info!(
            container_name = %params.name,
            image = %params.image,
            "Creating Podman coast container"
        );

        let host_config = bollard::models::HostConfig {
            privileged: Some(params.privileged),
            binds: if params.binds.is_empty() {
                None
            } else {
                Some(params.binds)
            },
            tmpfs: if params.tmpfs.is_empty() {
                None
            } else {
                Some(params.tmpfs)
            },
            ..Default::default()
        };

        let container_config = Config {
            image: Some(params.image),
            env: if params.env.is_empty() {
                None
            } else {
                Some(params.env)
            },
            host_config: Some(host_config),
            labels: Some(params.labels),
            working_dir: params.working_dir,
            entrypoint: params.entrypoint,
            cmd: params.cmd,
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: params.name.clone(),
            ..Default::default()
        };

        let response = self
            .docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to create Podman coast container '{}'. Error: {e}",
                    params.name
                ),
                source: Some(Box::new(e)),
            })?;

        info!(
            container_id = %response.id,
            container_name = %params.name,
            "Podman coast container created"
        );

        Ok(response.id)
    }

    async fn start_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Starting Podman coast container");

        self.docker
            .start_container::<String>(container_id, None)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to start Podman coast container '{container_id}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "Podman coast container started");
        Ok(())
    }

    async fn stop_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Stopping Podman coast container");

        self.docker
            .stop_container(container_id, None)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to stop Podman coast container '{container_id}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "Podman coast container stopped");
        Ok(())
    }

    async fn remove_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Removing Podman coast container");

        let options = RemoveContainerOptions {
            force: true,
            v: false,
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to remove Podman coast container '{container_id}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "Podman coast container removed");
        Ok(())
    }

    async fn exec_in_coast(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult> {
        debug!(
            container_id = %container_id,
            cmd = ?cmd,
            "Executing command in Podman coast container"
        );

        let exec_options = CreateExecOptions {
            cmd: Some(cmd.iter().map(std::string::ToString::to_string).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = self
            .docker
            .create_exec(container_id, exec_options)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to create exec in Podman container '{container_id}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        let start_options = StartExecOptions {
            detach: false,
            ..Default::default()
        };

        let output = self
            .docker
            .start_exec(&exec.id, Some(start_options))
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to start exec in Podman container '{container_id}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        let mut stdout = String::new();
        let mut stderr = String::new();

        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            use futures_util::StreamExt;
            while let Some(Ok(msg)) = output.next().await {
                match msg {
                    bollard::container::LogOutput::StdOut { message } => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }
        }

        let exec_inspect =
            self.docker
                .inspect_exec(&exec.id)
                .await
                .map_err(|e| CoastError::Docker {
                    message: format!("Failed to inspect exec result. Error: {e}"),
                    source: Some(Box::new(e)),
                })?;

        let exit_code = exec_inspect.exit_code.unwrap_or(-1);

        Ok(ExecResult {
            exit_code,
            stdout,
            stderr,
        })
    }

    async fn get_container_ip(&self, container_id: &str) -> Result<IpAddr> {
        let inspect = self
            .docker
            .inspect_container(container_id, None)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to inspect Podman container '{container_id}' for IP address. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        let network_settings = inspect.network_settings.ok_or_else(|| {
            CoastError::docker(format!(
                "Podman container '{container_id}' has no network settings. Is it running?"
            ))
        })?;

        let ip_str = network_settings
            .ip_address
            .as_deref()
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| {
                CoastError::docker(format!(
                    "Podman container '{container_id}' has no IP address. \
                     Is it running and connected to a network?"
                ))
            })?;

        ip_str.parse().map_err(|e| CoastError::Docker {
            message: format!(
                "Podman container '{container_id}' has invalid IP address '{ip_str}'. Error: {e}"
            ),
            source: None,
        })
    }

    fn requires_privileged(&self) -> bool {
        true
    }
}

/// Check whether Podman is available on this system.
///
/// Returns Ok(()) if podman is found, or an actionable error if not.
pub fn check_podman_available() -> Result<()> {
    match std::process::Command::new("podman")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(CoastError::RuntimeUnavailable {
            runtime: "podman".to_string(),
            reason: "podman is not installed or not on PATH. \
                     Install Podman from https://podman.io \
                     or use runtime = \"dind\" in your Coastfile."
                .to_string(),
        }),
    }
}

/// Check whether podman-compose is available on this system.
///
/// Returns Ok(()) if podman-compose is found, or an actionable error if not.
pub fn check_podman_compose_available() -> Result<()> {
    match std::process::Command::new("podman-compose")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err(CoastError::RuntimeUnavailable {
            runtime: "podman".to_string(),
            reason: "podman-compose is not installed or not on PATH. \
                     Install it with `pip install podman-compose` \
                     or use runtime = \"dind\" in your Coastfile."
                .to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::runtime::{BindMount, VolumeMount};

    #[test]
    fn test_build_container_config_name() {
        let config = ContainerConfig::new("my-app", "feature-oauth", PODMAN_IMAGE);
        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(params.name, "my-app-coasts-feature-oauth");
    }

    #[test]
    fn test_build_container_config_privileged() {
        let config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        let params = PodmanRuntime::build_container_config(&config);
        assert!(params.privileged);
    }

    #[test]
    fn test_build_container_config_env_vars() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.env_vars.insert("FOO".to_string(), "bar".to_string());

        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(params.env.len(), 1);
        assert!(params.env.contains(&"FOO=bar".to_string()));
    }

    #[test]
    fn test_build_container_config_bind_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.bind_mounts.push(BindMount {
            host_path: PathBuf::from("/home/user/project"),
            container_path: "/workspace".to_string(),
            read_only: false,
            propagation: None,
        });

        let params = PodmanRuntime::build_container_config(&config);
        assert!(params
            .binds
            .contains(&"/home/user/project:/workspace:rw".to_string()));
    }

    #[test]
    fn test_build_container_config_volume_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.volume_mounts.push(VolumeMount {
            volume_name: "my-vol".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
        });

        let params = PodmanRuntime::build_container_config(&config);
        assert!(params.binds.contains(&"my-vol:/data:rw".to_string()));
    }

    #[test]
    fn test_build_container_config_tmpfs() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.tmpfs_mounts.push("/run/secrets".to_string());

        let params = PodmanRuntime::build_container_config(&config);
        assert!(params.tmpfs.contains_key("/run/secrets"));
    }

    #[test]
    fn test_build_container_config_labels() {
        let config = ContainerConfig::new("my-app", "test-inst", PODMAN_IMAGE);
        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(
            params.labels.get("coast.project"),
            Some(&"my-app".to_string())
        );
        assert_eq!(
            params.labels.get("coast.instance"),
            Some(&"test-inst".to_string())
        );
    }

    #[test]
    fn test_build_container_config_empty() {
        let config = ContainerConfig::new("proj", "inst", PODMAN_IMAGE);
        let params = PodmanRuntime::build_container_config(&config);
        assert!(params.env.is_empty());
        assert!(params.binds.is_empty());
        assert!(params.tmpfs.is_empty());
        assert!(params.working_dir.is_none());
    }

    #[test]
    fn test_build_container_config_networks() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.networks.push("coast-shared-my-app".to_string());

        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(params.networks, vec!["coast-shared-my-app"]);
    }

    #[test]
    fn test_podman_image_constant() {
        assert_eq!(PODMAN_IMAGE, "quay.io/podman/stable:latest");
    }

    #[test]
    fn test_build_container_config_working_dir() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.working_dir = Some("/workspace".to_string());

        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(params.working_dir, Some("/workspace".to_string()));
    }

    #[test]
    fn test_build_container_config_entrypoint_and_cmd() {
        let mut config = ContainerConfig::new("my-app", "test", PODMAN_IMAGE);
        config.entrypoint = Some(vec!["/bin/sh".to_string()]);
        config.cmd = Some(vec!["-c".to_string(), "sleep infinity".to_string()]);

        let params = PodmanRuntime::build_container_config(&config);
        assert_eq!(params.entrypoint, Some(vec!["/bin/sh".to_string()]));
        assert_eq!(
            params.cmd,
            Some(vec!["-c".to_string(), "sleep infinity".to_string()])
        );
    }

    #[test]
    fn test_check_podman_not_found() {
        // This test documents the expected error when podman is not installed.
        // It may succeed or fail depending on the test environment.
        // We only verify the error shape if it does fail.
        if let Err(e) = check_podman_available() {
            let err_str = e.to_string();
            assert!(err_str.contains("podman"));
        }
    }

    #[test]
    fn test_check_podman_compose_not_found() {
        // Similar to above - documents expected error shape.
        if let Err(e) = check_podman_compose_available() {
            let err_str = e.to_string();
            assert!(err_str.contains("podman-compose"));
        }
    }
}
