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

/// Check whether the given instance status allows starting.
///
/// Returns `Ok(())` for `Stopped`, `Idle`, `Enqueued`, and `Unassigning`
/// (statuses that the original code did not reject). Returns an error for
/// `Running`/`CheckedOut` (already running) and transitional states
/// (`Provisioning`, `Assigning`, `Starting`, `Stopping`).
fn validate_startable(status: &InstanceStatus, name: &str) -> Result<()> {
    match status {
        InstanceStatus::Stopped
        | InstanceStatus::Idle
        | InstanceStatus::Enqueued
        | InstanceStatus::Unassigning => Ok(()),
        InstanceStatus::Running | InstanceStatus::CheckedOut => Err(CoastError::state(format!(
            "Instance '{name}' is already running (status: {status}). Run `coast stop {name}` first if you want to restart it."
        ))),
        InstanceStatus::Provisioning
        | InstanceStatus::Assigning
        | InstanceStatus::Starting
        | InstanceStatus::Stopping => Err(CoastError::state(format!(
            "Instance '{name}' is currently {status}. Wait for the operation to complete."
        ))),
    }
}

/// Verify the inner Docker daemon is healthy by running `docker info` with a timeout.
///
/// On success, normalizes the Docker socket permissions. Returns an error
/// with actionable guidance on failure or timeout.
async fn verify_inner_daemon_health(
    rt: &dyn Runtime,
    container_id: &str,
    name: &str,
) -> Result<()> {
    let health_timeout = tokio::time::Duration::from_secs(10);
    let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
    match tokio::time::timeout(health_timeout, health_check).await {
        Ok(Ok(r)) if r.success() => {
            info!("start: inner daemon healthy");
            normalize_inner_docker_socket_permissions(rt, container_id).await;
            Ok(())
        }
        Ok(Ok(r)) => Err(CoastError::docker(format!(
            "Inner Docker daemon in instance '{name}' is not healthy (exit {}). \
             Try `coast stop {name} && coast start {name}`.",
            r.exit_code,
        ))),
        Ok(Err(e)) => Err(CoastError::docker(format!(
            "Cannot reach inner Docker daemon in instance '{name}': {e}. \
             Try `coast stop {name} && coast start {name}`.",
        ))),
        Err(_) => Err(CoastError::docker(format!(
            "Inner Docker daemon in instance '{name}' is unresponsive (timed out after {}s). \
             The DinD container may need to be recreated. Try `coast rm {name} && coast run {name}`.",
            health_timeout.as_secs(),
        ))),
    }
}

/// Build the shell command to re-apply the `/workspace` bind mount.
fn build_workspace_mount_command(
    mount_src: &str,
    symlink_fix: &str,
    private_paths: &[String],
    bare_services: &[coast_core::types::BareServiceConfig],
) -> String {
    let private_cmds =
        coast_core::coastfile::Coastfile::build_private_paths_mount_commands(private_paths);
    let cache_cmds = coast_core::coastfile::Coastfile::build_cache_mount_commands(bare_services);
    format!(
        "mkdir -p /workspace && mount --bind {mount_src} /workspace && mount --make-rshared /workspace{symlink_fix}{private_cmds}{cache_cmds}"
    )
}

