/// `coast remote` handlers — manage remote VMs for remote development.
use std::path::PathBuf;
use std::sync::Arc;

use tracing::{error, info, warn};

use crate::remote::{MutagenManager, RemoteSetup};
use crate::server::AppState;
use crate::state::remotes::{Remote, SyncStatus, TunnelStatus};
use coast_core::protocol::*;

/// Handle a RemoteRequest and return a RemoteResponse.
pub async fn handle_remote(req: RemoteRequest, state: &Arc<AppState>) -> Response {
    match req {
        RemoteRequest::Add(r) => handle_remote_add(r, state).await,
        RemoteRequest::Remove(r) => handle_remote_remove(r, state).await,
        RemoteRequest::List(_) => handle_remote_list(state).await,
        RemoteRequest::Setup(r) => handle_remote_setup(r, state).await,
        RemoteRequest::Ping(r) => handle_remote_ping(r, state).await,
        RemoteRequest::Connect(r) => handle_remote_connect(r, state).await,
        RemoteRequest::Disconnect(r) => handle_remote_disconnect(r, state).await,
    }
}

/// Handle a SyncRequest and return a SyncResponse.
pub async fn handle_sync(req: SyncRequest, state: &Arc<AppState>) -> Response {
    match req {
        SyncRequest::Status(r) => handle_sync_status(r, state).await,
        SyncRequest::Pause(r) => handle_sync_pause(r, state).await,
        SyncRequest::Resume(r) => handle_sync_resume(r, state).await,
        SyncRequest::Flush(r) => handle_sync_flush(r, state).await,
        SyncRequest::Create(r) => handle_sync_create(r, state).await,
        SyncRequest::Terminate(r) => handle_sync_terminate(r, state).await,
    }
}

// ---------------------------------------------------------------------------
// Remote Handlers
// ---------------------------------------------------------------------------

async fn handle_remote_add(req: RemoteAddRequest, state: &Arc<AppState>) -> Response {
    let remote = Remote {
        name: req.name.clone(),
        host: req.host,
        user: req.user,
        port: req.port,
        workspace_root: req.workspace_root,
        ssh_key_path: req.ssh_key_path,
        created_at: chrono::Utc::now(),
    };

    let db = state.db.lock().await;
    match db.upsert_remote(&remote) {
        Ok(_) => {
            info!(remote = %req.name, "remote added");
            Response::Remote(RemoteResponse::Add(RemoteAddResponse {
                name: req.name,
                message: "Remote added successfully".to_string(),
            }))
        }
        Err(e) => {
            error!(remote = %req.name, error = %e, "failed to add remote");
            Response::Error(ErrorResponse {
                error: format!("Failed to add remote: {}", e),
            })
        }
    }
}

#[allow(clippy::cognitive_complexity)]
async fn handle_remote_remove(req: RemoteRemoveRequest, state: &Arc<AppState>) -> Response {
    // First, disconnect any active tunnel
    if let Some(tunnel_manager) = state.tunnel_manager.as_ref() {
        match tunnel_manager.disconnect(&req.name).await {
            Ok(result) => {
                // Persist tunnel state if we have one
                if let Some(ref tunnel_state) = result.tunnel_state {
                    let db = state.db.lock().await;
                    if let Err(e) = db.upsert_tunnel(tunnel_state) {
                        warn!(remote = %req.name, error = %e, "failed to update tunnel state during removal");
                    }
                }
            }
            Err(e) => {
                warn!(remote = %req.name, error = %e, "failed to disconnect tunnel during removal");
            }
        }
    }

    let db = state.db.lock().await;
    match db.delete_remote(&req.name) {
        Ok(removed) => {
            if removed {
                info!(remote = %req.name, "remote removed");
                Response::Remote(RemoteResponse::Remove(RemoteRemoveResponse {
                    removed: true,
                    message: format!("Remote '{}' removed", req.name),
                }))
            } else {
                Response::Remote(RemoteResponse::Remove(RemoteRemoveResponse {
                    removed: false,
                    message: format!("Remote '{}' not found", req.name),
                }))
            }
        }
        Err(e) => {
            error!(remote = %req.name, error = %e, "failed to remove remote");
            Response::Error(ErrorResponse {
                error: format!("Failed to remove remote: {}", e),
            })
        }
    }
}

