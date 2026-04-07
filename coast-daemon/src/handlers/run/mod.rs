/// Handler for the `coast run` command.
///
/// Creates a new coast instance: records it in the state DB,
/// creates the coast container with project root bind-mounted,
/// loads cached images, starts the inner compose stack, and allocates ports.
mod compose_rewrite;
mod finalize;
mod host_builds;
mod image_loading;
mod mcp_setup;
pub(crate) mod paths;
mod provision;
mod secrets;
mod service_start;
mod shared_services_setup;
mod validate;

pub(crate) use service_start::compose_ps_output_is_ready;

use tracing::{info, warn};

use coast_core::error::Result;
use coast_core::protocol::{BuildProgressEvent, RunRequest, RunResponse};
use coast_core::types::PortMapping;

use crate::server::AppState;

fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

/// Resolve the per-type `latest` symlink to get the actual build_id for a project.
///
/// For the default type (None), reads `latest`. For a named type, reads `latest-{type}`.
pub fn resolve_latest_build_id(project: &str, coastfile_type: Option<&str>) -> Option<String> {
    let home = paths::active_coast_home();
    let latest_name = match coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = home.join("images").join(project).join(latest_name);
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
    let project_dir = paths::project_images_dir(project);
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
                warn!(project = %project, dir = %project_root.display(), "branch detection returned empty/HEAD, storing None");
                None
            } else {
                Some(b)
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(project = %project, dir = %project_root.display(), stderr = %stderr.trim(), "git branch detection failed");
            None
        }
        Err(e) => {
            warn!(project = %project, dir = %project_root.display(), error = %e, "git branch detection failed");
            None
        }
    }
}

/// Handle a run request.
pub async fn handle(
    req: RunRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<RunResponse> {
    info!(name = %req.name, project = %req.project, branch = ?req.branch, "handling run request");

    if state.docker.is_none() {
        return Err(coast_core::error::CoastError::docker(
            "Host Docker is not available. `coast run` requires a Docker-compatible host engine. \
             If you use Docker contexts (OrbStack, Colima, Rancher Desktop, Docker Desktop), \
             ensure coastd can resolve the active context and then restart the daemon.",
        ));
    }

    // Phase 1: Validate, resolve build_id, insert instance record
    let validated = validate::validate_and_insert(&req, state, &progress).await?;

    // Check if this is a remote coastfile type — if so, take the remote path.
    if coast_core::coastfile::Coastfile::is_remote_type(req.coastfile_type.as_deref()) {
        return handle_remote_run(req, state, &validated, &progress).await;
    }

    // Phase 2: Docker provisioning (container, images, services)
    let mut container_id = format!("{}-coasts-{}", req.project, req.name);
    let mut pre_allocated_ports: Vec<(String, u16, u16)> = Vec::new();

    if let Some(docker) = state.docker.as_ref() {
        let result =
            provision::provision_instance(&docker, &validated, &req, state, &progress).await?;
        container_id = result.container_id;
        pre_allocated_ports = result.pre_allocated_ports;
    }

    // Phase 3: Finalize (port allocations, status transition)
    let ports = finalize::finalize_instance(
        state,
        &req.project,
        &req.name,
        &container_id,
        validated.build_id.as_deref(),
        &pre_allocated_ports,
        &validated.final_status,
        validated.total_steps,
        &progress,
    )
    .await?;

    // Phase 4: Optional worktree assignment
    if let Some(ref worktree_name) = req.worktree {
        assign_worktree(&req, worktree_name, state, &progress, validated.total_steps).await;
    }

    Ok(RunResponse {
        name: req.name,
        container_id,
        ports,
    })
}

/// Load the coastfile and resolve the remote configuration from it.
fn resolve_remote_config(
    req: &RunRequest,
    validated: &validate::ValidatedRun,
) -> Result<(
    coast_core::coastfile::Coastfile,
    coast_core::types::RemoteConfig,
    std::path::PathBuf,
)> {
    let artifact_dir = paths::project_images_dir(&req.project);
    let coastfile_path = validated
        .build_id
        .as_ref()
        .map(|bid| artifact_dir.join(bid).join("coastfile.toml"))
        .filter(|p| p.exists())
        .or_else(|| {
            let p = artifact_dir.join("coastfile.toml");
            p.exists().then_some(p)
        })
        .unwrap_or_else(|| {
            let cwd = std::env::current_dir().unwrap_or_default();
            let cf_name = match req.coastfile_type.as_deref() {
                Some(t) if t != "default" => format!("Coastfile.{t}.toml"),
                _ => "Coastfile.remote.toml".to_string(),
            };
            let candidate = cwd.join(&cf_name);
            if candidate.exists() {
                candidate
            } else {
                cwd.join("Coastfile")
            }
        });

    let cf_content = std::fs::read_to_string(&coastfile_path).map_err(|e| {
        coast_core::error::CoastError::coastfile(format!("failed to read remote coastfile: {e}"))
    })?;
    let cf_dir = coastfile_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let cf = coast_core::coastfile::Coastfile::parse(&cf_content, cf_dir).map_err(|e| {
        coast_core::error::CoastError::coastfile(format!("failed to parse remote coastfile: {e}"))
    })?;

    let cf_remote = cf.remote.clone().ok_or_else(|| {
        coast_core::error::CoastError::coastfile(
            "Coastfile.remote requires a [remote] section with host configuration",
        )
    })?;

    Ok((cf, cf_remote.clone(), coastfile_path))
}

/// Resolve which remote to use at runtime.
///
/// Resolution order:
/// 1. `--remote <name>` CLI flag (via `remote_name` parameter)
/// 2. Single registered remote (auto-select)
/// 3. Error if multiple remotes and no `--remote`
async fn resolve_remote_entry(
    cf_remote: &coast_core::types::RemoteConfig,
    state: &AppState,
    remote_name: Option<&str>,
) -> Result<(coast_core::types::RemoteConnection, String)> {
    let db = state.db.lock().await;
    let remotes = db.list_remotes()?;
    drop(db);

    if remotes.is_empty() {
        return Err(coast_core::error::CoastError::state(
            "No remotes registered. Run `coast remote add <name> <user@host>` first.",
        ));
    }

    let entry = if let Some(name) = remote_name {
        remotes
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| {
                coast_core::error::CoastError::state(format!(
                    "remote '{}' is not registered. Run `coast remote add {}` first.",
                    name, name
                ))
            })?
    } else if remotes.len() == 1 {
        remotes.into_iter().next().unwrap()
    } else {
        let names: Vec<_> = remotes.iter().map(|r| r.name.as_str()).collect();
        return Err(coast_core::error::CoastError::state(format!(
            "Multiple remotes registered ({}). Use `--remote <name>` to specify which one.",
            names.join(", ")
        )));
    };

    let name = entry.name.clone();
    let connection = coast_core::types::RemoteConnection::from_entry(&entry, cf_remote);
    Ok((connection, name))
}

