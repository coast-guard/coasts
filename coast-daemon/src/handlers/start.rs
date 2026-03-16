/// Handler for the `coast start` command.
///
/// Starts a previously stopped coast instance: restarts the coast container,
/// waits for the inner daemon, starts the compose stack, and restarts socat.
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, StartRequest, StartResponse};
use coast_core::types::{InstanceStatus, PortMapping};
use coast_docker::runtime::Runtime;

use crate::handlers::shared_service_routing::{
    ensure_shared_service_proxies, plan_shared_service_routing,
};
use crate::server::AppState;

/// Emit a progress event if a sender is provided.
fn emit(tx: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>, event: BuildProgressEvent) {
    if let Some(tx) = tx {
        let _ = tx.try_send(event);
    }
}

/// Revert instance status to Stopped on error, and emit a WebSocket event.
async fn revert_to_stopped(state: &AppState, project: &str, name: &str) {
    if let Ok(db) = state.db.try_lock() {
        let _ = db.update_instance_status(project, name, &InstanceStatus::Stopped);
    }
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: name.to_string(),
        project: project.to_string(),
        status: "stopped".to_string(),
    });
}

const TOTAL_START_STEPS: u32 = 4;

/// Handle a start request with optional progress streaming.
///
/// Steps:
/// 1. Verify the instance exists and is stopped.
/// 2. Start the coast container on the host daemon.
/// 3. Wait for the inner Docker daemon to become ready.
/// 4. Start `docker compose up -d` inside the coast container.
/// 5. Wait for all services to be healthy/running.
/// 6. Restart socat forwarders for dynamic ports.
/// 7. Update instance status to "running" in state DB.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: StartRequest,
    state: &AppState,
    progress: Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<StartResponse> {
    info!(name = %req.name, project = %req.project, "handling start request");

    // Phase 1: Validate instance and set transitional state (locked)
    let instance = {
        let db = state.db.lock().await;
        let inst = db.get_instance(&req.project, &req.name)?;
        let inst = inst.ok_or_else(|| CoastError::InstanceNotFound {
            name: req.name.clone(),
            project: req.project.clone(),
        })?;
        if inst.status == InstanceStatus::Running || inst.status == InstanceStatus::CheckedOut {
            return Err(CoastError::state(format!(
                "Instance '{}' is already running (status: {}). Run `coast stop {}` first if you want to restart it.",
                req.name, inst.status, req.name
            )));
        }
        if inst.status == InstanceStatus::Provisioning
            || inst.status == InstanceStatus::Assigning
            || inst.status == InstanceStatus::Starting
            || inst.status == InstanceStatus::Stopping
        {
            return Err(CoastError::state(format!(
                "Instance '{}' is currently {}. Wait for the operation to complete.",
                req.name, inst.status
            )));
        }
        db.update_instance_status(&req.project, &req.name, &InstanceStatus::Starting)?;
        inst
    };

    // Backfill build_id for pre-migration instances
    if instance.build_id.is_none() {
        if let Some(home) = dirs::home_dir() {
            let latest_link = home
                .join(".coast")
                .join("images")
                .join(&req.project)
                .join("latest");
            if let Ok(target) = std::fs::read_link(&latest_link) {
                if let Some(bid) = target.file_name().map(|f| f.to_string_lossy().into_owned()) {
                    let db = state.db.lock().await;
                    let _ = db.set_build_id(&req.project, &req.name, Some(&bid));
                    info!(name = %req.name, build_id = %bid, "backfilled build_id for pre-migration instance");
                }
            }
        }
    }

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "starting".to_string(),
    });

    emit(
        &progress,
        BuildProgressEvent::build_plan(vec![
            "Starting container".into(),
            "Waiting for inner daemon".into(),
            "Running compose up".into(),
            "Waiting for services".into(),
        ]),
    );

    // Phase 2: Docker operations (unlocked)
    if let Some(ref container_id) = instance.container_id {
        if let Some(ref docker) = state.docker {
            // Step 1: Start the coast container
            emit(
                &progress,
                BuildProgressEvent::started("Starting container", 1, TOTAL_START_STEPS),
            );
            let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
            if let Err(e) = runtime.start_coast_container(container_id).await {
                revert_to_stopped(state, &req.project, &req.name).await;
                return Err(CoastError::docker(format!(
                    "Failed to start container for instance '{}': {}. \
                     Try `coast rm {}` and `coast run` again.",
                    req.name, e, req.name
                )));
            }
            emit(
                &progress,
                BuildProgressEvent::item("Starting container", "container", "ok"),
            );

            // Step 2: Wait for inner Docker daemon
            emit(
                &progress,
                BuildProgressEvent::started("Waiting for inner daemon", 2, TOTAL_START_STEPS),
            );
            let manager = coast_docker::container::ContainerManager::new(runtime);
            if let Err(e) = manager.wait_for_inner_daemon(container_id).await {
                revert_to_stopped(state, &req.project, &req.name).await;
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' failed to start: {}. \
                     Try `coast rm {}` and `coast run` again.",
                    req.name, e, req.name
                )));
            }

            let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            let health_timeout = tokio::time::Duration::from_secs(10);
            let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
            match tokio::time::timeout(health_timeout, health_check).await {
                Ok(Ok(r)) if r.success() => {
                    info!("start: inner daemon healthy");
                    emit(
                        &progress,
                        BuildProgressEvent::item("Waiting for inner daemon", "docker info", "ok"),
                    );
                }
                Ok(Ok(r)) => {
                    revert_to_stopped(state, &req.project, &req.name).await;
                    return Err(CoastError::docker(format!(
                        "Inner Docker daemon in instance '{}' is not healthy (exit {}). \
                         Try `coast stop {} && coast start {}`.",
                        req.name, r.exit_code, req.name, req.name,
                    )));
                }
                Ok(Err(e)) => {
                    revert_to_stopped(state, &req.project, &req.name).await;
                    return Err(CoastError::docker(format!(
                        "Cannot reach inner Docker daemon in instance '{}': {e}. \
                         Try `coast stop {} && coast start {}`.",
                        req.name, req.name, req.name,
                    )));
                }
                Err(_) => {
                    revert_to_stopped(state, &req.project, &req.name).await;
                    return Err(CoastError::docker(format!(
                        "Inner Docker daemon in instance '{}' is unresponsive (timed out after {}s). \
                         The DinD container may need to be recreated. Try `coast rm {} && coast run {}`.",
                        req.name, health_timeout.as_secs(), req.name, req.name,
                    )));
                }
            }

            // Step 3: Start compose
            emit(
                &progress,
                BuildProgressEvent::started("Running compose up", 3, TOTAL_START_STEPS),
            );

            let coastfile_path =
                super::artifact_coastfile_path(&req.project, instance.build_id.as_deref());
            let parsed_coastfile = coastfile_path
                .exists()
                .then(|| coast_core::coastfile::Coastfile::from_file(&coastfile_path).ok())
                .flatten();
            let project_has_compose = parsed_coastfile
                .as_ref()
                .map(|coastfile| coastfile.compose.is_some())
                .unwrap_or(true);

            // Re-apply the /workspace bind mount (project root or worktree).
            {
                let mount_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
                let mount_src = compute_start_mount_src(
                    &req.project,
                    instance.worktree_name.as_deref(),
                    parsed_coastfile.as_ref(),
                );
                let home = dirs::home_dir().unwrap_or_default();
                let project_dir = home.join(".coast").join("images").join(&req.project);
                let manifest_path = project_dir.join("latest").join("manifest.json");
                let project_root_str = manifest_path
                    .exists()
                    .then(|| std::fs::read_to_string(&manifest_path).ok())
                    .flatten()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("project_root")?.as_str().map(String::from))
                    .unwrap_or_default();
                let symlink_fix = if instance.worktree_name.is_some()
                    && !project_root_str.is_empty()
                {
                    let parent = std::path::Path::new(&project_root_str)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    format!(" && mkdir -p '{parent}' && ln -sfn /host-project '{project_root_str}'")
                } else {
                    String::new()
                };
                let mount_cmd = format!(
                    "mkdir -p /workspace && mount --bind {mount_src} /workspace && mount --make-rshared /workspace{symlink_fix}"
                );
                let mount_result = mount_rt
                    .exec_in_coast(container_id, &["sh", "-c", &mount_cmd])
                    .await;
                match mount_result {
                    Ok(r) if r.success() => {
                        info!(name = %req.name, src = %mount_src, "re-applied /workspace bind mount");
                    }
                    Ok(r) => {
                        warn!(name = %req.name, stderr = %r.stderr, "failed to re-apply /workspace bind mount");
                    }
                    Err(e) => {
                        warn!(name = %req.name, error = %e, "failed to re-apply /workspace bind mount");
                    }
                }
            }

            if let Some(ref coastfile) = parsed_coastfile {
                if !coastfile.shared_services.is_empty() {
                    let shared_service_targets = coastfile
                        .shared_services
                        .iter()
                        .map(|service| {
                            (
                                service.name.clone(),
                                crate::shared_services::shared_container_name(
                                    &req.project,
                                    &service.name,
                                ),
                            )
                        })
                        .collect();

                    let routing = match plan_shared_service_routing(
                        docker,
                        container_id,
                        &coastfile.shared_services,
                        &shared_service_targets,
                    )
                    .await
                    {
                        Ok(routing) => routing,
                        Err(error) => {
                            revert_to_stopped(state, &req.project, &req.name).await;
                            return Err(error);
                        }
                    };

                    if let Err(error) =
                        ensure_shared_service_proxies(docker, container_id, &routing).await
                    {
                        revert_to_stopped(state, &req.project, &req.name).await;
                        return Err(error);
                    }
                }
            }

            if project_has_compose {
                let ctx =
                    super::compose_context_for_build(&req.project, instance.build_id.as_deref());
                let up_subcmd = "up -d --remove-orphans --force-recreate";
                let compose_cmd = ctx.compose_shell(up_subcmd);
                let compose_refs: Vec<&str> = compose_cmd
                    .iter()
                    .map(std::string::String::as_str)
                    .collect();

                let runtime2 = coast_docker::dind::DindRuntime::with_client(docker.clone());
                let _ = runtime2.exec_in_coast(container_id, &compose_refs).await;

                emit(
                    &progress,
                    BuildProgressEvent::item("Running compose up", "compose up -d", "ok"),
                );

                // Step 4: Wait for services to be healthy
                emit(
                    &progress,
                    BuildProgressEvent::started("Waiting for services", 4, TOTAL_START_STEPS),
                );

                let health_cmd = ctx.compose_shell("ps --format json");
                let health_refs: Vec<&str> =
                    health_cmd.iter().map(std::string::String::as_str).collect();
                for _ in 0..30 {
                    let result = runtime2.exec_in_coast(container_id, &health_refs).await;
                    if let Ok(exec_result) = result {
                        if exec_result.success() && !exec_result.stdout.is_empty() {
                            let all_running = exec_result
                                .stdout
                                .lines()
                                .all(|line| line.contains("running") || line.contains("healthy"));
                            if all_running {
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
                emit(
                    &progress,
                    BuildProgressEvent::item("Waiting for services", "all services", "ok"),
                );
            }

            // Also start bare services (may coexist with compose)
            let has_svc = crate::bare_services::has_bare_services(docker, container_id).await;
            if has_svc {
                let start_cmd = crate::bare_services::generate_start_command();
                let svc_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
                let _ = svc_rt
                    .exec_in_coast(container_id, &["sh", "-c", &start_cmd])
                    .await;
                emit(
                    &progress,
                    BuildProgressEvent::item("Running compose up", "bare services started", "ok"),
                );
            } else if !project_has_compose {
                emit(
                    &progress,
                    BuildProgressEvent::item("Running compose up", "no compose", "skip"),
                );
            }
        }
    }

    // Phase 3: Final DB writes (locked)
    let db = state.db.lock().await;
    let port_allocs = db.get_port_allocations(&req.project, &req.name)?;
    let ports: Vec<PortMapping> = port_allocs.iter().map(PortMapping::from).collect();
    db.update_instance_status(&req.project, &req.name, &InstanceStatus::Running)?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "running".to_string(),
    });

    info!(name = %req.name, project = %req.project, "instance started");

    Ok(StartResponse {
        name: req.name,
        ports,
    })
}

