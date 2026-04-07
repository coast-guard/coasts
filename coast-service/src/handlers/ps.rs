use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{PsRequest, PsResponse, ServiceStatus};

use crate::state::ServiceState;

use super::assign::COMPOSE_FILE_SH;

pub async fn handle(req: PsRequest, state: &ServiceState) -> Result<PsResponse> {
    info!(name = %req.name, project = %req.project, "remote ps request");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    drop(db);

    let container_id = match instance.container_id {
        Some(ref cid) => cid.clone(),
        None => {
            return Ok(PsResponse {
                name: req.name,
                services: vec![],
            });
        }
    };

    let Some(ref docker) = state.docker else {
        return Ok(PsResponse {
            name: req.name,
            services: vec![],
        });
    };

    let artifact_dir = super::run::resolve_artifact_dir(&req.project, Some("remote"))
        .or_else(|| super::run::resolve_artifact_dir(&req.project, None));

    let project_dir = artifact_dir
        .as_ref()
        .map(|d| super::run::read_compose_project_dir(d))
        .unwrap_or_else(|| "/workspace".to_string());

    let ps_cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} ps --format json 2>/dev/null || echo '[]'"
    );

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let result = rt
        .exec_in_coast(&container_id, &["sh", "-c", &ps_cmd])
        .await?;

    let mut services = parse_compose_ps_output(&result.stdout);

    let config_cmd = format!(
        "{COMPOSE_FILE_SH}; docker compose -f \"$CF\" --project-directory {project_dir} config 2>/dev/null"
    );
    if let Ok(config_result) = rt
        .exec_in_coast(&container_id, &["sh", "-c", &config_cmd])
        .await
    {
        if config_result.success() {
            enrich_with_expected_services(&mut services, &config_result.stdout);
        }
    }

    Ok(PsResponse {
        name: req.name,
        services,
    })
}