/// Create a shell coast container locally (sleep infinity, bind-mounted workspace).
///
/// The shell container doesn't need DinD -- it's just a workspace mirror
/// with SSH/mutagen for syncing to the remote. Uses the project's
/// `coast_image` (which has mutagen, rsync, etc.) instead of bare `docker:dind`.
async fn create_shell_coast(
    req: &RunRequest,
    state: &AppState,
    code_path: &std::path::Path,
) -> Result<()> {
    let Some(docker) = state.docker.as_ref() else {
        return Ok(());
    };

    let coast_image = resolve_coast_image_for_shell(&req.project, req.coastfile_type.as_deref());

    let shell_instance_name = format!("{}-shell", req.name);
    let mut shell_params =
        coast_docker::dind::DindConfigParams::new(&req.project, &shell_instance_name, code_path);
    shell_params.coast_image = coast_image.as_deref();
    shell_params
        .extra_hosts
        .push("host.docker.internal:host-gateway".to_string());
    let mut shell_config = coast_docker::dind::build_dind_config(shell_params);
    shell_config.entrypoint = Some(vec!["sleep".to_string(), "infinity".to_string()]);
    shell_config.cmd = None;

    let external_worktree_dirs = resolve_shell_worktree_dirs(code_path);
    for ext_dir in &external_worktree_dirs {
        shell_config
            .bind_mounts
            .push(coast_docker::runtime::BindMount {
                host_path: ext_dir.resolved_path.clone(),
                container_path: coast_core::coastfile::Coastfile::external_mount_path(
                    ext_dir.mount_index,
                ),
                read_only: false,
                propagation: None,
            });
    }

    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;
    let container_id = rt.create_coast_container(&shell_config).await?;
    rt.start_coast_container(&container_id).await?;

    let bind_cmd = "mkdir -p /workspace && mount --bind /host-project /workspace && mount --make-rshared /workspace";
    let _ = rt
        .exec_in_coast(&container_id, &["sh", "-c", bind_cmd])
        .await;

    let db = state.db.lock().await;
    db.update_instance_container_id(&req.project, &req.name, Some(&container_id))?;
    drop(db);

    info!(
        container_id = %container_id,
        image = coast_image.as_deref().unwrap_or("docker:dind"),
        "shell coast container created"
    );
    Ok(())
}

