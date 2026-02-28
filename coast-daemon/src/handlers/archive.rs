/// Handlers for `coast archive` and `coast unarchive`.
///
/// Archiving stops all running instances and shared services for a project,
/// then marks it as archived in the state DB so it's hidden from the main list.
/// Unarchiving simply removes the archived flag.
use tracing::{info, warn};

use coast_core::error::Result;
use coast_core::protocol::{
    ArchiveProjectRequest, ArchiveProjectResponse, SharedRequest, StopRequest,
    UnarchiveProjectRequest, UnarchiveProjectResponse,
};
use coast_core::types::InstanceStatus;

use crate::server::AppState;

use super::{shared, stop};

/// Handle an archive request.
#[allow(clippy::cognitive_complexity)]
pub async fn handle_archive(
    req: ArchiveProjectRequest,
    state: &AppState,
) -> Result<ArchiveProjectResponse> {
    info!(project = %req.project, "handling archive request");

    let mut instances_stopped: usize = 0;
    let mut shared_services_stopped: usize = 0;

    // Stop all running instances
    let running_instances = {
        let db = state.db.lock().await;
        let all = db.list_instances_for_project(&req.project)?;
        all.into_iter()
            .filter(|i| {
                i.status == InstanceStatus::Running
                    || i.status == InstanceStatus::CheckedOut
                    || i.status == InstanceStatus::Idle
            })
            .collect::<Vec<_>>()
    };

    for inst in &running_instances {
        let stop_req = StopRequest {
            name: inst.name.clone(),
            project: req.project.clone(),
        };
        match stop::handle(stop_req, state, None).await {
            Ok(_) => instances_stopped += 1,
            Err(e) => warn!(
                name = %inst.name,
                error = %e,
                "failed to stop instance during archive"
            ),
        }
    }

    // Stop all shared services
    let shared_services = {
        let db = state.db.lock().await;
        db.list_shared_services(Some(&req.project))?
    };

    let running_shared: Vec<_> = shared_services
        .iter()
        .filter(|s| s.status == "running")
        .collect();

    if !running_shared.is_empty() {
        let stop_req = SharedRequest::Stop {
            project: req.project.clone(),
            service: None,
        };
        match shared::handle(stop_req, state).await {
            Ok(_) => shared_services_stopped = running_shared.len(),
            Err(e) => warn!(
                error = %e,
                "failed to stop shared services during archive"
            ),
        }
    }

    // Mark as archived in DB
    {
        let db = state.db.lock().await;
        db.archive_project(&req.project)?;
    }

    info!(
        project = %req.project,
        instances_stopped,
        shared_services_stopped,
        "project archived"
    );

    Ok(ArchiveProjectResponse {
        project: req.project,
        instances_stopped,
        shared_services_stopped,
    })
}

/// Handle an unarchive request.
pub async fn handle_unarchive(
    req: UnarchiveProjectRequest,
    state: &AppState,
) -> Result<UnarchiveProjectResponse> {
    info!(project = %req.project, "handling unarchive request");

    let db = state.db.lock().await;
    db.unarchive_project(&req.project)?;

    info!(project = %req.project, "project unarchived");

    Ok(UnarchiveProjectResponse {
        project: req.project,
    })
}
