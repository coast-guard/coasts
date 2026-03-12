/// Docker bridge network management for shared services.
///
/// Manages the Docker bridge networks that connect coast containers
/// to shared services running on the host Docker daemon. Each project
/// gets its own bridge network named `coast-shared-{project}`.
use std::collections::HashMap;

use bollard::network::{CreateNetworkOptions, ListNetworksOptions};
use bollard::Docker;
use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};

use crate::host::connect_to_host_docker;

/// Prefix for coast shared network names.
pub const NETWORK_PREFIX: &str = "coast-shared-";

/// Label key for identifying coast-managed networks.
pub const COAST_NETWORK_LABEL: &str = "coast.network";

/// Generate the network name for a project's shared services.
///
/// Format: `coast-shared-{project}`
pub fn shared_network_name(project: &str) -> String {
    format!("{NETWORK_PREFIX}{project}")
}

/// Manager for Docker bridge networks used by shared services.
///
/// Creates and manages the bridge network that connects coast containers
/// to shared services (e.g., shared postgres) running on the host daemon.
pub struct NetworkManager {
    /// Bollard Docker client.
    docker: Docker,
}

impl NetworkManager {
    /// Create a new network manager connected to the default Docker socket.
    pub fn new() -> Result<Self> {
        let docker = connect_to_host_docker()?;
        Ok(Self { docker })
    }

    /// Create a new network manager with an existing Docker client.
    pub fn with_client(docker: Docker) -> Self {
        Self { docker }
    }

    /// Create a shared bridge network for a project if it doesn't exist.
    ///
    /// Returns the network name. If the network already exists, this is a no-op.
    pub async fn create_shared_network(&self, project: &str) -> Result<String> {
        let network_name = shared_network_name(project);

        // Check if network already exists
        if self.network_exists(&network_name).await? {
            debug!(network = %network_name, "Shared network already exists");
            return Ok(network_name);
        }

        info!(network = %network_name, project = %project, "Creating shared bridge network");

        let mut labels = HashMap::new();
        labels.insert(COAST_NETWORK_LABEL.to_string(), project.to_string());
        labels.insert("coast.managed".to_string(), "true".to_string());

        let options = CreateNetworkOptions {
            name: network_name.clone(),
            driver: "bridge".to_string(),
            labels,
            ..Default::default()
        };

        self.docker
            .create_network(options)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to create shared network '{network_name}' for project '{project}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(network = %network_name, "Shared bridge network created");
        Ok(network_name)
    }

    /// Connect a container to the shared network.
    pub async fn connect_container(&self, network_name: &str, container_id: &str) -> Result<()> {
        debug!(
            network = %network_name,
            container = %container_id,
            "Connecting container to shared network"
        );

        let connect_config = bollard::network::ConnectNetworkOptions {
            container: container_id.to_string(),
            ..Default::default()
        };

        self.docker
            .connect_network(network_name, connect_config)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to connect container '{container_id}' to network '{network_name}'. Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(
            network = %network_name,
            container = %container_id,
            "Container connected to shared network"
        );
        Ok(())
    }

    /// Disconnect a container from the shared network.
    pub async fn disconnect_container(&self, network_name: &str, container_id: &str) -> Result<()> {
        debug!(
            network = %network_name,
            container = %container_id,
            "Disconnecting container from shared network"
        );

        let disconnect_config = bollard::network::DisconnectNetworkOptions {
            container: container_id.to_string(),
            force: false,
        };

        self.docker
            .disconnect_network(network_name, disconnect_config)
            .await
            .map_err(|e| {
                // Not an error if container is not connected
                warn!(
                    network = %network_name,
                    container = %container_id,
                    error = %e,
                    "Failed to disconnect container from network (may already be disconnected)"
                );
                CoastError::Docker {
                    message: format!(
                        "Failed to disconnect container '{container_id}' from network '{network_name}'. Error: {e}"
                    ),
                    source: Some(Box::new(e)),
                }
            })?;

        info!(
            network = %network_name,
            container = %container_id,
            "Container disconnected from shared network"
        );
        Ok(())
    }

    /// Remove a shared network.
    ///
    /// All containers must be disconnected first. Typically called when
    /// removing the last shared service for a project.
    pub async fn remove_network(&self, network_name: &str) -> Result<()> {
        info!(network = %network_name, "Removing shared network");

        self.docker
            .remove_network(network_name)
            .await
            .map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to remove network '{network_name}'. \
                     Are all containers disconnected? Error: {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        info!(network = %network_name, "Shared network removed");
        Ok(())
    }

    /// Check whether a network exists.
    async fn network_exists(&self, network_name: &str) -> Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("name", vec![network_name]);

        let options = ListNetworksOptions { filters };

        let networks = self
            .docker
            .list_networks(Some(options))
            .await
            .map_err(|e| CoastError::Docker {
                message: format!("Failed to list networks: {e}"),
                source: Some(Box::new(e)),
            })?;

        // Docker name filter is a prefix match, so verify exact match
        Ok(networks
            .iter()
            .any(|n| n.name.as_deref() == Some(network_name)))
    }