/// Resolve the coast_image for the shell container from the local build artifact.
fn resolve_coast_image_for_shell(project: &str, coastfile_type: Option<&str>) -> Option<String> {
    let project_dir = paths::project_images_dir(project);
    for link_name in &[
        coastfile_type
            .filter(|t| *t != "default")
            .map(|t| format!("latest-{t}")),
        Some("latest-remote".to_string()),
        Some("latest".to_string()),
    ] {
        let Some(ref name) = link_name else {
            continue;
        };
        let manifest = project_dir.join(name).join("manifest.json");
        if !manifest.exists() {
            continue;
        }
        let image = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("coast_image")?.as_str().map(String::from));
        if image.is_some() {
            return image;
        }
    }
    None
}

fn resolve_shell_worktree_dirs(
    code_path: &std::path::Path,
) -> Vec<coast_core::coastfile::ResolvedExternalDir> {
    let cf_path = coast_core::coastfile::Coastfile::find_coastfile(code_path, "Coastfile");
    let cf = cf_path.and_then(|p| coast_core::coastfile::Coastfile::from_file(&p).ok());
    match cf {
        Some(cf) => coast_core::coastfile::Coastfile::resolve_external_worktree_dirs_expanded(
            &cf.worktree_dirs,
            &cf.project_root,
        ),
        None => Vec::new(),
    }
}

/// Read the compose project directory from the local build artifact.
/// Returns `/workspace/{dir}` where `dir` is the parent of the compose path.
pub(crate) fn read_compose_project_dir_from_artifact(
    project_images_dir: &std::path::Path,
) -> String {
    for link_name in &["latest-remote", "latest"] {
        let cf_path = project_images_dir.join(link_name).join("coastfile.toml");
        if let Ok(content) = std::fs::read_to_string(&cf_path) {
            if let Ok(toml_val) = content.parse::<toml::Value>() {
                if let Some(compose_str) = toml_val
                    .get("coast")
                    .and_then(|c| c.get("compose"))
                    .and_then(|v| v.as_str())
                {
                    let p = std::path::Path::new(compose_str);
                    if let Some(dir) = p.parent().and_then(|d| d.to_str()) {
                        if !dir.is_empty() && dir != "." {
                            let clean = dir.trim_start_matches("./");
                            return format!("/workspace/{clean}");
                        }
                    }
                }
            }
        }
    }
    "/workspace".to_string()
}

/// Load the primary_port from the local build artifact for a project.
fn load_primary_port(project: &str, coastfile_type: Option<&str>) -> Option<String> {
    let project_dir = paths::project_images_dir(project);
    for link_name in &[
        coastfile_type
            .filter(|t| *t != "default")
            .map(|t| format!("latest-{t}")),
        Some("latest-remote".to_string()),
        Some("latest".to_string()),
    ] {
        let Some(ref name) = link_name else {
            continue;
        };
        let cf_path = project_dir.join(name).join("coastfile.toml");
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&cf_path) {
            if let Some(ref primary) = cf.primary_port {
                return Some(primary.clone());
            }
            if cf.ports.len() == 1 {
                return cf.ports.keys().next().cloned();
            }
        }
    }
    None
}

/// Trigger a build on the remote coast-service.
///
/// Instead of building locally and transferring artifacts, this syncs the project
/// source to the remote and calls POST /build on coast-service, which builds
/// natively on the remote's architecture.
async fn trigger_remote_build(
    client: &super::remote::RemoteClient,
    _remote_config: &coast_core::types::RemoteConnection,
    project: &str,
    instance_name: &str,
    coastfile_type: Option<&str>,
    service_home: &str,
) -> Result<coast_core::protocol::BuildResponse> {
    let coastfile_name = match coastfile_type {
        Some(t) if t != "default" => format!("Coastfile.{t}.toml"),
        _ => "Coastfile.remote.toml".to_string(),
    };

    let coastfile_path = std::path::PathBuf::from(format!(
        "{service_home}/workspaces/{project}/{instance_name}/{coastfile_name}"
    ));

    let build_req = coast_core::protocol::BuildRequest {
        coastfile_path: coastfile_path.clone(),
        refresh: false,
        remote: None,
    };

    info!(coastfile_path = %coastfile_path.display(), "triggering remote build on coast-service");

    super::remote::forward::forward_build(client, &build_req).await
}

