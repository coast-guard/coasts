use tracing::info;

use coast_core::error::{CoastError, Result};

use crate::server::AppState;

/// Result of starting shared services on the host daemon.
pub(super) struct SharedServicesResult {
    pub service_names: Vec<String>,
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
#[allow(clippy::cognitive_complexity)]
pub(super) async fn start_shared_services(
    project: &str,
    shared_service_configs: &[coast_core::types::SharedServiceConfig],
    docker: &bollard::Docker,
    state: &AppState,
) -> Result<SharedServicesResult> {
    let mut service_names = Vec::new();
    let mut service_hosts = std::collections::HashMap::new();

    let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
    let network_name = nm.create_shared_network(project).await?;

    for svc_config in shared_service_configs {
        let container_name =
            crate::shared_services::shared_container_name(project, &svc_config.name);

        let existing = {
            let db = state.db.lock().await;
            db.get_shared_service(project, &svc_config.name)?
        };

        let already_running = if let Some(ref rec) = existing {
            if rec.status == "running" {
                docker
                    .inspect_container(&container_name, None)
                    .await
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };

        if !already_running {
            let _ = docker.stop_container(&container_name, None).await;
            let _ = docker.remove_container(&container_name, None).await;

            let shared_cfg =
                crate::shared_services::build_shared_container_config(project, svc_config);

            let mut port_bindings = std::collections::HashMap::new();
            for port in &svc_config.ports {
                let key = format!("{port}/tcp");
                port_bindings.insert(
                    key,
                    Some(vec![bollard::models::PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        host_port: Some(port.to_string()),
                    }]),
                );
            }

            let host_config = bollard::models::HostConfig {
                binds: Some(shared_cfg.volumes.clone()),
                port_bindings: Some(port_bindings),
                restart_policy: Some(bollard::models::RestartPolicy {
                    name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let mut exposed: std::collections::HashMap<String, std::collections::HashMap<(), ()>> =
                std::collections::HashMap::new();
            for port in &svc_config.ports {
                exposed.insert(format!("{port}/tcp"), std::collections::HashMap::new());
            }

            let create_config = bollard::container::Config {
                image: Some(shared_cfg.image.clone()),
                env: Some(shared_cfg.env.clone()),
                host_config: Some(host_config),
                labels: Some(shared_cfg.labels.clone()),
                exposed_ports: Some(exposed),
                ..Default::default()
            };

            let create_opts = bollard::container::CreateContainerOptions {
                name: container_name.clone(),
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

            docker
                .start_container::<String>(&container_name, None)
                .await
                .map_err(|e| {
                    let raw = e.to_string();
                    if let Some(msg) =
                        humanize_port_conflict(&raw, &svc_config.name, &svc_config.ports)
                    {
                        CoastError::docker(msg)
                    } else {
                        CoastError::docker(format!(
                            "Failed to start shared service container '{}': {}",
                            container_name, e
                        ))
                    }
                })?;

            if let Err(e) = nm.connect_container(&network_name, &container_name).await {
                tracing::warn!(error = %e, container = %container_name, "failed to connect shared service to network (may already be connected)");
            }

            info!(
                service = %svc_config.name,
                container = %container_name,
                "shared service started on host daemon"
            );

            {
                let db = state.db.lock().await;
                let _ = db.insert_shared_service(
                    project,
                    &svc_config.name,
                    Some(&container_name),
                    "running",
                );
            }
        } else {
            info!(
                service = %svc_config.name,
                "shared service already running, skipping"
            );
        }

        service_names.push(svc_config.name.clone());
        service_hosts.insert(svc_config.name.clone(), container_name);
    }

    Ok(SharedServicesResult {
        service_names,
        service_hosts,
        network_name: Some(network_name),
    })
}

/// Rewrite a raw Docker "port already allocated" error into a human-friendly message.
///
/// Docker errors look like:
///   "...Bind for 0.0.0.0:6379 failed: port is already allocated"
///
/// We extract the port number and tell the user what's actually wrong.
fn humanize_port_conflict(raw: &str, service_name: &str, declared_ports: &[u16]) -> Option<String> {
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
            .map(std::string::ToString::to_string)
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
