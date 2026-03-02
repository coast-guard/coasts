/// Handler for the `coast shared-services` command.
///
/// Manages shared services that run on the host Docker daemon and are
/// accessible to multiple coast instances via a bridge network.
use tokio::time::Instant;
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, SharedRequest, SharedResponse, SharedServiceInfo};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

const CACHE_FRESH_SECS: u64 = 5;

/// Handle a shared service request.
///
/// Dispatches to ps, stop, start, restart, or rm operations
/// based on the request variant.
pub async fn handle(req: SharedRequest, state: &AppState) -> Result<SharedResponse> {
    match req {
        SharedRequest::Ps { project } => handle_ps(project, state).await,
        SharedRequest::Stop { project, service } => handle_stop(project, service, state).await,
        SharedRequest::Start { project, service } => handle_start(project, service, state).await,
        SharedRequest::Restart { project, service } => {
            handle_restart(project, service, state).await
        }
        SharedRequest::Rm { project, service } => handle_rm(project, service, state).await,
    }
}

/// Show shared service status for a project.
///
/// Uses a stale-while-revalidate cache to avoid blocking on slow Docker
/// inspect calls. Returns cached data immediately when available, refreshing
/// in the background if stale.
async fn handle_ps(project: String, state: &AppState) -> Result<SharedResponse> {
    info!(project = %project, "handling shared-services ps request");

    {
        let cache = state.shared_services_cache.lock().await;
        if let Some((cached_at, resp)) = cache.get(&project) {
            let age = cached_at.elapsed();
            if age.as_secs() < CACHE_FRESH_SECS {
                info!(project = %project, count = resp.services.len(), age_ms = age.as_millis(), "returning cached shared services");
                return Ok(resp.clone());
            }
        }
    }

    let resp = fetch_shared_services(&project, state).await?;

    {
        let mut cache = state.shared_services_cache.lock().await;
        cache.insert(project, (Instant::now(), resp.clone()));
    }

    Ok(resp)
}

/// Fetch shared services from Docker, enriching with live container data.
pub async fn fetch_shared_services(project: &str, state: &AppState) -> Result<SharedResponse> {
    let rows = {
        let db = state.db.lock().await;
        db.list_shared_services(Some(project))?
    };

    let futs: Vec<_> = rows
        .iter()
        .map(|row| {
            let docker = state.docker.clone();
            let name = row.service_name.clone();
            let cid = row.container_id.clone();
            let db_status = row.status.clone();
            async move {
                let mut svc = SharedServiceInfo {
                    name,
                    container_id: cid.clone(),
                    status: db_status,
                    image: None,
                    ports: None,
                };
                if let (Some(cid), Some(docker)) = (cid, docker) {
                    if let Ok(inspect) = docker.inspect_container(&cid, None).await {
                        svc.image = inspect.config.as_ref().and_then(|c| c.image.clone());
                        let live_status = inspect
                            .state
                            .as_ref()
                            .and_then(|s| s.status)
                            .map(|s| format!("{s:?}").to_lowercase());
                        if let Some(s) = live_status {
                            svc.status = s;
                        }
                        if let Some(bindings) = inspect
                            .host_config
                            .as_ref()
                            .and_then(|h| h.port_bindings.as_ref())
                        {
                            let port_strs: Vec<String> = bindings
                                .iter()
                                .filter_map(|(container_port, host_bindings)| {
                                    host_bindings.as_ref().and_then(|bs| {
                                        let mapped: Vec<String> = bs
                                            .iter()
                                            .filter_map(|b| {
                                                let hip = b.host_ip.as_deref().unwrap_or("0.0.0.0");
                                                let hp = b.host_port.as_deref()?;
                                                Some(format!("{hip}:{hp}->{container_port}"))
                                            })
                                            .collect();
                                        if mapped.is_empty() {
                                            None
                                        } else {
                                            Some(mapped.join(", "))
                                        }
                                    })
                                })
                                .collect();
                            if !port_strs.is_empty() {
                                svc.ports = Some(port_strs.join(", "));
                            }
                        }
                    }
                }
                svc
            }
        })
        .collect();
    let services: Vec<SharedServiceInfo> = futures_util::future::join_all(futs).await;

    info!(project = %project, count = services.len(), "listing shared services");

    Ok(SharedResponse {
        message: format!("Shared services for project '{project}'."),
        services,
    })
}