pub(crate) async fn download_remote_artifact(
    build_response: &coast_core::protocol::BuildResponse,
    project: &str,
    coastfile_type: Option<&str>,
    remote_config: &coast_core::types::RemoteConnection,
    local_project_root: &std::path::Path,
    has_sudo: bool,
) -> Result<String> {
    let remote_artifact_path = build_response.artifact_path.display().to_string();
    let build_id = build_response
        .artifact_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let local_images_dir = paths::project_images_dir(project);
    let local_artifact_dir = local_images_dir.join(&build_id);

    super::remote::sync::rsync_from_remote(
        &remote_artifact_path,
        &local_artifact_dir,
        remote_config,
        has_sudo,
    )
    .await?;

    let manifest_path = local_artifact_dir.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(mut manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                manifest["project_root"] =
                    serde_json::Value::String(local_project_root.display().to_string());
                let _ = std::fs::write(
                    &manifest_path,
                    serde_json::to_string_pretty(&manifest).unwrap_or_default(),
                );
            }
        }
    }

    let latest_name = match coastfile_type {
        Some(t) if t != "default" => format!("latest-{t}"),
        _ => "latest-remote".to_string(),
    };
    let symlink_path = local_images_dir.join(&latest_name);
    let _ = std::fs::remove_file(&symlink_path);
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(&build_id, &symlink_path);

    super::build::utils::auto_prune_builds(
        &local_images_dir,
        5,
        &std::collections::HashSet::new(),
        coastfile_type,
    );

    Ok(build_id)
}

