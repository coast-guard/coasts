use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{AssignRequest, BuildProgressEvent};
use coast_core::types::{AssignAction, AssignConfig, InstanceStatus};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

use super::classify::classify_services;
use super::gitignored_sync::build_gitignored_sync_script;
use super::util::{emit, health_poll_interval, CoastfileData, TOTAL_STEPS};
use super::worktree::{create_worktree_fallback, detect_worktree_dir_from_git};

/// Parameters for the Docker-dependent assign steps (steps 2-7).
pub(super) struct DockerStepsParams<'a> {
    pub req: &'a AssignRequest,
    pub state: &'a AppState,
    pub progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
    pub docker: &'a bollard::Docker,
    pub container_id: &'a str,
    pub instance_status: &'a InstanceStatus,
    pub instance_build_id: Option<&'a str>,
    pub cf_data: &'a CoastfileData,
    pub assign_config: &'a AssignConfig,
    pub project_root: &'a Option<std::path::PathBuf>,
    pub previous_branch: &'a Option<String>,
}

/// Run all Docker-dependent assign steps (2-7).
pub(super) async fn run_docker_steps(p: DockerStepsParams<'_>) -> Result<()> {
    let rt = coast_docker::dind::DindRuntime::with_client(p.docker.clone());
    let home = dirs::home_dir().unwrap_or_default();
    let artifact_dir = home
        .join(".coast")
        .join("images")
        .join(&p.req.project)
        .join("latest");

    check_inner_daemon(&rt, p.container_id, &p.req.name).await?;
    emit(
        p.progress,
        BuildProgressEvent::done("Checking inner daemon", "ok"),
    )
    .await;

    let (service_actions, all_hot) = discover_and_classify(
        &rt,
        p.container_id,
        &p.req.project,
        p.cf_data,
        p.assign_config,
        p.project_root,
        p.previous_branch,
        &p.req.worktree,
        p.instance_build_id,
    )
    .await;
    let restart_svcs: Vec<&str> = services_with_action(&service_actions, &AssignAction::Restart);
    let rebuild_svcs: Vec<&str> = services_with_action(&service_actions, &AssignAction::Rebuild);

    let (wt_dir, worktree_path) =
        detect_worktree_path(p.project_root, &p.cf_data.worktree_dir, &p.req.worktree).await;

    let wt_child = spawn_worktree_creation(p.project_root, &worktree_path, &p.req.worktree);
    let wt_spawn_t = std::time::Instant::now();

    step(p.progress, "Stopping services", 3).await;
    stop_affected_services(
        &rt,
        p.container_id,
        p.instance_status,
        &restart_svcs,
        &rebuild_svcs,
        &p.req.project,
        p.instance_build_id,
        p.progress,
    )
    .await;
    done(p.progress, "Stopping services").await;

    step(p.progress, "Switching worktree", 4).await;
    switch_worktree(
        &rt,
        p.container_id,
        p.state,
        p.req,
        p.project_root,
        &wt_dir,
        &worktree_path,
        wt_child,
        wt_spawn_t,
        p.assign_config,
        p.progress,
    )
    .await?;
    done(p.progress, "Switching worktree").await;

    recreate_containers(
        &rt,
        p.container_id,
        p.docker,
        p.cf_data.has_compose,
        all_hot,
        &p.req.project,
        p.instance_build_id,
    )
    .await;

    step(p.progress, "Building images", 5).await;
    let image_tags = build_images(
        &rt,
        p.container_id,
        &artifact_dir,
        &rebuild_svcs,
        p.project_root,
        &p.req.project,
        &p.req.name,
        p.progress,
    )
    .await;
    if !image_tags.is_empty() {
        write_image_overrides(&rt, p.container_id, &image_tags).await;
    }
    done(p.progress, "Building images").await;

    step(p.progress, "Starting services", 6).await;
    start_services(
        &rt,
        p.container_id,
        &p.req.project,
        p.instance_build_id,
        &restart_svcs,
        &rebuild_svcs,
        p.progress,
    )
    .await;
    done(p.progress, "Starting services").await;

    step(p.progress, "Waiting for healthy", 7).await;
    wait_for_healthy(
        &rt,
        p.container_id,
        &p.req.project,
        p.instance_build_id,
        &p.req.name,
        &restart_svcs,
        &rebuild_svcs,
        p.progress,
    )
    .await?;
    done(p.progress, "Waiting for healthy").await;

    Ok(())
}

