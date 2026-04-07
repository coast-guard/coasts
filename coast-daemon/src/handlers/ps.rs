/// Handler for the `coast ps` command.
///
/// Gets the status of inner compose services by executing
/// `docker compose ps` inside the coast container.
use tracing::info;

use std::collections::HashSet;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{PsRequest, PsResponse, ServiceStatus};
use coast_core::types::{CoastInstance, InstanceStatus};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

use super::compose_context_for_build;

/// Result of validating whether an instance is ready for `ps`.
#[derive(Debug)]
enum PsValidation {
    /// Instance is idle — return empty services.
    Idle,
    /// Instance is ready — use these identifiers for Docker queries.
    Ready {
        container_id: String,
        build_id: Option<String>,
    },
}

/// Check that the instance is in a state where `ps` makes sense.
///
/// Returns `Ready` with the container_id and build_id when the instance is
/// running, or `Idle` when the instance exists but has no services yet.
/// Errors for stopped, provisioning, or corrupt states.
fn validate_ps_ready(instance: &CoastInstance) -> Result<PsValidation> {
    if instance.status == InstanceStatus::Stopped {
        return Err(CoastError::state(format!(
            "Instance '{}' is stopped. No services are running. Run `coast start {}` first.",
            instance.name, instance.name
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
            instance.name
        )));
    }

    if instance.status == InstanceStatus::Idle {
        return Ok(PsValidation::Idle);
    }

    let container_id = instance.container_id.clone().ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{}' has no container ID. This may indicate a corrupt state. \
             Try `coast rm {}` and `coast run` again.",
            instance.name, instance.name
        ))
    })?;

    Ok(PsValidation::Ready {
        container_id,
        build_id: instance.build_id.clone(),
    })
}

/// Cross-reference `docker compose config` with the current service list to
/// filter out non-port exited services and add "down" entries for missing ones.
fn enrich_compose_services(
    services: &mut Vec<ServiceStatus>,
    config_yaml: &str,
    shared_names: &HashSet<String>,
) {
    let port_services: HashSet<String> = extract_services_with_ports(config_yaml)
        .into_iter()
        .collect();

    services.retain(|s| {
        s.kind.as_deref() != Some("compose")
            || s.status == "running"
            || port_services.contains(&s.name)
    });

    let found_names: HashSet<String> = services.iter().map(|s| s.name.clone()).collect();
    for svc_name in &port_services {
        if !found_names.contains(svc_name) && !shared_names.contains(svc_name) {
            services.push(ServiceStatus {
                name: svc_name.clone(),
                status: "down".to_string(),
                ports: String::new(),
                image: String::new(),
                kind: Some("compose".to_string()),
            });
        }
    }
}

/// If the instance is remote, forward the ps request via SSH tunnel.
/// Returns `Some(response)` if forwarded, `None` if the instance is local.
async fn try_forward_to_remote(req: &PsRequest, state: &AppState) -> Result<Option<PsResponse>> {
    let db = state.db.lock().await;
    let is_remote = db
        .get_instance(&req.project, &req.name)?
        .is_some_and(|i| i.remote_host.is_some());
    drop(db);

    if !is_remote {
        return Ok(None);
    }

    let remote_config =
        super::remote::resolve_remote_for_instance(&req.project, &req.name, state).await?;
    let client = super::remote::RemoteClient::connect(&remote_config).await?;
    super::remote::forward::forward_ps(&client, req)
        .await
        .map(Some)
}

/// Query compose services inside the container.
async fn query_compose_services(
    runtime: &coast_docker::dind::DindRuntime,
    container_id: &str,
    instance_name: &str,
    build_id: Option<&str>,
    project: &str,
    shared_names: &HashSet<String>,
) -> Result<Vec<ServiceStatus>> {
    let ctx = compose_context_for_build(project, build_id);
    let cmd_parts = ctx.compose_shell("ps --format json");
    let cmd_refs: Vec<&str> = cmd_parts.iter().map(std::string::String::as_str).collect();

    let exec_result = runtime
        .exec_in_coast(container_id, &cmd_refs)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to get service status for instance '{}': {}",
                instance_name, e
            ))
        })?;

    let mut services = Vec::new();
    if exec_result.success() {
        let mut compose_svcs = parse_compose_ps_output(&exec_result.stdout)?;
        for svc in &mut compose_svcs {
            svc.kind = Some("compose".to_string());
        }
        services.extend(compose_svcs);
    }

    let config_cmd = ctx.compose_shell("config");
    let config_refs: Vec<&str> = config_cmd.iter().map(String::as_str).collect();
    if let Ok(config_result) = runtime.exec_in_coast(container_id, &config_refs).await {
        if config_result.success() {
            enrich_compose_services(&mut services, &config_result.stdout, shared_names);
        }
    }

    Ok(services)
}

/// Query bare services inside the container.
async fn query_bare_services(
    runtime: &coast_docker::dind::DindRuntime,
    container_id: &str,
    instance_name: &str,
) -> Result<Vec<ServiceStatus>> {
    let ps_cmd = crate::bare_services::generate_ps_command();
    let bare_cmd_parts = ["sh".to_string(), "-c".to_string(), ps_cmd];
    let bare_refs: Vec<&str> = bare_cmd_parts
        .iter()
        .map(std::string::String::as_str)
        .collect();

    let bare_result = runtime
        .exec_in_coast(container_id, &bare_refs)
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to get bare service status for instance '{}': {}",
                instance_name, e
            ))
        })?;

    let mut services = Vec::new();
    if bare_result.success() {
        let mut bare_svcs = parse_compose_ps_output(&bare_result.stdout)?;
        for svc in &mut bare_svcs {
            svc.kind = Some("bare".to_string());
        }
        services.extend(bare_svcs);
    }

    Ok(services)
}