/// Resolve the shared service container name, verifying it exists in the DB.
async fn resolve_shared_container(
    project: &str,
    service: &str,
    state: &AppState,
) -> Result<(String, String)> {
    let db = state.db.lock().await;
    let svc = db.get_shared_service(project, service)?.ok_or_else(|| {
        CoastError::state(format!(
            "Shared service '{service}' not found in project '{project}'. \
             Run `coast shared-services ps` to see available services."
        ))
    })?;
    let container_name = crate::shared_services::shared_container_name(project, service);
    Ok((container_name, svc.status))
}

/// Collect all service names for a project from the DB.
async fn all_service_names(project: &str, state: &AppState) -> Result<Vec<String>> {
    let db = state.db.lock().await;
    let rows = db.list_shared_services(Some(project))?;
    Ok(rows.into_iter().map(|r| r.service_name).collect())
}

/// Stop a single shared service container by name.
///
/// If the container no longer exists in Docker (dangling DB record), treats
/// the stop as a no-op and updates the DB status to "stopped".
async fn stop_one(project: &str, service: &str, state: &AppState) -> Result<SharedServiceInfo> {
    let (container_name, _) = resolve_shared_container(project, service, state).await?;

    if let Some(ref docker) = state.docker {
        if let Err(e) = docker.stop_container(&container_name, None).await {
            let is_not_found = e.to_string().contains("404")
                || e.to_string().contains("No such container")
                || e.to_string().contains("not found");
            let is_not_running = e.to_string().contains("304")
                || e.to_string().contains("not running")
                || e.to_string().contains("already stopped");
            if is_not_found || is_not_running {
                warn!(
                    service = %service,
                    container = %container_name,
                    error = %e,
                    "shared service container missing or already stopped, treating stop as no-op"
                );
            } else {
                return Err(CoastError::docker(format!(
                    "Failed to stop shared service '{service}': {e}"
                )));
            }
        }
    }

    {
        let db = state.db.lock().await;
        let _ = db.update_shared_service_status(project, service, "stopped");
    }

    Ok(SharedServiceInfo {
        name: service.to_string(),
        container_id: None,
        status: "stopped".to_string(),
        image: None,
        ports: None,
    })
}

/// Start a single shared service container by name.
async fn start_one(project: &str, service: &str, state: &AppState) -> Result<SharedServiceInfo> {
    let (container_name, _) = resolve_shared_container(project, service, state).await?;

    if let Some(ref docker) = state.docker {
        docker
            .start_container::<String>(&container_name, None)
            .await
            .map_err(|e| {
                CoastError::docker(format!("Failed to start shared service '{service}': {e}"))
            })?;
    }

    {
        let db = state.db.lock().await;
        let _ = db.update_shared_service_status(project, service, "running");
    }

    Ok(SharedServiceInfo {
        name: service.to_string(),
        container_id: None,
        status: "running".to_string(),
        image: None,
        ports: None,
    })
}

/// Restart a single shared service container by name.
async fn restart_one(project: &str, service: &str, state: &AppState) -> Result<SharedServiceInfo> {
    let (container_name, _) = resolve_shared_container(project, service, state).await?;

    if let Some(ref docker) = state.docker {
        docker
            .restart_container(&container_name, None)
            .await
            .map_err(|e| {
                CoastError::docker(format!("Failed to restart shared service '{service}': {e}"))
            })?;
    }

    {
        let db = state.db.lock().await;
        let _ = db.update_shared_service_status(project, service, "running");
    }

    Ok(SharedServiceInfo {
        name: service.to_string(),
        container_id: None,
        status: "running".to_string(),
        image: None,
        ports: None,
    })
}