fn services_with_action<'a>(
    actions: &'a std::collections::HashMap<String, AssignAction>,
    target: &AssignAction,
) -> Vec<&'a str> {
    actions
        .iter()
        .filter(|(_, a)| *a == target)
        .map(|(s, _)| s.as_str())
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn discover_and_classify(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    cf_data: &CoastfileData,
    assign_config: &AssignConfig,
    project_root: &Option<std::path::PathBuf>,
    previous_branch: &Option<String>,
    worktree: &str,
    build_id: Option<&str>,
) -> (std::collections::HashMap<String, AssignAction>, bool) {
    let all_service_names =
        discover_service_names(rt, container_id, cf_data.has_compose, project, build_id).await;
    let changed_files =
        diff_changed_files(assign_config, project_root, previous_branch, worktree).await;
    let actions = classify_services(&all_service_names, assign_config, &changed_files);

    let hot_svcs: Vec<&str> = services_with_action(&actions, &AssignAction::Hot);
    let all_hot = !actions.is_empty()
        && actions
            .values()
            .all(|a| *a == AssignAction::Hot || *a == AssignAction::None);

    info!(
        none_count = services_with_action(&actions, &AssignAction::None).len(),
        hot_count = hot_svcs.len(),
        restart_count = services_with_action(&actions, &AssignAction::Restart).len(),
        rebuild_count = services_with_action(&actions, &AssignAction::Rebuild).len(),
        all_hot,
        "classified services for assign"
    );

    (actions, all_hot)
}

async fn detect_worktree_path(
    project_root: &Option<std::path::PathBuf>,
    default_wt_dir: &str,
    worktree_name: &str,
) -> (Option<String>, Option<std::path::PathBuf>) {
    if let Some(ref root) = project_root {
        let step_t = std::time::Instant::now();
        let root_clone = root.clone();
        let detected =
            tokio::task::spawn_blocking(move || detect_worktree_dir_from_git(&root_clone))
                .await
                .ok()
                .flatten();
        let dir = detected.unwrap_or_else(|| default_wt_dir.to_string());
        info!(elapsed_ms = step_t.elapsed().as_millis() as u64, wt_dir = %dir, "detected worktree directory");
        let path = root.join(&dir).join(worktree_name);
        (Some(dir), Some(path))
    } else {
        (None, None)
    }
}

