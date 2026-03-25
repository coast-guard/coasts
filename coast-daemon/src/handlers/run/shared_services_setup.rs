use std::collections::HashMap;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::types::{SharedServiceConfig, SharedServicePort};

use crate::server::AppState;

/// Result of starting shared services on the host daemon.
pub(super) struct SharedServicesResult {
    pub service_hosts: std::collections::HashMap<String, String>,
    pub network_name: Option<String>,
}

/// Start shared services on the host Docker daemon, recording them in the state DB.
///
/// For each shared service in the coastfile:
/// 1. Check if already running (skip if so)
/// 2. Remove any stale container
/// 3. Create and start the container
/// 4. Connect to the shared bridge network
/// 5. Record in the state DB
///
/// Returns the names, host mappings, and network name for use in compose overrides.
pub(super) async fn start_shared_services(
    project: &str,
    shared_service_configs: &[SharedServiceConfig],
    docker: &bollard::Docker,
    state: &AppState,
) -> Result<SharedServicesResult> {
    let mut service_hosts = HashMap::new();

    let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
    let network_name = nm.create_shared_network(project).await?;

    for svc_config in shared_service_configs {
        let container_name =
            crate::shared_services::shared_container_name(project, &svc_config.name);

        start_or_reuse_shared_service(
            project,
            svc_config,
            &container_name,
            &network_name,
            docker,
            state,
            &nm,
        )
        .await?;

        service_hosts.insert(svc_config.name.clone(), container_name);
    }

    Ok(SharedServicesResult {
        service_hosts,
        network_name: Some(network_name),
    })
}

async fn start_or_reuse_shared_service(
    project: &str,
    svc_config: &SharedServiceConfig,
    container_name: &str,
    network_name: &str,
    docker: &bollard::Docker,
    state: &AppState,
    network_manager: &coast_docker::network::NetworkManager,
) -> Result<()> {
    if shared_service_already_running(project, &svc_config.name, container_name, state, docker)
        .await?
    {
        info!(
            service = %svc_config.name,
            "shared service already running, skipping"
        );
        return Ok(());
    }

    recreate_shared_service(
        project,
        svc_config,
        container_name,
        network_name,
        docker,
        network_manager,
    )
    .await?;
    record_shared_service_running(project, &svc_config.name, container_name, state).await;

    info!(
        service = %svc_config.name,
        container = %container_name,
        "shared service started on host daemon"
    );

    Ok(())
}

async fn shared_service_already_running(
    project: &str,
    service_name: &str,
    container_name: &str,
    state: &AppState,
    docker: &bollard::Docker,
) -> Result<bool> {
    let existing = {
        let db = state.db.lock().await;
        db.get_shared_service(project, service_name)?
    };

    if !has_running_shared_service_record(existing.as_ref().map(|rec| rec.status.as_str())) {
        return Ok(false);
    }

    Ok(docker.inspect_container(container_name, None).await.is_ok())
}

fn has_running_shared_service_record(status: Option<&str>) -> bool {
    status == Some("running")
}

async fn recreate_shared_service(
    project: &str,
    svc_config: &SharedServiceConfig,
    container_name: &str,
    network_name: &str,
    docker: &bollard::Docker,
    network_manager: &coast_docker::network::NetworkManager,
) -> Result<()> {
    remove_existing_shared_service(container_name, docker).await;
    create_shared_service_container(project, svc_config, container_name, docker).await?;
    start_shared_service_container(svc_config, container_name, docker).await?;
    connect_shared_service_to_network(network_name, container_name, network_manager).await;
    Ok(())
}

async fn remove_existing_shared_service(container_name: &str, docker: &bollard::Docker) {
    let _ = docker.stop_container(container_name, None).await;
    let _ = docker.remove_container(container_name, None).await;
}