async fn handle_remote_list(state: &Arc<AppState>) -> Response {
    let db = state.db.lock().await;
    match db.list_remotes() {
        Ok(remotes) => {
            let tunnel_statuses = if let Some(tm) = state.tunnel_manager.as_ref() {
                tm.get_tunnel_statuses().await
            } else {
                std::collections::HashMap::new()
            };

            let remote_infos: Vec<RemoteInfo> = remotes
                .into_iter()
                .map(|r| {
                    let tunnel_status = tunnel_statuses
                        .get(&r.name)
                        .map(|s| match s {
                            TunnelStatus::Connected => "connected",
                            TunnelStatus::Connecting => "connecting",
                            TunnelStatus::Disconnected => "disconnected",
                            TunnelStatus::Error => "error",
                        })
                        .map(String::from);

                    // Count projects using this remote
                    let project_count = db.count_projects_for_remote(&r.name).unwrap_or(0) as u32;

                    RemoteInfo {
                        name: r.name,
                        host: r.host,
                        user: r.user,
                        port: r.port,
                        workspace_root: r.workspace_root,
                        ssh_key_path: r.ssh_key_path,
                        tunnel_status,
                        project_count,
                    }
                })
                .collect();

            Response::Remote(RemoteResponse::List(RemoteListResponse {
                remotes: remote_infos,
            }))
        }
        Err(e) => {
            error!(error = %e, "failed to list remotes");
            Response::Error(ErrorResponse {
                error: format!("Failed to list remotes: {}", e),
            })
        }
    }
}

async fn handle_remote_setup(req: RemoteSetupRequest, state: &Arc<AppState>) -> Response {
    // Get the remote configuration
    let remote = {
        let db = state.db.lock().await;
        match db.get_remote(&req.name) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::Error(ErrorResponse {
                    error: format!("Remote '{}' not found", req.name),
                });
            }
            Err(e) => {
                return Response::Error(ErrorResponse {
                    error: format!("Failed to get remote: {}", e),
                });
            }
        }
    };

    // Check if coastd is already installed (unless force is set)
    let setup = RemoteSetup::new();
    if !req.force {
        match setup.check_coastd(&remote).await {
            Ok(Some(version)) => {
                return Response::Remote(RemoteResponse::Setup(RemoteSetupResponse {
                    success: true,
                    version: Some(version),
                    message: "coastd is already installed".to_string(),
                }));
            }
            Ok(None) => {
                // Not installed, continue with setup
            }
            Err(e) => {
                warn!(remote = %req.name, error = %e, "failed to check coastd status");
                // Continue with setup attempt
            }
        }
    }

    // Perform full setup
    match setup.full_setup(&remote, None).await {
        Ok(_) => {
            // Get the installed version
            let version = setup.check_coastd(&remote).await.ok().flatten();

            Response::Remote(RemoteResponse::Setup(RemoteSetupResponse {
                success: true,
                version,
                message: "coastd installed and started successfully".to_string(),
            }))
        }
        Err(e) => {
            error!(remote = %req.name, error = %e, "remote setup failed");
            Response::Remote(RemoteResponse::Setup(RemoteSetupResponse {
                success: false,
                version: None,
                message: format!("Setup failed: {}", e),
            }))
        }
    }
}

async fn handle_remote_ping(req: RemotePingRequest, state: &Arc<AppState>) -> Response {
    // Get the remote configuration
    let remote = {
        let db = state.db.lock().await;
        match db.get_remote(&req.name) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::Remote(RemoteResponse::Ping(RemotePingResponse {
                    reachable: false,
                    ssh_ok: false,
                    daemon_ok: false,
                    daemon_version: None,
                    latency_ms: None,
                    error: Some(format!("Remote '{}' not found", req.name)),
                }));
            }
            Err(e) => {
                return Response::Remote(RemoteResponse::Ping(RemotePingResponse {
                    reachable: false,
                    ssh_ok: false,
                    daemon_ok: false,
                    daemon_version: None,
                    latency_ms: None,
                    error: Some(format!("Failed to get remote: {}", e)),
                }));
            }
        }
    };

    let setup = RemoteSetup::new();
    let start = std::time::Instant::now();

    // Check SSH connectivity
    let ssh_ok = match setup.check_connectivity(&remote).await {
        Ok(true) => true,
        Ok(false) => false,
        Err(_) => false,
    };

    if !ssh_ok {
        return Response::Remote(RemoteResponse::Ping(RemotePingResponse {
            reachable: false,
            ssh_ok: false,
            daemon_ok: false,
            daemon_version: None,
            latency_ms: None,
            error: Some("SSH connection failed".to_string()),
        }));
    }

    // Check coastd status
    let (daemon_ok, daemon_version) = match setup.verify_coastd(&remote).await {
        Ok(true) => {
            let version = setup.check_coastd(&remote).await.ok().flatten();
            (true, version)
        }
        Ok(false) => (false, None),
        Err(_) => (false, None),
    };

    let latency_ms = start.elapsed().as_millis() as u64;

    Response::Remote(RemoteResponse::Ping(RemotePingResponse {
        reachable: true,
        ssh_ok,
        daemon_ok,
        daemon_version,
        latency_ms: Some(latency_ms),
        error: None,
    }))
}

