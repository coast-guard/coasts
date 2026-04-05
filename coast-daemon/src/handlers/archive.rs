/// Handlers for `coast archive` and `coast unarchive`.
///
/// Archiving stops all running instances and shared services for a project,
/// then marks it as archived in the state DB so it's hidden from the main list.
/// Unarchiving simply removes the archived flag.
use std::future::Future;

use tracing::{info, warn};

use coast_core::error::Result;
use coast_core::protocol::{
    ArchiveProjectRequest, ArchiveProjectResponse, SharedRequest, StopRequest,
    UnarchiveProjectRequest, UnarchiveProjectResponse,
};
use coast_core::types::{CoastInstance, InstanceStatus};

use crate::server::AppState;

use super::{shared, stop};

/// Handle an archive request.
pub async fn handle_archive(
    req: ArchiveProjectRequest,
    state: &AppState,
) -> Result<ArchiveProjectResponse> {
    info!(project = %req.project, "handling archive request");

    let running_instances = running_instances_for_project(&req.project, state).await?;
    let instances_stopped = stop_running_instances(&req.project, &running_instances, state).await;
    let running_shared_services = running_shared_services_for_project(&req.project, state).await?;
    let shared_services_stopped =
        stop_project_shared_services_if_needed(&req.project, running_shared_services, state).await;
    archive_project_in_db(&req.project, state).await?;

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

async fn running_instances_for_project(
    project: &str,
    state: &AppState,
) -> Result<Vec<CoastInstance>> {
    let db = state.db.lock().await;
    let all = db.list_instances_for_project(project)?;
    Ok(all.into_iter().filter(is_running_instance).collect())
}

fn is_running_instance(instance: &CoastInstance) -> bool {
    matches!(
        instance.status,
        InstanceStatus::Running | InstanceStatus::CheckedOut | InstanceStatus::Idle
    )
}

async fn stop_running_instances(
    project: &str,
    running_instances: &[CoastInstance],
    state: &AppState,
) -> usize {
    stop_instances_with(project, running_instances, |stop_req| async move {
        stop::handle(stop_req, state, None).await.map(|_| ())
    })
    .await
}

async fn stop_instances_with<F, Fut>(
    project: &str,
    running_instances: &[CoastInstance],
    mut stop_instance: F,
) -> usize
where
    F: FnMut(StopRequest) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut instances_stopped = 0;
    for instance in running_instances {
        let stop_req = StopRequest {
            name: instance.name.clone(),
            project: project.to_string(),
        };
        match stop_instance(stop_req).await {
            Ok(()) => instances_stopped += 1,
            Err(error) => warn!(
                name = %instance.name,
                error = %error,
                "failed to stop instance during archive"
            ),
        }
    }
    instances_stopped
}

async fn running_shared_services_for_project(project: &str, state: &AppState) -> Result<usize> {
    let db = state.db.lock().await;
    let shared_services = db.list_shared_services(Some(project))?;
    Ok(shared_services
        .iter()
        .filter(|shared_service| shared_service.status == "running")
        .count())
}

async fn stop_project_shared_services_if_needed(
    project: &str,
    running_shared_services: usize,
    state: &AppState,
) -> usize {
    stop_shared_services_with(project, running_shared_services, |stop_req| async move {
        shared::handle(stop_req, state).await.map(|_| ())
    })
    .await
}

async fn stop_shared_services_with<F, Fut>(
    project: &str,
    running_shared_services: usize,
    mut stop_shared_services: F,
) -> usize
where
    F: FnMut(SharedRequest) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if running_shared_services == 0 {
        return 0;
    }

    let stop_req = SharedRequest::Stop {
        project: project.to_string(),
        service: None,
    };
    match stop_shared_services(stop_req).await {
        Ok(()) => running_shared_services,
        Err(error) => {
            warn!(
                error = %error,
                "failed to stop shared services during archive"
            );
            0
        }
    }
}