    /// List all coast-managed networks.
    pub async fn list_coast_networks(&self) -> Result<Vec<NetworkInfo>> {
        let mut filters = HashMap::new();
        filters.insert("label", vec![COAST_NETWORK_LABEL]);

        let options = ListNetworksOptions { filters };

        let networks = self
            .docker
            .list_networks(Some(options))
            .await
            .map_err(|e| CoastError::Docker {
                message: format!("Failed to list coast networks: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut result = Vec::new();
        for network in networks {
            let name = network.name.unwrap_or_default();
            let id = network.id.unwrap_or_default();
            let project = network
                .labels
                .as_ref()
                .and_then(|l| l.get(COAST_NETWORK_LABEL))
                .cloned()
                .unwrap_or_default();

            result.push(NetworkInfo { name, id, project });
        }

        Ok(result)
    }
}

/// Resolve the Docker bridge network's gateway IP (the host machine's IP
/// from the perspective of containers on the bridge network).
///
/// Inspects the default `bridge` network and returns the IPAM gateway address.
/// This is typically something like `172.17.0.1` on Linux or `192.168.65.1`
/// on Docker Desktop.
pub async fn resolve_bridge_gateway(docker: &Docker) -> Result<String> {
    let network = docker
        .inspect_network::<String>("bridge", None)
        .await
        .map_err(|e| CoastError::Docker {
            message: format!("Failed to inspect Docker bridge network: {e}"),
            source: Some(Box::new(e)),
        })?;

    let gateway = network
        .ipam
        .and_then(|ipam| ipam.config)
        .and_then(|configs| {
            let configs: Vec<_> = configs.into_iter().collect();
            // Prefer IPv4 (no colons) -- IPv6 gateway addresses are often
            // unreachable from inside nested DinD containers
            configs
                .iter()
                .find_map(|c| {
                    c.gateway
                        .as_ref()
                        .filter(|g| !g.is_empty() && !g.contains(':'))
                        .cloned()
                })
                .or_else(|| {
                    configs
                        .iter()
                        .find_map(|c| c.gateway.as_ref().filter(|g| !g.is_empty()).cloned())
                })
        })
        .ok_or_else(|| {
            CoastError::docker(
                "Docker bridge network has no IPAM gateway configured. \
                 Cannot determine host IP for egress.",
            )
        })?;

    debug!(gateway = %gateway, "Resolved Docker bridge gateway IP");
    Ok(gateway)
}

/// Information about a coast-managed network.
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    /// Network name.
    pub name: String,
    /// Network ID.
    pub id: String,
    /// Project this network belongs to.
    pub project: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_network_name() {
        assert_eq!(shared_network_name("my-app"), "coast-shared-my-app");
    }

    #[test]
    fn test_shared_network_name_with_hyphens() {
        assert_eq!(
            shared_network_name("my-cool-project"),
            "coast-shared-my-cool-project"
        );
    }

    #[test]
    fn test_shared_network_name_simple() {
        assert_eq!(shared_network_name("app"), "coast-shared-app");
    }

    #[test]
    fn test_network_prefix_constant() {
        assert_eq!(NETWORK_PREFIX, "coast-shared-");
    }

    #[test]
    fn test_coast_network_label_constant() {
        assert_eq!(COAST_NETWORK_LABEL, "coast.network");
    }

    #[test]
    fn test_network_info_fields() {
        let info = NetworkInfo {
            name: "coast-shared-my-app".to_string(),
            id: "abc123".to_string(),
            project: "my-app".to_string(),
        };
        assert_eq!(info.name, "coast-shared-my-app");
        assert_eq!(info.id, "abc123");
        assert_eq!(info.project, "my-app");
    }

    #[test]
    fn test_shared_network_name_underscores() {
        assert_eq!(shared_network_name("my_app"), "coast-shared-my_app");
    }

    #[test]
    fn test_shared_network_name_empty() {
        assert_eq!(shared_network_name(""), "coast-shared-");
    }

    #[test]
    fn test_shared_network_name_with_numbers() {
        assert_eq!(shared_network_name("app123"), "coast-shared-app123");
    }
}
