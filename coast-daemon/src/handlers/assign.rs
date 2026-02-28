/// Handler for the `coast assign` command.
///
/// Reassigns a worktree to an existing coast instance (runtime slot) without
/// recreating the DinD container. Uses the `[assign]` Coastfile config to
/// selectively stop/restart/rebuild only the services that need it.
///
/// Internal flow:
/// 1. Verify instance exists and is Running or Idle (reject CheckedOut)
/// 2. Read `AssignConfig` from the artifact coastfile
/// 3. Classify services: none / restart / rebuild (with rebuild_triggers optimization)
/// 4. `docker compose stop <affected_services>` (skip services marked `none`)
/// 5. Create git worktree on host, remount /workspace inside DinD
/// 6. For `rebuild` services: `docker compose up --build -d <svcs>`
/// 7. For `restart` services: `docker compose up -d <svcs>`
/// 8. Wait for affected services healthy
/// 9. Update `branch` in state DB, set status to Running
use std::collections::HashMap;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{AssignRequest, AssignResponse, BuildProgressEvent, CoastEvent};
use coast_core::types::{AssignAction, AssignConfig, InstanceStatus};
use coast_docker::runtime::Runtime;

use crate::server::AppState;

const TOTAL_STEPS: u32 = 7;

/// Compute the adaptive health polling interval based on elapsed time.
fn health_poll_interval(elapsed: tokio::time::Duration) -> tokio::time::Duration {
    if elapsed.as_secs() < 5 {
        tokio::time::Duration::from_millis(500)
    } else if elapsed.as_secs() < 30 {
        tokio::time::Duration::from_secs(1)
    } else {
        tokio::time::Duration::from_secs(2)
    }
}

/// Read the `AssignConfig` from the artifact's coastfile.toml.
fn read_assign_config(project: &str) -> AssignConfig {
    let home = dirs::home_dir().unwrap_or_default();
    let coastfile_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            return cf.assign;
        }
    }
    AssignConfig::default()
}

/// Read the worktree directory from the Coastfile (default: ".coasts").
fn read_worktree_dir(project: &str) -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let coastfile_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            return cf.worktree_dir;
        }
    }
    ".coasts".to_string()
}

/// Check if this project has a compose file configured.
pub fn has_compose(project: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let coastfile_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            return cf.compose.is_some();
        }
    }
    true
}

/// Read the project root from manifest.json.
pub fn read_project_root(project: &str) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("project_root")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from)
}

/// Directories excluded from gitignored file sync (heavy or generated dirs).
const SYNC_EXCLUDE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "__pycache__",
    "dist",
    ".next",
    ".nuxt",
    "target",
    ".cache",
    ".worktrees",
    ".coast-synced",
    "__debug_bin",
];

/// Build the shell script that syncs gitignored files from the project root
/// into a worktree. Prefers `rsync --link-dest` (hardlinks, near-instant)
/// and falls back to the `git ls-files | tar` pipeline if rsync is missing.
/// Touches `.coast-synced` on success so subsequent assigns skip the copy.
fn build_gitignored_sync_script(root: &str, wt_path: &str, extra_excludes: &[String]) -> String {
    let mut all_rsync_excludes: Vec<String> = SYNC_EXCLUDE_DIRS
        .iter()
        .map(|d| format!("--exclude='{d}'"))
        .collect();
    for path in extra_excludes {
        all_rsync_excludes.push(format!("--exclude='{path}'"));
    }
    let excludes = all_rsync_excludes.join(" ");

    let mut grep_parts: Vec<String> = SYNC_EXCLUDE_DIRS
        .iter()
        .filter(|d| **d != ".git" && **d != ".coast-synced")
        .map(|d| d.replace('.', "\\."))
        .collect();
    for path in extra_excludes {
        grep_parts.push(path.replace('.', "\\."));
    }
    let grep_excludes = grep_parts.join("|");

    format!(
        "if command -v rsync >/dev/null 2>&1; then \
           rsync -a --link-dest='{root}' {excludes} --ignore-existing \
             '{root}/' '{wt_path}/' 2>/dev/null; \
         else \
           cd '{root}' && \
           git ls-files --others --ignored --exclude-standard 2>/dev/null | \
           grep -v -E '{grep_excludes}' | \
           tar -T - -cf - 2>/dev/null | \
           tar -xf - -C '{wt_path}' 2>/dev/null; \
         fi; \
         touch '{wt_path}/.coast-synced'; true"
    )
}

/// Classify each compose service into an AssignAction based on the config,
/// then apply rebuild_triggers optimization (downgrade rebuild -> restart
/// if no trigger files changed between branches).
fn classify_services(
    service_names: &[String],
    config: &AssignConfig,
    changed_files: &[String],
) -> HashMap<String, AssignAction> {
    let mut result = HashMap::new();
    for svc in service_names {
        let mut action = config.action_for_service(svc);

        if action == AssignAction::Rebuild {
            if let Some(triggers) = config.rebuild_triggers.get(svc) {
                if !triggers.is_empty() {
                    let any_trigger_changed = triggers.iter().any(|trigger| {
                        changed_files
                            .iter()
                            .any(|f| f == trigger || f.ends_with(trigger))
                    });
                    if !any_trigger_changed {
                        info!(
                            service = %svc,
                            "no rebuild trigger files changed, downgrading rebuild -> restart"
                        );
                        action = AssignAction::Restart;
                    }
                }
            }
        }

        result.insert(svc.clone(), action);
    }
    result
}

