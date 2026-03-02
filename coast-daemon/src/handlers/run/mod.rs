/// Handler for the `coast run` command.
///
/// Creates a new coast instance: records it in the state DB,
/// creates the coast container with project root bind-mounted,
/// loads cached images, starts the inner compose stack, and allocates ports.
mod archive_build;
mod compose_rewrite;
mod host_builds;
mod image_loading;
mod mcp_setup;
mod secrets;
mod shared_services_setup;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, RunRequest, RunResponse};
use coast_core::types::PortMapping;
use coast_docker::runtime::Runtime;

use crate::server::AppState;

fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

/// Compute the adaptive health polling interval based on elapsed time.
///
/// - First 5s: poll every 500ms (services are starting)
/// - 5-30s: poll every 1s
/// - 30s+: poll every 2s
fn health_poll_interval(elapsed: tokio::time::Duration) -> tokio::time::Duration {
    if elapsed.as_secs() < 5 {
        tokio::time::Duration::from_millis(500)
    } else if elapsed.as_secs() < 30 {
        tokio::time::Duration::from_secs(1)
    } else {
        tokio::time::Duration::from_secs(2)
    }
}

/// Resolve the per-type `latest` symlink to get the actual build_id for a project.
///
/// For the default type (None), reads `latest`. For a named type, reads `latest-{type}`.
pub fn resolve_latest_build_id(project: &str, coastfile_type: Option<&str>) -> Option<String> {
    let home = dirs::home_dir()?;
    let latest_name = match coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = home
        .join(".coast")
        .join("images")
        .join(project)
        .join(latest_name);
    std::fs::read_link(&latest_link)
        .ok()
        .and_then(|target| target.file_name().map(|f| f.to_string_lossy().into_owned()))
}

fn port_mappings_from_pre_allocated_ports(
    pre_allocated_ports: &[(String, u16, u16)],
) -> Vec<PortMapping> {
    pre_allocated_ports
        .iter()
        .map(|(logical_name, canonical, dynamic)| PortMapping {
            logical_name: logical_name.clone(),
            canonical_port: *canonical,
            dynamic_port: *dynamic,
            is_primary: false,
        })
        .collect()
}

fn merge_dynamic_port_env_vars(
    env_vars: &mut std::collections::HashMap<String, String>,
    pre_allocated_ports: &[(String, u16, u16)],
) {
    let mappings = port_mappings_from_pre_allocated_ports(pre_allocated_ports);
    let dynamic_env = super::ports::dynamic_port_env_vars_from_mappings(&mappings);
    for (key, value) in dynamic_env {
        if env_vars.contains_key(&key) {
            warn!(
                env_var = %key,
                "dynamic port env var conflicts with existing env var; preserving existing value"
            );
            continue;
        }
        env_vars.insert(key, value);
    }
}

/// Detect whether the project uses compose, bare services, or neither.
///
/// Reads the coastfile from the build artifact to determine the startup mode and
/// extract the compose-relative directory for project naming.
fn detect_coastfile_info(
    project: &str,
    resolved_build_id: Option<&str>,
) -> (
    bool,
    Option<String>,
    bool,
    Vec<coast_core::types::BareServiceConfig>,
) {
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let coastfile_path = resolved_build_id
        .map(|bid| project_dir.join(bid).join("coastfile.toml"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| project_dir.join("coastfile.toml"));
    if !coastfile_path.exists() {
        return (true, None, false, vec![]);
    }
    let raw_text = std::fs::read_to_string(&coastfile_path).unwrap_or_default();
    let has_autostart_false = raw_text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "autostart = false" || trimmed.starts_with("autostart = false ")
    });
    if has_autostart_false {
        return (false, None, false, vec![]);
    }
    match coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
        Ok(cf) => {
            let svc_list = cf.services.clone();
            let has_svc = !svc_list.is_empty();
            let rel_dir = cf.compose.as_ref().and_then(|p| {
                let parent = p.parent()?;
                let artifact_parent = coastfile_path.parent()?;
                if parent == artifact_parent {
                    return None;
                }
                parent
                    .strip_prefix(artifact_parent)
                    .ok()
                    .and_then(|rel| rel.to_str())
                    .filter(|s| !s.is_empty())
                    .map(std::string::ToString::to_string)
            });
            (cf.compose.is_some(), rel_dir, has_svc, svc_list)
        }
        Err(_) => (true, None, false, vec![]),
    }
}

/// Resolve the branch name: use explicit value if provided, otherwise detect from git HEAD.
async fn resolve_branch(
    explicit_branch: Option<&str>,
    project: &str,
    resolved_build_id: Option<&str>,
) -> Option<String> {
    if let Some(b) = explicit_branch {
        return Some(b.to_string());
    }
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = resolved_build_id
        .map(|bid| project_dir.join(bid).join("manifest.json"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| project_dir.join("manifest.json"));
    let project_root = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("project_root")?
                    .as_str()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&project_root)
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if b.is_empty() || b == "HEAD" {
                None
            } else {
                Some(b)
            }
        }
        _ => None,
    }
}