/// Check for bare services and start them if present.
///
/// Returns `true` if bare services were found and started, `false` otherwise.
async fn start_bare_services_if_present(
    rt: &dyn Runtime,
    container_id: &str,
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> bool {
    let has_svc = match rt
        .exec_in_coast(
            container_id,
            &["test", "-d", crate::bare_services::SUPERVISOR_DIR],
        )
        .await
    {
        Ok(r) => r.success(),
        Err(_) => false,
    };
    if has_svc {
        let start_cmd = crate::bare_services::generate_start_command();
        let _ = rt
            .exec_in_coast(container_id, &["sh", "-c", &start_cmd])
            .await;
        emit(
            progress,
            BuildProgressEvent::item("Running compose up", "bare services started", "ok"),
        );
    }
    has_svc
}

/// Re-apply the `/workspace` bind mount inside the coast container.
///
/// Computes the mount source from the project/worktree configuration,
/// builds the symlink fix for worktrees, and executes the mount command.
/// Failures are logged as warnings — they do not fail the start.
async fn reapply_workspace_mount(
    rt: &dyn Runtime,
    container_id: &str,
    project: &str,
    worktree_name: Option<&str>,
    parsed_coastfile: Option<&coast_core::coastfile::Coastfile>,
    name: &str,
) {
    let mount_src = compute_start_mount_src(project, worktree_name, parsed_coastfile);
    let private_paths = parsed_coastfile
        .map(|cf| cf.private_paths.as_slice())
        .unwrap_or(&[]);
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let project_root_str = manifest_path
        .exists()
        .then(|| std::fs::read_to_string(&manifest_path).ok())
        .flatten()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("project_root")?.as_str().map(String::from))
        .unwrap_or_default();
    let symlink_fix = if worktree_name.is_some() && !project_root_str.is_empty() {
        let parent = std::path::Path::new(&project_root_str)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        format!(" && mkdir -p '{parent}' && ln -sfn /host-project '{project_root_str}'")
    } else {
        String::new()
    };
    let bare_services = parsed_coastfile
        .map(|cf| cf.services.as_slice())
        .unwrap_or(&[]);
    let mount_cmd =
        build_workspace_mount_command(&mount_src, &symlink_fix, private_paths, bare_services);
    match rt
        .exec_in_coast(container_id, &["sh", "-c", &mount_cmd])
        .await
    {
        Ok(r) if r.success() => {
            info!(name = %name, src = %mount_src, "re-applied /workspace bind mount");
        }
        Ok(r) => {
            warn!(name = %name, stderr = %r.stderr, "failed to re-apply /workspace bind mount");
        }
        Err(e) => {
            warn!(name = %name, error = %e, "failed to re-apply /workspace bind mount");
        }
    }
}

/// Set up shared service proxies for the coast container.
///
/// Builds routing targets from the coastfile's shared services, plans the
/// routing, and ensures proxies are running. Returns early if no shared
/// services are configured.
async fn setup_shared_services(
    docker: &bollard::Docker,
    container_id: &str,
    coastfile: &coast_core::coastfile::Coastfile,
    project: &str,
) -> Result<()> {
    if coastfile.shared_services.is_empty() {
        return Ok(());
    }
    let shared_service_targets = coastfile
        .shared_services
        .iter()
        .map(|service| {
            (
                service.name.clone(),
                crate::shared_services::shared_container_name(project, &service.name),
            )
        })
        .collect();
    let routing = plan_shared_service_routing(
        docker,
        container_id,
        &coastfile.shared_services,
        &shared_service_targets,
    )
    .await?;
    ensure_shared_service_proxies(docker, container_id, &routing).await
}