/// Check that a compatible build exists for the remote's architecture.
/// If the latest build doesn't match, scans all builds to find one that does.
async fn check_arch_compatibility(
    project: &str,
    coastfile_type: Option<&str>,
    remote_config: &coast_core::types::RemoteConnection,
    remote_host: &str,
) -> Result<()> {
    let local_images_dir = paths::project_images_dir(project);
    let latest_name = match coastfile_type {
        Some(t) if t != "default" => format!("latest-{t}"),
        _ => "latest-remote".to_string(),
    };

    let Some(remote_arch) = query_remote_arch_simple(remote_config).await else {
        return Ok(());
    };

    let existing_arch = std::fs::read_link(local_images_dir.join(&latest_name))
        .ok()
        .map(|target| local_images_dir.join(target).join("manifest.json"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("arch").and_then(|a| a.as_str().map(String::from)));

    if existing_arch.as_deref() == Some(&remote_arch) {
        return Ok(());
    }

    let target_type = coastfile_type.unwrap_or("remote");
    if find_latest_build_for_arch(project, target_type, &remote_arch).is_some() {
        info!(
            remote_arch,
            "latest build is wrong arch, compatible build found"
        );
        return Ok(());
    }

    Err(coast_core::error::CoastError::state(format!(
        "No build found for architecture '{remote_arch}'. \
         Run `coast build --type remote --remote {remote_host}` to build for this architecture.",
    )))
}

/// Scan all builds for a project and return the newest build_id matching
/// the given coastfile type and architecture.
fn find_latest_build_for_arch(project: &str, target_type: &str, arch: &str) -> Option<String> {
    let project_dir = paths::project_images_dir(project);
    let entries = std::fs::read_dir(&project_dir).ok()?;

    let mut candidates: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("latest") {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        let Some(content) = std::fs::read_to_string(&manifest_path).ok() else {
            continue;
        };
        let Some(manifest) = serde_json::from_str::<serde_json::Value>(&content).ok() else {
            continue;
        };

        let build_arch = manifest
            .get("arch")
            .and_then(|a| a.as_str())
            .unwrap_or("unknown");
        let build_type = manifest
            .get("coastfile_type")
            .and_then(|t| t.as_str())
            .unwrap_or("default");
        let timestamp = manifest
            .get("build_timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if build_arch == arch && build_type == target_type {
            candidates.push((name, timestamp));
        }
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    candidates.into_iter().next().map(|(id, _)| id)
}

pub(crate) async fn query_remote_arch_simple(
    config: &coast_core::types::RemoteConnection,
) -> Option<String> {
    let ssh_key_str = config.ssh_key.display().to_string();
    let output = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/coast-ssh-%r@%h:%p",
            "-o",
            "ControlPersist=300",
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
        ])
        .arg(format!("{}@{}", config.user, config.host))
        .arg("uname -m")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Allocate local ports and set up SSH tunnels for each remote port.
async fn setup_port_tunnels(
    remote_config: &coast_core::types::RemoteConnection,
    remote_ports: &[coast_core::types::PortMapping],
) -> Vec<coast_core::types::PortMapping> {
    let mut local_port_mappings = Vec::new();

    if remote_ports.is_empty() {
        return local_port_mappings;
    }

    let port_tunnel_pairs: Vec<(u16, u16)> = remote_ports
        .iter()
        .map(|p| {
            let local_dynamic =
                crate::port_manager::allocate_dynamic_port().unwrap_or(p.canonical_port + 50000);
            local_port_mappings.push(coast_core::types::PortMapping {
                logical_name: p.logical_name.clone(),
                canonical_port: p.canonical_port,
                dynamic_port: local_dynamic,
                is_primary: p.is_primary,
            });
            (local_dynamic, p.dynamic_port)
        })
        .collect();

    let _tunnel_pids = super::remote::tunnel::forward_ports(remote_config, &port_tunnel_pairs)
        .await
        .unwrap_or_default();

    local_port_mappings
}

/// Start shared services locally and set up SSH reverse tunnels so the
/// remote DinD container can reach them.
async fn setup_shared_service_tunnels(
    cf: &coast_core::coastfile::Coastfile,
    req: &RunRequest,
    state: &AppState,
    remote_config: &coast_core::types::RemoteConnection,
) -> Result<Vec<coast_core::protocol::SharedServicePortForward>> {
    if cf.shared_services.is_empty() {
        return Ok(Vec::new());
    }

    let Some(docker) = state.docker.as_ref() else {
        return Ok(Vec::new());
    };

    let _result = shared_services_setup::start_shared_services(
        &req.project,
        &cf.shared_services,
        &docker,
        state,
    )
    .await?;

    let forwards: Vec<coast_core::protocol::SharedServicePortForward> = cf
        .shared_services
        .iter()
        .flat_map(|svc| {
            svc.ports
                .iter()
                .map(|p| coast_core::protocol::SharedServicePortForward {
                    name: svc.name.clone(),
                    port: p.container_port,
                })
        })
        .collect();

    if !forwards.is_empty() {
        let reverse_pairs: Vec<(u16, u16)> =
            forwards.iter().map(|fwd| (fwd.port, fwd.port)).collect();
        match super::remote::tunnel::reverse_forward_ports(remote_config, &reverse_pairs).await {
            Ok(pids) => {
                info!(
                    host = %remote_config.host,
                    tunnels = pids.len(),
                    "shared service reverse tunnels created"
                );
            }
            Err(_) => {
                info!(
                    host = %remote_config.host,
                    "shared service tunnels already bound (reusing existing)"
                );
            }
        }
    }

    Ok(forwards)
}

/// Build, download artifact, and forward the run request to coast-service.
async fn remote_build_and_provision(
    req: &RunRequest,
    client: &super::remote::RemoteClient,
    remote_config: &coast_core::types::RemoteConnection,
    remote_host: &str,
    code_path: &std::path::Path,
    service_home: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    total_steps: u32,
) -> Result<(coast_core::protocol::RunResponse, String)> {
    emit(
        progress,
        BuildProgressEvent::started("Building on remote", 8, total_steps),
    );

    check_arch_compatibility(
        &req.project,
        req.coastfile_type.as_deref(),
        remote_config,
        remote_host,
    )
    .await?;

    let build_response = trigger_remote_build(
        client,
        remote_config,
        &req.project,
        &req.name,
        req.coastfile_type.as_deref(),
        service_home,
    )
    .await?;

    emit(
        progress,
        BuildProgressEvent::done(
            "Building on remote",
            &format!("project: {}", build_response.project),
        ),
    );

    emit(
        progress,
        BuildProgressEvent::started("Downloading build artifact", 9, total_steps),
    );

    let build_id = download_remote_artifact(
        &build_response,
        &req.project,
        req.coastfile_type.as_deref(),
        remote_config,
        code_path,
        client.has_sudo,
    )
    .await?;

    emit(
        progress,
        BuildProgressEvent::done("Downloading build artifact", &format!("build {build_id}")),
    );

    emit(
        progress,
        BuildProgressEvent::started("Creating remote instance", 10, total_steps),
    );

    let remote_response = match super::remote::forward::forward_run(client, req).await {
        Ok(resp) => resp,
        Err(e)
            if e.to_string().contains("already exists")
                || e.to_string().contains("already in use")
                || e.to_string().contains("409") =>
        {
            tracing::warn!("stale instance on coast-service, removing before retry");
            let rm_req = coast_core::protocol::RmRequest {
                project: req.project.clone(),
                name: req.name.clone(),
            };
            let _ = super::remote::forward::forward_rm(client, &rm_req).await;
            super::remote::forward::forward_run(client, req).await?
        }
        Err(e) => return Err(e),
    };

    emit(
        progress,
        BuildProgressEvent::done("Creating remote instance", "ok"),
    );

    Ok((remote_response, build_id))
}

/// Handle a remote coast run request.
///
/// Instead of provisioning a DinD container locally, this:
/// 1. Creates a shell coast container (bind mounts, no inner docker)
/// 2. Starts shared services locally and sets up reverse SSH tunnels
/// 3. Connects to the remote coast-service via SSH tunnel
/// 4. Syncs /workspace to the remote
/// 5. Forwards the RunRequest to coast-service for DinD provisioning
/// 6. Sets up port forwarding tunnels for each exposed port
/// 7. Stores the instance as a shadow with remote_host set
async fn handle_remote_run(
    mut req: RunRequest,
    state: &AppState,
    validated: &validate::ValidatedRun,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<RunResponse> {
    let total_steps = 13u32;
    info!(name = %req.name, project = %req.project, "remote run: loading coastfile for remote config");

    // --- Step 2: Load remote config ---
    emit(
        progress,
        BuildProgressEvent::started("Loading remote config", 2, total_steps),
    );
    let (cf, cf_remote, coastfile_path) = resolve_remote_config(&req, validated)?;
    let (remote_config, remote_host) =
        resolve_remote_entry(&cf_remote, state, req.remote.as_deref()).await?;
    emit(
        progress,
        BuildProgressEvent::done("Loading remote config", &format!("host: {remote_host}")),
    );

    // --- Step 3: Create shell coast ---
    emit(
        progress,
        BuildProgressEvent::started("Creating shell coast", 3, total_steps),
    );
    let code_path_buf = resolve_code_path(&coastfile_path, &cf);
    create_shell_coast(&req, state, &code_path_buf).await?;
    emit(
        progress,
        BuildProgressEvent::done("Creating shell coast", "ok"),
    );

    // --- Step 4: Start shared services + Step 6: Reverse tunnels ---
    emit(
        progress,
        BuildProgressEvent::started("Starting shared services", 4, total_steps),
    );
    let shared_service_forwards =
        setup_shared_service_tunnels(&cf, &req, state, &remote_config).await?;
    req.shared_service_ports = shared_service_forwards;
    emit(
        progress,
        BuildProgressEvent::done(
            "Starting shared services",
            &format!("{} service(s)", req.shared_service_ports.len()),
        ),
    );

    // --- Step 5: Connect to coast-service ---
    emit(
        progress,
        BuildProgressEvent::started("Connecting to coast-service", 5, total_steps),
    );
    let client = super::remote::RemoteClient::connect(&remote_config).await?;
    emit(
        progress,
        BuildProgressEvent::done("Connecting to coast-service", "tunnel established"),
    );

    // --- Step 6: Reverse tunnel status ---
    emit(
        progress,
        BuildProgressEvent::started("Setting up shared service tunnels", 6, total_steps),
    );
    emit(
        progress,
        BuildProgressEvent::done(
            "Setting up shared service tunnels",
            &format!("{} tunnel(s)", req.shared_service_ports.len()),
        ),
    );

    // --- Step 7: Sync project source ---
    emit(
        progress,
        BuildProgressEvent::started("Syncing project source", 7, total_steps),
    );
    let service_home = client.query_service_home().await;
    let remote_workspace =
        super::remote::remote_workspace_path(&service_home, &req.project, &req.name);
    client
        .sync_workspace(&code_path_buf, &remote_workspace)
        .await?;
    emit(
        progress,
        BuildProgressEvent::done("Syncing project source", "ok"),
    );

    // --- Steps 8-10: Build, download, provision ---
    let (remote_response, remote_build_id) = remote_build_and_provision(
        &req,
        &client,
        &remote_config,
        &remote_host,
        &code_path_buf,
        &service_home,
        progress,
        total_steps,
    )
    .await?;

    // --- Step 11: Port forwarding ---
    emit(
        progress,
        BuildProgressEvent::started("Setting up port forwarding", 11, total_steps),
    );
    let local_port_mappings = setup_port_tunnels(&remote_config, &remote_response.ports).await;
    emit(
        progress,
        BuildProgressEvent::done(
            "Setting up port forwarding",
            &format!("{} port(s)", local_port_mappings.len()),
        ),
    );

    // --- Steps 12-13: File sync + finalize ---
    finalize_remote_run(
        &req,
        state,
        progress,
        &code_path_buf,
        &remote_workspace,
        &remote_config,
        &remote_host,
        &local_port_mappings,
        &remote_response.ports,
        &remote_build_id,
    )
    .await?;

    if let Some(ref worktree_name) = req.worktree {
        assign_worktree(&req, worktree_name, state, progress, total_steps).await;
    }

    Ok(RunResponse {
        name: req.name,
        container_id: remote_response.container_id,
        ports: local_port_mappings,
    })
}

/// Start mutagen sync inside the local shell container.
///
/// The shell container has mutagen installed and `/workspace` bind-mounted
/// from the host. This function:
/// 1. Copies the SSH key into the container
/// 2. Writes SSH config so mutagen can connect non-interactively
/// 3. Starts the mutagen daemon inside the container
/// 4. Creates a one-way sync session from /workspace to the remote
///
/// The remote host is translated to `host.docker.internal` since
/// `localhost` inside the container refers to the container itself,
/// not the host machine where the SSH tunnel listens.
fn build_mutagen_setup_script(
    session_name: &str,
    remote_host: &str,
    port: u16,
    key_path: &str,
    remote_url: &str,
) -> String {
    format!(
        r#"set -e
mkdir -p /root/.ssh
chmod 600 {key_path} 2>/dev/null

cat > /root/.ssh/config << 'EOF'
Host {remote_host}
  StrictHostKeyChecking accept-new
  BatchMode yes
  IdentityFile {key_path}
  Port {port}
EOF

eval $(ssh-agent -s)
ssh-add {key_path} 2>/dev/null

mutagen daemon start 2>/dev/null || true
mutagen sync terminate {session} 2>/dev/null || true
mutagen sync create \
  --name {session} \
  --sync-mode one-way-safe \
  --ignore-vcs \
  --ignore node_modules \
  --ignore target \
  --ignore __pycache__ \
  --ignore .next \
  /workspace \
  {remote_url}
"#,
        remote_host = remote_host,
        port = port,
        key_path = key_path,
        session = session_name,
        remote_url = remote_url,
    )
}

pub(crate) async fn start_mutagen_in_shell<'a>(
    docker: &bollard::Docker,
    shell_container: &str,
    project: &str,
    instance_name: &str,
    remote_workspace: &str,
    remote_config: &coast_core::types::RemoteConnection,
) -> &'a str {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;

    let session_name = super::remote::sync::mutagen_session_name(project, instance_name);

    let remote_host = if remote_config.host == "localhost" || remote_config.host == "127.0.0.1" {
        "host.docker.internal".to_string()
    } else {
        remote_config.host.clone()
    };
    let remote_url = format!(
        "{}@{}:{}",
        remote_config.user, remote_host, remote_workspace
    );
    let ssh_key_host_path = remote_config.ssh_key.display().to_string();
    let port = remote_config.port;
    let key_path = "/root/.ssh/coast_remote_key";

    let copy_key_result =
        docker_cp_file(docker, shell_container, &ssh_key_host_path, key_path).await;
    if let Err(e) = copy_key_result {
        warn!(error = %e, "failed to copy SSH key into shell container");
        return "key copy failed (rsync only)";
    }

    let setup_script =
        build_mutagen_setup_script(&session_name, &remote_host, port, key_path, &remote_url);

    match rt
        .exec_in_coast(shell_container, &["sh", "-c", &setup_script])
        .await
    {
        Ok(result) => {
            interpret_mutagen_exec_result(&result, &session_name, shell_container, &remote_url)
        }
        Err(e) => {
            warn!(
                error = %e,
                container = %shell_container,
                "failed to exec mutagen in shell container"
            );
            "exec failed (rsync only)"
        }
    }
}