/// Copy snapshot_source volumes before the coast container starts.
///
/// Stops containers using the source volume for a consistent copy, runs the copy,
/// then restarts the stopped containers.
#[allow(clippy::cognitive_complexity)]
async fn copy_snapshot_volumes(
    volumes: &[coast_core::types::VolumeConfig],
    instance_name: &str,
    project: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    for vol_config in volumes {
        let Some(ref src) = vol_config.snapshot_source else {
            continue;
        };
        let dest = coast_core::volume::resolve_volume_name(vol_config, instance_name, project);
        info!(source = %src, dest = %dest, "copying snapshot_source volume");
        emit(
            progress,
            BuildProgressEvent::done(
                format!("Copying volume {} \u{2192} {}", src, dest),
                "started",
            ),
        );

        let using_output = tokio::process::Command::new("docker")
            .args(["ps", "-q", "--filter", &format!("volume={src}")])
            .output()
            .await
            .map_err(|e| {
                CoastError::docker(format!(
                    "failed to check containers using volume '{src}': {e}"
                ))
            })?;
        let stopped_ids: Vec<String> = String::from_utf8_lossy(&using_output.stdout)
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(std::string::ToString::to_string)
            .collect();

        if !stopped_ids.is_empty() {
            info!(
                volume = %src,
                containers = ?stopped_ids,
                "stopping containers for consistent snapshot copy"
            );
            let mut stop_args = vec!["stop".to_string()];
            stop_args.extend(stopped_ids.clone());
            let stop_out = tokio::process::Command::new("docker")
                .args(&stop_args)
                .output()
                .await
                .map_err(|e| {
                    CoastError::docker(format!(
                        "failed to stop containers using volume '{src}': {e}"
                    ))
                })?;
            if !stop_out.status.success() {
                warn!(
                    "failed to stop containers on volume '{}': {}",
                    src,
                    String::from_utf8_lossy(&stop_out.stderr)
                );
            }
        }

        let cmd = coast_core::volume::snapshot_copy_command(src, &dest);
        let output = tokio::process::Command::new(&cmd[0])
            .args(&cmd[1..])
            .output()
            .await
            .map_err(|e| {
                CoastError::docker(format!(
                    "failed to run snapshot copy for volume '{}': {e}",
                    vol_config.name
                ))
            })?;

        if !stopped_ids.is_empty() {
            info!(containers = ?stopped_ids, "restarting containers after snapshot copy");
            let mut start_args = vec!["start".to_string()];
            start_args.extend(stopped_ids);
            let _ = tokio::process::Command::new("docker")
                .args(&start_args)
                .output()
                .await;
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::docker(format!(
                "snapshot copy failed for volume '{}' (source: '{src}'): {stderr}. \
                 Verify the source volume exists with: docker volume ls | grep {src}",
                vol_config.name
            )));
        }
    }
    Ok(())
}