fn parse_compose_ps_output(stdout: &str) -> Vec<ServiceStatus> {
    let mut services = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line == "[]" {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(obj) = val.as_object() {
                let name = obj
                    .get("Service")
                    .or_else(|| obj.get("service"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let mut status = obj
                    .get("State")
                    .or_else(|| obj.get("state"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                if let Some(health) = obj
                    .get("Health")
                    .or_else(|| obj.get("health"))
                    .and_then(|v| v.as_str())
                {
                    if !health.is_empty() {
                        status = format!("{status} ({health})");
                    }
                }

                let ports = obj
                    .get("Ports")
                    .or_else(|| obj.get("ports"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let image = obj
                    .get("Image")
                    .or_else(|| obj.get("image"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                services.push(ServiceStatus {
                    name,
                    status,
                    ports,
                    image,
                    kind: Some("compose".to_string()),
                });
            }
        }
    }
    services
}

fn enrich_with_expected_services(services: &mut Vec<ServiceStatus>, config_yaml: &str) {
    let Ok(yaml) = serde_yaml::from_str::<serde_yaml::Value>(config_yaml) else {
        return;
    };
    let Some(svc_map) = yaml.get("services").and_then(|s| s.as_mapping()) else {
        return;
    };

    let port_services: std::collections::HashSet<String> = svc_map
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
        .collect();

    services.retain(|s| {
        s.kind.as_deref() != Some("compose")
            || s.status.starts_with("running")
            || port_services.contains(&s.name)
    });

    let found: std::collections::HashSet<String> =
        services.iter().map(|s| s.name.clone()).collect();
    for svc_name in &port_services {
        if !found.contains(svc_name) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};
    use std::sync::Arc;

    fn test_state() -> Arc<ServiceState> {
        Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ))
    }

    async fn insert_instance(
        state: &ServiceState,
        name: &str,
        project: &str,
        container_id: Option<&str>,
    ) {
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "running".to_string(),
            container_id: container_id.map(String::from),
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
    }

    #[test]
    fn test_parse_compose_ps_ndjson() {
        let output = r#"{"Service":"web","State":"running","Status":"Up 2 minutes","Health":"healthy","Ports":"0.0.0.0:3000->3000/tcp","Image":"app:latest","Name":"proj-web-1"}
{"Service":"db","State":"running","Status":"Up 2 minutes","Health":"","Ports":"5432/tcp","Image":"postgres:16","Name":"proj-db-1"}"#;

        let services = parse_compose_ps_output(output);
        assert_eq!(services.len(), 2);

        assert_eq!(services[0].name, "web");
        assert!(services[0].status.contains("running"));
        assert!(services[0].status.contains("healthy"));
        assert_eq!(services[0].ports, "0.0.0.0:3000->3000/tcp");
        assert_eq!(services[0].image, "app:latest");
        assert_eq!(services[0].kind, Some("compose".to_string()));

        assert_eq!(services[1].name, "db");
        assert!(services[1].status.contains("running"));
        assert!(!services[1].status.contains("healthy"));
        assert_eq!(services[1].image, "postgres:16");
    }

    #[test]
    fn test_parse_compose_ps_empty_output() {
        assert!(parse_compose_ps_output("").is_empty());
        assert!(parse_compose_ps_output("[]").is_empty());
        assert!(parse_compose_ps_output("\n\n").is_empty());
    }

    #[test]
    fn test_parse_compose_ps_invalid_json() {
        let output = "not json at all\n{bad json too}";
        assert!(parse_compose_ps_output(output).is_empty());
    }

    #[test]
    fn test_parse_compose_ps_lowercase_keys() {
        let output = r#"{"service":"api","state":"exited","status":"Exited (1)","health":"","ports":"","image":"api:dev"}"#;
        let services = parse_compose_ps_output(output);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "api");
        assert!(services[0].status.contains("exited"));
    }

    #[tokio::test]
    async fn test_ps_nonexistent_errors() {
        let state = test_state();
        let err = handle(
            PsRequest {
                name: "nope".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }

    #[tokio::test]
    async fn test_ps_no_docker_returns_empty_services() {
        let state = test_state();
        insert_instance(&state, "web", "proj", Some("cid-123")).await;

        let resp = handle(
            PsRequest {
                name: "web".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.name, "web");
        assert!(resp.services.is_empty());
    }

    #[tokio::test]
    async fn test_ps_no_container_id_returns_empty_services() {
        let state = test_state();
        insert_instance(&state, "web", "proj", None).await;

        let resp = handle(
            PsRequest {
                name: "web".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.name, "web");
        assert!(resp.services.is_empty());
    }

    fn svc(name: &str, status: &str) -> ServiceStatus {
        ServiceStatus {
            name: name.into(),
            status: status.into(),
            ports: String::new(),
            image: String::new(),
            kind: Some("compose".into()),
        }
    }

    fn yaml_with_ports(services: &[(&str, bool)]) -> String {
        let mut yaml = String::from("services:\n");
        for (name, has_ports) in services {
            yaml.push_str(&format!("  {name}:\n    image: test\n"));
            if *has_ports {
                yaml.push_str("    ports:\n      - \"8080:8080\"\n");
            }
        }
        yaml
    }

    #[test]
    fn test_enrich_keeps_running_service() {
        let yaml = yaml_with_ports(&[("web", true)]);
        let mut services = vec![svc("web", "running")];
        enrich_with_expected_services(&mut services, &yaml);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
        assert_eq!(services[0].status, "running");
    }

    #[test]
    fn test_enrich_adds_down_for_missing_port_service() {
        let yaml = yaml_with_ports(&[("web", true), ("test-redis", true)]);
        let mut services = vec![svc("web", "running")];
        enrich_with_expected_services(&mut services, &yaml);
        assert_eq!(services.len(), 2);
        let redis = services
            .iter()
            .find(|s| s.name == "test-redis")
            .expect("test-redis should be added");
        assert_eq!(redis.status, "down");
    }

    #[test]
    fn test_enrich_filters_exited_no_port_service() {
        let yaml = yaml_with_ports(&[("web", true), ("migrate", false)]);
        let mut services = vec![svc("web", "running"), svc("migrate", "exited")];
        enrich_with_expected_services(&mut services, &yaml);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }

    #[test]
    fn test_enrich_does_not_add_portless_service() {
        let yaml = yaml_with_ports(&[("web", true), ("backend-test", false)]);
        let mut services = vec![svc("web", "running")];
        enrich_with_expected_services(&mut services, &yaml);
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "web");
    }

    #[test]
    fn test_enrich_invalid_yaml_is_noop() {
        let mut services = vec![svc("web", "running"), svc("db", "running")];
        enrich_with_expected_services(&mut services, "not valid yaml {{{}}}");
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].name, "web");
        assert_eq!(services[1].name, "db");
    }
}