fn interpret_mutagen_exec_result(
    result: &coast_docker::runtime::ExecResult,
    session_name: &str,
    shell_container: &str,
    remote_url: &str,
) -> &'static str {
    if result.success() {
        info!(
            session = %session_name,
            container = %shell_container,
            remote = %remote_url,
            "mutagen sync started inside shell container"
        );
        "mutagen continuous sync active"
    } else {
        warn!(
            session = %session_name,
            stderr = %result.stderr.trim(),
            stdout = %result.stdout.trim(),
            "mutagen sync failed inside shell container"
        );
        "mutagen failed (rsync only)"
    }
}

/// Copy a file from the host into a running container using docker cp.
async fn docker_cp_file(
    docker: &bollard::Docker,
    container: &str,
    host_path: &str,
    container_path: &str,
) -> Result<()> {
    let _ = docker; // we use the CLI for cp since bollard's cp API is cumbersome
    let container_dest = format!("{container}:{container_path}");

    let mkdir_output = tokio::process::Command::new("docker")
        .args(["exec", container, "mkdir", "-p"])
        .arg(
            std::path::Path::new(container_path)
                .parent()
                .map(|p| p.to_str().unwrap_or("/root/.ssh"))
                .unwrap_or("/root/.ssh"),
        )
        .output()
        .await
        .map_err(|e| {
            coast_core::error::CoastError::state(format!("docker exec mkdir failed: {e}"))
        })?;

    if !mkdir_output.status.success() {
        return Err(coast_core::error::CoastError::state(
            "failed to create directory in container for SSH key",
        ));
    }

    let output = tokio::process::Command::new("docker")
        .args(["cp", host_path, &container_dest])
        .output()
        .await
        .map_err(|e| coast_core::error::CoastError::state(format!("docker cp failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(coast_core::error::CoastError::state(format!(
            "docker cp failed: {stderr}"
        )));
    }

    Ok(())
}

