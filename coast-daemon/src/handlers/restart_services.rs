/// Handler for the `coast restart-services` command.
///
/// Tears down and restarts all compose services (or bare services) inside
/// a running coast instance, returning it to its original state.
///
/// Internal flow:
/// 1. Verify instance exists and is Running or CheckedOut
/// 2. Read cached Coastfile to determine compose vs bare services + autostart
/// 3. For compose: discover the running project name via `docker compose ls`,
///    then `docker compose -p <project> down -t 2` + `up -d`
/// 4. For bare services: `stop-all.sh` then `start-all.sh`
/// 5. If autostart=false, skip the start phase
///
/// Shared services are NOT affected.
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RestartServicesRequest, RestartServicesResponse};
use coast_core::types::InstanceStatus;
use coast_docker::runtime::Runtime;

use crate::handlers::compose_context_for_build;
use crate::server::AppState;

/// Handle a restart-services request.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: RestartServicesRequest,
    state: &AppState,
) -> Result<RestartServicesResponse> {
    info!(
        name = %req.name,
        project = %req.project,
        "handling restart-services request"
    );

    let (container_id, build_id) = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
            CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            }
        })?;

        if instance.remote_host.is_some() {
            drop(db);
            let remote_config =
                super::remote::resolve_remote_for_instance(&req.project, &req.name, state).await?;
            let client = super::remote::RemoteClient::connect(&remote_config).await?;
            return super::remote::forward::forward_restart_services(&client, &req).await;
        }

        if instance.status != InstanceStatus::Running
            && instance.status != InstanceStatus::CheckedOut
        {
            return Err(CoastError::state(format!(
                "Instance '{}' is in '{}' state and cannot have services restarted. \
                 Only Running or CheckedOut instances are supported. \
                 Run `coast start {}` first.",
                req.name, instance.status, req.name,
            )));
        }

        let cid = instance.container_id.ok_or_else(|| {
            CoastError::state(format!(
                "Instance '{}' has no container ID. This should not happen for a Running instance. \
                 Try `coast rm {} && coast run {}`.",
                req.name, req.name, req.name,
            ))
        })?;

        (cid, instance.build_id)
    };

    let home = dirs::home_dir().unwrap_or_default();
    let images_dir = home.join(".coast").join("images").join(&req.project);
    let coastfile_path = build_id
        .as_deref()
        .map(|bid| images_dir.join(bid).join("coastfile.toml"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| images_dir.join("coastfile.toml"));

    let (has_compose, has_services, autostart) = if coastfile_path.exists() {
        let raw_text = std::fs::read_to_string(&coastfile_path).unwrap_or_default();
        let autostart_false = raw_text.lines().any(|line| {
            let trimmed = line.trim();
            trimmed == "autostart = false" || trimmed.starts_with("autostart = false ")
        });
        match coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            Ok(cf) => (
                cf.compose.is_some(),
                !cf.services.is_empty(),
                !autostart_false,
            ),
            Err(_) => (true, false, !autostart_false),
        }
    } else {
        (true, false, true)
    };

    let mut services_restarted = Vec::new();

    if let Some(docker) = state.docker.as_ref() {
        let dind_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

        if has_compose {
            let ctx = compose_context_for_build(&req.project, build_id.as_deref());

            // Discover the running compose project name (may differ from ctx)
            let ls_result = dind_rt
                .exec_in_coast(&container_id, &["docker", "compose", "ls", "-q"])
                .await;
            let project_name = ls_result
                .ok()
                .and_then(|r| {
                    if r.success() {
                        r.stdout.lines().next().map(|s| s.trim().to_string())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())
                .unwrap_or(ctx.project_name);

            // Detect compose file paths inside DinD so `up` works after `down`
            // removes all containers (and their config labels).
            let has_override = dind_rt
                .exec_in_coast(
                    &container_id,
                    &["test", "-f", "/coast-override/docker-compose.coast.yml"],
                )
                .await
                .map(|r| r.success())
                .unwrap_or(false);
            let has_artifact = dind_rt
                .exec_in_coast(
                    &container_id,
                    &["test", "-f", "/coast-artifact/compose.yml"],
                )
                .await
                .map(|r| r.success())
                .unwrap_or(false);

            let project_dir = match &ctx.compose_rel_dir {
                Some(dir) => format!("/workspace/{dir}"),
                None => "/workspace".to_string(),
            };

            let mut base_args: Vec<String> =
                vec!["docker".into(), "compose".into(), "-p".into(), project_name];
            if has_override {
                base_args.extend([
                    "-f".into(),
                    "/coast-override/docker-compose.coast.yml".into(),
                    "--project-directory".into(),
                    project_dir,
                ]);
            } else if has_artifact {
                base_args.extend([
                    "-f".into(),
                    "/coast-artifact/compose.yml".into(),
                    "--project-directory".into(),
                    project_dir,
                ]);
            }
            info!(base_args = ?base_args, "resolved compose base args");

            let build_compose_cmd = |subcmd_args: &[&str]| -> Vec<String> {
                let mut cmd = base_args.clone();
                cmd.extend(subcmd_args.iter().map(std::string::ToString::to_string));
                cmd
            };

            let down_cmd = build_compose_cmd(&["down", "-t", "2", "--remove-orphans"]);
            info!(cmd = ?down_cmd, "tearing down compose services");
            let down_refs: Vec<&str> = down_cmd.iter().map(String::as_str).collect();
            let down_result = dind_rt.exec_in_coast(&container_id, &down_refs).await;
            match down_result {
                Ok(r) if r.success() => info!("compose down completed"),
                Ok(r) => {
                    return Err(CoastError::docker(format!(
                        "docker compose down failed in instance '{}': {}",
                        req.name,
                        r.stderr.trim()
                    )));
                }
                Err(e) => {
                    return Err(CoastError::docker(format!(
                        "Failed to exec docker compose down in instance '{}': {e}",
                        req.name
                    )));
                }
            }

            if autostart {
                let up_cmd = build_compose_cmd(&["up", "-d", "--remove-orphans"]);
                info!(cmd = ?up_cmd, "starting compose services");
                let up_refs: Vec<&str> = up_cmd.iter().map(String::as_str).collect();
                let up_result = dind_rt.exec_in_coast(&container_id, &up_refs).await;
                match up_result {
                    Ok(r) if r.success() => {
                        info!("compose up completed");
                        services_restarted.push("(all compose services)".to_string());
                    }
                    Ok(r) => {
                        return Err(CoastError::docker(format!(
                            "docker compose up failed in instance '{}': {}",
                            req.name,
                            r.stderr.trim()
                        )));
                    }
                    Err(e) => {
                        return Err(CoastError::docker(format!(
                            "Failed to exec docker compose up in instance '{}': {e}",
                            req.name
                        )));
                    }
                }
            } else {
                info!("autostart=false, skipping compose up");
            }
        }

        if has_services {
            let stop_cmd = vec!["sh", "/coast-supervisor/stop-all.sh"];
            info!("stopping bare services");
            let stop_result = dind_rt.exec_in_coast(&container_id, &stop_cmd).await;
            match stop_result {
                Ok(r) if r.success() => info!("bare services stopped"),
                Ok(r) => {
                    return Err(CoastError::docker(format!(
                        "stop-all.sh failed in instance '{}': {}",
                        req.name,
                        r.stderr.trim()
                    )));
                }
                Err(e) => {
                    return Err(CoastError::docker(format!(
                        "Failed to exec stop-all.sh in instance '{}': {e}",
                        req.name
                    )));
                }
            }

            if autostart {
                let start_cmd = vec!["sh", "/coast-supervisor/start-all.sh"];
                info!("starting bare services");
                let start_result = dind_rt.exec_in_coast(&container_id, &start_cmd).await;
                match start_result {
                    Ok(r) if r.success() => {
                        info!("bare services started");
                        services_restarted.push("(all bare services)".to_string());
                    }
                    Ok(r) => {
                        return Err(CoastError::docker(format!(
                            "start-all.sh failed in instance '{}': {}",
                            req.name,
                            r.stderr.trim()
                        )));
                    }
                    Err(e) => {
                        return Err(CoastError::docker(format!(
                            "Failed to exec start-all.sh in instance '{}': {e}",
                            req.name
                        )));
                    }
                }
            } else {
                info!("autostart=false, skipping bare service start");
            }
        }

        if !has_compose && !has_services {
            info!("no compose or bare services configured, nothing to restart");
        }
    }

    info!(
        name = %req.name,
        services = ?services_restarted,
        "restart-services completed"
    );

    Ok(RestartServicesResponse {
        name: req.name,
        services_restarted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::AppState;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn sample_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("{project}-coasts-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        }
    }

    #[tokio::test]
    async fn test_restart_services_instance_not_found() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);
        let req = RestartServicesRequest {
            name: "nonexistent".to_string(),
            project: "proj".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_restart_services_stopped_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Stopped))
            .unwrap();
        let state = AppState::new_for_testing(db);
        let req = RestartServicesRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot have services restarted"));
    }

    #[tokio::test]
    async fn test_restart_services_idle_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Idle))
            .unwrap();
        let state = AppState::new_for_testing(db);
        let req = RestartServicesRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot have services restarted"));
    }

    #[tokio::test]
    async fn test_restart_services_running_without_docker() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);
        let req = RestartServicesRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert!(resp.services_restarted.is_empty());
    }

    #[tokio::test]
    async fn test_restart_services_no_container_id_errors() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_host: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);
        let req = RestartServicesRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no container ID"));
    }
}
