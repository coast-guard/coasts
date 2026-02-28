/// Handler for the `coast ps` command.
///
/// Gets the status of inner compose services by executing
/// `docker compose ps` inside the coast container.
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{PsRequest, PsResponse, ServiceStatus};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

use super::compose_context_for_build;

/// Handle a ps request.
pub async fn handle(req: PsRequest, state: &AppState) -> Result<PsResponse> {
    info!(name = %req.name, project = %req.project, "handling ps request");

    // Phase 1: DB read (locked)
    let (container_id, build_id) = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?;
        let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
            name: req.name.clone(),
            project: req.project.clone(),
        })?;

        if instance.status == InstanceStatus::Stopped {
            return Err(CoastError::state(format!(
                "Instance '{}' is stopped. No services are running. Run `coast start {}` first.",
                req.name, req.name
            )));
        }
        if instance.status == InstanceStatus::Provisioning
            || instance.status == InstanceStatus::Assigning
        {
            let action = if instance.status == InstanceStatus::Provisioning {
                "provisioned"
            } else {
                "assigned"
            };
            return Err(CoastError::state(format!(
                "Instance '{}' is still being {action}. Wait for the operation to complete.",
                req.name
            )));
        }

        if instance.status == InstanceStatus::Idle {
            return Ok(PsResponse {
                name: req.name.clone(),
                services: vec![],
            });
        }

        let cid = instance.container_id.clone().ok_or_else(|| {
            CoastError::state(format!(
                "Instance '{}' has no container ID. This may indicate a corrupt state. \
                 Try `coast rm {}` and `coast run` again.",
                req.name, req.name
            ))
        })?;
        (cid, instance.build_id.clone())
    };

    // Phase 2: Docker operations (unlocked)
    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let is_bare = crate::bare_services::has_bare_services(docker, &container_id).await;

    let cmd_parts = if is_bare {
        let ps_cmd = crate::bare_services::generate_ps_command();
        vec!["sh".to_string(), "-c".to_string(), ps_cmd]
    } else {
        let ctx = compose_context_for_build(&req.project, build_id.as_deref());
        ctx.compose_shell("ps --format json")
    };
    let cmd_refs: Vec<&str> = cmd_parts.iter().map(std::string::String::as_str).collect();

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let exec_result = runtime
        .exec_in_coast(&container_id, &cmd_refs)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to get service status for instance '{}': {}",
                req.name, e
            ))
        })?;

    if !exec_result.success() {
        return Err(CoastError::docker(format!(
            "ps command failed in instance '{}' (exit code {}): {}",
            req.name, exec_result.exit_code, exec_result.stderr
        )));
    }

    let kind_value = if is_bare { "bare" } else { "compose" };
    let mut services = parse_compose_ps_output(&exec_result.stdout)?;
    for svc in &mut services {
        svc.kind = Some(kind_value.to_string());
    }

    // Load shared service names so we can exclude them
    let shared_names: std::collections::HashSet<String> = {
        let db = state.db.lock().await;
        db.list_shared_services(Some(&req.project))
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.service_name)
            .collect()
    };

    // Filter out shared services from the ps output
    if !shared_names.is_empty() {
        services.retain(|s| !shared_names.contains(&s.name));
    }

    // Detect missing/crashed services and filter one-shot jobs.
    // Only services with `ports:` in the compose config are long-running.
    // One-shot jobs (like migrations) are expected to exit and should not
    // appear as "down" or clutter the service list when not running.
    if !is_bare {
        let ctx = compose_context_for_build(&req.project, build_id.as_deref());
        let config_cmd = ctx.compose_shell("config");
        let config_refs: Vec<&str> = config_cmd.iter().map(String::as_str).collect();
        if let Ok(config_result) = runtime.exec_in_coast(&container_id, &config_refs).await {
            if config_result.success() {
                let port_services: std::collections::HashSet<String> =
                    extract_services_with_ports(&config_result.stdout)
                        .into_iter()
                        .collect();

                // Remove non-running one-shot services (no ports, exited)
                services.retain(|s| s.status == "running" || port_services.contains(&s.name));

                // Add missing long-running services as "down"
                let found_names: std::collections::HashSet<String> =
                    services.iter().map(|s| s.name.clone()).collect();
                for svc_name in &port_services {
                    if !found_names.contains(svc_name) && !shared_names.contains(svc_name) {
                        services.push(ServiceStatus {
                            name: svc_name.clone(),
                            status: "down".to_string(),
                            ports: String::new(),
                            image: String::new(),
                            kind: Some(kind_value.to_string()),
                        });
                    }
                }
            }
        }
    }

    info!(
        name = %req.name,
        service_count = services.len(),
        "compose service status retrieved"
    );

    Ok(PsResponse {
        name: req.name,
        services,
    })
}

