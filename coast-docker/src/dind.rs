/// Docker-in-Docker (DinD) runtime implementation.
///
/// Creates coast containers using the `docker:dind` image with `--privileged` mode.
/// Each coast container runs its own Docker daemon, inside which the user's
/// `docker-compose.yml` runs unmodified.
use std::collections::HashMap;
use std::net::IpAddr;

use async_trait::async_trait;
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use bollard::exec::{CreateExecOptions, StartExecOptions};
use bollard::image::CreateImageOptions;
use bollard::Docker;
use futures_util::StreamExt;
use tracing::{debug, info};

use coast_core::error::{CoastError, Result};

use crate::runtime::{BindMount, ContainerConfig, ExecResult, Runtime, VolumeMount};

/// The default Docker image used for DinD coast containers.
pub const DIND_IMAGE: &str = "docker:dind";

/// Docker-in-Docker runtime.
///
/// Runs coast containers with `--privileged` flag, using the `docker:dind`
/// image. The inner Docker daemon starts automatically as the container's
/// entrypoint.
pub struct DindRuntime {
    /// Bollard Docker client connected to the host daemon.
    docker: Docker,
}

impl DindRuntime {
    /// Create a new DinD runtime connected to the default Docker socket.
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults().map_err(|e| CoastError::Docker {
            message: format!("Failed to connect to Docker daemon. Is Docker running? Error: {e}"),
            source: Some(Box::new(e)),
        })?;
        Ok(Self { docker })
    }

    /// Create a new DinD runtime with an existing Docker client.
    ///
    /// Useful for testing with a custom Docker connection.
    pub fn with_client(docker: Docker) -> Self {
        Self { docker }
    }

    /// Ensure a Docker image is available locally, pulling it if not found.
    #[allow(clippy::cognitive_complexity)]
    async fn ensure_image(&self, image: &str) -> Result<()> {
        // Check if the image already exists locally
        if self.docker.inspect_image(image).await.is_ok() {
            debug!(image = %image, "Image already available locally");
            return Ok(());
        }

        info!(image = %image, "Pulling image (not found locally)");

        // Parse image into repo and tag
        let (repo, tag) = if let Some((r, t)) = image.rsplit_once(':') {
            (r.to_string(), t.to_string())
        } else {
            (image.to_string(), "latest".to_string())
        };

        let options = CreateImageOptions {
            from_image: repo,
            tag,
            ..Default::default()
        };

        let mut stream = self.docker.create_image(Some(options), None, None);
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(ref status) = info.status {
                        debug!(status = %status, "pull progress");
                    }
                }
                Err(e) => {
                    return Err(CoastError::Docker {
                        message: format!(
                            "Failed to pull image '{image}'. \
                             Ensure you have network access and the image name is correct. \
                             Error: {e}"
                        ),
                        source: Some(Box::new(e)),
                    });
                }
            }
        }

        info!(image = %image, "Image pulled successfully");
        Ok(())
    }

    /// Build the bollard container configuration from a `ContainerConfig`.
    ///
    /// This is a pure function that translates our config into Docker API types.
    /// It does not make any Docker API calls, making it suitable for unit testing.
    pub fn build_container_config(config: &ContainerConfig) -> ContainerCreateParams {
        let container_name = config.container_name();

        // Build environment variables
        let env: Vec<String> = config
            .env_vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let mut binds: Vec<String> = Vec::new();
        let mut mounts: Vec<bollard::models::Mount> = Vec::new();
        for mount in &config.bind_mounts {
            if let Some(ref prop) = mount.propagation {
                mounts.push(bollard::models::Mount {
                    target: Some(mount.container_path.clone()),
                    source: Some(mount.host_path.to_string_lossy().to_string()),
                    typ: Some(bollard::models::MountTypeEnum::BIND),
                    read_only: Some(mount.read_only),
                    bind_options: Some(bollard::models::MountBindOptions {
                        propagation: Some(
                            prop.parse().unwrap_or(
                                bollard::models::MountBindOptionsPropagationEnum::RPRIVATE,
                            ),
                        ),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            } else {
                let mode = if mount.read_only { "ro" } else { "rw" };
                binds.push(format!(
                    "{}:{}:{mode}",
                    mount.host_path.display(),
                    mount.container_path
                ));
            }
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

        // Build labels
        let labels = config.labels.clone();

        ContainerCreateParams {
            name: container_name,
            image: config.image.clone(),
            env,
            binds,
            mounts,
            tmpfs,
            labels,
            privileged: true,
            working_dir: config.working_dir.clone(),
            entrypoint: config.entrypoint.clone(),
            cmd: config.cmd.clone(),
            networks: config.networks.clone(),
            published_ports: config.published_ports.clone(),
            extra_hosts: config.extra_hosts.clone(),
        }
    }
}

/// Parameters for creating a container, extracted for testability.
///
/// This struct holds the final Docker API parameters after translation
/// from `ContainerConfig`. It can be inspected in unit tests without
/// making actual Docker API calls.
#[derive(Debug, Clone)]
pub struct ContainerCreateParams {
    /// Container name.
    pub name: String,
    /// Docker image.
    pub image: String,
    /// Environment variables in "KEY=VALUE" format.
    pub env: Vec<String>,
    /// Bind mounts in "host:container:mode" format (for mounts without propagation).
    pub binds: Vec<String>,
    /// Structured mounts with propagation settings (uses bollard's Mount API).
    pub mounts: Vec<bollard::models::Mount>,
    /// Tmpfs mounts as path -> options.
    pub tmpfs: HashMap<String, String>,
    /// Container labels.
    pub labels: HashMap<String, String>,
    /// Whether the container runs in privileged mode.
    pub privileged: bool,
    /// Working directory override.
    pub working_dir: Option<String>,
    /// Entrypoint override.
    pub entrypoint: Option<Vec<String>>,
    /// Command arguments.
    pub cmd: Option<Vec<String>>,
    /// Networks to connect to.
    pub networks: Vec<String>,
    /// Ports to publish (host_port -> container_port).
    pub published_ports: Vec<crate::runtime::PortPublish>,
    /// Extra /etc/hosts entries ("hostname:ip").
    pub extra_hosts: Vec<String>,
}

#[async_trait]
impl Runtime for DindRuntime {
    fn name(&self) -> &str {
        "dind"
    }

    async fn create_coast_container(&self, config: &ContainerConfig) -> Result<String> {
        let params = Self::build_container_config(config);

        info!(
            container_name = %params.name,
            image = %params.image,
            "Creating DinD coast container"
        );

        // Ensure the image is available locally, pulling it if necessary.
        self.ensure_image(&params.image).await?;

        // Build port bindings for published ports
        let port_bindings = if params.published_ports.is_empty() {
            None
        } else {
            let mut bindings: HashMap<String, Option<Vec<bollard::models::PortBinding>>> =
                HashMap::new();
            for pp in &params.published_ports {
                let key = format!("{}/tcp", pp.container_port);
                bindings.insert(
                    key,
                    Some(vec![bollard::models::PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        host_port: Some(pp.host_port.to_string()),
                    }]),
                );
            }
            Some(bindings)
        };

        let host_config = bollard::models::HostConfig {
            privileged: Some(true),
            binds: if params.binds.is_empty() {
                None
            } else {
                Some(params.binds)
            },
            mounts: if params.mounts.is_empty() {
                None
            } else {
                Some(params.mounts)
            },
            tmpfs: if params.tmpfs.is_empty() {
                None
            } else {
                Some(params.tmpfs)
            },
            port_bindings,
            extra_hosts: if params.extra_hosts.is_empty() {
                None
            } else {
                Some(params.extra_hosts)
            },
            ..Default::default()
        };

        // Build exposed ports
        let exposed_ports = if params.published_ports.is_empty() {
            None
        } else {
            let mut exposed: HashMap<String, HashMap<(), ()>> = HashMap::new();
            for pp in &params.published_ports {
                exposed.insert(format!("{}/tcp", pp.container_port), HashMap::new());
            }
            Some(exposed)
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
            exposed_ports,
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
                    "Failed to create coast container '{}'. Error: {e}",
                    params.name
                ),
                source: Some(Box::new(e)),
            })?;

        info!(
            container_id = %response.id,
            container_name = %params.name,
            "DinD coast container created"
        );

        Ok(response.id)
    }

    async fn start_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Starting DinD coast container");

        self.docker
            .start_container::<String>(container_id, None)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to start coast container '{container_id}'. \
                     Is Docker running? Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "DinD coast container started");
        Ok(())
    }

    async fn stop_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Stopping DinD coast container");

        self.docker
            .stop_container(container_id, None)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!("Failed to stop coast container '{container_id}'. Error: {e}"),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "DinD coast container stopped");
        Ok(())
    }

    async fn remove_coast_container(&self, container_id: &str) -> Result<()> {
        debug!(container_id = %container_id, "Removing DinD coast container");

        let options = RemoveContainerOptions {
            force: true,
            v: false,
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await
            .map_err(|e| CoastError::Docker {
                message: format!("Failed to remove coast container '{container_id}'. Error: {e}"),
                source: Some(Box::new(e)),
            })?;

        info!(container_id = %container_id, "DinD coast container removed");
        Ok(())
    }

    async fn exec_in_coast(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult> {
        debug!(
            container_id = %container_id,
            cmd = ?cmd,
            "Executing command in DinD coast container"
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
                message: format!("Failed to create exec in container '{container_id}'. Error: {e}"),
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
                message: format!("Failed to start exec in container '{container_id}'. Error: {e}"),
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
                    "Failed to inspect container '{container_id}' for IP address. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        let network_settings = inspect.network_settings.ok_or_else(|| {
            CoastError::docker(format!(
                "Container '{container_id}' has no network settings. Is it running?"
            ))
        })?;

        let ip_str = network_settings
            .ip_address
            .as_deref()
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| {
                CoastError::docker(format!(
                    "Container '{container_id}' has no IP address. \
                     Is it running and connected to a network?"
                ))
            })?;

        ip_str.parse().map_err(|e| CoastError::Docker {
            message: format!(
                "Container '{container_id}' has invalid IP address '{ip_str}'. Error: {e}"
            ),
            source: None,
        })
    }

    fn requires_privileged(&self) -> bool {
        true
    }
}