async fn handle_remote_connect(req: RemoteConnectRequest, state: &Arc<AppState>) -> Response {
    // Get the remote configuration
    let remote = {
        let db = state.db.lock().await;
        match db.get_remote(&req.name) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::Remote(RemoteResponse::Connect(RemoteConnectResponse {
                    connected: false,
                    local_port: None,
                    message: format!("Remote '{}' not found", req.name),
                }));
            }
            Err(e) => {
                return Response::Remote(RemoteResponse::Connect(RemoteConnectResponse {
                    connected: false,
                    local_port: None,
                    message: format!("Failed to get remote: {}", e),
                }));
            }
        }
    };

    // Get or create tunnel manager
    let Some(tunnel_manager) = state.tunnel_manager.as_ref() else {
        return Response::Remote(RemoteResponse::Connect(RemoteConnectResponse {
            connected: false,
            local_port: None,
            message: "Tunnel manager not initialized".to_string(),
        }));
    };

    // Establish connection
    match tunnel_manager.connect(&remote).await {
        Ok(result) => {
            // Persist tunnel state
            let db = state.db.lock().await;
            if let Err(e) = db.upsert_tunnel(&result.tunnel_state) {
                warn!(remote = %req.name, error = %e, "failed to persist tunnel state");
            }

            info!(remote = %req.name, local_port = result.local_port, "connected to remote");
            Response::Remote(RemoteResponse::Connect(RemoteConnectResponse {
                connected: true,
                local_port: Some(result.local_port),
                message: format!("Connected to '{}'", req.name),
            }))
        }
        Err(e) => {
            error!(remote = %req.name, error = %e, "failed to connect to remote");
            Response::Remote(RemoteResponse::Connect(RemoteConnectResponse {
                connected: false,
                local_port: None,
                message: format!("Failed to connect: {}", e),
            }))
        }
    }
}