/// Send a progress event, ignoring channel-closed errors.
async fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.send(event).await;
}

/// Revert instance status after a failed assign.
async fn revert_assign_status(
    state: &AppState,
    project: &str,
    name: &str,
    prev_status: &InstanceStatus,
) {
    if let Ok(db) = state.db.try_lock() {
        let _ = db.update_instance_status(project, name, prev_status);
    }
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: name.to_string(),
        project: project.to_string(),
        status: prev_status.as_db_str().into(),
    });
}

/// Handle an assign request with streaming progress.
/// `transition_status` controls the intermediate status shown during the operation
/// (defaults to `Assigning`, use `Unassigning` for unassign flows).
pub async fn handle(
    req: AssignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<AssignResponse> {
    handle_with_status(req, state, progress, InstanceStatus::Assigning).await
}

/// Handle assign with an explicit transition status.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle_with_status(
    req: AssignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
    transition_status: InstanceStatus,
) -> Result<AssignResponse> {
    let started_at = tokio::time::Instant::now();

    info!(
        name = %req.name,
        project = %req.project,
        worktree = %req.worktree,
        "handling assign request"
    );

    // Emit the plan
    emit(
        &progress,
        BuildProgressEvent::build_plan(vec![
            "Validating instance".into(),
            "Checking inner daemon".into(),
            "Stopping services".into(),
            "Switching worktree".into(),
            "Building images".into(),
            "Starting services".into(),
            "Waiting for healthy".into(),
        ]),
    )
    .await;

    // --- Step 1: Validate instance ---
    emit(
        &progress,
        BuildProgressEvent::started("Validating instance", 1, TOTAL_STEPS),
    )
    .await;

    let db = state.db.lock().await;

    let instance =
        db.get_instance(&req.project, &req.name)?
            .ok_or_else(|| CoastError::InstanceNotFound {
                name: req.name.clone(),
                project: req.project.clone(),
            })?;

    if !instance.status.can_assign() {
        return Err(CoastError::state(format!(
            "Instance '{}' is in '{}' state and cannot be assigned a worktree. \
             Only Running or Idle instances can be assigned. \
             Run `coast start {}` to start it first.",
            req.name, instance.status, req.name,
        )));
    }

    let previous_branch = instance.branch.clone();
    let container_id = instance.container_id.clone().ok_or_else(|| {
        CoastError::state(format!(
            "Instance '{}' has no container ID. This should not happen for a Running/Idle instance. \
             Try `coast rm {} && coast run {}`.",
            req.name, req.name, req.name,
        ))
    })?;

    let assign_config = read_assign_config(&req.project);
    let project_root = read_project_root(&req.project);

    db.update_instance_status(&req.project, &req.name, &transition_status)?;
    drop(db);

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: transition_status.as_db_str().into(),
    });

    emit(
        &progress,
        BuildProgressEvent::done("Validating instance", "ok"),
    )
    .await;

    let prev_status = instance.status.clone();
    let revert_project = req.project.clone();
    let revert_name = req.name.clone();

    // --- Step 2: Check inner daemon ---
    emit(
        &progress,
        BuildProgressEvent::started("Checking inner daemon", 2, TOTAL_STEPS),
    )
    .await;

    if let Some(ref docker) = state.docker {
        let home = dirs::home_dir().unwrap_or_default();
        let artifact_dir = home
            .join(".coast")
            .join("images")
            .join(&req.project)
            .join("latest");
        let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());

        let health_timeout = tokio::time::Duration::from_secs(10);
        let health_check = rt.exec_in_coast(&container_id, &["docker", "info"]);
        match tokio::time::timeout(health_timeout, health_check).await {
            Ok(Ok(r)) if r.success() => {
                info!("assign: inner daemon healthy");
            }
            Ok(Ok(r)) => {
                revert_assign_status(state, &revert_project, &revert_name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is not healthy (exit {}). \
                     Try `coast stop {} && coast start {}`.",
                    req.name, r.exit_code, req.name, req.name,
                )));
            }
            Ok(Err(e)) => {
                revert_assign_status(state, &revert_project, &revert_name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Cannot reach inner Docker daemon in instance '{}': {e}. \
                     Try `coast stop {} && coast start {}`.",
                    req.name, req.name, req.name,
                )));
            }
            Err(_) => {
                revert_assign_status(state, &revert_project, &revert_name, &prev_status).await;
                return Err(CoastError::docker(format!(
                    "Inner Docker daemon in instance '{}' is unresponsive (timed out after {}s). \
                     The DinD container may need to be recreated. Try `coast rm {} && coast run {}`.",
                    req.name, health_timeout.as_secs(), req.name, req.name,
                )));
            }
        }

        emit(
            &progress,
            BuildProgressEvent::done("Checking inner daemon", "ok"),
        )
        .await;

        {
            // Step 3: Discover compose service names (skip for bare services)
            let all_service_names: Vec<String> = if has_compose(&req.project) {
                let svc_ctx =
                    super::compose_context_for_build(&req.project, instance.build_id.as_deref());
                let svc_cmd = svc_ctx.compose_shell("config --services");
                let svc_refs: Vec<&str> = svc_cmd.iter().map(std::string::String::as_str).collect();
                let services_result = tokio::time::timeout(
                    tokio::time::Duration::from_secs(30),
                    rt.exec_in_coast(&container_id, &svc_refs),
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
                services_result
                    .filter(coast_docker::runtime::ExecResult::success)
                    .map(|r| {
                        r.stdout
                            .lines()
                            .filter(|l| !l.trim().is_empty())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            // Step 3b: Check rebuild triggers by diffing changed files between branches
            let changed_files: Vec<String> = if let Some(ref root) = project_root {
                if let Some(ref prev) = previous_branch {
                    let diff_output = tokio::process::Command::new("git")
                        .args([
                            "diff",
                            "--name-only",
                            &format!("{}..{}", prev, req.worktree),
                        ])
                        .current_dir(root)
                        .output()
                        .await;
                    diff_output
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| {
                            String::from_utf8_lossy(&o.stdout)
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .map(String::from)
                                .collect()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Step 3c: Classify services
            let service_actions =
                classify_services(&all_service_names, &assign_config, &changed_files);

            let restart_svcs: Vec<&str> = service_actions
                .iter()
                .filter(|(_, a)| **a == AssignAction::Restart)
                .map(|(s, _)| s.as_str())
                .collect();
            let rebuild_svcs: Vec<&str> = service_actions
                .iter()
                .filter(|(_, a)| **a == AssignAction::Rebuild)
                .map(|(s, _)| s.as_str())
                .collect();

            let hot_svcs: Vec<&str> = service_actions
                .iter()
                .filter(|(_, a)| **a == AssignAction::Hot)
                .map(|(s, _)| s.as_str())
                .collect();
            let all_hot = !service_actions.is_empty()
                && service_actions
                    .values()
                    .all(|a| *a == AssignAction::Hot || *a == AssignAction::None);

            info!(
                none_count = service_actions
                    .iter()
                    .filter(|(_, a)| **a == AssignAction::None)
                    .count(),
                hot_count = hot_svcs.len(),
                restart_count = restart_svcs.len(),
                rebuild_count = rebuild_svcs.len(),
                all_hot,
                "classified services for assign"
            );

            // --- Step 3 (progress): Stopping services ---
            emit(
                &progress,
                BuildProgressEvent::started("Stopping services", 3, TOTAL_STEPS),
            )
            .await;

            if instance.status != InstanceStatus::Idle {
                let affected_svcs: Vec<&str> = restart_svcs
                    .iter()
                    .chain(rebuild_svcs.iter())
                    .copied()
                    .collect();

                if !affected_svcs.is_empty() {
                    for svc in &affected_svcs {
                        emit(
                            &progress,
                            BuildProgressEvent::item("Stopping services", *svc, "started"),
                        )
                        .await;
                    }

                    let stop_ctx = super::compose_context_for_build(
                        &req.project,
                        instance.build_id.as_deref(),
                    );
                    let svc_list = affected_svcs.clone().join(" ");
                    let stop_cmd = stop_ctx.compose_shell(&format!("stop -t 2 {svc_list}"));
                    let stop_refs: Vec<&str> =
                        stop_cmd.iter().map(std::string::String::as_str).collect();

                    info!(services = ?affected_svcs, "stopping affected compose services");
                    let stop_result = rt.exec_in_coast(&container_id, &stop_refs).await;
                    match stop_result {
                        Ok(r) if r.success() => {
                            info!("affected compose services stopped");
                            for svc in &affected_svcs {
                                emit(
                                    &progress,
                                    BuildProgressEvent::item("Stopping services", *svc, "ok"),
                                )
                                .await;
                            }
                        }
                        Ok(r) => {
                            tracing::warn!(
                                exit_code = r.exit_code,
                                stderr = %r.stderr,
                                "docker compose stop exited non-zero, continuing anyway"
                            );
                            for svc in &affected_svcs {
                                emit(
                                    &progress,
                                    BuildProgressEvent::item("Stopping services", *svc, "warn"),
                                )
                                .await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "docker compose stop failed, continuing anyway");
                            for svc in &affected_svcs {
                                emit(
                                    &progress,
                                    BuildProgressEvent::item("Stopping services", *svc, "warn"),
                                )
                                .await;
                            }
                        }
                    }
                } else {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Stopping services",
                            "no services to stop",
                            "skip",
                        ),
                    )
                    .await;
                }
            } else {
                emit(
                    &progress,
                    BuildProgressEvent::item("Stopping services", "instance idle, skip", "skip"),
                )
                .await;
            }

            emit(
                &progress,
                BuildProgressEvent::done("Stopping services", "ok"),
            )
            .await;

            // --- Step 4: Switching worktree ---
            emit(
                &progress,
                BuildProgressEvent::started("Switching worktree", 4, TOTAL_STEPS),
            )
            .await;

            if let Some(ref root) = project_root {
                {
                    let wt_dir = read_worktree_dir(&req.project);
                    let worktree_path = root.join(&wt_dir).join(&req.worktree);
                    if !worktree_path.exists() {
                        emit(
                            &progress,
                            BuildProgressEvent::item(
                                "Switching worktree",
                                format!("creating {}", req.worktree),
                                "started",
                            ),
                        )
                        .await;
                        let wt_output = tokio::process::Command::new("git")
                            .args([
                                "worktree",
                                "add",
                                &worktree_path.to_string_lossy(),
                                &req.worktree,
                            ])
                            .current_dir(root)
                            .output()
                            .await
                            .map_err(|e| {
                                CoastError::git(format!("Failed to create worktree: {e}"))
                            })?;
                        if !wt_output.status.success() {
                            let wt_create = tokio::process::Command::new("git")
                                .args([
                                    "worktree",
                                    "add",
                                    "-b",
                                    &req.worktree,
                                    &worktree_path.to_string_lossy(),
                                ])
                                .current_dir(root)
                                .output()
                                .await
                                .map_err(|e| {
                                    CoastError::git(format!("Failed to create worktree: {e}"))
                                })?;
                            if !wt_create.status.success() {
                                let stderr = String::from_utf8_lossy(&wt_create.stderr);
                                revert_assign_status(
                                    state,
                                    &revert_project,
                                    &revert_name,
                                    &prev_status,
                                )
                                .await;
                                return Err(CoastError::git(format!(
                                    "Failed to create worktree for branch '{}': {}",
                                    req.worktree,
                                    stderr.trim()
                                )));
                            }
                        }
                        info!(worktree = %req.worktree, path = %worktree_path.display(), "created git worktree");
                        emit(
                            &progress,
                            BuildProgressEvent::item(
                                "Switching worktree",
                                format!("created {}", req.worktree),
                                "ok",
                            ),
                        )
                        .await;
                    } else {
                        emit(
                            &progress,
                            BuildProgressEvent::item(
                                "Switching worktree",
                                format!("worktree {} exists", req.worktree),
                                "ok",
                            ),
                        )
                        .await;
                    }

                    // Sync gitignored files from project root into worktree.
                    // Uses rsync with --link-dest for hardlinks (near-instant, no
                    // data copy). Falls back to the tar pipeline if rsync is missing.
                    // A .coast-synced marker skips the copy on revisits.
                    let wt_path_str = worktree_path.to_string_lossy().to_string();
                    let root_str = root.to_string_lossy().to_string();
                    let marker = worktree_path.join(".coast-synced");
                    if marker.exists() {
                        info!(worktree = %req.worktree, "worktree already synced, skipping gitignored copy");
                    } else {
                        let copy_script = build_gitignored_sync_script(
                            &root_str,
                            &wt_path_str,
                            &assign_config.exclude_paths,
                        );
                        let copy_result = tokio::process::Command::new("sh")
                            .args(["-c", &copy_script])
                            .output()
                            .await;
                        if let Ok(output) = &copy_result {
                            if output.status.success() {
                                info!(worktree = %req.worktree, "synced gitignored files to worktree (hardlinks)");
                            } else {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                tracing::warn!(worktree = %req.worktree, %stderr, "gitignored sync had issues");
                            }
                        }
                    }

                    let mount_src = format!("/host-project/{}/{}", wt_dir, req.worktree);
                    let host_root = root.to_string_lossy();
                    let mount_cmd =
                        format!(
                    "umount -l /workspace 2>/dev/null; mount --bind {mount_src} /workspace && \
                     mount --make-rshared /workspace && \
                     mkdir -p '{parent}' && ln -sfn /host-project '{host_root}'",
                    parent = root.parent().map(|p| p.to_string_lossy()).unwrap_or_default(),
                    host_root = host_root,
                );
                    let mount_result = rt
                        .exec_in_coast(&container_id, &["sh", "-c", &mount_cmd])
                        .await;
                    match &mount_result {
                        Ok(r) if r.success() => {
                            info!(worktree = %req.worktree, "remounted /workspace to worktree");
                        }
                        Ok(r) => {
                            tracing::warn!(stderr = %r.stderr, "failed to remount /workspace to worktree");
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to remount /workspace to worktree");
                        }
                    }
                    let _ = state.db.lock().await.set_worktree(
                        &req.project,
                        &req.name,
                        Some(&req.worktree),
                    );
                }
            }

            emit(
                &progress,
                BuildProgressEvent::done("Switching worktree", "ok"),
            )
            .await;

            // Recreate inner containers so their bind mounts resolve through
            // the newly remounted /workspace.
            //
            // "hot" services use a fast path: skip compose down entirely and
            // go straight to `compose up --force-recreate -t 1`. This avoids
            // waiting for graceful shutdown and cuts assign time roughly in half.
            if all_hot {
                let ctx =
                    super::compose_context_for_build(&req.project, instance.build_id.as_deref());
                let up_cmd = ctx.compose_shell("up -d --force-recreate --remove-orphans -t 1");
                let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
                info!("hot assign: force-recreating containers (skipping compose down)");
                let up_result = rt.exec_in_coast(&container_id, &up_refs).await;
                match &up_result {
                    Ok(r) if r.success() => {
                        info!("hot assign: compose up --force-recreate completed");
                    }
                    Ok(r) => {
                        tracing::warn!(stderr = %r.stderr, "hot assign: compose up had issues");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "hot assign: compose up failed");
                    }
                }
            } else if crate::bare_services::has_bare_services(docker, &container_id).await {
                // Bare services: stop, re-run install, start
                let stop_cmd = crate::bare_services::generate_stop_command();
                let _ = rt
                    .exec_in_coast(&container_id, &["sh", "-c", &stop_cmd])
                    .await;
                info!("bare services stopped for branch switch");

                // Read service definitions from the coastfile to get install steps
                let home = dirs::home_dir().unwrap_or_default();
                let cf_path = instance
                    .build_id
                    .as_ref()
                    .map(|bid| {
                        home.join(".coast")
                            .join("images")
                            .join(&req.project)
                            .join(bid)
                            .join("coastfile.toml")
                    })
                    .filter(|p| p.exists())
                    .unwrap_or_else(|| {
                        home.join(".coast")
                            .join("images")
                            .join(&req.project)
                            .join("latest")
                            .join("coastfile.toml")
                    });
                let svc_list = coast_core::coastfile::Coastfile::from_file(&cf_path)
                    .map(|cf| cf.services)
                    .unwrap_or_default();

                let start_cmd = crate::bare_services::generate_install_and_start_command(&svc_list);
                let start_result = rt
                    .exec_in_coast(&container_id, &["sh", "-c", &start_cmd])
                    .await;
                match &start_result {
                    Ok(r) if r.success() => {
                        info!("bare services install + start completed after branch switch");
                    }
                    Ok(r) => {
                        tracing::warn!(
                            stderr = %r.stderr,
                            "bare services install after branch switch had issues"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "bare services install after branch switch failed"
                        );
                    }
                }
            } else {
                let ctx =
                    super::compose_context_for_build(&req.project, instance.build_id.as_deref());
                let down_cmd = ctx.compose_shell("down --remove-orphans -t 2");
                let down_refs: Vec<&str> =
                    down_cmd.iter().map(std::string::String::as_str).collect();
                let _ = rt.exec_in_coast(&container_id, &down_refs).await;
                info!("compose down completed after workspace remount");

                let up_cmd = ctx.compose_shell("up -d --remove-orphans");
                let up_refs: Vec<&str> = up_cmd.iter().map(std::string::String::as_str).collect();
                let up_result = rt.exec_in_coast(&container_id, &up_refs).await;
                match &up_result {
                    Ok(r) if r.success() => {
                        info!("compose up completed after workspace remount");
                    }
                    Ok(r) => {
                        tracing::warn!(stderr = %r.stderr, "compose up after workspace remount had issues");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "compose up after workspace remount failed");
                    }
                }
            }

            // --- Step 5: Building images ---
            emit(
                &progress,
                BuildProgressEvent::started("Building images", 5, TOTAL_STEPS),
            )
            .await;

            let compose_path = artifact_dir.join("compose.yml");
            let mut per_instance_image_tags: Vec<(String, String)> = Vec::new();

            if !rebuild_svcs.is_empty() && compose_path.exists() {
                let code_path = project_root.clone().unwrap_or_default();
                let original_compose_path = code_path.join("docker-compose.yml");
                let compose_to_parse = if original_compose_path.exists() {
                    original_compose_path
                } else {
                    compose_path.clone()
                };

                if let Ok(compose_content) = std::fs::read_to_string(&compose_to_parse) {
                    if let Ok(parsed) = coast_docker::compose_build::parse_compose_file(
                        &compose_content,
                        &req.project,
                    ) {
                        for directive in &parsed.build_directives {
                            if !rebuild_svcs.contains(&directive.service_name.as_str()) {
                                continue;
                            }

                            let tag = coast_docker::compose_build::coast_built_instance_image_tag(
                                &req.project,
                                &directive.service_name,
                                &req.name,
                            );

                            let build_context = if directive.context == "." {
                                "/workspace".to_string()
                            } else {
                                format!("/workspace/{}", directive.context)
                            };

                            emit(
                                &progress,
                                BuildProgressEvent::item(
                                    "Building images",
                                    &directive.service_name,
                                    "started",
                                ),
                            )
                            .await;

                            info!(
                                service = %directive.service_name,
                                tag = %tag,
                                context = %build_context,
                                "building per-instance image inside DinD"
                            );

                            let _ = rt
                                .exec_in_coast(
                                    &container_id,
                                    &["docker", "builder", "prune", "-af"],
                                )
                                .await;

                            let build_result = rt
                                .exec_in_coast(
                                    &container_id,
                                    &["docker", "build", "-t", &tag, &build_context],
                                )
                                .await;

                            match build_result {
                                Ok(r) if r.success() => {
                                    info!(
                                        service = %directive.service_name,
                                        tag = %tag,
                                        "per-instance image built inside DinD"
                                    );
                                    emit(
                                        &progress,
                                        BuildProgressEvent::item(
                                            "Building images",
                                            &directive.service_name,
                                            "ok",
                                        ),
                                    )
                                    .await;
                                    per_instance_image_tags
                                        .push((directive.service_name.clone(), tag));
                                }
                                Ok(r) => {
                                    tracing::warn!(
                                        service = %directive.service_name,
                                        stderr = %r.stderr,
                                        "failed to build per-instance image inside DinD"
                                    );
                                    emit(
                                        &progress,
                                        BuildProgressEvent::item(
                                            "Building images",
                                            &directive.service_name,
                                            "warn",
                                        )
                                        .with_verbose(r.stderr.clone()),
                                    )
                                    .await;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        service = %directive.service_name,
                                        error = %e,
                                        "failed to exec docker build inside DinD"
                                    );
                                    emit(
                                        &progress,
                                        BuildProgressEvent::item(
                                            "Building images",
                                            &directive.service_name,
                                            "fail",
                                        ),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                }
            } else {
                emit(
                    &progress,
                    BuildProgressEvent::item("Building images", "no images to build", "skip"),
                )
                .await;
            }

            // Write per-instance image overrides
            if !per_instance_image_tags.is_empty() {
                let mut override_yaml = String::from("services:\n");
                for (svc, tag) in &per_instance_image_tags {
                    override_yaml.push_str(&format!("  {svc}:\n    image: {tag}\n"));
                }
                let write_cmd = format!(
                    "printf '%s' '{}' > /coast-override/docker-compose.override.yml",
                    override_yaml.replace('\'', "'\\''")
                );
                let _ = rt
                    .exec_in_coast(&container_id, &["sh", "-c", &write_cmd])
                    .await;
            }

            emit(&progress, BuildProgressEvent::done("Building images", "ok")).await;

            // --- Step 6: Starting services ---
            emit(
                &progress,
                BuildProgressEvent::started("Starting services", 6, TOTAL_STEPS),
            )
            .await;

            let ctx = super::compose_context_for_build(&req.project, instance.build_id.as_deref());

            if !rebuild_svcs.is_empty() {
                let svc_list = rebuild_svcs.join(" ");
                let cmd = ctx.compose_shell(&format!("up --force-recreate -d {svc_list}"));
                let cmd_refs: Vec<&str> = cmd.iter().map(std::string::String::as_str).collect();
                info!(services = ?rebuild_svcs, "starting rebuild services with force-recreate");
                for svc in &rebuild_svcs {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Starting services",
                            format!("{svc} (rebuild)"),
                            "started",
                        ),
                    )
                    .await;
                }
                let result = rt.exec_in_coast(&container_id, &cmd_refs).await;
                let status = if result.is_ok() { "ok" } else { "warn" };
                for svc in &rebuild_svcs {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Starting services",
                            format!("{svc} (rebuild)"),
                            status,
                        ),
                    )
                    .await;
                }
                if let Err(e) = &result {
                    tracing::warn!(error = %e, "docker compose up --force-recreate failed for rebuild services");
                }
            }

            if !restart_svcs.is_empty() {
                let svc_list = restart_svcs.join(" ");
                let cmd = ctx.compose_shell(&format!("up --force-recreate -d {svc_list}"));
                let cmd_refs: Vec<&str> = cmd.iter().map(std::string::String::as_str).collect();
                info!(services = ?restart_svcs, "starting restart services with force-recreate");
                for svc in &restart_svcs {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Starting services",
                            format!("{svc} (restart)"),
                            "started",
                        ),
                    )
                    .await;
                }
                let result = rt.exec_in_coast(&container_id, &cmd_refs).await;
                let status = if result.is_ok() { "ok" } else { "warn" };
                for svc in &restart_svcs {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Starting services",
                            format!("{svc} (restart)"),
                            status,
                        ),
                    )
                    .await;
                }
                if let Err(e) = &result {
                    tracing::warn!(error = %e, "docker compose up failed for restart services");
                }
            }

            emit(
                &progress,
                BuildProgressEvent::done("Starting services", "ok"),
            )
            .await;

            // --- Step 7: Waiting for healthy ---
            emit(
                &progress,
                BuildProgressEvent::started("Waiting for healthy", 7, TOTAL_STEPS),
            )
            .await;

            let affected_svcs: Vec<&str> = restart_svcs
                .iter()
                .chain(rebuild_svcs.iter())
                .copied()
                .collect();

            if !affected_svcs.is_empty() {
                let start_time = tokio::time::Instant::now();
                let timeout = tokio::time::Duration::from_secs(60);
                loop {
                    if start_time.elapsed() >= timeout {
                        let log_cmd = ctx.compose_shell("logs --tail 50");
                        let log_refs: Vec<&str> =
                            log_cmd.iter().map(std::string::String::as_str).collect();
                        let log_result = rt.exec_in_coast(&container_id, &log_refs).await;
                        let logs = log_result.map(|r| r.stdout).unwrap_or_default();
                        revert_assign_status(state, &revert_project, &revert_name, &prev_status)
                            .await;
                        return Err(CoastError::docker(format!(
                        "Services in instance '{}' did not become healthy within 60s after assign. \
                         Check service logs:\n{}",
                        req.name, logs
                    )));
                    }

                    let ps_cmd = ctx.compose_shell("ps --format json");
                    let ps_refs: Vec<&str> =
                        ps_cmd.iter().map(std::string::String::as_str).collect();
                    let ps_result = rt.exec_in_coast(&container_id, &ps_refs).await;
                    if let Ok(ps_output) = ps_result {
                        if ps_output.success() && !ps_output.stdout.trim().is_empty() {
                            let all_healthy = ps_output
                                .stdout
                                .lines()
                                .filter(|l| !l.trim().is_empty())
                                .all(|line| {
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                                        let state =
                                            v.get("State").and_then(|s| s.as_str()).unwrap_or("");
                                        state == "running"
                                    } else {
                                        false
                                    }
                                });
                            if all_healthy {
                                info!("all compose services are running after assign");
                                break;
                            }
                        }
                    }

                    tokio::time::sleep(health_poll_interval(start_time.elapsed())).await;
                }
            } else {
                emit(
                    &progress,
                    BuildProgressEvent::item("Waiting for healthy", "no services to check", "skip"),
                )
                .await;
            }

            emit(
                &progress,
                BuildProgressEvent::done("Waiting for healthy", "ok"),
            )
            .await;
        } // end compose/bare services block
    } else {
        // No Docker client — skip all Docker steps
        emit(
            &progress,
            BuildProgressEvent::done("Checking inner daemon", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::started("Stopping services", 3, TOTAL_STEPS),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Stopping services", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::started("Switching worktree", 4, TOTAL_STEPS),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Switching worktree", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::started("Building images", 5, TOTAL_STEPS),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Building images", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::started("Starting services", 6, TOTAL_STEPS),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Starting services", "skip"),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::started("Waiting for healthy", 7, TOTAL_STEPS),
        )
        .await;
        emit(
            &progress,
            BuildProgressEvent::done("Waiting for healthy", "skip"),
        )
        .await;
    }

    // Step 9: Update state DB. Restore the previous status (preserves CheckedOut).
    let final_status = if prev_status == InstanceStatus::Idle {
        InstanceStatus::Running
    } else {
        prev_status.clone()
    };
    let db = state.db.lock().await;
    db.update_instance_branch(
        &req.project,
        &req.name,
        Some(&req.worktree),
        req.commit_sha.as_deref(),
        &final_status,
    )?;

    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().into(),
    });

    info!(
        name = %req.name,
        worktree = %req.worktree,
        previous = ?previous_branch,
        "worktree assigned successfully"
    );

    let elapsed_ms = started_at.elapsed().as_millis() as u64;

    Ok(AssignResponse {
        name: req.name,
        worktree: req.worktree,
        previous_worktree: previous_branch,
        time_elapsed_ms: elapsed_ms,
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
            branch: Some("old-branch".to_string()),
            commit_sha: None,
            container_id: Some(format!("{project}-coasts-{name}")),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        }
    }

    /// Create a progress sender that discards events.
    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    #[tokio::test]
    async fn test_assign_instance_not_found() {
        let db = StateDb::open_in_memory().unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "nonexistent".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found") || err.contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_assign_stopped_instance_rejected() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Stopped))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("stopped"));
        assert!(err.contains("coast start"));
    }

    #[tokio::test]
    async fn test_assign_checked_out_instance_preserves_status() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance(
            "dev-1",
            "proj",
            InstanceStatus::CheckedOut,
        ))
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.worktree, "feature/x");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);
    }

    #[tokio::test]
    async fn test_assign_idle_instance_no_compose_down() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&CoastInstance {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Idle,
            branch: None,
            commit_sha: None,
            container_id: Some("proj-coasts-dev-1".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert_eq!(resp.worktree, "feature/x");
        assert!(resp.previous_worktree.is_none());
    }

    #[tokio::test]
    async fn test_assign_running_instance_without_docker() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&sample_instance("dev-1", "proj", InstanceStatus::Running))
            .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/new".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "dev-1");
        assert_eq!(resp.worktree, "feature/new");
        assert_eq!(resp.previous_worktree, Some("old-branch".to_string()));

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.branch, Some("feature/new".to_string()));
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[tokio::test]
    async fn test_assign_no_container_id_errors() {
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
        })
        .unwrap();
        let state = AppState::new_for_testing(db);

        let req = AssignRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            worktree: "feature/x".to_string(),
            commit_sha: None,
        };

        let result = handle(req, &state, discard_progress()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no container ID"));
    }

    // --- classify_services tests ---

    #[test]
    fn test_classify_services_all_default_restart() {
        let config = AssignConfig::default();
        let services = vec!["api".to_string(), "db".to_string(), "redis".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
        assert_eq!(result.get("db"), Some(&AssignAction::Restart));
        assert_eq!(result.get("redis"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_classify_services_with_overrides() {
        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("db".to_string(), AssignAction::None);
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: HashMap::new(),
            exclude_paths: vec![],
        };

        let services = vec!["api".to_string(), "db".to_string(), "worker".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
        assert_eq!(result.get("db"), Some(&AssignAction::None));
        assert_eq!(result.get("worker"), Some(&AssignAction::Rebuild));
    }

    #[test]
    fn test_classify_services_rebuild_trigger_downgrade() {
        let mut triggers = HashMap::new();
        triggers.insert(
            "worker".to_string(),
            vec!["Dockerfile".to_string(), "package.json".to_string()],
        );

        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: triggers,
            exclude_paths: vec![],
        };

        let changed = vec!["src/main.rs".to_string(), "README.md".to_string()];
        let result = classify_services(
            &["worker".to_string(), "api".to_string()],
            &config,
            &changed,
        );
        assert_eq!(result.get("worker"), Some(&AssignAction::Restart));
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_classify_services_rebuild_trigger_keeps_rebuild() {
        let mut triggers = HashMap::new();
        triggers.insert(
            "worker".to_string(),
            vec!["Dockerfile".to_string(), "package.json".to_string()],
        );

        let mut svc_overrides = HashMap::new();
        svc_overrides.insert("worker".to_string(), AssignAction::Rebuild);

        let config = AssignConfig {
            default: AssignAction::Restart,
            services: svc_overrides,
            rebuild_triggers: triggers,
            exclude_paths: vec![],
        };

        let changed = vec!["Dockerfile".to_string(), "src/main.rs".to_string()];
        let result = classify_services(&["worker".to_string()], &config, &changed);
        assert_eq!(result.get("worker"), Some(&AssignAction::Rebuild));
    }

    #[test]
    fn test_classify_services_default_none() {
        let config = AssignConfig {
            default: AssignAction::None,
            services: HashMap::new(),
            rebuild_triggers: HashMap::new(),
            exclude_paths: vec![],
        };

        let services = vec!["api".to_string(), "db".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("api"), Some(&AssignAction::None));
        assert_eq!(result.get("db"), Some(&AssignAction::None));
    }

    #[test]
    fn test_classify_services_hot() {
        let config = AssignConfig {
            default: AssignAction::Hot,
            services: Default::default(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("web"), Some(&AssignAction::Hot));
        assert_eq!(result.get("api"), Some(&AssignAction::Hot));
    }

    #[test]
    fn test_classify_services_mixed_hot_restart() {
        let config = AssignConfig {
            default: AssignAction::Restart,
            services: [("web".to_string(), AssignAction::Hot)]
                .into_iter()
                .collect(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string()];
        let result = classify_services(&services, &config, &[]);
        assert_eq!(result.get("web"), Some(&AssignAction::Hot));
        assert_eq!(result.get("api"), Some(&AssignAction::Restart));
    }

    #[test]
    fn test_hot_services_excluded_from_restart_and_rebuild_lists() {
        let config = AssignConfig {
            default: AssignAction::Hot,
            services: [("db".to_string(), AssignAction::Restart)]
                .into_iter()
                .collect(),
            rebuild_triggers: Default::default(),
            exclude_paths: vec![],
        };
        let services = vec!["web".to_string(), "api".to_string(), "db".to_string()];
        let result = classify_services(&services, &config, &[]);
        let restart: Vec<&str> = result
            .iter()
            .filter(|(_, a)| **a == AssignAction::Restart)
            .map(|(s, _)| s.as_str())
            .collect();
        let hot: Vec<&str> = result
            .iter()
            .filter(|(_, a)| **a == AssignAction::Hot)
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(restart, vec!["db"]);
        assert!(hot.contains(&"web"));
        assert!(hot.contains(&"api"));
    }

    #[test]
    fn test_copy_script_uses_rsync_with_link_dest() {
        let script =
            build_gitignored_sync_script("/home/user/project", "/home/user/.worktrees/feat", &[]);
        assert!(
            script.contains("rsync -a --link-dest='/home/user/project'"),
            "should use rsync with --link-dest pointing to project root"
        );
        assert!(
            script.contains("'/home/user/project/' '/home/user/.worktrees/feat/'"),
            "should rsync from root/ to wt_path/"
        );
    }

    #[test]
    fn test_copy_script_excludes_all_heavy_dirs() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        for dir in SYNC_EXCLUDE_DIRS {
            assert!(
                script.contains(&format!("--exclude='{dir}'")),
                "rsync should exclude '{dir}'"
            );
        }
    }

    #[test]
    fn test_copy_script_creates_marker() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        assert!(
            script.contains("touch '/wt/.coast-synced'"),
            "should create .coast-synced marker on success"
        );
    }

    #[test]
    fn test_copy_script_has_tar_fallback() {
        let script = build_gitignored_sync_script("/root", "/wt", &[]);
        assert!(
            script.contains("if command -v rsync"),
            "should check for rsync availability"
        );
        assert!(
            script.contains("tar -T - -cf -"),
            "should fall back to tar pipeline when rsync is missing"
        );
    }

    #[test]
    fn test_exclude_paths_in_sync_script() {
        let extras = vec!["apps/ide".to_string(), "apps/extension".to_string()];
        let script = build_gitignored_sync_script("/root", "/wt", &extras);
        assert!(
            script.contains("--exclude='apps/ide'"),
            "rsync should exclude extra path 'apps/ide'"
        );
        assert!(
            script.contains("--exclude='apps/extension'"),
            "rsync should exclude extra path 'apps/extension'"
        );
    }

    #[test]
    fn test_exclude_paths_in_tar_fallback() {
        let extras = vec!["apps/ide".to_string(), "apps/extension".to_string()];
        let script = build_gitignored_sync_script("/root", "/wt", &extras);
        assert!(
            script.contains("apps/ide"),
            "tar fallback grep pattern should include 'apps/ide'"
        );
        assert!(
            script.contains("apps/extension"),
            "tar fallback grep pattern should include 'apps/extension'"
        );
        let grep_idx = script.find("grep -v -E").expect("should have grep");
        let grep_section = &script[grep_idx..];
        assert!(
            grep_section.contains("apps/ide"),
            "grep section should contain apps/ide"
        );
        assert!(
            grep_section.contains("apps/extension"),
            "grep section should contain apps/extension"
        );
    }

    #[test]
    fn test_marker_skip_when_exists() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".coast-synced");
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists(), "marker should exist");
    }

    #[test]
    fn test_marker_not_present_initially() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join(".coast-synced");
        assert!(!marker.exists(), "marker should not exist in fresh dir");
    }
}