/// Build a `ContainerConfig` for a DinD coast instance.
///
/// This is a helper function that creates a properly configured
/// `ContainerConfig` for the DinD runtime, including all the
/// standard bind mounts, environment variables, and labels.
///
/// If `coast_image` is provided (from `[coast.setup]` in the Coastfile),
/// it will be used instead of the default `docker:dind` image.
#[allow(clippy::too_many_arguments)]
pub fn build_dind_config(
    project: &str,
    instance_name: &str,
    code_path: &std::path::Path,
    env_vars: HashMap<String, String>,
    bind_mounts: Vec<BindMount>,
    volume_mounts: Vec<VolumeMount>,
    tmpfs_mounts: Vec<String>,
    image_cache_path: Option<&std::path::Path>,
    artifact_dir: Option<&std::path::Path>,
    coast_image: Option<&str>,
    override_dir: Option<&std::path::Path>,
    extra_hosts: Vec<String>,
) -> ContainerConfig {
    let image = coast_image.unwrap_or(DIND_IMAGE);
    let mut config = ContainerConfig::new(project, instance_name, image);
    config.env_vars = env_vars;
    config.bind_mounts = bind_mounts;
    config.volume_mounts = volume_mounts;
    config.tmpfs_mounts = tmpfs_mounts;
    config.extra_hosts = extra_hosts;

    // Bind-mount the project root at /host-project. The run/start handlers
    // create a switchable `mount --bind /host-project /workspace` (or a
    // worktree subdirectory) inside the container after it starts.
    config.bind_mounts.push(BindMount {
        host_path: code_path.to_path_buf(),
        container_path: "/host-project".to_string(),
        read_only: false,
        propagation: None,
    });

    // Mount image cache read-only if available
    if let Some(cache_path) = image_cache_path {
        config.bind_mounts.push(BindMount {
            host_path: cache_path.to_path_buf(),
            container_path: "/image-cache".to_string(),
            read_only: true,
            propagation: None,
        });
    }

    // Mount artifact directory read-only if available (contains rewritten compose, etc.)
    if let Some(artifact_path) = artifact_dir {
        config.bind_mounts.push(BindMount {
            host_path: artifact_path.to_path_buf(),
            container_path: "/coast-artifact".to_string(),
            read_only: true,
            propagation: None,
        });
    }

    // Mount compose override directory if available (written to ~/.coast/overrides/)
    if let Some(ovr_path) = override_dir {
        config.bind_mounts.push(BindMount {
            host_path: ovr_path.to_path_buf(),
            container_path: "/coast-override".to_string(),
            read_only: true,
            propagation: None,
        });
    }

    // Persist the inner Docker daemon's /var/lib/docker in a named volume.
    // This means cached images, build layers, and daemon state survive
    // container removal (coast rm + coast run), dramatically speeding up
    // subsequent runs for the same instance name (~21s → ~8s) by avoiding
    // the expensive `docker load` of OCI tarballs into the inner daemon.
    let dind_volume_name = dind_cache_volume_name(project, instance_name);
    config.volume_mounts.push(VolumeMount {
        volume_name: dind_volume_name,
        container_path: "/var/lib/docker".to_string(),
        read_only: false,
    });

    // Set the working directory to the workspace
    config.working_dir = Some("/workspace".to_string());

    // Add DinD-specific environment
    config
        .env_vars
        .insert("DOCKER_TLS_CERTDIR".to_string(), String::new());

    config
}