/// Start compose or bare services inside the DinD container and wait for health.
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
async fn start_and_wait_for_services(
    docker: &bollard::Docker,
    container_id: &str,
    instance_name: &str,
    project: &str,
    has_compose: bool,
    has_services: bool,
    uses_archive_build: bool,
    compose_rel_dir: Option<&str>,
    artifact_dir_opt: Option<&std::path::Path>,
    bare_services: &[coast_core::types::BareServiceConfig],
    total_steps: u32,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    if has_compose {
        let starting_step = total_steps - 1;
        emit(
            progress,
            BuildProgressEvent::started("Starting services", starting_step, total_steps),
        );
        let compose_runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let artifact_compose_exists = artifact_dir_opt.is_some();

        let project_dir = if uses_archive_build {
            "/tmp/coast-build".to_string()
        } else if let Some(dir) = compose_rel_dir {
            format!("/workspace/{}", dir)
        } else {
            "/workspace".to_string()
        };

        let merged_compose_path = "/coast-override/docker-compose.coast.yml".to_string();
        let compose_project_name = compose_rel_dir
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| format!("coast-{}", project));

        let compose_base_args: Vec<String> = if !uses_archive_build {
            let check_merged = compose_runtime
                .exec_in_coast(container_id, &["test", "-f", &merged_compose_path])
                .await;
            let has_merged = check_merged.map(|r| r.success()).unwrap_or(false);

            if has_merged {
                vec![
                    "docker".into(),
                    "compose".into(),
                    "-p".into(),
                    compose_project_name.clone(),
                    "-f".into(),
                    merged_compose_path.clone(),
                    "--project-directory".into(),
                    project_dir.clone(),
                ]
            } else if artifact_compose_exists {
                vec![
                    "docker".into(),
                    "compose".into(),
                    "-p".into(),
                    compose_project_name.clone(),
                    "-f".into(),
                    "/coast-artifact/compose.yml".into(),
                    "--project-directory".into(),
                    project_dir.clone(),
                ]
            } else {
                vec![
                    "docker".into(),
                    "compose".into(),
                    "-p".into(),
                    compose_project_name.clone(),
                ]
            }
        } else {
            vec![
                "docker".into(),
                "compose".into(),
                "-p".into(),
                compose_project_name.clone(),
                "--project-directory".into(),
                project_dir.clone(),
            ]
        };

        let base_refs: Vec<&str> = compose_base_args
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let mut compose_cmd: Vec<&str> = base_refs.clone();
        compose_cmd.extend_from_slice(&["up", "-d", "--remove-orphans"]);

        let compose_result = compose_runtime
            .exec_in_coast(container_id, &compose_cmd)
            .await;
        if let Err(e) = &compose_result {
            tracing::warn!(error = %e, "docker compose up failed");
        }

        // Wait for all services to be healthy/running (timeout: 120s)
        let health_runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let start_time = tokio::time::Instant::now();
        let timeout = tokio::time::Duration::from_secs(120);
        loop {
            if start_time.elapsed() >= timeout {
                let log_refs: Vec<&str> = compose_base_args
                    .iter()
                    .map(std::string::String::as_str)
                    .collect();
                let mut log_cmd: Vec<&str> = log_refs;
                log_cmd.extend_from_slice(&["logs", "--tail", "50"]);
                let log_result = health_runtime.exec_in_coast(container_id, &log_cmd).await;
                let logs = log_result.map(|r| r.stdout).unwrap_or_default();
                return Err(CoastError::docker(format!(
                    "Services in instance '{}' did not become healthy within 120s. \
                     Check the service logs below and fix any issues, then retry with \
                     `coast rm {} && coast run {}`.\nRecent logs:\n{}",
                    instance_name, instance_name, instance_name, logs
                )));
            }

            let ps_refs: Vec<&str> = compose_base_args
                .iter()
                .map(std::string::String::as_str)
                .collect();
            let mut ps_cmd: Vec<&str> = ps_refs;
            ps_cmd.extend_from_slice(&["ps", "--format", "json"]);
            if let Ok(result) = health_runtime.exec_in_coast(container_id, &ps_cmd).await {
                if result.success() && !result.stdout.is_empty() {
                    let all_healthy = result
                        .stdout
                        .lines()
                        .all(|line| line.contains("running") || line.contains("healthy"));
                    if all_healthy {
                        info!(instance = %instance_name, "all compose services are healthy");
                        break;
                    }
                }
            }
            tokio::time::sleep(health_poll_interval(start_time.elapsed())).await;
        }

        if uses_archive_build {
            let cleanup_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            let _ = cleanup_rt
                .exec_in_coast(container_id, &["rm", "-rf", "/tmp/coast-build"])
                .await;
        }
        emit(
            progress,
            BuildProgressEvent::done("Starting services", "ok"),
        );
    } else if has_services {
        let starting_step = total_steps - 1;
        emit(
            progress,
            BuildProgressEvent::started("Starting services", starting_step, total_steps),
        );

        let svc_runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let setup_cmd = crate::bare_services::generate_setup_and_start_command(bare_services);
        let setup_result = svc_runtime
            .exec_in_coast(container_id, &["sh", "-c", &setup_cmd])
            .await
            .map_err(|e| {
                CoastError::docker(format!(
                    "Failed to start bare services for instance '{}': {}",
                    instance_name, e
                ))
            })?;

        if !setup_result.success() {
            return Err(CoastError::docker(format!(
                "Failed to start bare services for instance '{}' (exit code {}): {}",
                instance_name, setup_result.exit_code, setup_result.stderr
            )));
        }

        for svc in bare_services {
            emit(
                progress,
                BuildProgressEvent::item(
                    "Starting services",
                    format!("{} ({})", svc.name, svc.command),
                    "started",
                ),
            );
        }

        emit(
            progress,
            BuildProgressEvent::done("Starting services", "ok"),
        );
    } else {
        info!(instance = %instance_name, "no compose file configured — skipping compose up. Instance is Idle.");
    }
    Ok(())
}