/// Compute the container mount source for `/workspace` during `coast start`.
///
/// For local worktrees, returns `/host-project/{wt_dir}/{name}`.
/// For external worktrees, uses `git worktree list --porcelain` to find the
/// actual path, then maps it to `/host-external-wt/{index}/{relative}`.
fn compute_start_mount_src(
    project: &str,
    worktree_name: Option<&str>,
    coastfile: Option<&coast_core::coastfile::Coastfile>,
) -> String {
    use coast_core::coastfile::Coastfile;

    let Some(wt) = worktree_name else {
        return "/host-project".to_string();
    };

    let project_root = super::assign::read_project_root(project);
    let detected = project_root
        .as_ref()
        .and_then(|root| super::assign::detect_worktree_dir_from_git(root));

    if let Some(ref d) = detected {
        if !Coastfile::is_external_worktree_dir(d) {
            return format!("/host-project/{d}/{wt}");
        }
    }

    let worktree_dirs = coastfile
        .map(|cf| cf.worktree_dirs.clone())
        .unwrap_or_else(|| vec![".worktrees".to_string()]);
    let default_dir = coastfile
        .map(|cf| cf.default_worktree_dir.clone())
        .unwrap_or_else(|| ".worktrees".to_string());

    if let Some(ref root) = project_root {
        for (idx, dir) in worktree_dirs.iter().enumerate() {
            if Coastfile::is_external_worktree_dir(dir) {
                let resolved = Coastfile::resolve_worktree_dir(root, dir);
                if let Some(mount) = find_external_wt_mount_src(root, &resolved, idx, wt) {
                    return mount;
                }
            } else {
                let candidate = root.join(dir).join(wt);
                if candidate.exists() {
                    return format!("/host-project/{dir}/{wt}");
                }
            }
        }
    }

    format!("/host-project/{default_dir}/{wt}")
}