/// Handle a ps request.
pub async fn handle(req: PsRequest, state: &AppState) -> Result<PsResponse> {
    info!(name = %req.name, project = %req.project, "handling ps request");

    if let Some(resp) = try_forward_to_remote(&req, state).await? {
        return Ok(resp);
    }

    let (container_id, build_id) = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?;
        let instance = instance.ok_or_else(|| CoastError::InstanceNotFound {
            name: req.name.clone(),
            project: req.project.clone(),
        })?;

        match validate_ps_ready(&instance)? {
            PsValidation::Idle => {
                return Ok(PsResponse {
                    name: req.name.clone(),
                    services: vec![],
                });
            }
            PsValidation::Ready {
                container_id,
                build_id,
            } => (container_id, build_id),
        }
    };

    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Ensure Docker is running and restart coastd.")
    })?;

    let has_bare = crate::bare_services::has_bare_services(&docker, &container_id).await;
    let has_compose = super::assign::has_compose(&req.project);

    if !has_compose && !has_bare {
        return Err(CoastError::docker(format!(
            "No compose or bare services configured for instance '{}'",
            req.name
        )));
    }

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let shared_names: HashSet<String> = {
        let db = state.db.lock().await;
        db.list_shared_services(Some(&req.project))
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.service_name)
            .collect()
    };

    let mut services: Vec<ServiceStatus> = Vec::new();

    if has_compose {
        services.extend(
            query_compose_services(
                &runtime,
                &container_id,
                &req.name,
                build_id.as_deref(),
                &req.project,
                &shared_names,
            )
            .await?,
        );
    }

    if has_bare {
        services.extend(query_bare_services(&runtime, &container_id, &req.name).await?);
    }

    if !shared_names.is_empty() {
        services.retain(|s| !shared_names.contains(&s.name));
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
    use coast_core::types::RuntimeType;

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
            remote_host: None,
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

    // --- validate_ps_ready tests ---

    #[test]
    fn test_validate_ps_ready_running_with_container_id() {
        let instance = make_instance("feat-a", InstanceStatus::Running, Some("cid-123"));
        let result = validate_ps_ready(&instance).unwrap();
        assert!(matches!(
            result,
            PsValidation::Ready { ref container_id, .. } if container_id == "cid-123"
        ));
    }

    #[test]
    fn test_validate_ps_ready_stopped() {
        let instance = make_instance("feat-a", InstanceStatus::Stopped, Some("cid"));
        let result = validate_ps_ready(&instance);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("stopped"));
    }

    #[test]
    fn test_validate_ps_ready_provisioning() {
        let instance = make_instance("feat-a", InstanceStatus::Provisioning, Some("cid"));
        let result = validate_ps_ready(&instance);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("provisioned"));
    }

    #[test]
    fn test_validate_ps_ready_idle() {
        let instance = make_instance("feat-a", InstanceStatus::Idle, None);
        let result = validate_ps_ready(&instance).unwrap();
        assert!(matches!(result, PsValidation::Idle));
    }

    #[test]
    fn test_validate_ps_ready_no_container_id() {
        let instance = make_instance("feat-a", InstanceStatus::Running, None);
        let result = validate_ps_ready(&instance);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no container ID"));
    }

    // --- enrich_compose_services tests ---

    fn svc(name: &str, status: &str, kind: Option<&str>) -> ServiceStatus {
        ServiceStatus {
            name: name.to_string(),
            status: status.to_string(),
            ports: String::new(),
            image: String::new(),
            kind: kind.map(|k| k.to_string()),
        }
    }

    fn yaml_with_ports(services: &[(&str, bool)]) -> String {
        let mut y = "services:\n".to_string();
        for (name, has_ports) in services {
            y.push_str(&format!("  {name}:\n    image: img\n"));
            if *has_ports {
                y.push_str("    ports:\n      - \"3000:3000\"\n");
            }
        }
        y
    }

    #[test]
    fn test_enrich_keeps_running_compose_service() {
        let yaml = yaml_with_ports(&[("web", true)]);
        let mut services = vec![svc("web", "running", Some("compose"))];
        enrich_compose_services(&mut services, &yaml, &HashSet::new());
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].status, "running");
    }

    #[test]
    fn test_enrich_filters_exited_no_port_service() {
        let yaml = yaml_with_ports(&[("web", true), ("migrate", false)]);
        let mut services = vec![
            svc("web", "running", Some("compose")),
            svc("migrate", "exited", Some("compose")),
        ];
        enrich_compose_services(&mut services, &yaml, &HashSet::new());
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }

    #[test]
    fn test_enrich_adds_down_for_missing_port_service() {
        let yaml = yaml_with_ports(&[("web", true), ("worker", true)]);
        let mut services = vec![svc("web", "running", Some("compose"))];
        enrich_compose_services(&mut services, &yaml, &HashSet::new());
        assert_eq!(services.len(), 2);
        let down = services.iter().find(|s| s.name == "worker").unwrap();
        assert_eq!(down.status, "down");
    }

    #[test]
    fn test_enrich_excludes_shared_from_down() {
        let yaml = yaml_with_ports(&[("web", true), ("redis", true)]);
        let mut services = vec![svc("web", "running", Some("compose"))];
        let shared: HashSet<String> = ["redis".to_string()].into();
        enrich_compose_services(&mut services, &yaml, &shared);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }
}