/// Extract service names that have `ports:` defined from `docker compose config` YAML output.
/// Services without ports (like migrations) are one-shot jobs and should not be flagged as "down".
fn extract_services_with_ports(config_yaml: &str) -> Vec<String> {
    let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(config_yaml) else {
        return Vec::new();
    };
    let Some(services) = yaml.get("services").and_then(|s| s.as_mapping()) else {
        return Vec::new();
    };
    services
        .iter()
        .filter_map(|(name, def)| {
            let name_str = name.as_str()?;
            let has_ports = def
                .get("ports")
                .and_then(|p| p.as_sequence())
                .is_some_and(|seq| !seq.is_empty());
            if has_ports {
                Some(name_str.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Parse the output of `docker compose ps --format json` into `ServiceStatus` entries.
fn parse_compose_ps_output(output: &str) -> Result<Vec<ServiceStatus>> {
    let mut services = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            let name = value
                .get("Service")
                .or_else(|| value.get("Name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let status = value
                .get("State")
                .or_else(|| value.get("Status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let ports = value
                .get("Ports")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let image = value
                .get("Image")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            services.push(ServiceStatus {
                name,
                status,
                ports,
                image,
                kind: None,
            });
        }
    }

    Ok(services)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(
        name: &str,
        status: InstanceStatus,
        container_id: Option<&str>,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: "my-app".to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: container_id.map(|s| s.to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    #[tokio::test]
    async fn test_ps_running_instance_no_docker() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "feat-a",
                InstanceStatus::Running,
                Some("container-123"),
            ))
            .unwrap();
        }

        let req = PsRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Docker is not available"));
    }

    #[tokio::test]
    async fn test_ps_nonexistent_instance() {
        let state = test_state();
        let req = PsRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_ps_stopped_instance_fails() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "stopped",
                InstanceStatus::Stopped,
                Some("cid"),
            ))
            .unwrap();
        }

        let req = PsRequest {
            name: "stopped".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
    }

    #[test]
    fn test_parse_compose_ps_json_output() {
        let output = r#"
{"Service":"web","State":"running","Ports":"0.0.0.0:3000->3000/tcp"}
{"Service":"db","State":"running","Ports":"5432/tcp"}
"#;
        let services = parse_compose_ps_output(output).unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "web");
        assert_eq!(services[0].status, "running");
        assert_eq!(services[0].ports, "0.0.0.0:3000->3000/tcp");
        assert!(services[0].kind.is_none());
        assert_eq!(services[1].name, "db");
        assert!(services[1].kind.is_none());
    }

    #[test]
    fn test_kind_set_after_parsing() {
        let output = r#"{"Service":"web","State":"running"}"#;
        let mut services = parse_compose_ps_output(output).unwrap();
        for svc in &mut services {
            svc.kind = Some("bare".to_string());
        }
        assert_eq!(services[0].kind.as_deref(), Some("bare"));
    }

    #[test]
    fn test_parse_compose_ps_empty_output() {
        let services = parse_compose_ps_output("").unwrap();
        assert!(services.is_empty());
    }

    #[test]
    fn test_parse_compose_ps_invalid_json() {
        let services = parse_compose_ps_output("not json\nalso not json").unwrap();
        assert!(services.is_empty());
    }
}