async fn archive_project_in_db(project: &str, state: &AppState) -> Result<()> {
    let db = state.db.lock().await;
    db.archive_project(project)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use super::*;
    use crate::state::StateDb;
    use coast_core::{error::CoastError, types::RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn make_instance(name: &str, project: &str, status: InstanceStatus) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("container-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    async fn archived_flag(state: &AppState, project: &str) -> bool {
        let db = state.db.lock().await;
        db.is_project_archived(project).unwrap()
    }

    #[tokio::test]
    async fn test_handle_archive_with_no_instances_or_shared_services() {
        let state = test_state();

        let response = handle_archive(
            ArchiveProjectRequest {
                project: "my-app".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        assert_eq!(response.project, "my-app");
        assert_eq!(response.instances_stopped, 0);
        assert_eq!(response.shared_services_stopped, 0);
        assert!(archived_flag(&state, "my-app").await);
    }

    #[tokio::test]
    async fn test_handle_archive_stops_only_running_checked_out_and_idle_instances() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "running-inst",
                "my-app",
                InstanceStatus::Running,
            ))
            .unwrap();
            db.insert_instance(&make_instance(
                "checked-out-inst",
                "my-app",
                InstanceStatus::CheckedOut,
            ))
            .unwrap();
            db.insert_instance(&make_instance("idle-inst", "my-app", InstanceStatus::Idle))
                .unwrap();
            db.insert_instance(&make_instance(
                "stopped-inst",
                "my-app",
                InstanceStatus::Stopped,
            ))
            .unwrap();
        }

        let response = handle_archive(
            ArchiveProjectRequest {
                project: "my-app".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        assert_eq!(response.instances_stopped, 3);
        assert_eq!(response.shared_services_stopped, 0);

        let db = state.db.lock().await;
        assert_eq!(
            db.get_instance("my-app", "running-inst")
                .unwrap()
                .unwrap()
                .status,
            InstanceStatus::Stopped
        );
        assert_eq!(
            db.get_instance("my-app", "checked-out-inst")
                .unwrap()
                .unwrap()
                .status,
            InstanceStatus::Stopped
        );
        assert_eq!(
            db.get_instance("my-app", "idle-inst")
                .unwrap()
                .unwrap()
                .status,
            InstanceStatus::Stopped
        );
        assert_eq!(
            db.get_instance("my-app", "stopped-inst")
                .unwrap()
                .unwrap()
                .status,
            InstanceStatus::Stopped
        );
        assert!(db.is_project_archived("my-app").unwrap());
    }

    #[tokio::test]
    async fn test_handle_archive_counts_running_shared_services() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_shared_service("my-app", "postgres", None, "running")
                .unwrap();
            db.insert_shared_service("my-app", "redis", None, "running")
                .unwrap();
            db.insert_shared_service("my-app", "minio", None, "stopped")
                .unwrap();
        }

        let response = handle_archive(
            ArchiveProjectRequest {
                project: "my-app".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        assert_eq!(response.instances_stopped, 0);
        assert_eq!(response.shared_services_stopped, 2);

        let services = {
            let db = state.db.lock().await;
            db.list_shared_services(Some("my-app")).unwrap()
        };
        assert_eq!(services.len(), 3);
        assert!(services.iter().all(|service| service.status == "stopped"));
        assert!(archived_flag(&state, "my-app").await);
    }

    #[tokio::test]
    async fn test_handle_unarchive_clears_archived_flag() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.archive_project("my-app").unwrap();
        }

        let response = handle_unarchive(
            UnarchiveProjectRequest {
                project: "my-app".to_string(),
            },
            &state,
        )
        .await
        .unwrap();

        assert_eq!(response.project, "my-app");
        assert!(!archived_flag(&state, "my-app").await);
    }

    #[tokio::test]
    async fn test_stop_instances_with_warns_and_continues_on_errors() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let running_instances = vec![
            make_instance("one", "my-app", InstanceStatus::Running),
            make_instance("two", "my-app", InstanceStatus::CheckedOut),
            make_instance("three", "my-app", InstanceStatus::Idle),
        ];

        let instances_stopped = stop_instances_with("my-app", &running_instances, {
            let seen = Arc::clone(&seen);
            move |stop_req| {
                let seen = Arc::clone(&seen);
                async move {
                    seen.lock().unwrap().push(stop_req.name.clone());
                    if stop_req.name == "two" {
                        Err(CoastError::state("boom"))
                    } else {
                        Ok(())
                    }
                }
            }
        })
        .await;

        assert_eq!(instances_stopped, 2);
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }

    #[tokio::test]
    async fn test_stop_shared_services_with_skips_call_when_none_running() {
        let calls = Arc::new(AtomicUsize::new(0));

        let shared_services_stopped = stop_shared_services_with("my-app", 0, {
            let calls = Arc::clone(&calls);
            move |_stop_req| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(()) }
            }
        })
        .await;

        assert_eq!(shared_services_stopped, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_stop_shared_services_with_warns_and_continues_on_error() {
        let shared_services_stopped = stop_shared_services_with("my-app", 2, |_stop_req| async {
            Err(CoastError::state("boom"))
        })
        .await;

        assert_eq!(shared_services_stopped, 0);
    }
}