fn spawn_worktree_creation(
    project_root: &Option<std::path::PathBuf>,
    worktree_path: &Option<std::path::PathBuf>,
    worktree_name: &str,
) -> Option<Option<tokio::process::Child>> {
    if let (Some(ref root), Some(ref wt_path)) = (project_root, worktree_path) {
        if !wt_path.exists() {
            let child = tokio::process::Command::new("git")
                .args(["worktree", "add", &wt_path.to_string_lossy(), worktree_name])
                .current_dir(root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .ok();
            Some(child)
        } else {
            None
        }
    } else {
        None
    }
}

async fn step(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, name: &str, num: u32) {
    emit(tx, BuildProgressEvent::started(name, num, TOTAL_STEPS)).await;
}

async fn done(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, name: &str) {
    emit(tx, BuildProgressEvent::done(name, "ok")).await;
}

/// Run a command inside the DinD container and log success/failure.
/// Returns true on success, false on any error.
async fn exec_and_log(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    cmd: &[&str],
    success_msg: &str,
    failure_msg: &str,
) -> bool {
    let step_t = std::time::Instant::now();
    match rt.exec_in_coast(container_id, cmd).await {
        Ok(r) if r.success() => {
            info!(
                elapsed_ms = step_t.elapsed().as_millis() as u64,
                "{success_msg}"
            );
            true
        }
        Ok(r) => {
            tracing::warn!(exit_code = r.exit_code, stderr = %r.stderr, "{failure_msg}");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "{failure_msg}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Step 2: Check inner daemon
// ---------------------------------------------------------------------------

async fn check_inner_daemon(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    instance_name: &str,
) -> Result<()> {
    let step_t = std::time::Instant::now();
    let health_timeout = tokio::time::Duration::from_secs(10);
    let health_check = rt.exec_in_coast(container_id, &["docker", "info"]);
    match tokio::time::timeout(health_timeout, health_check).await {
        Ok(Ok(r)) if r.success() => {
            info!(elapsed_ms = step_t.elapsed().as_millis() as u64, "assign: inner daemon healthy");
            Ok(())
        }
        Ok(Ok(r)) => Err(CoastError::docker(format!(
            "Inner Docker daemon in instance '{instance_name}' is not healthy (exit {}). \
             Try `coast stop {instance_name} && coast start {instance_name}`.",
            r.exit_code,
        ))),
        Ok(Err(e)) => Err(CoastError::docker(format!(
            "Cannot reach inner Docker daemon in instance '{instance_name}': {e}. \
             Try `coast stop {instance_name} && coast start {instance_name}`.",
        ))),
        Err(_) => Err(CoastError::docker(format!(
            "Inner Docker daemon in instance '{instance_name}' is unresponsive (timed out after {}s). \
             The DinD container may need to be recreated. Try `coast rm {instance_name} && coast run {instance_name}`.",
            health_timeout.as_secs(),
        ))),
    }
}

// ---------------------------------------------------------------------------
// Step 3: Discover services + diff
// ---------------------------------------------------------------------------

async fn discover_service_names(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    has_compose: bool,
    project: &str,
    build_id: Option<&str>,
) -> Vec<String> {
    if !has_compose {
        return Vec::new();
    }
    let step_t = std::time::Instant::now();
    let svc_ctx = crate::handlers::compose_context_for_build(project, build_id);
    let svc_cmd = svc_ctx.compose_shell("config --services");
    let svc_refs: Vec<&str> = svc_cmd.iter().map(std::string::String::as_str).collect();
    let services_result = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        rt.exec_in_coast(container_id, &svc_refs),
    )
    .await;
    let services_result = match services_result {
        Ok(r) => r.ok(),
        Err(_) => {
            tracing::warn!(
                "compose config --services timed out, proceeding with empty service list"
            );
            None
        }
    };
    let names: Vec<String> = services_result
        .filter(coast_docker::runtime::ExecResult::success)
        .map(|r| {
            r.stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    info!(
        elapsed_ms = step_t.elapsed().as_millis() as u64,
        count = names.len(),
        "discovered compose services"
    );
    names
}

async fn diff_changed_files(
    assign_config: &AssignConfig,
    project_root: &Option<std::path::PathBuf>,
    previous_branch: &Option<String>,
    worktree: &str,
) -> Vec<String> {
    if assign_config.rebuild_triggers.is_empty() {
        return Vec::new();
    }
    let (Some(ref root), Some(ref prev)) = (project_root, previous_branch) else {
        return Vec::new();
    };
    let step_t = std::time::Instant::now();
    let changed: Vec<String> = tokio::process::Command::new("git")
        .args(["diff", "--name-only", &format!("{prev}..{worktree}")])
        .current_dir(root)
        .output()
        .await
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    info!(
        elapsed_ms = step_t.elapsed().as_millis() as u64,
        count = changed.len(),
        "git diff for rebuild triggers"
    );
    changed
}

// ---------------------------------------------------------------------------
// Step 3b: Stop affected services
// ---------------------------------------------------------------------------

async fn stop_affected_services(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    instance_status: &InstanceStatus,
    restart_svcs: &[&str],
    rebuild_svcs: &[&str],
    project: &str,
    build_id: Option<&str>,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    if *instance_status == InstanceStatus::Idle {
        emit(
            progress,
            BuildProgressEvent::item("Stopping services", "instance idle, skip", "skip"),
        )
        .await;
        return;
    }

    let affected_svcs: Vec<&str> = restart_svcs
        .iter()
        .chain(rebuild_svcs.iter())
        .copied()
        .collect();
    if affected_svcs.is_empty() {
        emit(
            progress,
            BuildProgressEvent::item("Stopping services", "no services to stop", "skip"),
        )
        .await;
        return;
    }

    for svc in &affected_svcs {
        emit(
            progress,
            BuildProgressEvent::item("Stopping services", *svc, "started"),
        )
        .await;
    }

    let stop_ctx = crate::handlers::compose_context_for_build(project, build_id);
    let svc_list = affected_svcs.join(" ");
    let stop_cmd = stop_ctx.compose_shell(&format!("stop -t 2 {svc_list}"));
    let stop_refs: Vec<&str> = stop_cmd.iter().map(std::string::String::as_str).collect();

    info!(services = ?affected_svcs, "stopping affected compose services");
    let ok = exec_and_log(
        rt,
        container_id,
        &stop_refs,
        "affected compose services stopped",
        "docker compose stop exited non-zero, continuing anyway",
    )
    .await;
    let status = if ok { "ok" } else { "warn" };
    for svc in &affected_svcs {
        emit(
            progress,
            BuildProgressEvent::item("Stopping services", *svc, status),
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Step 4: Switch worktree
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn switch_worktree(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    state: &AppState,
    req: &AssignRequest,
    project_root: &Option<std::path::PathBuf>,
    wt_dir: &Option<String>,
    worktree_path: &Option<std::path::PathBuf>,
    wt_child: Option<Option<tokio::process::Child>>,
    wt_spawn_t: std::time::Instant,
    assign_config: &AssignConfig,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    let Some(ref root) = project_root else {
        return Ok(());
    };
    let wt_dir = wt_dir.clone().unwrap_or_else(|| ".worktrees".to_string());
    let worktree_path = worktree_path
        .clone()
        .unwrap_or_else(|| root.join(".worktrees").join(&req.worktree));

    ensure_worktree_exists(
        root,
        &worktree_path,
        &req.worktree,
        wt_child,
        wt_spawn_t,
        progress,
    )
    .await?;
    sync_gitignored_files(root, &worktree_path, &wt_dir, &req.worktree, assign_config).await;
    remount_workspace(rt, container_id, root, &wt_dir, &req.worktree).await;

    let _ = state
        .db
        .lock()
        .await
        .set_worktree(&req.project, &req.name, Some(&req.worktree));
    Ok(())
}

async fn ensure_worktree_exists(
    root: &std::path::Path,
    worktree_path: &std::path::Path,
    worktree_name: &str,
    wt_child: Option<Option<tokio::process::Child>>,
    wt_spawn_t: std::time::Instant,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    if let Some(child_opt) = wt_child {
        if let Some(child) = child_opt {
            let wt_output = child
                .wait_with_output()
                .await
                .map_err(|e| CoastError::git(format!("Failed to create worktree: {e}")))?;
            if !wt_output.status.success() {
                try_worktree_fallback(root, worktree_path, worktree_name).await?;
            }
            info!(elapsed_ms = wt_spawn_t.elapsed().as_millis() as u64, worktree = %worktree_name, path = %worktree_path.display(), "created git worktree");
        }
        emit(
            progress,
            BuildProgressEvent::item(
                "Switching worktree",
                format!("created {worktree_name}"),
                "ok",
            ),
        )
        .await;
    } else if !worktree_path.exists() {
        emit(
            progress,
            BuildProgressEvent::item(
                "Switching worktree",
                format!("creating {worktree_name}"),
                "started",
            ),
        )
        .await;
        let wt_output = tokio::process::Command::new("git")
            .args([
                "worktree",
                "add",
                &worktree_path.to_string_lossy(),
                worktree_name,
            ])
            .current_dir(root)
            .output()
            .await
            .map_err(|e| CoastError::git(format!("Failed to create worktree: {e}")))?;
        if !wt_output.status.success() {
            try_worktree_fallback(root, worktree_path, worktree_name).await?;
        }
        info!(elapsed_ms = wt_spawn_t.elapsed().as_millis() as u64, worktree = %worktree_name, path = %worktree_path.display(), "created git worktree");
        emit(
            progress,
            BuildProgressEvent::item(
                "Switching worktree",
                format!("created {worktree_name}"),
                "ok",
            ),
        )
        .await;
    } else {
        emit(
            progress,
            BuildProgressEvent::item(
                "Switching worktree",
                format!("worktree {worktree_name} exists"),
                "ok",
            ),
        )
        .await;
    }
    Ok(())
}

async fn try_worktree_fallback(
    root: &std::path::Path,
    worktree_path: &std::path::Path,
    branch: &str,
) -> Result<()> {
    let wt_create = create_worktree_fallback(root, worktree_path, branch).await?;
    if !wt_create.status.success() {
        let stderr = String::from_utf8_lossy(&wt_create.stderr);
        return Err(CoastError::git(format!(
            "Failed to create worktree for branch '{branch}': {}",
            stderr.trim()
        )));
    }
    Ok(())
}

async fn sync_gitignored_files(
    root: &std::path::Path,
    worktree_path: &std::path::Path,
    wt_dir: &str,
    worktree_name: &str,
    assign_config: &AssignConfig,
) {
    let marker = worktree_path.join(".coast-synced");
    if marker.exists() {
        info!(worktree = %worktree_name, "worktree already synced, skipping gitignored copy");
        return;
    }

    let step_t = std::time::Instant::now();
    let wt_path_str = worktree_path.to_string_lossy().to_string();
    let root_str = root.to_string_lossy().to_string();
    let mut sync_excludes = assign_config.exclude_paths.clone();
    if !sync_excludes.iter().any(|p| p == wt_dir) {
        sync_excludes.push(wt_dir.to_string());
    }
    let copy_script = build_gitignored_sync_script(&root_str, &wt_path_str, &sync_excludes);
    let copy_result = tokio::process::Command::new("sh")
        .args(["-c", &copy_script])
        .output()
        .await;
    if let Ok(output) = &copy_result {
        if output.status.success() {
            info!(elapsed_ms = step_t.elapsed().as_millis() as u64, worktree = %worktree_name, "synced gitignored files to worktree (hardlinks)");
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(elapsed_ms = step_t.elapsed().as_millis() as u64, worktree = %worktree_name, %stderr, "gitignored sync had issues");
        }
    }
}

async fn remount_workspace(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    root: &std::path::Path,
    wt_dir: &str,
    worktree_name: &str,
) {
    let mount_src = format!("/host-project/{wt_dir}/{worktree_name}");
    let host_root = root.to_string_lossy();
    let parent = root
        .parent()
        .map(|p| p.to_string_lossy())
        .unwrap_or_default();
    let mount_cmd = format!(
        "umount -l /workspace 2>/dev/null; mount --bind {mount_src} /workspace && \
         mount --make-rshared /workspace && \
         mkdir -p '{parent}' && ln -sfn /host-project '{host_root}'"
    );
    exec_and_log(
        rt,
        container_id,
        &["sh", "-c", &mount_cmd],
        "remounted /workspace to worktree",
        "failed to remount /workspace to worktree",
    )
    .await;
}

// ---------------------------------------------------------------------------
// Step 4b: Recreate containers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn recreate_containers(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    docker: &bollard::Docker,
    has_compose: bool,
    all_hot: bool,
    project: &str,
    build_id: Option<&str>,
) {
    if has_compose {
        let ctx = crate::handlers::compose_context_for_build(project, build_id);
        if all_hot {
            compose_force_recreate(rt, container_id, &ctx).await;
        } else {
            compose_down_up(rt, container_id, &ctx).await;
        }
    }

    if crate::bare_services::has_bare_services(docker, container_id).await {
        restart_bare_services(rt, container_id, project, build_id).await;
    }
}

async fn compose_force_recreate(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    ctx: &crate::handlers::ComposeContext,
) {
    let up_cmd = ctx.compose_shell("up -d --force-recreate --remove-orphans -t 1");
    let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
    info!("hot assign: force-recreating containers (skipping compose down)");
    exec_and_log(
        rt,
        container_id,
        &up_refs,
        "hot assign: compose up --force-recreate completed",
        "hot assign: compose up had issues",
    )
    .await;
}

#[allow(clippy::cognitive_complexity)]
async fn compose_down_up(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    ctx: &crate::handlers::ComposeContext,
) {
    let step_t = std::time::Instant::now();
    let down_cmd = ctx.compose_shell("down --remove-orphans -t 2");
    let down_refs: Vec<&str> = down_cmd.iter().map(std::string::String::as_str).collect();
    let _ = rt.exec_in_coast(container_id, &down_refs).await;
    info!(
        elapsed_ms = step_t.elapsed().as_millis() as u64,
        "compose down completed after workspace remount"
    );

    let up_cmd = ctx.compose_shell("up -d --remove-orphans");
    let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
    let up_result = rt.exec_in_coast(container_id, &up_refs).await;
    match &up_result {
        Ok(r) if r.success() => info!(
            elapsed_ms = step_t.elapsed().as_millis() as u64,
            "compose up completed after workspace remount"
        ),
        Ok(r) => {
            tracing::warn!(stderr = %r.stderr, "compose up after workspace remount had issues")
        }
        Err(e) => tracing::warn!(error = %e, "compose up after workspace remount failed"),
    }
}

fn resolve_coastfile_path(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    build_id
        .map(|bid| {
            home.join(".coast")
                .join("images")
                .join(project)
                .join(bid)
                .join("coastfile.toml")
        })
        .filter(|p| p.exists())
        .unwrap_or_else(|| {
            home.join(".coast")
                .join("images")
                .join(project)
                .join("latest")
                .join("coastfile.toml")
        })
}

#[allow(clippy::cognitive_complexity)]
async fn restart_bare_services(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    build_id: Option<&str>,
) {
    let cf_path = resolve_coastfile_path(project, build_id);
    let svc_list = coast_core::coastfile::Coastfile::from_file(&cf_path)
        .map(|cf| cf.services)
        .unwrap_or_default();

    if let Some(save_cmd) = crate::bare_services::generate_cache_save_command(&svc_list) {
        let step_t = std::time::Instant::now();
        let _ = rt
            .exec_in_coast(container_id, &["sh", "-c", &save_cmd])
            .await;
        info!(
            elapsed_ms = step_t.elapsed().as_millis() as u64,
            "bare services cache saved"
        );
    }

    let step_t = std::time::Instant::now();
    let stop_cmd = crate::bare_services::generate_stop_command();
    let _ = rt
        .exec_in_coast(container_id, &["sh", "-c", &stop_cmd])
        .await;
    info!(
        elapsed_ms = step_t.elapsed().as_millis() as u64,
        "bare services stopped for branch switch"
    );

    let start_cmd = crate::bare_services::generate_install_and_start_command(&svc_list);
    let step_t = std::time::Instant::now();
    let start_result = rt
        .exec_in_coast(container_id, &["sh", "-c", &start_cmd])
        .await;
    match &start_result {
        Ok(r) if r.success() => info!(
            elapsed_ms = step_t.elapsed().as_millis() as u64,
            "bare services install + start completed after branch switch"
        ),
        Ok(r) => {
            tracing::warn!(stderr = %r.stderr, "bare services install after branch switch had issues")
        }
        Err(e) => tracing::warn!(error = %e, "bare services install after branch switch failed"),
    }
}

// ---------------------------------------------------------------------------
// Step 5: Build images
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn build_images(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    artifact_dir: &std::path::Path,
    rebuild_svcs: &[&str],
    project_root: &Option<std::path::PathBuf>,
    project: &str,
    instance_name: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Vec<(String, String)> {
    let compose_path = artifact_dir.join("compose.yml");
    if rebuild_svcs.is_empty() || !compose_path.exists() {
        emit(
            progress,
            BuildProgressEvent::item("Building images", "no images to build", "skip"),
        )
        .await;
        return Vec::new();
    }

    let compose_to_parse = resolve_compose_path(project_root, &compose_path);
    let Some(directives) = parse_build_directives(&compose_to_parse, project) else {
        return Vec::new();
    };

    let mut tags = Vec::new();
    for directive in &directives {
        if !rebuild_svcs.contains(&directive.service_name.as_str()) {
            continue;
        }
        let result = build_single_image(
            rt,
            container_id,
            project,
            instance_name,
            directive,
            progress,
        )
        .await;
        if let Some(tag_pair) = result {
            tags.push(tag_pair);
        }
    }
    tags
}

fn resolve_compose_path(
    project_root: &Option<std::path::PathBuf>,
    artifact_compose: &std::path::Path,
) -> std::path::PathBuf {
    let code_path = project_root.clone().unwrap_or_default();
    let original = code_path.join("docker-compose.yml");
    if original.exists() {
        original
    } else {
        artifact_compose.to_path_buf()
    }
}

fn parse_build_directives(
    compose_path: &std::path::Path,
    project: &str,
) -> Option<Vec<coast_docker::compose_build::ComposeBuildDirective>> {
    let content = std::fs::read_to_string(compose_path).ok()?;
    let parsed = coast_docker::compose_build::parse_compose_file(&content, project).ok()?;
    Some(parsed.build_directives)
}

fn image_build_context(directive_context: &str) -> String {
    if directive_context == "." {
        "/workspace".to_string()
    } else {
        format!("/workspace/{directive_context}")
    }
}

async fn build_single_image(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    instance_name: &str,
    directive: &coast_docker::compose_build::ComposeBuildDirective,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Option<(String, String)> {
    let svc = &directive.service_name;
    let tag =
        coast_docker::compose_build::coast_built_instance_image_tag(project, svc, instance_name);
    let build_context = image_build_context(&directive.context);

    emit(
        progress,
        BuildProgressEvent::item("Building images", svc, "started"),
    )
    .await;
    info!(service = %svc, tag = %tag, context = %build_context, "building per-instance image inside DinD");

    let _ = rt
        .exec_in_coast(container_id, &["docker", "builder", "prune", "-af"])
        .await;
    let build_result = rt
        .exec_in_coast(
            container_id,
            &["docker", "build", "-t", &tag, &build_context],
        )
        .await;

    report_build_result(build_result, svc, &tag, progress).await
}

async fn report_build_result(
    result: Result<coast_docker::runtime::ExecResult>,
    svc: &str,
    tag: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Option<(String, String)> {
    match result {
        Ok(r) if r.success() => {
            info!(service = %svc, tag = %tag, "per-instance image built inside DinD");
            emit(
                progress,
                BuildProgressEvent::item("Building images", svc, "ok"),
            )
            .await;
            Some((svc.to_string(), tag.to_string()))
        }
        Ok(r) => {
            tracing::warn!(service = %svc, stderr = %r.stderr, "failed to build per-instance image inside DinD");
            emit(
                progress,
                BuildProgressEvent::item("Building images", svc, "warn")
                    .with_verbose(r.stderr.clone()),
            )
            .await;
            None
        }
        Err(e) => {
            tracing::warn!(service = %svc, error = %e, "failed to exec docker build inside DinD");
            emit(
                progress,
                BuildProgressEvent::item("Building images", svc, "fail"),
            )
            .await;
            None
        }
    }
}

async fn write_image_overrides(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    image_tags: &[(String, String)],
) {
    let mut override_yaml = String::from("services:\n");
    for (svc, tag) in image_tags {
        override_yaml.push_str(&format!("  {svc}:\n    image: {tag}\n"));
    }
    let write_cmd = format!(
        "printf '%s' '{}' > /coast-override/docker-compose.override.yml",
        override_yaml.replace('\'', "'\\''")
    );
    let _ = rt
        .exec_in_coast(container_id, &["sh", "-c", &write_cmd])
        .await;
}

// ---------------------------------------------------------------------------
// Step 6: Start services
// ---------------------------------------------------------------------------

async fn start_services(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    build_id: Option<&str>,
    restart_svcs: &[&str],
    rebuild_svcs: &[&str],
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    let ctx = crate::handlers::compose_context_for_build(project, build_id);
    let step_t = std::time::Instant::now();

    if !rebuild_svcs.is_empty() {
        start_service_group(
            rt,
            container_id,
            &ctx,
            rebuild_svcs,
            "rebuild",
            "docker compose up --force-recreate failed for rebuild services",
            progress,
        )
        .await;
    }
    if !restart_svcs.is_empty() {
        start_service_group(
            rt,
            container_id,
            &ctx,
            restart_svcs,
            "restart",
            "docker compose up failed for restart services",
            progress,
        )
        .await;
    }

    info!(
        elapsed_ms = step_t.elapsed().as_millis() as u64,
        "compose services started"
    );
}

async fn start_service_group(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    ctx: &crate::handlers::ComposeContext,
    svcs: &[&str],
    label: &str,
    failure_msg: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    let svc_list = svcs.join(" ");
    let cmd = ctx.compose_shell(&format!("up --force-recreate -d {svc_list}"));
    let cmd_refs: Vec<&str> = cmd.iter().map(std::string::String::as_str).collect();
    info!(services = ?svcs, "starting {label} services with force-recreate");

    for svc in svcs {
        emit(
            progress,
            BuildProgressEvent::item("Starting services", format!("{svc} ({label})"), "started"),
        )
        .await;
    }

    let result = rt.exec_in_coast(container_id, &cmd_refs).await;
    let status = if result.is_ok() { "ok" } else { "warn" };

    for svc in svcs {
        emit(
            progress,
            BuildProgressEvent::item("Starting services", format!("{svc} ({label})"), status),
        )
        .await;
    }

    if let Err(e) = &result {
        tracing::warn!(error = %e, "{failure_msg}");
    }
}

// ---------------------------------------------------------------------------
// Step 7: Wait for healthy
// ---------------------------------------------------------------------------

async fn wait_for_healthy(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    project: &str,
    build_id: Option<&str>,
    instance_name: &str,
    restart_svcs: &[&str],
    rebuild_svcs: &[&str],
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    let affected_svcs: Vec<&str> = restart_svcs
        .iter()
        .chain(rebuild_svcs.iter())
        .copied()
        .collect();

    if affected_svcs.is_empty() {
        emit(
            progress,
            BuildProgressEvent::item("Waiting for healthy", "no services to check", "skip"),
        )
        .await;
        return Ok(());
    }

    let ctx = crate::handlers::compose_context_for_build(project, build_id);
    let start_time = tokio::time::Instant::now();
    let timeout = tokio::time::Duration::from_secs(60);

    loop {
        if start_time.elapsed() >= timeout {
            let logs = fetch_compose_logs(rt, container_id, &ctx).await;
            return Err(CoastError::docker(format!(
                "Services in instance '{instance_name}' did not become healthy within 60s after assign. \
                 Check service logs:\n{logs}",
            )));
        }

        if all_services_running(rt, container_id, &ctx).await {
            info!(
                elapsed_ms = start_time.elapsed().as_millis() as u64,
                "all compose services are running after assign"
            );
            break;
        }

        tokio::time::sleep(health_poll_interval(start_time.elapsed())).await;
    }

    Ok(())
}

async fn fetch_compose_logs(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    ctx: &crate::handlers::ComposeContext,
) -> String {
    let log_cmd = ctx.compose_shell("logs --tail 50");
    let log_refs: Vec<&str> = log_cmd.iter().map(std::string::String::as_str).collect();
    rt.exec_in_coast(container_id, &log_refs)
        .await
        .map(|r| r.stdout)
        .unwrap_or_default()
}

/// Parse `docker compose ps --format json` output and check if all services
/// are in the "running" state.
fn parse_compose_ps_healthy(json_lines: &str) -> bool {
    if json_lines.trim().is_empty() {
        return false;
    }
    json_lines
        .lines()
        .filter(|l| !l.trim().is_empty())
        .all(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| {
                    v.get("State")
                        .and_then(|s| s.as_str())
                        .map(|s| s == "running")
                })
                .unwrap_or(false)
        })
}

async fn all_services_running(
    rt: &coast_docker::dind::DindRuntime,
    container_id: &str,
    ctx: &crate::handlers::ComposeContext,
) -> bool {
    let ps_cmd = ctx.compose_shell("ps --format json");
    let ps_refs: Vec<&str> = ps_cmd.iter().map(std::string::String::as_str).collect();
    let ps_result = rt.exec_in_coast(container_id, &ps_refs).await;
    match ps_result {
        Ok(ps_output) if ps_output.success() => parse_compose_ps_healthy(&ps_output.stdout),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Skip helper
// ---------------------------------------------------------------------------

/// Emit skip events for all Docker steps when no Docker client is available.
pub(super) async fn emit_skip_all(progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>) {
    emit(
        progress,
        BuildProgressEvent::done("Checking inner daemon", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Stopping services", 3, TOTAL_STEPS),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Stopping services", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Switching worktree", 4, TOTAL_STEPS),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Switching worktree", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Building images", 5, TOTAL_STEPS),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Building images", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Starting services", 6, TOTAL_STEPS),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Starting services", "skip"),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::started("Waiting for healthy", 7, TOTAL_STEPS),
    )
    .await;
    emit(
        progress,
        BuildProgressEvent::done("Waiting for healthy", "skip"),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_compose_ps_healthy_all_running() {
        let json = r#"{"Name":"web","State":"running"}
{"Name":"db","State":"running"}"#;
        assert!(parse_compose_ps_healthy(json));
    }

    #[test]
    fn test_parse_compose_ps_healthy_one_exited() {
        let json = r#"{"Name":"web","State":"running"}
{"Name":"worker","State":"exited"}"#;
        assert!(!parse_compose_ps_healthy(json));
    }

    #[test]
    fn test_parse_compose_ps_healthy_empty() {
        assert!(!parse_compose_ps_healthy(""));
        assert!(!parse_compose_ps_healthy("   \n  "));
    }

    #[test]
    fn test_parse_compose_ps_healthy_invalid_json() {
        assert!(!parse_compose_ps_healthy("not json at all"));
    }

    #[test]
    fn test_parse_compose_ps_healthy_missing_state() {
        let json = r#"{"Name":"web"}"#;
        assert!(!parse_compose_ps_healthy(json));
    }

    #[test]
    fn test_parse_compose_ps_healthy_with_blank_lines() {
        let json = r#"{"Name":"web","State":"running"}

{"Name":"db","State":"running"}
"#;
        assert!(parse_compose_ps_healthy(json));
    }

    #[test]
    fn test_services_with_action_filters_correctly() {
        let mut actions = std::collections::HashMap::new();
        actions.insert("web".to_string(), AssignAction::Hot);
        actions.insert("api".to_string(), AssignAction::Restart);
        actions.insert("db".to_string(), AssignAction::None);
        actions.insert("worker".to_string(), AssignAction::Restart);

        let mut restart = services_with_action(&actions, &AssignAction::Restart);
        restart.sort();
        assert_eq!(restart, vec!["api", "worker"]);

        let hot = services_with_action(&actions, &AssignAction::Hot);
        assert_eq!(hot, vec!["web"]);

        let rebuild = services_with_action(&actions, &AssignAction::Rebuild);
        assert!(rebuild.is_empty());
    }

    #[test]
    fn test_services_with_action_empty() {
        let actions = std::collections::HashMap::new();
        assert!(services_with_action(&actions, &AssignAction::Restart).is_empty());
    }

    #[test]
    fn test_resolve_coastfile_path_with_build_id_fallback() {
        // When the build_id path doesn't exist on disk, falls back to latest
        let path = resolve_coastfile_path("myproj", Some("abc123"));
        assert!(path.to_string_lossy().contains("latest"));
        assert!(path.to_string_lossy().contains("coastfile.toml"));
    }

    #[test]
    fn test_resolve_coastfile_path_without_build_id() {
        let path = resolve_coastfile_path("myproj", None);
        assert!(path.to_string_lossy().contains("latest"));
        assert!(path.to_string_lossy().contains("coastfile.toml"));
    }

    #[test]
    fn test_resolve_compose_path_falls_back_to_artifact() {
        let artifact = std::path::PathBuf::from("/tmp/artifact/compose.yml");
        let result = resolve_compose_path(&None, &artifact);
        assert_eq!(result, artifact);
    }
}
