//! Handler for `coast ssg *` requests (non-streaming variants).
//!
//! Phase 2 landed `Ps`. Phase 3 lands the full lifecycle:
//! `Stop`, `Rm`, `Logs` (non-follow), `Exec`, `Ports`. The streaming
//! variants (`Build`, `Run`, `Start`, `Restart`, `Logs { follow: true }`)
//! never reach this handler — they are intercepted by the streaming
//! routers in `server.rs`.
//!
//! Mutating verbs acquire `AppState.ssg_mutex` before dispatching
//! into `coast_ssg::daemon_integration`. Read-only verbs do not.
//! See `coast-ssg/DESIGN.md §17-5` for mutex scope.
//!
//! Lifecycle functions do not touch the SQLite state DB themselves —
//! this handler reads the current state before the async Docker
//! section and applies writes afterwards. That split exists because
//! `StateDb` wraps a `!Sync` `rusqlite::Connection`, which would
//! otherwise reject the `Send` bound on streaming futures.

use std::sync::Arc;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SsgRequest, SsgResponse};
use coast_ssg::state::{SsgRecord, SsgStateExt};

use crate::server::AppState;

/// Dispatch a non-streaming SSG request.
///
/// `Build`, `Run`, `Start`, `Restart`, and `Logs { follow: true }`
/// never reach this handler — they are intercepted upstream.
pub async fn handle(state: Arc<AppState>, req: SsgRequest) -> Result<SsgResponse> {
    match req {
        SsgRequest::Ps => coast_ssg::daemon_integration::ps_ssg(),
        SsgRequest::Ports => {
            let db = state.db.lock().await;
            coast_ssg::daemon_integration::ports_ssg(&*db)
        }

        SsgRequest::Stop => handle_stop(&state).await,
        SsgRequest::Rm { with_data } => handle_rm(&state, with_data).await,

        SsgRequest::Logs {
            service,
            tail,
            follow,
        } => {
            if follow {
                unreachable!(
                    "SsgRequest::Logs {{ follow: true }} handled by handle_ssg_logs_streaming"
                )
            }
            handle_logs(&state, service, tail).await
        }

        SsgRequest::Exec { service, command } => handle_exec(&state, service, command).await,

        SsgRequest::Run => {
            unreachable!("SsgRequest::Run handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Start => {
            unreachable!("SsgRequest::Start handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Restart => {
            unreachable!("SsgRequest::Restart handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Build { .. } => {
            unreachable!("SsgRequest::Build handled by handle_ssg_build_streaming")
        }

        SsgRequest::Checkout { .. } | SsgRequest::Uncheckout { .. } => Err(CoastError::state(
            "coast ssg checkout / uncheckout are not yet implemented (phase 6)",
        )),
    }
}

async fn handle_stop(state: &Arc<AppState>) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot stop the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };
    let Some(record) = record else {
        return Ok(SsgResponse {
            message: "SSG has not been created. Nothing to stop.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
        });
    };

    coast_ssg::daemon_integration::stop_ssg(&docker, &record).await?;

    let db = state.db.lock().await;
    db.upsert_ssg(
        "stopped",
        record.container_id.as_deref(),
        record.build_id.as_deref(),
    )?;
    for svc in db.list_ssg_services()? {
        db.update_ssg_service_status(&svc.service_name, "stopped")?;
    }

    Ok(SsgResponse {
        message: "SSG stopped.".to_string(),
        status: Some("stopped".to_string()),
        services: Vec::new(),
        ports: Vec::new(),
    })
}

async fn handle_rm(state: &Arc<AppState>, with_data: bool) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot remove the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };
    let Some(record) = record else {
        return Ok(SsgResponse {
            message: "SSG has not been created. Nothing to remove.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
        });
    };

    coast_ssg::daemon_integration::rm_ssg(&docker, &record, with_data).await?;

    let db = state.db.lock().await;
    db.clear_ssg()?;
    db.clear_ssg_services()?;

    let suffix = if with_data { " (with data)" } else { "" };
    Ok(SsgResponse {
        message: format!("SSG removed{suffix}."),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
    })
}

async fn handle_logs(
    state: &Arc<AppState>,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot tail SSG logs."))?;

    let record = fetch_required_record(state).await?;
    let text = coast_ssg::daemon_integration::logs_ssg(&docker, &record, service, tail).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
    })
}

async fn handle_exec(
    state: &Arc<AppState>,
    service: Option<String>,
    command: Vec<String>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot exec against the SSG."))?;

    let record = fetch_required_record(state).await?;
    let text = coast_ssg::daemon_integration::exec_ssg(&docker, &record, service, command).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
    })
}

async fn fetch_required_record(state: &Arc<AppState>) -> Result<SsgRecord> {
    let db = state.db.lock().await;
    db.get_ssg()?.ok_or_else(|| {
        CoastError::coastfile("SSG has not been created. Run `coast ssg run` first.")
    })
}