async fn create_shared_service_container(
    project: &str,
    svc_config: &SharedServiceConfig,
    container_name: &str,
    docker: &bollard::Docker,
) -> Result<()> {
    use futures_util::StreamExt;

    let image = &svc_config.image;
    if docker.inspect_image(image).await.is_err() {
        tracing::info!(image = %image, "Pulling shared service image (not found locally)");
        let mut stream = docker.create_image(
            Some(bollard::image::CreateImageOptions {
                from_image: image.as_str(),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(result) = stream.next().await {
            if let Err(e) = result {
                return Err(CoastError::docker(format!(
                    "Failed to pull shared service image '{}': {}",
                    image, e
                )));
            }
        }
        tracing::info!(image = %image, "Shared service image pulled successfully");
    }

    let shared_cfg = crate::shared_services::build_shared_container_config(project, svc_config);
    let host_config = bollard::models::HostConfig {
        binds: Some(shared_cfg.volumes.clone()),
        port_bindings: Some(build_port_bindings(&svc_config.ports)),
        restart_policy: Some(bollard::models::RestartPolicy {
            name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
            ..Default::default()
        }),
        ..Default::default()
    };

    let create_config = bollard::container::Config {
        image: Some(shared_cfg.image.clone()),
        env: Some(shared_cfg.env.clone()),
        host_config: Some(host_config),
        labels: Some(shared_cfg.labels.clone()),
        exposed_ports: Some(build_exposed_ports(&svc_config.ports)),
        ..Default::default()
    };

    let create_opts = bollard::container::CreateContainerOptions {
        name: container_name.to_string(),
        ..Default::default()
    };

    docker
        .create_container(Some(create_opts), create_config)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to create shared service container '{}': {}",
                container_name, e
            ))
        })?;

    Ok(())
}

fn build_port_bindings(
    ports: &[SharedServicePort],
) -> HashMap<String, Option<Vec<bollard::models::PortBinding>>> {
    let mut port_bindings = HashMap::new();
    for port in ports {
        port_bindings
            .entry(format!("{}/tcp", port.container_port))
            .or_insert_with(|| Some(Vec::new()))
            .get_or_insert_with(Vec::new)
            .push(bollard::models::PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port.host_port.to_string()),
            });
    }
    port_bindings
}

fn build_exposed_ports(ports: &[SharedServicePort]) -> HashMap<String, HashMap<(), ()>> {
    let mut exposed_ports = HashMap::new();
    for port in ports {
        exposed_ports.insert(format!("{}/tcp", port.container_port), HashMap::new());
    }
    exposed_ports
}

async fn start_shared_service_container(
    svc_config: &SharedServiceConfig,
    container_name: &str,
    docker: &bollard::Docker,
) -> Result<()> {
    docker
        .start_container::<String>(container_name, None)
        .await
        .map_err(|e| {
            let raw = e.to_string();
            if let Some(msg) = humanize_port_conflict(&raw, &svc_config.name, &svc_config.ports) {
                CoastError::docker(msg)
            } else {
                CoastError::docker(format!(
                    "Failed to start shared service container '{}': {}",
                    container_name, e
                ))
            }
        })?;

    Ok(())
}

async fn connect_shared_service_to_network(
    network_name: &str,
    container_name: &str,
    network_manager: &coast_docker::network::NetworkManager,
) {
    if let Err(e) = network_manager
        .connect_container(network_name, container_name)
        .await
    {
        tracing::warn!(
            error = %e,
            container = %container_name,
            "failed to connect shared service to network (may already be connected)"
        );
    }
}

async fn record_shared_service_running(
    project: &str,
    service_name: &str,
    container_name: &str,
    state: &AppState,
) {
    let db = state.db.lock().await;
    let _ = db.insert_shared_service(project, service_name, Some(container_name), "running");
}