/// Search an external worktree dir for a worktree matching `worktree_name`
/// using `git worktree list --porcelain`, returning the container mount path.
fn find_external_wt_mount_src(
    project_root: &std::path::Path,
    external_dir: &std::path::Path,
    ext_index: usize,
    worktree_name: &str,
) -> Option<String> {
    use coast_core::coastfile::Coastfile;

    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(project_root)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let canon_ext = external_dir
        .canonicalize()
        .unwrap_or_else(|_| external_dir.to_path_buf());
    let mut current_path: Option<std::path::PathBuf> = None;

    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            current_path = Some(std::path::PathBuf::from(path_str));
        } else if line.starts_with("branch ") || line == "detached" {
            if let Some(ref wt_path) = current_path {
                let wt_canonical = wt_path.canonicalize().unwrap_or_else(|_| wt_path.clone());
                let branch_name = if let Some(branch_ref) = line.strip_prefix("branch ") {
                    branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref)
                } else {
                    wt_path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                };

                if wt_canonical.starts_with(&canon_ext) {
                    let relative = wt_canonical
                        .strip_prefix(&canon_ext)
                        .unwrap_or(&wt_canonical);
                    let relative_str = relative.display().to_string();
                    if branch_name == worktree_name || relative_str == worktree_name {
                        let ext_mount = Coastfile::external_mount_path(ext_index);
                        return Some(format!("{ext_mount}/{relative_str}"));
                    }
                }
            }
        } else if line.is_empty() {
            current_path = None;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

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
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    #[tokio::test]
    async fn test_start_stopped_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance("feat-a", "my-app", InstanceStatus::Stopped))
                .unwrap();
        }

        let req = StartRequest {
            name: "feat-a".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "feat-a");

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feat-a").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_start_nonexistent_instance() {
        let state = test_state();
        let req = StartRequest {
            name: "nonexistent".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_start_already_running_instance() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "running-inst",
                "my-app",
                InstanceStatus::Running,
            ))
            .unwrap();
        }

        let req = StartRequest {
            name: "running-inst".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already running"));
    }

    #[tokio::test]
    async fn test_start_returns_port_allocations() {
        let state = test_state();
        {
            let db = state.db.lock().await;
            db.insert_instance(&make_instance(
                "with-ports",
                "my-app",
                InstanceStatus::Stopped,
            ))
            .unwrap();
            db.insert_port_allocation(
                "my-app",
                "with-ports",
                &PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 52340,
                    is_primary: false,
                },
            )
            .unwrap();
            db.insert_port_allocation(
                "my-app",
                "with-ports",
                &PortMapping {
                    logical_name: "db".to_string(),
                    canonical_port: 5432,
                    dynamic_port: 52341,
                    is_primary: false,
                },
            )
            .unwrap();
        }

        let req = StartRequest {
            name: "with-ports".to_string(),
            project: "my-app".to_string(),
        };
        let result = handle(req, &state, None).await.unwrap();
        assert_eq!(result.ports.len(), 2);
    }
}