/// Run `docker compose up` and poll for service health.
///
/// Executes `compose up -d --remove-orphans --force-recreate`, then polls
/// `compose ps --format json` up to 30 times (2s interval) until all
/// services report running/healthy.
async fn run_compose_and_wait_for_health(
    rt: &dyn Runtime,
    container_id: &str,
    project: &str,
    build_id: Option<&str>,
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) {
    let ctx = super::compose_context_for_build(project, build_id);
    let up_subcmd = "up -d --remove-orphans --force-recreate";
    let compose_cmd = ctx.compose_shell(up_subcmd);
    let compose_refs: Vec<&str> = compose_cmd
        .iter()
        .map(std::string::String::as_str)
        .collect();
    let _ = rt.exec_in_coast(container_id, &compose_refs).await;

    emit(
        progress,
        BuildProgressEvent::item("Running compose up", "compose up -d", "ok"),
    );

    emit(
        progress,
        BuildProgressEvent::started("Waiting for services", 4, TOTAL_START_STEPS),
    );

    let health_cmd = ctx.compose_shell("ps --format json");
    let health_refs: Vec<&str> = health_cmd.iter().map(std::string::String::as_str).collect();
    for _ in 0..30 {
        let result = rt.exec_in_coast(container_id, &health_refs).await;
        if let Ok(ref exec_result) = result {
            if exec_result.success() && super::run::compose_ps_output_is_ready(&exec_result.stdout)
            {
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
    emit(
        progress,
        BuildProgressEvent::item("Waiting for services", "all services", "ok"),
    );
}

/// Backfill `build_id` for instances created before the build_id migration.
///
/// Reads the `latest` symlink to discover the current build ID and persists
/// it in the database. This is a no-op for instances that already have a
/// `build_id`.
async fn backfill_build_id(state: &AppState, project: &str, name: &str) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let latest_link = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest");
    let Ok(target) = std::fs::read_link(&latest_link) else {
        return;
    };
    if let Some(bid) = target.file_name().map(|f| f.to_string_lossy().into_owned()) {
        let db = state.db.lock().await;
        let _ = db.set_build_id(project, name, Some(&bid));
        info!(name = %name, build_id = %bid, "backfilled build_id for pre-migration instance");
    }
}

/// Execute all Docker operations for the start sequence.
///
/// Starts the container, waits for the inner daemon, re-applies the workspace
/// mount, sets up shared services, runs compose, and starts bare services.
async fn run_docker_operations(
    docker: &bollard::Docker,
    container_id: &str,
    req: &StartRequest,
    build_id: Option<&str>,
    worktree_name: Option<&str>,
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<()> {
    // Step 1: Start the coast container
    emit(
        progress,
        BuildProgressEvent::started("Starting container", 1, TOTAL_START_STEPS),
    );
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    if let Err(e) = runtime.start_coast_container(container_id).await {
        return Err(CoastError::docker(format!(
            "Failed to start container for instance '{}': {}. \
             Try `coast rm {}` and `coast run` again.",
            req.name, e, req.name
        )));
    }
    emit(
        progress,
        BuildProgressEvent::item("Starting container", "container", "ok"),
    );

    // Step 2: Wait for inner Docker daemon
    emit(
        progress,
        BuildProgressEvent::started("Waiting for inner daemon", 2, TOTAL_START_STEPS),
    );
    let manager = coast_docker::container::ContainerManager::new(runtime);
    if let Err(e) = manager.wait_for_inner_daemon(container_id).await {
        return Err(CoastError::docker(format!(
            "Inner Docker daemon in instance '{}' failed to start: {}. \
             Try `coast rm {}` and `coast run` again.",
            req.name, e, req.name
        )));
    }

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    verify_inner_daemon_health(&rt, container_id, &req.name).await?;
    emit(
        progress,
        BuildProgressEvent::item("Waiting for inner daemon", "docker info", "ok"),
    );

    // Step 3: Start compose
    emit(
        progress,
        BuildProgressEvent::started("Running compose up", 3, TOTAL_START_STEPS),
    );

    let coastfile_path = super::artifact_coastfile_path(&req.project, build_id);
    let parsed_coastfile = coastfile_path
        .exists()
        .then(|| coast_core::coastfile::Coastfile::from_file(&coastfile_path).ok())
        .flatten();
    let project_has_compose = parsed_coastfile
        .as_ref()
        .map(|coastfile| coastfile.compose.is_some())
        .unwrap_or(true);

    let mount_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    reapply_workspace_mount(
        &mount_rt,
        container_id,
        &req.project,
        worktree_name,
        parsed_coastfile.as_ref(),
        &req.name,
    )
    .await;

    if let Some(ref coastfile) = parsed_coastfile {
        setup_shared_services(docker, container_id, coastfile, &req.project).await?;
    }

    if project_has_compose {
        let compose_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
        run_compose_and_wait_for_health(
            &compose_rt,
            container_id,
            &req.project,
            build_id,
            progress,
        )
        .await;
    }

    // Also start bare services (may coexist with compose)
    let svc_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let has_svc = start_bare_services_if_present(&svc_rt, container_id, progress).await;
    if !has_svc && !project_has_compose {
        emit(
            progress,
            BuildProgressEvent::item("Running compose up", "no compose", "skip"),
        );
    }

    Ok(())
}

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
        validate_startable(&inst.status, &req.name)?;
        db.update_instance_status(&req.project, &req.name, &InstanceStatus::Starting)?;
        inst
    };

    if instance.build_id.is_none() {
        backfill_build_id(state, &req.project, &req.name).await;
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
        if let Some(docker) = state.docker.as_ref() {
            if let Err(e) = run_docker_operations(
                &docker,
                container_id,
                &req,
                instance.build_id.as_deref(),
                instance.worktree_name.as_deref(),
                &progress,
            )
            .await
            {
                revert_to_stopped(state, &req.project, &req.name).await;
                return Err(e);
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

async fn normalize_inner_docker_socket_permissions(rt: &dyn Runtime, container_id: &str) {
    let cmd = [
        "sh",
        "-c",
        "for _ in $(seq 1 20); do \
           if [ -S /var/run/docker.sock ]; then chmod 666 /var/run/docker.sock && exit 0; fi; \
           sleep 1; \
         done; \
         exit 1",
    ];
    match rt.exec_in_coast(container_id, &cmd).await {
        Ok(result) if result.success() => {}
        Ok(result) => {
            warn!(
                container_id,
                stderr = %result.stderr,
                "failed to normalize inner Docker socket permissions"
            );
        }
        Err(error) => {
            warn!(
                container_id,
                error = %error,
                "failed to normalize inner Docker socket permissions"
            );
        }
    }
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

    let worktree_dirs = coastfile
        .map(|cf| cf.worktree_dirs.clone())
        .unwrap_or_else(|| vec![".worktrees".to_string()]);
    let default_dir = coastfile
        .map(|cf| cf.default_worktree_dir.clone())
        .unwrap_or_else(|| ".worktrees".to_string());

    // Phase 1: Directory name match in local worktree dirs (handles branch != dir name).
    if let Some(ref root) = project_root {
        for dir in &worktree_dirs {
            if Coastfile::is_external_worktree_dir(dir) {
                continue;
            }
            let candidate = root.join(dir).join(wt);
            if candidate.exists() {
                return format!("/host-project/{dir}/{wt}");
            }
        }
    }

    // Phase 2: External worktree dirs (directory + branch match, with glob expansion).
    if let Some(ref root) = project_root {
        let expanded = Coastfile::resolve_external_worktree_dirs_expanded(&worktree_dirs, root);
        for ext_dir in &expanded {
            if let Some(mount) =
                find_external_wt_mount_src(root, &ext_dir.resolved_path, ext_dir.mount_index, wt)
            {
                return mount;
            }
        }
    }

    // Phase 3: Git-detected worktree dir (creation fallback).
    if let Some(ref d) = detected {
        if !Coastfile::is_external_worktree_dir(d) {
            if let Some(ref root) = project_root {
                let candidate = root.join(d).join(wt);
                if candidate.exists() {
                    return format!("/host-project/{d}/{wt}");
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
    use async_trait::async_trait;
    use coast_core::types::{CoastInstance, RuntimeType};
    use coast_docker::runtime::{ContainerConfig, ExecResult};
    use std::net::IpAddr;

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

    // --- validate_startable tests ---

    #[test]
    fn test_validate_startable_stopped_ok() {
        assert!(validate_startable(&InstanceStatus::Stopped, "inst").is_ok());
    }

    #[test]
    fn test_validate_startable_idle_ok() {
        assert!(validate_startable(&InstanceStatus::Idle, "inst").is_ok());
    }

    #[test]
    fn test_validate_startable_enqueued_ok() {
        assert!(validate_startable(&InstanceStatus::Enqueued, "inst").is_ok());
    }

    #[test]
    fn test_validate_startable_unassigning_ok() {
        assert!(validate_startable(&InstanceStatus::Unassigning, "inst").is_ok());
    }

    #[test]
    fn test_validate_startable_running_errors() {
        let err = validate_startable(&InstanceStatus::Running, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already running"));
    }

    #[test]
    fn test_validate_startable_checked_out_errors() {
        let err = validate_startable(&InstanceStatus::CheckedOut, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already running"));
    }

    #[test]
    fn test_validate_startable_provisioning_errors() {
        let err = validate_startable(&InstanceStatus::Provisioning, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_startable_assigning_errors() {
        let err = validate_startable(&InstanceStatus::Assigning, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_startable_starting_errors() {
        let err = validate_startable(&InstanceStatus::Starting, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    #[test]
    fn test_validate_startable_stopping_errors() {
        let err = validate_startable(&InstanceStatus::Stopping, "inst")
            .unwrap_err()
            .to_string();
        assert!(err.contains("currently"));
    }

    // --- MockRuntime for testing extracted helpers ---

    struct MockRuntime {
        exec_results: std::sync::Mutex<Vec<Result<ExecResult>>>,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                exec_results: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn push_exec_result(&self, result: Result<ExecResult>) {
            self.exec_results.lock().unwrap().push(result);
        }
    }

    #[async_trait]
    impl Runtime for MockRuntime {
        fn name(&self) -> &str {
            "mock"
        }

        async fn create_coast_container(&self, _config: &ContainerConfig) -> Result<String> {
            Ok("mock-id".to_string())
        }

        async fn start_coast_container(&self, _container_id: &str) -> Result<()> {
            Ok(())
        }

        async fn stop_coast_container(&self, _container_id: &str) -> Result<()> {
            Ok(())
        }

        async fn remove_coast_container(&self, _container_id: &str) -> Result<()> {
            Ok(())
        }

        async fn exec_in_coast(&self, _container_id: &str, _cmd: &[&str]) -> Result<ExecResult> {
            let mut results = self.exec_results.lock().unwrap();
            if results.is_empty() {
                Ok(ExecResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            } else {
                results.remove(0)
            }
        }

        async fn get_container_ip(&self, _container_id: &str) -> Result<IpAddr> {
            Ok("172.17.0.2".parse().unwrap())
        }

        fn requires_privileged(&self) -> bool {
            false
        }
    }

    // --- verify_inner_daemon_health tests ---

    #[tokio::test]
    async fn test_verify_inner_daemon_health_healthy() {
        let rt = MockRuntime::new();
        // docker info succeeds
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "docker info output".to_string(),
            stderr: String::new(),
        }));
        // normalize_inner_docker_socket_permissions exec
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }));

        let result = verify_inner_daemon_health(&rt, "ctr-1", "my-inst").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_verify_inner_daemon_health_unhealthy_exit() {
        let rt = MockRuntime::new();
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "daemon not running".to_string(),
        }));

        let result = verify_inner_daemon_health(&rt, "ctr-1", "my-inst").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not healthy"));
        assert!(err.contains("my-inst"));
    }

    #[tokio::test]
    async fn test_verify_inner_daemon_health_exec_error() {
        let rt = MockRuntime::new();
        rt.push_exec_result(Err(CoastError::docker("connection refused")));

        let result = verify_inner_daemon_health(&rt, "ctr-1", "my-inst").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Cannot reach"));
        assert!(err.contains("my-inst"));
    }

    // --- build_workspace_mount_command tests ---

    #[test]
    fn test_build_workspace_mount_command_basic() {
        let cmd = build_workspace_mount_command("/host-project", "", &[], &[]);
        assert_eq!(
            cmd,
            "mkdir -p /workspace && mount --bind /host-project /workspace && mount --make-rshared /workspace"
        );
    }

    #[test]
    fn test_build_workspace_mount_command_with_symlink_fix() {
        let symlink_fix = " && mkdir -p '/home/user' && ln -sfn /host-project '/home/user/project'";
        let cmd =
            build_workspace_mount_command("/host-project/.worktrees/feat", symlink_fix, &[], &[]);
        assert!(cmd.contains("mount --bind /host-project/.worktrees/feat /workspace"));
        assert!(cmd.contains("mkdir -p '/home/user'"));
        assert!(cmd.contains("ln -sfn /host-project '/home/user/project'"));
    }

    #[test]
    fn test_build_workspace_mount_command_with_private_paths() {
        let private = vec!["frontend/.next".to_string()];
        let cmd = build_workspace_mount_command("/host-project", "", &private, &[]);
        assert!(cmd.contains("mount --make-rshared /workspace"));
        assert!(
            cmd.contains("mkdir -p '/coast-private/frontend/.next' '/workspace/frontend/.next'")
        );
        assert!(cmd
            .contains("mount --bind '/coast-private/frontend/.next' '/workspace/frontend/.next'"));
    }

    // --- start_bare_services_if_present tests ---

    #[tokio::test]
    async fn test_start_bare_services_present() {
        let rt = MockRuntime::new();
        // test -d /coast-supervisor succeeds
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }));
        // sh -c start-all.sh succeeds
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "started".to_string(),
            stderr: String::new(),
        }));

        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let progress = Some(tx);
        let has = start_bare_services_if_present(&rt, "ctr-1", &progress).await;
        assert!(has);
    }

    #[tokio::test]
    async fn test_start_bare_services_not_present() {
        let rt = MockRuntime::new();
        // test -d /coast-supervisor fails (no supervisor dir)
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        }));

        let has = start_bare_services_if_present(&rt, "ctr-1", &None).await;
        assert!(!has);
    }

    #[tokio::test]
    async fn test_start_bare_services_exec_error_treated_as_absent() {
        let rt = MockRuntime::new();
        // test -d exec errors out entirely
        rt.push_exec_result(Err(CoastError::docker("exec failed")));

        let has = start_bare_services_if_present(&rt, "ctr-1", &None).await;
        assert!(!has);
    }

    // --- reapply_workspace_mount tests ---

    #[tokio::test]
    async fn test_reapply_workspace_mount_success() {
        let rt = MockRuntime::new();
        // mount exec succeeds
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }));

        // Should not panic — success is logged, no return value to check
        reapply_workspace_mount(&rt, "ctr-1", "my-app", None, None, "inst").await;
    }

    #[tokio::test]
    async fn test_reapply_workspace_mount_failure_logs_warning() {
        let rt = MockRuntime::new();
        // mount exec fails
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "permission denied".to_string(),
        }));

        // Should not panic — failure is a warning, not an error
        reapply_workspace_mount(&rt, "ctr-1", "my-app", None, None, "inst").await;
    }

    // --- run_compose_and_wait_for_health tests ---

    #[tokio::test]
    async fn test_run_compose_and_wait_for_health_ready_first_poll() {
        let rt = MockRuntime::new();
        // compose up exec
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }));
        // compose ps returns healthy on first poll
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: r#"{"State":"running"}"#.to_string(),
            stderr: String::new(),
        }));

        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        run_compose_and_wait_for_health(&rt, "ctr-1", "my-app", None, &Some(tx)).await;
    }

    #[tokio::test]
    async fn test_run_compose_and_wait_for_health_ready_after_retry() {
        let rt = MockRuntime::new();
        // compose up exec
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }));
        // first poll: not ready
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: r#"{"State":"starting"}"#.to_string(),
            stderr: String::new(),
        }));
        // second poll: ready
        rt.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: r#"{"State":"running"}"#.to_string(),
            stderr: String::new(),
        }));

        run_compose_and_wait_for_health(&rt, "ctr-1", "my-app", None, &None).await;
    }
}