/// Generate the Docker volume name used to persist the inner daemon's
/// `/var/lib/docker` directory across container recreations.
///
/// Naming convention: `coast-dind--{project}--{instance}`
pub fn dind_cache_volume_name(project: &str, instance: &str) -> String {
    format!("coast-dind--{project}--{instance}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_container_config_name() {
        let config = ContainerConfig::new("my-app", "feature-oauth", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.name, "my-app-coasts-feature-oauth");
    }

    #[test]
    fn test_build_container_config_image() {
        let config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.image, DIND_IMAGE);
    }

    #[test]
    fn test_build_container_config_privileged() {
        let config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert!(params.privileged);
    }

    #[test]
    fn test_build_container_config_env_vars() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config
            .env_vars
            .insert("PGPASSWORD".to_string(), "secret".to_string());
        config
            .env_vars
            .insert("NODE_ENV".to_string(), "development".to_string());

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.env.len(), 2);
        assert!(params.env.contains(&"PGPASSWORD=secret".to_string()));
        assert!(params.env.contains(&"NODE_ENV=development".to_string()));
    }

    #[test]
    fn test_build_container_config_bind_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.bind_mounts.push(BindMount {
            host_path: PathBuf::from("/home/user/project"),
            container_path: "/workspace".to_string(),
            read_only: false,
            propagation: None,
        });
        config.bind_mounts.push(BindMount {
            host_path: PathBuf::from("/home/user/.coast/image-cache"),
            container_path: "/image-cache".to_string(),
            read_only: true,
            propagation: None,
        });

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.binds.len(), 2);
        assert!(params
            .binds
            .contains(&"/home/user/project:/workspace:rw".to_string()));
        assert!(params
            .binds
            .contains(&"/home/user/.coast/image-cache:/image-cache:ro".to_string()));
    }

    #[test]
    fn test_build_container_config_volume_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.volume_mounts.push(VolumeMount {
            volume_name: "coast--test--pg_data".to_string(),
            container_path: "/volumes/pg_data".to_string(),
            read_only: false,
        });

        let params = DindRuntime::build_container_config(&config);
        assert!(params
            .binds
            .contains(&"coast--test--pg_data:/volumes/pg_data:rw".to_string()));
    }

    #[test]
    fn test_build_container_config_tmpfs() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.tmpfs_mounts.push("/run/secrets".to_string());

        let params = DindRuntime::build_container_config(&config);
        assert!(params.tmpfs.contains_key("/run/secrets"));
        assert_eq!(
            params.tmpfs.get("/run/secrets").unwrap(),
            "rw,noexec,nosuid,size=64m"
        );
    }

    #[test]
    fn test_build_container_config_labels() {
        let config = ContainerConfig::new("my-app", "feature-x", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert_eq!(
            params.labels.get("coast.project"),
            Some(&"my-app".to_string())
        );
        assert_eq!(
            params.labels.get("coast.instance"),
            Some(&"feature-x".to_string())
        );
        assert_eq!(
            params.labels.get("coast.managed"),
            Some(&"true".to_string())
        );
        assert_eq!(
            params.labels.get("com.docker.compose.project"),
            Some(&"my-app-coasts".to_string())
        );
        assert_eq!(
            params.labels.get("com.docker.compose.service"),
            Some(&"feature-x".to_string())
        );
        assert_eq!(
            params.labels.get("com.docker.compose.container-number"),
            Some(&"1".to_string())
        );
        assert_eq!(
            params.labels.get("com.docker.compose.oneoff"),
            Some(&"False".to_string())
        );
    }

    #[test]
    fn test_build_container_config_working_dir() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.working_dir = Some("/workspace".to_string());

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.working_dir, Some("/workspace".to_string()));
    }

    #[test]
    fn test_build_container_config_entrypoint() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.entrypoint = Some(vec!["dockerd-entrypoint.sh".to_string()]);

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(
            params.entrypoint,
            Some(vec!["dockerd-entrypoint.sh".to_string()])
        );
    }

    #[test]
    fn test_build_container_config_cmd() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.cmd = Some(vec!["--storage-driver".to_string(), "overlay2".to_string()]);

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(
            params.cmd,
            Some(vec!["--storage-driver".to_string(), "overlay2".to_string()])
        );
    }

    #[test]
    fn test_build_container_config_empty() {
        let config = ContainerConfig::new("proj", "inst", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert!(params.env.is_empty());
        assert!(params.binds.is_empty());
        assert!(params.tmpfs.is_empty());
        assert!(params.working_dir.is_none());
        assert!(params.entrypoint.is_none());
        assert!(params.cmd.is_none());
    }

    #[test]
    fn test_build_container_config_networks() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.networks.push("coast-shared-my-app".to_string());

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.networks, vec!["coast-shared-my-app"]);
    }

    #[test]
    fn test_build_dind_config_basic() {
        let code_path = PathBuf::from("/home/user/project");
        let config = build_dind_config(
            "my-app",
            "feature-oauth",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            Vec::new(),
        );

        assert_eq!(config.project, "my-app");
        assert_eq!(config.instance_name, "feature-oauth");
        assert_eq!(config.image, DIND_IMAGE);
        assert_eq!(config.working_dir, Some("/workspace".to_string()));

        // Code directory should be bind-mounted at /host-project (the run/start
        // handlers create a switchable `mount --bind` to /workspace after boot).
        assert_eq!(config.bind_mounts.len(), 1);
        assert_eq!(
            config.bind_mounts[0].host_path,
            PathBuf::from("/home/user/project")
        );
        assert_eq!(config.bind_mounts[0].container_path, "/host-project");
        assert!(!config.bind_mounts[0].read_only);

        // DinD cache volume should be mounted at /var/lib/docker
        assert_eq!(config.volume_mounts.len(), 1);
        assert_eq!(
            config.volume_mounts[0].volume_name,
            "coast-dind--my-app--feature-oauth"
        );
        assert_eq!(config.volume_mounts[0].container_path, "/var/lib/docker");
        assert!(!config.volume_mounts[0].read_only);

        // DOCKER_TLS_CERTDIR should be set to empty
        assert_eq!(
            config.env_vars.get("DOCKER_TLS_CERTDIR"),
            Some(&String::new())
        );
    }

    #[test]
    fn test_build_dind_config_with_image_cache() {
        let code_path = PathBuf::from("/home/user/project");
        let cache_path = PathBuf::from("/home/user/.coast/image-cache");
        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(&cache_path),
            None,
            None,
            None,
            Vec::new(),
        );

        // Should have both code and cache bind mounts
        assert_eq!(config.bind_mounts.len(), 2);

        let cache_mount = config
            .bind_mounts
            .iter()
            .find(|m| m.container_path == "/image-cache")
            .expect("image cache mount not found");
        assert_eq!(
            cache_mount.host_path,
            PathBuf::from("/home/user/.coast/image-cache")
        );
        assert!(cache_mount.read_only);
    }

    #[test]
    fn test_build_dind_config_with_artifact_dir() {
        let code_path = PathBuf::from("/home/user/project");
        let artifact_path = PathBuf::from("/home/user/.coast/images/my-app");
        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Some(&artifact_path),
            None,
            None,
            Vec::new(),
        );

        // Should have code + artifact bind mounts
        assert_eq!(config.bind_mounts.len(), 2);

        let artifact_mount = config
            .bind_mounts
            .iter()
            .find(|m| m.container_path == "/coast-artifact")
            .expect("artifact mount not found");
        assert_eq!(
            artifact_mount.host_path,
            PathBuf::from("/home/user/.coast/images/my-app")
        );
        assert!(artifact_mount.read_only);
    }

    #[test]
    fn test_build_dind_config_with_env_and_mounts() {
        let code_path = PathBuf::from("/home/user/project");
        let mut env = HashMap::new();
        env.insert("PGPASSWORD".to_string(), "secret".to_string());

        let bind_mounts = vec![BindMount {
            host_path: PathBuf::from("/home/user/.ssh"),
            container_path: "/root/.ssh".to_string(),
            read_only: true,
            propagation: None,
        }];

        let volume_mounts = vec![VolumeMount {
            volume_name: "coast--test--pg".to_string(),
            container_path: "/volumes/pg".to_string(),
            read_only: false,
        }];

        let tmpfs = vec!["/run/secrets".to_string()];

        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            env,
            bind_mounts,
            volume_mounts,
            tmpfs,
            None,
            None,
            None,
            None,
            Vec::new(),
        );

        // Env should include both user env and DOCKER_TLS_CERTDIR
        assert_eq!(
            config.env_vars.get("PGPASSWORD"),
            Some(&"secret".to_string())
        );
        assert!(config.env_vars.contains_key("DOCKER_TLS_CERTDIR"));

        // Bind mounts: user mount + code mount
        assert_eq!(config.bind_mounts.len(), 2);

        // Volume mounts: user mount + DinD cache
        assert_eq!(config.volume_mounts.len(), 2);
        assert_eq!(config.volume_mounts[0].volume_name, "coast--test--pg");
        assert_eq!(
            config.volume_mounts[1].volume_name,
            "coast-dind--my-app--test"
        );
        assert_eq!(config.volume_mounts[1].container_path, "/var/lib/docker");

        // Tmpfs
        assert_eq!(config.tmpfs_mounts.len(), 1);
        assert_eq!(config.tmpfs_mounts[0], "/run/secrets");
    }

    #[test]
    fn test_dind_image_constant() {
        assert_eq!(DIND_IMAGE, "docker:dind");
    }

    #[test]
    fn test_build_container_config_multiple_volume_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.volume_mounts.push(VolumeMount {
            volume_name: "vol1".to_string(),
            container_path: "/data1".to_string(),
            read_only: false,
        });
        config.volume_mounts.push(VolumeMount {
            volume_name: "vol2".to_string(),
            container_path: "/data2".to_string(),
            read_only: true,
        });

        let params = DindRuntime::build_container_config(&config);
        assert!(params.binds.contains(&"vol1:/data1:rw".to_string()));
        assert!(params.binds.contains(&"vol2:/data2:ro".to_string()));
    }

    #[test]
    fn test_build_container_config_mixed_mounts() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.bind_mounts.push(BindMount {
            host_path: PathBuf::from("/host/path"),
            container_path: "/container/path".to_string(),
            read_only: false,
            propagation: None,
        });
        config.volume_mounts.push(VolumeMount {
            volume_name: "my-volume".to_string(),
            container_path: "/vol/path".to_string(),
            read_only: false,
        });

        let params = DindRuntime::build_container_config(&config);
        // Both bind and volume mounts should be in the binds array
        assert_eq!(params.binds.len(), 2);
        assert!(params
            .binds
            .contains(&"/host/path:/container/path:rw".to_string()));
        assert!(params.binds.contains(&"my-volume:/vol/path:rw".to_string()));
    }

    #[test]
    fn test_build_container_config_published_ports() {
        use crate::runtime::PortPublish;
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config.published_ports.push(PortPublish {
            host_port: 59000,
            container_port: 3000,
        });
        config.published_ports.push(PortPublish {
            host_port: 60000,
            container_port: 5432,
        });

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.published_ports.len(), 2);
        assert_eq!(params.published_ports[0].host_port, 59000);
        assert_eq!(params.published_ports[0].container_port, 3000);
        assert_eq!(params.published_ports[1].host_port, 60000);
        assert_eq!(params.published_ports[1].container_port, 5432);
    }

    #[test]
    fn test_build_container_config_no_published_ports() {
        let config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        let params = DindRuntime::build_container_config(&config);
        assert!(params.published_ports.is_empty());
    }

    #[test]
    fn test_build_dind_config_with_custom_coast_image() {
        let code_path = PathBuf::from("/home/user/project");
        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            Some("coast-image/my-app:latest"),
            None,
            Vec::new(),
        );

        assert_eq!(config.image, "coast-image/my-app:latest");
    }

    #[test]
    fn test_build_dind_config_without_custom_coast_image() {
        let code_path = PathBuf::from("/home/user/project");
        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            Vec::new(),
        );

        assert_eq!(config.image, DIND_IMAGE);
    }

    #[test]
    fn test_build_dind_config_published_ports_passthrough() {
        use crate::runtime::PortPublish;
        let code_path = PathBuf::from("/home/user/project");
        let mut config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            Vec::new(),
        );

        // Simulate what run.rs does: add published ports after build_dind_config
        config.published_ports.push(PortPublish {
            host_port: 59000,
            container_port: 3000,
        });

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.published_ports.len(), 1);
        assert_eq!(params.published_ports[0].host_port, 59000);
        assert_eq!(params.published_ports[0].container_port, 3000);
    }

    #[test]
    fn test_build_container_config_extra_hosts() {
        let mut config = ContainerConfig::new("my-app", "test", DIND_IMAGE);
        config
            .extra_hosts
            .push("host.docker.internal:host-gateway".to_string());

        let params = DindRuntime::build_container_config(&config);
        assert_eq!(params.extra_hosts.len(), 1);
        assert_eq!(params.extra_hosts[0], "host.docker.internal:host-gateway");
    }

    #[test]
    fn test_build_dind_config_with_extra_hosts() {
        let code_path = PathBuf::from("/home/user/project");
        let config = build_dind_config(
            "my-app",
            "test",
            &code_path,
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            None,
            vec!["host.docker.internal:host-gateway".to_string()],
        );

        assert_eq!(config.extra_hosts.len(), 1);
        assert_eq!(config.extra_hosts[0], "host.docker.internal:host-gateway");
    }

    #[test]
    fn test_dind_cache_volume_name() {
        assert_eq!(
            dind_cache_volume_name("my-app", "feature-oauth"),
            "coast-dind--my-app--feature-oauth"
        );
        assert_eq!(
            dind_cache_volume_name("coast-benchmark", "feat-1"),
            "coast-dind--coast-benchmark--feat-1"
        );
    }

    fn build_test_config() -> ContainerConfig {
        build_dind_config(
            "my-app",
            "test",
            &PathBuf::from("/home/user/project"),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(&PathBuf::from("/home/user/.coast/image-cache")),
            None,
            None,
            None,
            Vec::new(),
        )
    }

    #[test]
    fn test_host_project_mount_no_docker_propagation() {
        let config = build_test_config();
        let host_project = config
            .bind_mounts
            .iter()
            .find(|m| m.container_path == "/host-project")
            .expect("should have /host-project mount");
        assert_eq!(
            host_project.propagation, None,
            "host-project uses default propagation (rshared is set inside the container via mount --make-rshared)"
        );
    }

    #[test]
    fn test_propagation_mount_uses_mounts_api() {
        let mut config = build_test_config();
        config.bind_mounts.push(BindMount {
            host_path: PathBuf::from("/test/shared-path"),
            container_path: "/shared-mount".to_string(),
            read_only: false,
            propagation: Some("rshared".into()),
        });
        let params = DindRuntime::build_container_config(&config);
        assert!(
            !params.binds.iter().any(|b| b.contains("/shared-mount")),
            "propagation mounts should not appear in binds"
        );
        assert!(
            params
                .mounts
                .iter()
                .any(|m| m.target.as_deref() == Some("/shared-mount")),
            "propagation mounts should appear in mounts"
        );
        let sm = params
            .mounts
            .iter()
            .find(|m| m.target.as_deref() == Some("/shared-mount"))
            .unwrap();
        assert_eq!(sm.typ, Some(bollard::models::MountTypeEnum::BIND));
        let bind_opts = sm.bind_options.as_ref().unwrap();
        assert_eq!(
            bind_opts.propagation,
            Some(bollard::models::MountBindOptionsPropagationEnum::RSHARED)
        );
    }

    #[test]
    fn test_non_propagation_mounts_use_binds() {
        let config = build_test_config();
        let params = DindRuntime::build_container_config(&config);
        assert!(
            params.binds.iter().any(|b| b.contains("/image-cache")),
            "non-propagation mounts should appear in binds"
        );
        assert!(
            !params
                .mounts
                .iter()
                .any(|m| m.target.as_deref() == Some("/image-cache")),
            "non-propagation mounts should not appear in mounts"
        );
    }
}