fn resolve_code_path(
    coastfile_path: &std::path::Path,
    cf: &coast_core::coastfile::Coastfile,
) -> std::path::PathBuf {
    let manifest_path = coastfile_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("manifest.json");
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("project_root")?
                .as_str()
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| cf.project_root.clone())
}

async fn finalize_remote_run(
    req: &RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    _code_path: &std::path::Path,
    remote_workspace: &str,
    remote_config: &coast_core::types::RemoteConnection,
    remote_host: &str,
    local_port_mappings: &[coast_core::types::PortMapping],
    remote_port_mappings: &[coast_core::types::PortMapping],
    remote_build_id: &str,
) -> Result<()> {
    emit(
        progress,
        BuildProgressEvent::started("Starting file sync", 12, 13),
    );

    let sync_msg = if let Some(docker) = state.docker.as_ref() {
        let shell_container = format!("{}-coasts-{}-shell", req.project, req.name);
        start_mutagen_in_shell(
            &docker,
            &shell_container,
            &req.project,
            &req.name,
            remote_workspace,
            remote_config,
        )
        .await
    } else {
        "no docker (skipped)"
    };
    emit(
        progress,
        BuildProgressEvent::done("Starting file sync", sync_msg),
    );

    emit(progress, BuildProgressEvent::started("Finalizing", 13, 13));

    {
        let db = state.db.lock().await;
        db.update_instance_remote_host(&req.project, &req.name, Some(remote_host))?;
        db.set_build_id(&req.project, &req.name, Some(remote_build_id))?;
        db.update_instance_status(
            &req.project,
            &req.name,
            &coast_core::types::InstanceStatus::Running,
        )?;
        for pm in local_port_mappings {
            let remote_dyn = remote_port_mappings
                .iter()
                .find(|r| r.logical_name == pm.logical_name)
                .map(|r| r.dynamic_port);
            let _ = db.insert_port_allocation_with_remote(&req.project, &req.name, pm, remote_dyn);
        }

        if let Some(primary) = load_primary_port(&req.project, req.coastfile_type.as_deref()) {
            let key = format!("primary_port:{}:{}", req.project, remote_build_id);
            let _ = db.set_setting(&key, &primary);
        }
    }

    emit(progress, BuildProgressEvent::done("Finalizing", "ok"));

    info!(
        name = %req.name,
        host = %remote_host,
        ports = local_port_mappings.len(),
        "remote coast provisioned"
    );

    Ok(())
}