async fn handle_remote_disconnect(req: RemoteDisconnectRequest, state: &Arc<AppState>) -> Response {
    let Some(tunnel_manager) = state.tunnel_manager.as_ref() else {
        return Response::Remote(RemoteResponse::Disconnect(RemoteDisconnectResponse {
            disconnected: false,
            message: "Tunnel manager not initialized".to_string(),
        }));
    };

    match tunnel_manager.disconnect(&req.name).await {
        Ok(result) => {
            // Persist tunnel state if we have one
            if let Some(ref tunnel_state) = result.tunnel_state {
                let db = state.db.lock().await;
                if let Err(e) = db.upsert_tunnel(tunnel_state) {
                    warn!(remote = %req.name, error = %e, "failed to update tunnel state");
                }
            }

            if result.disconnected {
                info!(remote = %req.name, "disconnected from remote");
                Response::Remote(RemoteResponse::Disconnect(RemoteDisconnectResponse {
                    disconnected: true,
                    message: format!("Disconnected from '{}'", req.name),
                }))
            } else {
                Response::Remote(RemoteResponse::Disconnect(RemoteDisconnectResponse {
                    disconnected: false,
                    message: format!("No active connection to '{}'", req.name),
                }))
            }
        }
        Err(e) => {
            error!(remote = %req.name, error = %e, "failed to disconnect from remote");
            Response::Remote(RemoteResponse::Disconnect(RemoteDisconnectResponse {
                disconnected: false,
                message: format!("Failed to disconnect: {}", e),
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Sync Handlers
// ---------------------------------------------------------------------------

async fn handle_sync_status(req: SyncStatusRequest, state: &Arc<AppState>) -> Response {
    // If mutagen_manager is available, get live status from Mutagen
    if let Some(mutagen_manager) = state.mutagen_manager.as_ref() {
        match mutagen_manager.list_all_sessions().await {
            Ok(mutagen_sessions) => {
                // Also get database sessions for metadata
                let db_sessions = {
                    let db = state.db.lock().await;
                    db.list_sync_sessions().unwrap_or_default()
                };

                // Merge Mutagen live status with database metadata
                let session_infos: Vec<SyncSessionInfo> = if let Some(ref project) = req.project {
                    // Filter by project
                    mutagen_sessions
                        .iter()
                        .filter(|s| {
                            s.name.as_ref().map_or(false, |n| n.contains(project))
                        })
                        .map(|s| {
                            let db_session = db_sessions.iter().find(|ds| {
                                MutagenManager::generate_session_name(&ds.project, "main", &ds.remote_name)
                                    == s.name.clone().unwrap_or_default()
                            });
                            SyncSessionInfo {
                                project: db_session.map(|ds| ds.project.clone()).unwrap_or_else(|| {
                                    s.name.clone().unwrap_or_default()
                                }),
                                remote_name: db_session.map(|ds| ds.remote_name.clone()).unwrap_or_default(),
                                local_path: db_session.map(|ds| ds.local_path.clone()).unwrap_or_default(),
                                remote_path: db_session.map(|ds| ds.remote_path.clone()).unwrap_or_default(),
                                status: s.status.clone().unwrap_or_else(|| "unknown".to_string()),
                                last_sync_at: db_session.and_then(|ds| ds.last_sync_at.map(|t| t.to_rfc3339())),
                            }
                        })
                        .collect()
                } else {
                    // Return all sessions
                    mutagen_sessions
                        .iter()
                        .map(|s| {
                            let db_session = db_sessions.iter().find(|ds| {
                                MutagenManager::generate_session_name(&ds.project, "main", &ds.remote_name)
                                    == s.name.clone().unwrap_or_default()
                            });
                            SyncSessionInfo {
                                project: db_session.map(|ds| ds.project.clone()).unwrap_or_else(|| {
                                    s.name.clone().unwrap_or_default()
                                }),
                                remote_name: db_session.map(|ds| ds.remote_name.clone()).unwrap_or_default(),
                                local_path: db_session.map(|ds| ds.local_path.clone()).unwrap_or_default(),
                                remote_path: db_session.map(|ds| ds.remote_path.clone()).unwrap_or_default(),
                                status: s.status.clone().unwrap_or_else(|| "unknown".to_string()),
                                last_sync_at: db_session.and_then(|ds| ds.last_sync_at.map(|t| t.to_rfc3339())),
                            }
                        })
                        .collect()
                };

                return Response::Sync(SyncResponse::Status(SyncStatusResponse {
                    sessions: session_infos,
                }));
            }
            Err(e) => {
                warn!(error = %e, "failed to get Mutagen sessions, falling back to database");
            }
        }
    }

    // Fallback: Get sessions from database only
    let db = state.db.lock().await;
    match db.list_sync_sessions() {
        Ok(sessions) => {
            let session_infos: Vec<SyncSessionInfo> = sessions
                .into_iter()
                .filter(|s| req.project.as_ref().map_or(true, |p| &s.project == p))
                .map(|s| SyncSessionInfo {
                    project: s.project,
                    remote_name: s.remote_name,
                    local_path: s.local_path,
                    remote_path: s.remote_path,
                    status: format!("{:?}", s.status),
                    last_sync_at: s.last_sync_at.map(|t| t.to_rfc3339()),
                })
                .collect();

            Response::Sync(SyncResponse::Status(SyncStatusResponse {
                sessions: session_infos,
            }))
        }
        Err(e) => {
            error!(error = %e, "failed to list sync sessions");
            Response::Error(ErrorResponse {
                error: format!("Failed to list sync sessions: {}", e),
            })
        }
    }
}

async fn handle_sync_pause(req: SyncPauseRequest, state: &Arc<AppState>) -> Response {
    let Some(mutagen_manager) = state.mutagen_manager.as_ref() else {
        return Response::Sync(SyncResponse::Pause(SyncPauseResponse {
            paused: false,
            message: "Mutagen manager not initialized".to_string(),
        }));
    };

    // Find the session for this project
    let session = mutagen_manager.get_session_by_project(&req.project).await;

    let session_name = match session {
        Some(s) => s.session_name,
        None => {
            // Try to construct session name from database
            let db = state.db.lock().await;
            match db.get_sync_session(&req.project) {
                Ok(Some(s)) => {
                    MutagenManager::generate_session_name(&s.project, "main", &s.remote_name)
                }
                _ => {
                    return Response::Sync(SyncResponse::Pause(SyncPauseResponse {
                        paused: false,
                        message: format!("No sync session found for project '{}'", req.project),
                    }));
                }
            }
        }
    };

    match mutagen_manager.pause_session(&session_name).await {
        Ok(()) => {
            // Update database status
            let db = state.db.lock().await;
            if let Err(e) = db.update_sync_session_status(&req.project, SyncStatus::Paused) {
                warn!(project = %req.project, error = %e, "failed to update sync status in database");
            }

            info!(project = %req.project, session = %session_name, "sync session paused");
            Response::Sync(SyncResponse::Pause(SyncPauseResponse {
                paused: true,
                message: format!("Sync paused for project '{}'", req.project),
            }))
        }
        Err(e) => {
            error!(project = %req.project, error = %e, "failed to pause sync session");
            Response::Sync(SyncResponse::Pause(SyncPauseResponse {
                paused: false,
                message: format!("Failed to pause sync: {}", e),
            }))
        }
    }
}

async fn handle_sync_resume(req: SyncResumeRequest, state: &Arc<AppState>) -> Response {
    let Some(mutagen_manager) = state.mutagen_manager.as_ref() else {
        return Response::Sync(SyncResponse::Resume(SyncResumeResponse {
            resumed: false,
            message: "Mutagen manager not initialized".to_string(),
        }));
    };

    // Find the session for this project
    let session = mutagen_manager.get_session_by_project(&req.project).await;

    let session_name = match session {
        Some(s) => s.session_name,
        None => {
            // Try to construct session name from database
            let db = state.db.lock().await;
            match db.get_sync_session(&req.project) {
                Ok(Some(s)) => {
                    MutagenManager::generate_session_name(&s.project, "main", &s.remote_name)
                }
                _ => {
                    return Response::Sync(SyncResponse::Resume(SyncResumeResponse {
                        resumed: false,
                        message: format!("No sync session found for project '{}'", req.project),
                    }));
                }
            }
        }
    };

    match mutagen_manager.resume_session(&session_name).await {
        Ok(()) => {
            // Update database status
            let db = state.db.lock().await;
            if let Err(e) = db.update_sync_session_status(&req.project, SyncStatus::Syncing) {
                warn!(project = %req.project, error = %e, "failed to update sync status in database");
            }

            info!(project = %req.project, session = %session_name, "sync session resumed");
            Response::Sync(SyncResponse::Resume(SyncResumeResponse {
                resumed: true,
                message: format!("Sync resumed for project '{}'", req.project),
            }))
        }
        Err(e) => {
            error!(project = %req.project, error = %e, "failed to resume sync session");
            Response::Sync(SyncResponse::Resume(SyncResumeResponse {
                resumed: false,
                message: format!("Failed to resume sync: {}", e),
            }))
        }
    }
}

async fn handle_sync_flush(req: SyncFlushRequest, state: &Arc<AppState>) -> Response {
    let Some(mutagen_manager) = state.mutagen_manager.as_ref() else {
        return Response::Sync(SyncResponse::Flush(SyncFlushResponse {
            flushed: false,
            message: "Mutagen manager not initialized".to_string(),
        }));
    };

    // Find the session for this project
    let session = mutagen_manager.get_session_by_project(&req.project).await;

    let session_name = match session {
        Some(s) => s.session_name,
        None => {
            // Try to construct session name from database
            let db = state.db.lock().await;
            match db.get_sync_session(&req.project) {
                Ok(Some(s)) => {
                    MutagenManager::generate_session_name(&s.project, "main", &s.remote_name)
                }
                _ => {
                    return Response::Sync(SyncResponse::Flush(SyncFlushResponse {
                        flushed: false,
                        message: format!("No sync session found for project '{}'", req.project),
                    }));
                }
            }
        }
    };

    match mutagen_manager.flush_session(&session_name).await {
        Ok(()) => {
            // Update last_sync_at in database
            let db = state.db.lock().await;
            if let Err(e) = db.update_sync_last_sync_at(&req.project) {
                warn!(project = %req.project, error = %e, "failed to update last_sync_at in database");
            }

            info!(project = %req.project, session = %session_name, "sync session flushed");
            Response::Sync(SyncResponse::Flush(SyncFlushResponse {
                flushed: true,
                message: format!("Sync flushed for project '{}'", req.project),
            }))
        }
        Err(e) => {
            error!(project = %req.project, error = %e, "failed to flush sync session");
            Response::Sync(SyncResponse::Flush(SyncFlushResponse {
                flushed: false,
                message: format!("Failed to flush sync: {}", e),
            }))
        }
    }
}

async fn handle_sync_create(req: SyncCreateRequest, state: &Arc<AppState>) -> Response {
    let Some(mutagen_manager) = state.mutagen_manager.as_ref() else {
        return Response::Sync(SyncResponse::Create(SyncCreateResponse {
            created: false,
            session_name: None,
            message: "Mutagen manager not initialized".to_string(),
        }));
    };

    // Get the remote configuration
    let remote = {
        let db = state.db.lock().await;
        match db.get_remote(&req.remote_name) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::Sync(SyncResponse::Create(SyncCreateResponse {
                    created: false,
                    session_name: None,
                    message: format!("Remote '{}' not found", req.remote_name),
                }));
            }
            Err(e) => {
                return Response::Sync(SyncResponse::Create(SyncCreateResponse {
                    created: false,
                    session_name: None,
                    message: format!("Failed to get remote: {}", e),
                }));
            }
        }
    };

    // Determine local path - must be provided explicitly
    let local_path = match req.local_path {
        Some(path) => PathBuf::from(path),
        None => {
            return Response::Sync(SyncResponse::Create(SyncCreateResponse {
                created: false,
                session_name: None,
                message: format!(
                    "local_path is required for sync create. Specify the path to sync for project '{}'.",
                    req.project
                ),
            }));
        }
    };

    // Verify local path exists
    if !local_path.exists() {
        return Response::Sync(SyncResponse::Create(SyncCreateResponse {
            created: false,
            session_name: None,
            message: format!("Local path '{}' does not exist", local_path.display()),
        }));
    }

    // Create the sync session
    match mutagen_manager
        .create_session(&req.project, &req.branch, &remote, &local_path)
        .await
    {
        Ok(result) => {
            // Persist to database
            let db = state.db.lock().await;
            if let Err(e) = db.upsert_sync_session(&result.db_session) {
                warn!(project = %req.project, error = %e, "failed to persist sync session to database");
            }

            info!(
                project = %req.project,
                branch = %req.branch,
                remote = %req.remote_name,
                session = %result.session.session_name,
                "sync session created"
            );

            Response::Sync(SyncResponse::Create(SyncCreateResponse {
                created: true,
                session_name: Some(result.session.session_name),
                message: format!(
                    "Sync session created: {} → {}",
                    local_path.display(),
                    result.session.remote_path
                ),
            }))
        }
        Err(e) => {
            error!(project = %req.project, error = %e, "failed to create sync session");
            Response::Sync(SyncResponse::Create(SyncCreateResponse {
                created: false,
                session_name: None,
                message: format!("Failed to create sync session: {}", e),
            }))
        }
    }
}

async fn handle_sync_terminate(req: SyncTerminateRequest, state: &Arc<AppState>) -> Response {
    let Some(mutagen_manager) = state.mutagen_manager.as_ref() else {
        return Response::Sync(SyncResponse::Terminate(SyncTerminateResponse {
            terminated: false,
            message: "Mutagen manager not initialized".to_string(),
        }));
    };

    // Find the session for this project
    let session = mutagen_manager.get_session_by_project(&req.project).await;

    let session_name = match session {
        Some(s) => s.session_name,
        None => {
            // Try to construct session name from database
            let db = state.db.lock().await;
            match db.get_sync_session(&req.project) {
                Ok(Some(s)) => {
                    MutagenManager::generate_session_name(&s.project, "main", &s.remote_name)
                }
                _ => {
                    return Response::Sync(SyncResponse::Terminate(SyncTerminateResponse {
                        terminated: false,
                        message: format!("No sync session found for project '{}'", req.project),
                    }));
                }
            }
        }
    };

    match mutagen_manager.terminate_session(&session_name).await {
        Ok(()) => {
            // Remove from database
            let db = state.db.lock().await;
            if let Err(e) = db.delete_sync_session(&req.project) {
                warn!(project = %req.project, error = %e, "failed to delete sync session from database");
            }

            info!(project = %req.project, session = %session_name, "sync session terminated");
            Response::Sync(SyncResponse::Terminate(SyncTerminateResponse {
                terminated: true,
                message: format!("Sync session terminated for project '{}'", req.project),
            }))
        }
        Err(e) => {
            error!(project = %req.project, error = %e, "failed to terminate sync session");
            Response::Sync(SyncResponse::Terminate(SyncTerminateResponse {
                terminated: false,
                message: format!("Failed to terminate sync session: {}", e),
            }))
        }
    }
}