/// Rewrite a raw Docker "port already allocated" error into a human-friendly message.
///
/// Docker errors look like:
///   "...Bind for 0.0.0.0:6379 failed: port is already allocated"
///
/// We extract the port number and tell the user what's actually wrong.
fn humanize_port_conflict(
    raw: &str,
    service_name: &str,
    declared_ports: &[SharedServicePort],
) -> Option<String> {
    if !raw.contains("port is already allocated") {
        return None;
    }

    let port = raw.find("Bind for ").and_then(|start| {
        let after = &raw[start + "Bind for ".len()..];
        let colon = after.find(':')?;
        let port_str = &after[colon + 1..];
        let end = port_str.find(' ')?;
        port_str[..end].parse::<u16>().ok()
    });

    let port_display = port.map(|p| p.to_string()).unwrap_or_else(|| {
        declared_ports
            .iter()
            .map(|port| port.host_port.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    });

    Some(format!(
        "Port {port_display} is already in use on the host.\n\n\
         The shared service '{service_name}' needs this port but another process is already \
         listening on it. This is usually caused by:\n\
         \n\
         - Another Docker container bound to the same port (check `docker ps`)\n\
         - A local service running on the host (check `lsof -iTCP:{port_display} -sTCP:LISTEN`)\n\
         - Another shared service in your Coastfile that declares the same port\n\
         \n\
         Stop the conflicting process and try again."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_running_shared_service_record_only_for_running_status() {
        assert!(has_running_shared_service_record(Some("running")));
        assert!(!has_running_shared_service_record(Some("stopped")));
        assert!(!has_running_shared_service_record(None));
    }

    #[test]
    fn test_build_port_bindings_maps_all_declared_ports() {
        let port_bindings =
            build_port_bindings(&[SharedServicePort::same(5432), SharedServicePort::same(6379)]);

        assert_eq!(port_bindings.len(), 2);
        let postgres = port_bindings
            .get("5432/tcp")
            .and_then(|bindings| bindings.as_ref())
            .and_then(|bindings| bindings.first())
            .unwrap();
        assert_eq!(postgres.host_ip.as_deref(), Some("0.0.0.0"));
        assert_eq!(postgres.host_port.as_deref(), Some("5432"));

        let redis = port_bindings
            .get("6379/tcp")
            .and_then(|bindings| bindings.as_ref())
            .and_then(|bindings| bindings.first())
            .unwrap();
        assert_eq!(redis.host_port.as_deref(), Some("6379"));
    }

    #[test]
    fn test_build_exposed_ports_maps_all_declared_ports() {
        let exposed_ports =
            build_exposed_ports(&[SharedServicePort::same(5432), SharedServicePort::same(6379)]);

        assert_eq!(exposed_ports.len(), 2);
        assert!(exposed_ports.contains_key("5432/tcp"));
        assert!(exposed_ports.contains_key("6379/tcp"));
        assert!(exposed_ports["5432/tcp"].is_empty());
    }

    #[test]
    fn test_build_port_bindings_and_exposed_ports_use_container_ports_for_mappings() {
        let mapping = SharedServicePort::new(5433, 5432);
        let port_bindings = build_port_bindings(&[mapping]);
        let exposed_ports = build_exposed_ports(&[mapping]);

        let binding = port_bindings
            .get("5432/tcp")
            .and_then(|bindings| bindings.as_ref())
            .and_then(|bindings| bindings.first())
            .unwrap();

        assert_eq!(binding.host_port.as_deref(), Some("5433"));
        assert!(exposed_ports.contains_key("5432/tcp"));
        assert!(!exposed_ports.contains_key("5433/tcp"));
    }

    #[test]
    fn test_build_port_bindings_keeps_multiple_host_ports_for_same_container_port() {
        let port_bindings = build_port_bindings(&[
            SharedServicePort::new(5433, 5432),
            SharedServicePort::new(6433, 5432),
        ]);

        let bindings = port_bindings
            .get("5432/tcp")
            .and_then(|bindings| bindings.as_ref())
            .unwrap();

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].host_port.as_deref(), Some("5433"));
        assert_eq!(bindings[1].host_port.as_deref(), Some("6433"));
    }

    #[test]
    fn test_humanize_port_conflict_extracts_specific_bound_port() {
        let raw = "driver failed programming external connectivity on endpoint foo: Bind for 0.0.0.0:6379 failed: port is already allocated";
        let message =
            humanize_port_conflict(raw, "redis", &[SharedServicePort::same(6379)]).unwrap();

        assert!(message.contains("Port 6379 is already in use on the host."));
        assert!(message.contains("shared service 'redis'"));
        assert!(message.contains("lsof -iTCP:6379 -sTCP:LISTEN"));
    }

    #[test]
    fn test_humanize_port_conflict_falls_back_to_declared_ports_when_bind_is_missing() {
        let raw = "failed to start container: port is already allocated";
        let message = humanize_port_conflict(
            raw,
            "postgres",
            &[
                SharedServicePort::new(5433, 5432),
                SharedServicePort::same(6432),
            ],
        )
        .unwrap();

        assert!(message.contains("Port 5433, 6432 is already in use on the host."));
    }

    #[test]
    fn test_humanize_port_conflict_ignores_other_errors() {
        assert!(humanize_port_conflict(
            "permission denied",
            "redis",
            &[SharedServicePort::same(6379)],
        )
        .is_none());
    }
}