async fn assign_worktree(
    req: &RunRequest,
    worktree_name: &str,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    total_steps: u32,
) {
    info!(name = %req.name, worktree = %worktree_name, "auto-assigning worktree after provisioning");
    emit(
        progress,
        BuildProgressEvent::started("Assigning worktree", total_steps, total_steps),
    );

    let assign_req = coast_core::protocol::AssignRequest {
        name: req.name.clone(),
        project: req.project.clone(),
        worktree: worktree_name.to_string(),
        commit_sha: None,
        explain: false,
        force_sync: false,
        service_actions: Default::default(),
    };

    match super::assign::handle(assign_req, state, progress.clone()).await {
        Ok(resp) => {
            emit(
                progress,
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
                progress,
                BuildProgressEvent::item("Assigning worktree", format!("Warning: {e}"), "warn"),
            );
            emit(
                progress,
                BuildProgressEvent::done("Assigning worktree", "warn"),
            );
            warn!(
                name = %req.name, worktree = %worktree_name, error = %e,
                "post-provisioning worktree assignment failed (coast is still running)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::error::CoastError;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn test_state_with_docker() -> AppState {
        AppState::new_for_testing_with_docker(StateDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_run_without_docker_fails_before_inserting_instance() {
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
            remote: None,
            shared_service_ports: Vec::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Host Docker is not available"));

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feature-oauth").unwrap();
        assert!(instance.is_none());
    }

    #[tokio::test]
    async fn test_run_with_docker_stub_rejects_duplicate_instance() {
        let state = test_state_with_docker();
        {
            let db = state.db.lock().await;
            db.insert_instance(&CoastInstance {
                name: "dup".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::Running,
                branch: None,
                commit_sha: None,
                container_id: Some("existing-container".to_string()),
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_host: None,
            })
            .unwrap();
        }

        let req = RunRequest {
            name: "dup".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "dup").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Running);
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
    async fn test_run_with_force_remove_dangling_still_fails_without_docker() {
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
            remote: None,
            shared_service_ports: Vec::new(),
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Host Docker is not available"));
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