/// Handle a run request.
///
/// Steps:
/// 1. Check that the instance does not already exist.
/// 2. Insert the instance record into the state DB.
/// 3. Create the coast container on the host Docker daemon (project root bind-mounted).
/// 4. Wait for the inner Docker daemon to become ready.
/// 5. Detect if branch differs from host — if so, use git archive into DinD.
/// 6. Build per-instance images (on host or inside DinD depending on branch).
/// 7. Load cached OCI images into the inner daemon.
/// 8. Start `docker compose up` inside the coast container.
/// 9. Wait for all services to be healthy/running (timeout: 120s).
/// 10. Allocate dynamic ports and start socat forwarders.
/// 11. Update the state DB with the container ID.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: RunRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<RunResponse> {
    info!(name = %req.name, project = %req.project, branch = ?req.branch, "handling run request");
    let resolved_build_id = req
        .build_id
        .clone()
        .or_else(|| resolve_latest_build_id(&req.project, req.coastfile_type.as_deref()));

    let (has_compose, compose_rel_dir, has_services, bare_services) =
        detect_coastfile_info(&req.project, resolved_build_id.as_deref());

    // Emit progress plan
    let mut plan_steps = vec!["Preparing instance".to_string()];
    if has_compose {
        plan_steps.push("Building images".to_string());
    }
    plan_steps.push("Creating container".to_string());
    plan_steps.push("Loading cached images".to_string());
    if has_compose || has_services {
        plan_steps.push("Starting services".to_string());
    }
    plan_steps.push("Allocating ports".to_string());
    if req.worktree.is_some() {
        plan_steps.push("Assigning worktree".to_string());
    }
    let total_steps = plan_steps.len() as u32;
    emit(&progress, BuildProgressEvent::build_plan(plan_steps));
    emit(
        &progress,
        BuildProgressEvent::started("Preparing instance", 1, total_steps),
    );

    // Phase 1: DB validation + insert (locked)
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};
    let final_status = if has_compose || has_services {
        InstanceStatus::Running
    } else {
        InstanceStatus::Idle
    };

    let resolved_branch = resolve_branch(
        req.branch.as_deref(),
        &req.project,
        resolved_build_id.as_deref(),
    )
    .await;

    // Check for dangling Docker containers before acquiring the DB lock.
    // A dangling container is one that exists in Docker with the expected
    // name but has no corresponding record in the state DB (e.g. from a
    // crashed provisioning run or interrupted removal).
    let expected_container_name = format!("{}-coasts-{}", req.project, req.name);
    if let Some(ref docker) = state.docker {
        match docker
            .inspect_container(&expected_container_name, None)
            .await
        {
            Ok(_) => {
                if req.force_remove_dangling {
                    warn!(
                        container = %expected_container_name,
                        "force-removing dangling Docker container before run"
                    );
                    let opts = bollard::container::RemoveContainerOptions {
                        force: true,
                        v: true,
                        ..Default::default()
                    };
                    if let Err(e) = docker
                        .remove_container(&expected_container_name, Some(opts))
                        .await
                    {
                        warn!(
                            container = %expected_container_name,
                            error = %e,
                            "failed to remove dangling container"
                        );
                    }
                    let cache_vol =
                        coast_docker::dind::dind_cache_volume_name(&req.project, &req.name);
                    let _ = docker.remove_volume(&cache_vol, None).await;
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Preparing instance",
                            format!("Removed dangling container {}", expected_container_name),
                            "warn",
                        ),
                    );
                } else {
                    return Err(CoastError::DanglingContainerDetected {
                        name: req.name.clone(),
                        project: req.project.clone(),
                        container_name: expected_container_name,
                    });
                }
            }
            Err(_) => { /* No dangling container — proceed normally */ }
        }
    }

    {
        let db = state.db.lock().await;
        let existing = db.get_instance(&req.project, &req.name)?;
        match existing {
            Some(inst) if inst.status == InstanceStatus::Enqueued => {
                db.delete_instance(&req.project, &req.name)?;
            }
            Some(_) => {
                return Err(CoastError::InstanceAlreadyExists {
                    name: req.name.clone(),
                    project: req.project.clone(),
                });
            }
            None => {}
        }

        let instance = CoastInstance {
            name: req.name.clone(),
            project: req.project.clone(),
            status: InstanceStatus::Provisioning,
            branch: resolved_branch,
            commit_sha: req.commit_sha.clone(),
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: resolved_build_id.clone(),
            coastfile_type: req.coastfile_type.clone(),
        };
        db.insert_instance(&instance)?;
        state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
            name: req.name.clone(),
            project: req.project.clone(),
            status: "provisioning".to_string(),
        });
    }
    emit(
        &progress,
        BuildProgressEvent::done("Preparing instance", "ok"),
    );

    // Steps 3-7: Create coast container, wait for inner daemon, load images, run compose
    let mut container_id = format!("{}-coasts-{}", req.project, req.name);
    let mut pre_allocated_ports: Vec<(String, u16, u16)> = Vec::new();

    if let Some(ref docker) = state.docker {
        // Determine the code directory to bind-mount into the container.
        // Always use the project root from manifest.json (bind-mounted as /workspace).
        let code_path = {
            let home = dirs::home_dir().unwrap_or_default();
            let project_dir = home.join(".coast").join("images").join(&req.project);
            let manifest_path = resolved_build_id
                .as_ref()
                .map(|bid| project_dir.join(bid).join("manifest.json"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| project_dir.join("latest").join("manifest.json"));
            if manifest_path.exists() {
                std::fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| {
                        v.get("project_root")?
                            .as_str()
                            .map(std::path::PathBuf::from)
                    })
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
            } else {
                std::env::current_dir().unwrap_or_default()
            }
        };

        // Detect if requested branch differs from host — determines build strategy.
        // When the branch differs, we use `git archive` to pipe the right branch's code
        // into the DinD container and build images there instead of on the host.
        // Only relevant when compose is configured.
        let uses_archive_build = if !has_compose {
            false
        } else if let Some(ref branch) = req.branch {
            let host_branch_output = tokio::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(&code_path)
                .output()
                .await;
            let host_branch = host_branch_output
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            match host_branch {
                Some(ref hb) if hb != branch => {
                    info!(
                        host_branch = %hb,
                        requested_branch = %branch,
                        "branch differs from host — will use git archive build inside DinD"
                    );
                    true
                }
                _ => false,
            }
        } else {
            false
        };

        // Per-instance image tags (populated by either host build or archive build path)
        let mut per_instance_image_tags: Vec<(String, String)> = Vec::new();

        // Non-archive path: build per-instance images on HOST.
        if !uses_archive_build {
            if has_compose {
                emit(
                    &progress,
                    BuildProgressEvent::started("Building images", 2, total_steps),
                );
            }
            per_instance_image_tags = host_builds::build_per_instance_images_on_host(
                &code_path,
                &req.project,
                &req.name,
                &progress,
            )
            .await;
            if has_compose {
                emit(&progress, BuildProgressEvent::done("Building images", "ok"));
            }
        } // end if !uses_archive_build (per-instance host build)

        // Determine image cache path
        let home = dirs::home_dir().unwrap_or_default();
        let image_cache_dir = home.join(".coast").join("image-cache");
        let image_cache_path = if image_cache_dir.exists() {
            Some(image_cache_dir.as_path())
        } else {
            None
        };

        // Pre-allocate dynamic ports, resolve volumes, and read coastfile config.
        // We need these before container creation so we can publish ports and mount volumes.
        let home2 = dirs::home_dir().unwrap_or_default();
        let project_images_dir = home2.join(".coast").join("images").join(&req.project);
        let artifact_dir = if let Some(ref bid) = resolved_build_id {
            let resolved = project_images_dir.join(bid);
            if resolved.exists() {
                resolved
            } else {
                project_images_dir.join("latest")
            }
        } else {
            project_images_dir.join("latest")
        };
        let coastfile_path = artifact_dir.join("coastfile.toml");
        let mut volume_mounts: Vec<coast_docker::runtime::VolumeMount> = Vec::new();
        let mut _has_egress = false;
        let mut mcp_servers: Vec<coast_core::types::McpServerConfig> = Vec::new();
        let mut mcp_clients: Vec<coast_core::types::McpClientConnectorConfig> = Vec::new();
        let mut shared_service_names: Vec<String> = Vec::new();
        let mut _shared_service_hosts: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut shared_network: Option<String> = None;
        // (name, canonical, dynamic)
        if coastfile_path.exists() {
            if let Ok(coastfile) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
                // Port allocation
                for (port_name, port_num) in &coastfile.ports {
                    let dynamic_port = crate::port_manager::allocate_dynamic_port()?;
                    pre_allocated_ports.push((port_name.clone(), *port_num, dynamic_port));
                }

                // Volume mounting: resolve names and collect override fragments
                // (override is written after secrets are loaded so we can include file mounts)
                for vol_config in &coastfile.volumes {
                    let resolved_name = coast_core::volume::resolve_volume_name(
                        vol_config,
                        &req.name,
                        &req.project,
                    );
                    let container_mount = format!("/coast-volumes/{}", vol_config.name);
                    volume_mounts.push(coast_docker::runtime::VolumeMount {
                        volume_name: resolved_name,
                        container_path: container_mount.clone(),
                        read_only: false,
                    });
                }

                copy_snapshot_volumes(&coastfile.volumes, &req.name, &req.project, &progress)
                    .await?;

                _has_egress = !coastfile.egress.is_empty();
                mcp_servers = coastfile.mcp_servers.clone();
                mcp_clients = coastfile.mcp_clients.clone();

                // Shared services: start on host daemon, record in state DB
                if !coastfile.shared_services.is_empty() {
                    if let Some(ref docker) = state.docker {
                        let result = shared_services_setup::start_shared_services(
                            &req.project,
                            &coastfile.shared_services,
                            docker,
                            state,
                        )
                        .await?;
                        shared_service_names = result.service_names;
                        _shared_service_hosts = result.service_hosts;
                        shared_network = result.network_name;
                    }
                }
            }
        }

        // DB lock was released after instance insertion in Phase 1a.
        // Shared service operations above used brief scoped locks.
        // All subsequent work (secrets, container creation, image loading, compose up,
        // health polling) runs without the DB lock.

        // Step 4a: Load secrets from keystore and build injection plan
        let secret_plan = secrets::load_secrets_for_instance(&coastfile_path, &req.name);
        let mut secret_env_vars = secret_plan.env_vars;
        let secret_bind_mounts = secret_plan.bind_mounts;
        let secret_container_paths = secret_plan.container_paths;
        let secret_files_for_exec = secret_plan.files_for_exec;
        let secret_tmpfs_mounts: Vec<String> = Vec::new();

        let has_volume_mounts = !volume_mounts.is_empty();

        // Resolve host gateway IP. Inside a DinD container, `host-gateway` resolves
        // to the DinD's own bridge, not the real host. We need the outer Docker
        // bridge gateway so inner compose services can reach shared services on the host.
        let bridge_gateway_ip: Option<String> =
            match coast_docker::network::resolve_bridge_gateway(docker).await {
                Ok(gw) => Some(gw),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to resolve bridge gateway IP");
                    None
                }
            };

        // Generate a single merged compose file that incorporates all overrides.
        if !uses_archive_build {
            let compose_path = artifact_dir.join("compose.yml");
            let compose_content = if compose_path.exists() {
                std::fs::read_to_string(&compose_path).ok()
            } else {
                let ws_compose = code_path.join("docker-compose.yml");
                std::fs::read_to_string(&ws_compose).ok()
            };

            if let Some(ref content) = compose_content {
                let assign_cfg = coast_core::coastfile::Coastfile::from_file(&coastfile_path)
                    .map(|cf| cf.assign)
                    .unwrap_or_default();
                let hot_svcs: Vec<String> = assign_cfg
                    .services
                    .iter()
                    .filter(|(_, a)| **a == coast_core::types::AssignAction::Hot)
                    .map(|(s, _)| s.clone())
                    .collect();
                let default_hot = assign_cfg.default == coast_core::types::AssignAction::Hot;
                compose_rewrite::rewrite_compose_for_instance(
                    content,
                    &compose_rewrite::ComposeRewriteConfig {
                        shared_service_names: &shared_service_names,
                        coastfile_path: &coastfile_path,
                        per_instance_image_tags: &per_instance_image_tags,
                        has_volume_mounts: !volume_mounts.is_empty(),
                        bridge_gateway_ip: bridge_gateway_ip.as_deref(),
                        secret_container_paths: &secret_container_paths,
                        project: &req.project,
                        instance_name: &req.name,
                        hot_services: &hot_svcs,
                        default_hot,
                    },
                );
            }
        } // end if !uses_archive_build (compose generation)

        // Step 4-5: Create and start the coast container, wait for inner daemon
        // Mount the artifact directory — use the resolved build_id path so it remains
        // valid even if `latest` is later re-pointed to a different build.
        let artifact_dir_path = {
            let proj_img = home.join(".coast").join("images").join(&req.project);
            if let Some(ref bid) = resolved_build_id {
                let resolved = proj_img.join(bid);
                if resolved.exists() {
                    resolved
                } else {
                    proj_img.join("latest")
                }
            } else {
                proj_img.join("latest")
            }
        };
        let artifact_dir_opt = if artifact_dir_path.exists() {
            Some(artifact_dir_path.as_path())
        } else {
            None
        };

        // Read coast_image from manifest (set by [coast.setup] during build)
        let coast_image: Option<String> = {
            let manifest_path = artifact_dir_path.join("manifest.json");
            if manifest_path.exists() {
                std::fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("coast_image")?.as_str().map(String::from))
            } else {
                None
            }
        };

        let override_dir_path = {
            let h = dirs::home_dir().unwrap_or_default();
            h.join(".coast")
                .join("overrides")
                .join(&req.project)
                .join(&req.name)
        };
        let override_dir_opt = if override_dir_path.exists() {
            Some(override_dir_path.as_path())
        } else {
            None
        };

        // Always add host.docker.internal to the outer DinD container.
        // This ensures host connectivity works regardless of egress config.
        let dind_extra_hosts: Vec<String> = vec!["host.docker.internal:host-gateway".to_string()];

        let mut config = coast_docker::dind::build_dind_config(
            &req.project,
            &req.name,
            &code_path,
            {
                merge_dynamic_port_env_vars(&mut secret_env_vars, &pre_allocated_ports);
                secret_env_vars
            },
            secret_bind_mounts,
            volume_mounts,
            secret_tmpfs_mounts,
            image_cache_path,
            artifact_dir_opt,
            coast_image.as_deref(),
            override_dir_opt,
            dind_extra_hosts,
        );

        // Publish dynamic ports on the container so they're accessible on localhost
        for (_name, canonical, dynamic) in &pre_allocated_ports {
            config
                .published_ports
                .push(coast_docker::runtime::PortPublish {
                    host_port: *dynamic,
                    container_port: *canonical,
                });
        }

        // Step number for "Creating container" depends on whether "Building images" was shown
        let creating_step = if has_compose { 3 } else { 2 };
        emit(
            &progress,
            BuildProgressEvent::started("Creating container", creating_step, total_steps),
        );

        let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
        let manager = coast_docker::container::ContainerManager::new(runtime);
        container_id = manager.create_and_start(&config).await.map_err(|e| {
            CoastError::docker(format!(
                "Failed to create coast container for instance '{}': {}. \
                 Ensure Docker is running and the docker:dind image is available.",
                req.name, e
            ))
        })?;

        emit(
            &progress,
            BuildProgressEvent::done("Creating container", "ok"),
        );

        // Persist container_id immediately so the instance is usable even if later steps fail
        {
            let db = state.db.lock().await;
            db.update_instance_container_id(&req.project, &req.name, Some(&container_id))?;
        }

        // Connect DinD container to shared services bridge network
        if let Some(ref net_name) = shared_network {
            if let Some(ref docker) = state.docker {
                let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
                if let Err(e) = nm.connect_container(net_name, &container_id).await {
                    tracing::warn!(error = %e, "failed to connect coast container to shared network (may already be connected)");
                } else {
                    info!(network = %net_name, container = %container_id, "connected coast container to shared services network");
                }
            }
        }

        let loading_step = if has_compose { 4 } else { 3 };
        emit(
            &progress,
            BuildProgressEvent::started("Loading cached images", loading_step, total_steps),
        );

        // Step 5: Load cached OCI images into inner daemon.
        // This MUST happen before the archive build below, because `docker build`
        // inside DinD needs the base images (e.g. node:20-alpine) to be present.
        let home = dirs::home_dir().unwrap_or_default();
        let cache_dir = home.join(".coast").join("image-cache");
        if cache_dir.exists() {
            let tarball_names = image_loading::collect_project_tarballs(&cache_dir, &req.project);
            if !tarball_names.is_empty() {
                let existing_images =
                    image_loading::query_existing_images(docker, &container_id).await;
                let (tarballs_to_load, skipped) =
                    image_loading::filter_tarballs_to_load(tarball_names, &existing_images);

                if skipped > 0 {
                    emit(
                        &progress,
                        BuildProgressEvent::item(
                            "Loading cached images",
                            format!("{} already present (skipped)", skipped),
                            "skip",
                        ),
                    );
                    info!(
                        project = %req.project,
                        skipped = skipped,
                        "skipped loading images already present in inner daemon (persistent volume)"
                    );
                }

                info!(
                    project = %req.project,
                    tarball_count = tarballs_to_load.len(),
                    "loading project-relevant cached images"
                );

                image_loading::load_tarballs_into_inner_daemon(
                    &tarballs_to_load,
                    docker,
                    &container_id,
                    &progress,
                )
                .await;
            }
        }
        emit(
            &progress,
            BuildProgressEvent::done("Loading cached images", "ok"),
        );

        // === Archive path: branch differs from host ===
        // Use `git archive` to pipe the requested branch's code into the DinD container,
        // build per-instance images inside DinD, and write compose override there.
        if uses_archive_build {
            emit(
                &progress,
                BuildProgressEvent::started("Building images", 2, total_steps),
            );
            let archive_tags = archive_build::run_archive_build(
                docker,
                &container_id,
                &code_path,
                req.branch.as_deref().unwrap(),
                &req.project,
                &req.name,
                &artifact_dir,
                &coastfile_path,
                has_volume_mounts,
                &secret_container_paths,
                &progress,
            )
            .await?;
            per_instance_image_tags = archive_tags;
        }

        // Step 6a: Pipe per-instance images into inner daemon.
        // Only needed for non-archive path — archive images are already built inside DinD.
        if !uses_archive_build {
            image_loading::pipe_host_images_to_inner_daemon(
                &per_instance_image_tags,
                &container_id,
            )
            .await;
        }

        // Bind /host-project to /workspace so inner services see the project files.
        // The project root is mounted at /host-project by the DinD config.
        {
            let mount_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            let mount_result = mount_rt
                .exec_in_coast(
                    &container_id,
                    &[
                        "sh",
                        "-c",
                        "mkdir -p /workspace && mount --bind /host-project /workspace && mount --make-rshared /workspace",
                    ],
                )
                .await;
            match mount_result {
                Ok(r) if r.success() => {
                    info!(instance = %req.name, "bound /host-project -> /workspace");
                }
                Ok(r) => {
                    warn!(instance = %req.name, stderr = %r.stderr, "failed to bind /workspace");
                }
                Err(e) => {
                    warn!(instance = %req.name, error = %e, "failed to bind /workspace");
                }
            }
        }

        // Step 6.5: Install MCP servers and write client configs.
        if !mcp_servers.is_empty() || !mcp_clients.is_empty() {
            mcp_setup::install_mcp_servers(
                &container_id,
                &mcp_servers,
                &mcp_clients,
                docker,
                &progress,
            )
            .await?;
        }

        // Step 6b: Write file-type secrets directly into the DinD container via exec.
        secrets::write_secret_files_via_exec(&secret_files_for_exec, &container_id, docker).await;

        // Step 7: Start services inside the coast container.
        start_and_wait_for_services(
            docker,
            &container_id,
            &req.name,
            &req.project,
            has_compose,
            has_services,
            uses_archive_build,
            compose_rel_dir.as_deref(),
            artifact_dir_opt,
            &bare_services,
            total_steps,
            &progress,
        )
        .await?;
    }

    emit(
        &progress,
        BuildProgressEvent::started("Allocating ports", total_steps, total_steps),
    );

    // Phase 3: Re-acquire DB lock for final writes (port allocations, container ID).
    let db = state.db.lock().await;

    // Step 9: Store pre-allocated port mappings in the state DB.
    // Ports are published directly on the DinD container, so no socat needed for dynamic ports.
    let mut ports: Vec<PortMapping> = Vec::new();
    for mapping in port_mappings_from_pre_allocated_ports(&pre_allocated_ports) {
        db.insert_port_allocation(&req.project, &req.name, &mapping)?;
        emit(
            &progress,
            BuildProgressEvent::item(
                "Allocating ports",
                format!(
                    "{} :{} → :{}",
                    mapping.logical_name, mapping.canonical_port, mapping.dynamic_port
                ),
                "ok",
            ),
        );
        ports.push(mapping);
    }

    // Auto-set primary port if the build doesn't have one yet and there's only one port
    if let Some(ref bid) = resolved_build_id {
        let key = super::ports::primary_port_settings_key(&req.project, bid);
        if db.get_setting(&key)?.is_none() && ports.len() == 1 {
            db.set_setting(&key, &ports[0].logical_name)?;
        }
    }

    // Transition from Provisioning to final status (container_id was already persisted after creation)
    db.update_instance_status(&req.project, &req.name, &final_status)?;
    state.emit_event(coast_core::protocol::CoastEvent::InstanceStatusChanged {
        name: req.name.clone(),
        project: req.project.clone(),
        status: final_status.as_db_str().to_string(),
    });
    emit(
        &progress,
        BuildProgressEvent::done("Allocating ports", "ok"),
    );

    info!(
        name = %req.name,
        project = %req.project,
        container_id = %container_id,
        "instance created and running"
    );

    // Optional post-provisioning worktree assignment.
    // If the caller specified a worktree, assign it now that the instance is fully up.
    if let Some(ref worktree_name) = req.worktree {
        drop(db);

        info!(
            name = %req.name,
            worktree = %worktree_name,
            "auto-assigning worktree after provisioning"
        );
        emit(
            &progress,
            BuildProgressEvent::started("Assigning worktree", total_steps, total_steps),
        );

        let assign_req = coast_core::protocol::AssignRequest {
            name: req.name.clone(),
            project: req.project.clone(),
            worktree: worktree_name.clone(),
            commit_sha: None,
        };

        match super::assign::handle(assign_req, state, progress.clone()).await {
            Ok(resp) => {
                emit(
                    &progress,
                    BuildProgressEvent::done("Assigning worktree", "ok"),
                );
                state.emit_event(coast_core::protocol::CoastEvent::InstanceAssigned {
                    name: req.name.clone(),
                    project: req.project.clone(),
                    worktree: resp.worktree,
                });
            }
            Err(e) => {
                emit(
                    &progress,
                    BuildProgressEvent::item("Assigning worktree", format!("Warning: {e}"), "warn"),
                );
                emit(
                    &progress,
                    BuildProgressEvent::done("Assigning worktree", "warn"),
                );
                warn!(
                    name = %req.name,
                    worktree = %worktree_name,
                    error = %e,
                    "post-provisioning worktree assignment failed (coast is still running)"
                );
            }
        }
    }

    Ok(RunResponse {
        name: req.name,
        container_id,
        ports,
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
    async fn test_run_creates_instance() {
        let state = test_state();
        let req = RunRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            branch: Some("feature/oauth".to_string()),
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "feature-oauth");

        // Verify in DB
        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feature-oauth").unwrap();
        assert!(instance.is_some());
        let instance = instance.unwrap();
        assert_eq!(instance.status, coast_core::types::InstanceStatus::Running);
        assert_eq!(instance.branch, Some("feature/oauth".to_string()));
    }

    #[tokio::test]
    async fn test_run_duplicate_instance_fails() {
        let state = test_state();
        let req = RunRequest {
            name: "dup".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req.clone(), &state, tx).await;
        assert!(result.is_ok());

        // Second run should fail
        let req2 = RunRequest {
            name: "dup".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx2, _rx2) = tokio::sync::mpsc::channel(64);
        let result2 = handle(req2, &state, tx2).await;
        assert!(result2.is_err());
        let err = result2.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[tokio::test]
    async fn test_run_different_projects_same_name() {
        let state = test_state();
        let req1 = RunRequest {
            name: "main".to_string(),
            project: "project-a".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let req2 = RunRequest {
            name: "main".to_string(),
            project: "project-b".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx1, _rx1) = tokio::sync::mpsc::channel(64);
        assert!(handle(req1, &state, tx1).await.is_ok());
        let (tx2, _rx2) = tokio::sync::mpsc::channel(64);
        assert!(handle(req2, &state, tx2).await.is_ok());
    }

    #[test]
    fn test_port_mappings_from_pre_allocated_ports() {
        let pre_allocated = vec![
            ("web".to_string(), 3000, 52340),
            ("backend-test".to_string(), 8080, 52341),
        ];
        let mappings = port_mappings_from_pre_allocated_ports(&pre_allocated);
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings[0].logical_name, "web");
        assert_eq!(mappings[0].canonical_port, 3000);
        assert_eq!(mappings[0].dynamic_port, 52340);
        assert_eq!(mappings[1].logical_name, "backend-test");
        assert_eq!(mappings[1].canonical_port, 8080);
        assert_eq!(mappings[1].dynamic_port, 52341);
    }

    #[test]
    fn test_merge_dynamic_port_env_vars_inserts_vars() {
        let pre_allocated = vec![
            ("web".to_string(), 3000, 52340),
            ("backend-test".to_string(), 8080, 52341),
        ];
        let mut env = std::collections::HashMap::new();
        merge_dynamic_port_env_vars(&mut env, &pre_allocated);
        assert_eq!(env.get("WEB_DYNAMIC_PORT"), Some(&"52340".to_string()));
        assert_eq!(
            env.get("BACKEND_TEST_DYNAMIC_PORT"),
            Some(&"52341".to_string())
        );
    }

    #[test]
    fn test_merge_dynamic_port_env_vars_preserves_existing_key() {
        let pre_allocated = vec![("web".to_string(), 3000, 52340)];
        let mut env = std::collections::HashMap::new();
        env.insert("WEB_DYNAMIC_PORT".to_string(), "9999".to_string());
        merge_dynamic_port_env_vars(&mut env, &pre_allocated);
        assert_eq!(env.get("WEB_DYNAMIC_PORT"), Some(&"9999".to_string()));
    }

    #[test]
    fn test_expected_container_name_for_dangling_check() {
        let project = "my-app";
        let name = "dev-1";
        let expected = format!("{}-coasts-{}", project, name);
        assert_eq!(expected, "my-app-coasts-dev-1");
    }

    #[tokio::test]
    async fn test_run_with_force_remove_dangling_no_docker_succeeds() {
        let state = test_state();
        let req = RunRequest {
            name: "force-test".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: true,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.name, "force-test");
    }

    #[test]
    fn test_dangling_container_error_is_actionable() {
        let err = CoastError::DanglingContainerDetected {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            container_name: "my-app-coasts-dev-1".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("--force-remove-dangling"),
            "error should contain the flag hint"
        );
        assert!(
            msg.contains("coast run dev-1"),
            "error should contain the suggested command"
        );
    }

    #[test]
    fn test_dangling_cache_volume_name() {
        let vol = coast_docker::dind::dind_cache_volume_name("my-app", "dev-1");
        assert_eq!(vol, "coast-dind--my-app--dev-1");
    }
}