/// Stop shared service(s). If `service` is None, stop all.
async fn handle_stop(
    project: String,
    service: Option<String>,
    state: &AppState,
) -> Result<SharedResponse> {
    let names = match service {
        Some(s) => {
            info!(project = %project, service = %s, "handling shared-services stop request");
            vec![s]
        }
        None => {
            info!(project = %project, "handling shared-services stop --all request");
            all_service_names(&project, state).await?
        }
    };

    let mut services = Vec::new();
    let mut stopped = Vec::new();
    for name in &names {
        match stop_one(&project, name, state).await {
            Ok(svc) => {
                stopped.push(name.as_str());
                services.push(svc);
                state.emit_event(CoastEvent::SharedServiceStopped {
                    project: project.clone(),
                    service: name.clone(),
                });
            }
            Err(e) => {
                tracing::warn!(service = %name, error = %e, "failed to stop shared service");
                state.emit_event(CoastEvent::SharedServiceError {
                    project: project.clone(),
                    service: name.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    let message = if stopped.len() == 1 {
        format!("Shared service '{}' stopped.", stopped[0])
    } else {
        format!("Stopped {} shared services.", stopped.len())
    };

    Ok(SharedResponse { message, services })
}

/// Start shared service(s). If `service` is None, start all.
async fn handle_start(
    project: String,
    service: Option<String>,
    state: &AppState,
) -> Result<SharedResponse> {
    let names = match service {
        Some(s) => {
            info!(project = %project, service = %s, "handling shared-services start request");
            vec![s]
        }
        None => {
            info!(project = %project, "handling shared-services start --all request");
            all_service_names(&project, state).await?
        }
    };

    let mut services = Vec::new();
    let mut started = Vec::new();
    for name in &names {
        state.emit_event(CoastEvent::SharedServiceStarting {
            project: project.clone(),
            service: name.clone(),
        });
        match start_one(&project, name, state).await {
            Ok(svc) => {
                started.push(name.as_str());
                services.push(svc);
                state.emit_event(CoastEvent::SharedServiceStarted {
                    project: project.clone(),
                    service: name.clone(),
                });
            }
            Err(e) => {
                tracing::warn!(service = %name, error = %e, "failed to start shared service");
                state.emit_event(CoastEvent::SharedServiceError {
                    project: project.clone(),
                    service: name.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    let message = if started.len() == 1 {
        format!("Shared service '{}' started.", started[0])
    } else {
        format!("Started {} shared services.", started.len())
    };

    Ok(SharedResponse { message, services })
}

/// Restart shared service(s). If `service` is None, restart all.
async fn handle_restart(
    project: String,
    service: Option<String>,
    state: &AppState,
) -> Result<SharedResponse> {
    let names = match service {
        Some(s) => {
            info!(project = %project, service = %s, "handling shared-services restart request");
            vec![s]
        }
        None => {
            info!(project = %project, "handling shared-services restart --all request");
            all_service_names(&project, state).await?
        }
    };

    let mut services = Vec::new();
    let mut restarted = Vec::new();
    for name in &names {
        match restart_one(&project, name, state).await {
            Ok(svc) => {
                restarted.push(name.as_str());
                services.push(svc);
                state.emit_event(CoastEvent::SharedServiceRestarted {
                    project: project.clone(),
                    service: name.clone(),
                });
            }
            Err(e) => {
                tracing::warn!(service = %name, error = %e, "failed to restart shared service");
                state.emit_event(CoastEvent::SharedServiceError {
                    project: project.clone(),
                    service: name.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    let message = if restarted.len() == 1 {
        format!("Shared service '{}' restarted.", restarted[0])
    } else {
        format!("Restarted {} shared services.", restarted.len())
    };

    Ok(SharedResponse { message, services })
}

/// Remove a shared service.
///
/// Stops and removes the shared service container, disconnects it from
/// the bridge network, and removes associated Docker volumes.
#[allow(clippy::cognitive_complexity)]
async fn handle_rm(project: String, service: String, state: &AppState) -> Result<SharedResponse> {
    info!(project = %project, service = %service, "handling shared-services rm request");

    let container_id = {
        let db = state.db.lock().await;
        let svc = db.get_shared_service(&project, &service)?;
        match svc {
            Some(s) => s.container_id.clone(),
            None => {
                // Service not in DB — check for a dangling Docker container and clean it up.
                let container_name =
                    crate::shared_services::shared_container_name(&project, &service);
                if let Some(ref docker) = state.docker {
                    if let Ok(inspect) = docker.inspect_container(&container_name, None).await {
                        warn!(
                            service = %service,
                            project = %project,
                            container = %container_name,
                            "removing dangling shared service container during rm"
                        );
                        let mut volume_names: Vec<String> = Vec::new();
                        if let Some(binds) =
                            inspect.host_config.as_ref().and_then(|h| h.binds.as_ref())
                        {
                            for bind_str in binds {
                                if let Some(vol_name) =
                                    crate::shared_services::extract_named_volume(bind_str)
                                {
                                    volume_names.push(vol_name.to_string());
                                }
                            }
                        }
                        let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
                        let _ = runtime.stop_coast_container(&container_name).await;
                        if let Err(e) = runtime.remove_coast_container(&container_name).await {
                            warn!(container = %container_name, error = %e, "failed to remove dangling shared service container");
                        }
                        for vol_name in &volume_names {
                            let _ = docker.remove_volume(vol_name, None).await;
                        }
                        let vol_msg = if volume_names.is_empty() {
                            String::new()
                        } else {
                            format!(" Removed {} volume(s).", volume_names.len())
                        };
                        state.emit_event(CoastEvent::SharedServiceRemoved {
                            project: project.clone(),
                            service: service.clone(),
                        });
                        return Ok(SharedResponse {
                            message: format!(
                                "Removed dangling shared service '{service}' from project '{project}'.{vol_msg}"
                            ),
                            services: Vec::new(),
                        });
                    }
                }
                return Err(CoastError::state(format!(
                    "Shared service '{}' not found in project '{}'. \
                     Run `coast shared-services ps` to see available services.",
                    service, project
                )));
            }
        }
    };

    let mut volume_names: Vec<String> = Vec::new();

    if let Some(ref cid) = container_id {
        if let Some(ref docker) = state.docker {
            // Inspect the container before removal to discover its volumes.
            if let Ok(inspect) = docker.inspect_container(cid, None).await {
                if let Some(binds) = inspect.host_config.as_ref().and_then(|h| h.binds.as_ref()) {
                    for bind_str in binds {
                        if let Some(vol_name) =
                            crate::shared_services::extract_named_volume(bind_str)
                        {
                            volume_names.push(vol_name.to_string());
                        }
                    }
                }
            }

            let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
            if let Err(e) = runtime.stop_coast_container(cid).await {
                tracing::warn!(
                    container_id = %cid,
                    error = %e,
                    "failed to stop shared service container, it may already be stopped"
                );
            }
            if let Err(e) = runtime.remove_coast_container(cid).await {
                tracing::warn!(
                    container_id = %cid,
                    error = %e,
                    "failed to remove shared service container"
                );
            }

            for vol_name in &volume_names {
                match docker.remove_volume(vol_name, None).await {
                    Ok(_) => info!(volume = %vol_name, "removed shared service volume"),
                    Err(e) => tracing::warn!(
                        volume = %vol_name,
                        error = %e,
                        "failed to remove shared service volume (may be in use)"
                    ),
                }
            }
        }
    }

    {
        let db = state.db.lock().await;
        db.delete_shared_service(&project, &service)?;
    }

    let vol_msg = if volume_names.is_empty() {
        String::new()
    } else {
        format!(" Removed {} volume(s).", volume_names.len())
    };

    info!(
        project = %project,
        service = %service,
        volumes_removed = volume_names.len(),
        "shared service and its volumes removed"
    );

    state.emit_event(CoastEvent::SharedServiceRemoved {
        project: project.clone(),
        service: service.clone(),
    });

    Ok(SharedResponse {
        message: format!("Shared service '{service}' removed from project '{project}'.{vol_msg}",),
        services: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_shared_ps_empty() {
        let state = test_state();
        let req = SharedRequest::Ps {
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());
    }

    #[tokio::test]
    async fn test_shared_ps_with_services() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "running")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Ps {
            project: "my-app".to_string(),
        };
        let result = handle(req, &state).await.unwrap();
        assert_eq!(result.services.len(), 2);
    }

    #[tokio::test]
    async fn test_shared_stop_all_empty() {
        let state = test_state();
        let req = SharedRequest::Stop {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());
        assert!(resp.message.contains("0"));
    }

    #[tokio::test]
    async fn test_shared_start_all_empty() {
        let state = test_state();
        let req = SharedRequest::Start {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());
    }

    #[tokio::test]
    async fn test_shared_stop_single_nonexistent() {
        let state = test_state();
        let req = SharedRequest::Stop {
            project: "my-app".to_string(),
            service: Some("nonexistent".to_string()),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());
    }

    #[tokio::test]
    async fn test_shared_rm_existing() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Rm {
            project: "my-app".to_string(),
            service: "redis".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.message.contains("removed"));

        let db = state.db.lock().await;
        let svc = db.get_shared_service("my-app", "redis").unwrap();
        assert!(svc.is_none());
    }

    #[tokio::test]
    async fn test_shared_rm_nonexistent() {
        let state = test_state();
        let req = SharedRequest::Rm {
            project: "my-app".to_string(),
            service: "nonexistent".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_shared_start_all_with_services() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "stopped")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "stopped")
                .unwrap();
        }

        let req = SharedRequest::Start {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.services.len(), 2);
        assert!(resp.message.contains("2"));
        for svc in &resp.services {
            assert_eq!(svc.status, "running");
        }

        let mut starting_count = 0;
        let mut started_count = 0;
        while let Ok(event) = rx.try_recv() {
            match event {
                CoastEvent::SharedServiceStarting { .. } => starting_count += 1,
                CoastEvent::SharedServiceStarted { .. } => started_count += 1,
                _ => {}
            }
        }
        assert_eq!(starting_count, 2);
        assert_eq!(started_count, 2);
    }

    #[tokio::test]
    async fn test_shared_start_single_with_service() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "stopped")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "stopped")
                .unwrap();
        }

        let req = SharedRequest::Start {
            project: "my-app".to_string(),
            service: Some("postgres".to_string()),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.services.len(), 1);
        assert_eq!(resp.services[0].name, "postgres");
        assert_eq!(resp.services[0].status, "running");

        let mut started_services = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let CoastEvent::SharedServiceStarted { service, .. } = event {
                started_services.push(service);
            }
        }
        assert_eq!(started_services, vec!["postgres"]);
    }

    #[tokio::test]
    async fn test_shared_stop_all_with_services() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "running")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Stop {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.services.len(), 2);
        for svc in &resp.services {
            assert_eq!(svc.status, "stopped");
        }

        let mut stopped_count = 0;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, CoastEvent::SharedServiceStopped { .. }) {
                stopped_count += 1;
            }
        }
        assert_eq!(stopped_count, 2);
    }

    #[tokio::test]
    async fn test_shared_restart_single() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Restart {
            project: "my-app".to_string(),
            service: Some("postgres".to_string()),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.services.len(), 1);
        assert_eq!(resp.services[0].name, "postgres");
        assert_eq!(resp.services[0].status, "running");

        let mut restarted_services = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let CoastEvent::SharedServiceRestarted { service, .. } = event {
                restarted_services.push(service);
            }
        }
        assert_eq!(restarted_services, vec!["postgres"]);
    }

    #[tokio::test]
    async fn test_shared_restart_all() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "running")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Restart {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.services.len(), 2);
        assert!(resp.message.contains("2"));

        let mut restarted_count = 0;
        while let Ok(event) = rx.try_recv() {
            if matches!(event, CoastEvent::SharedServiceRestarted { .. }) {
                restarted_count += 1;
            }
        }
        assert_eq!(restarted_count, 2);
    }

    #[tokio::test]
    async fn test_shared_restart_all_empty() {
        let state = test_state();
        let req = SharedRequest::Restart {
            project: "my-app".to_string(),
            service: None,
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());
        assert!(resp.message.contains("0"));
    }

    #[tokio::test]
    async fn test_shared_rm_emits_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
        }

        let req = SharedRequest::Rm {
            project: "my-app".to_string(),
            service: "redis".to_string(),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());

        let mut found = false;
        while let Ok(event) = rx.try_recv() {
            if let CoastEvent::SharedServiceRemoved { project, service } = event {
                assert_eq!(project, "my-app");
                assert_eq!(service, "redis");
                found = true;
            }
        }
        assert!(found, "expected SharedServiceRemoved event");
    }

    #[tokio::test]
    async fn test_shared_start_nonexistent_emits_error_event() {
        let state = test_state();
        let mut rx = state.event_bus.subscribe();

        let req = SharedRequest::Start {
            project: "my-app".to_string(),
            service: Some("nonexistent".to_string()),
        };
        let result = handle(req, &state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.services.is_empty());

        let mut error_found = false;
        while let Ok(event) = rx.try_recv() {
            if let CoastEvent::SharedServiceError { service, .. } = event {
                assert_eq!(service, "nonexistent");
                error_found = true;
            }
        }
        assert!(error_found, "expected SharedServiceError event");
    }
}
