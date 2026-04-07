use std::path::{Path, PathBuf};

use tracing::{info, warn};

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{RunRequest, RunResponse};
use coast_core::types::PortMapping;
use coast_docker::dind::{DindConfigParams, DindRuntime};
use coast_docker::runtime::{ContainerConfig, PortPublish, Runtime};

use crate::state::ServiceState;

pub(crate) fn resolve_artifact_dir(project: &str, coastfile_type: Option<&str>) -> Option<PathBuf> {
    let home = crate::state::service_home();
    let project_dir = home.join("images").join(project);
    let latest_name = match coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = project_dir.join(latest_name);
    std::fs::read_link(&latest_link)
        .ok()
        .map(|target| project_dir.join(target))
}

fn workspace_path(project: &str, instance: &str) -> PathBuf {
    let home = crate::state::service_home();
    home.join("workspaces").join(project).join(instance)
}

/// Make a directory and all its ancestors (up to `service_home`) world-writable.
///
/// Coast-service runs as root inside its container, but the daemon rsyncs
/// workspace files as the host SSH user. Without `0o777` the SSH user can't
/// write into root-owned directories created via the bind mount.
fn make_world_writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let home = crate::state::service_home();
        let mut cur = path.to_path_buf();
        while cur.starts_with(&home) && cur != home {
            let _ = std::fs::set_permissions(&cur, std::fs::Permissions::from_mode(0o777));
            if !cur.pop() {
                break;
            }
        }
    }
}

fn read_coast_image(artifact_dir: &Path) -> Option<String> {
    let manifest_path = artifact_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("coast_image")?.as_str().map(String::from))
}

fn image_cache_path() -> PathBuf {
    let home = crate::state::service_home();
    home.join("image-cache")
}

fn load_secrets_for_project(project: &str) -> Vec<coast_secrets::keystore::StoredSecret> {
    let home = crate::state::service_home();
    let keystore_db = home.join("keystore.db");
    let keystore_key = home.join("keystore.key");
    if !keystore_db.exists() {
        return Vec::new();
    }
    match coast_secrets::keystore::Keystore::open(&keystore_db, &keystore_key) {
        Ok(ks) => ks.get_all_secrets(project).unwrap_or_default(),
        Err(e) => {
            warn!(error = %e, "failed to open keystore for secret injection");
            Vec::new()
        }
    }
}

fn read_ports_from_coastfile(artifact_dir: &Path) -> Vec<(String, u16)> {
    let cf_path = artifact_dir.join("coastfile.toml");
    let content = match std::fs::read_to_string(&cf_path) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to read coastfile.toml from artifact");
            return Vec::new();
        }
    };
    match Coastfile::parse(&content, artifact_dir) {
        Ok(cf) => {
            let shared_ports: std::collections::HashSet<u16> = cf
                .shared_services
                .iter()
                .flat_map(|svc| svc.ports.iter().map(|p| p.container_port))
                .collect();

            let mut ports: Vec<(String, u16)> = cf
                .ports
                .into_iter()
                .filter(|(_, port)| !shared_ports.contains(port))
                .collect();
            ports.sort_by_key(|(_, p)| *p);
            ports
        }
        Err(e) => {
            warn!(error = %e, "failed to parse coastfile.toml from artifact");
            Vec::new()
        }
    }
}

pub(crate) async fn wait_for_inner_daemon(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<()> {
    let rt = DindRuntime::with_client(docker.clone());
    for _attempt in 0..60 {
        let result = rt.exec_in_coast(container_id, &["docker", "info"]).await;
        if let Ok(r) = result {
            if r.success() {
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(CoastError::state(
        "inner Docker daemon failed to start within 60s",
    ))
}

async fn load_cached_images(docker: &bollard::Docker, container_id: &str) -> Result<usize> {
    let rt = DindRuntime::with_client(docker.clone());
    let result = rt
        .exec_in_coast(
            container_id,
            &[
                "sh",
                "-c",
                r#"for f in /image-cache/*.tar; do [ -f "$f" ] && docker load -i "$f"; done"#,
            ],
        )
        .await?;
    Ok(result
        .stdout
        .lines()
        .filter(|l| l.contains("Loaded image"))
        .count())
}

/// Build the shell command to write a secret value to a file path.
fn build_file_secret_cmd(target: &str, value: &str) -> String {
    format!(
        "mkdir -p \"$(dirname '{target}')\" && cat > '{target}' << 'COAST_SECRET_EOF'\n{value}\nCOAST_SECRET_EOF"
    )
}

/// Log the outcome of injecting a file secret.
fn log_file_secret_result(
    secret_name: &str,
    target: &str,
    result: &std::result::Result<coast_docker::runtime::ExecResult, coast_core::error::CoastError>,
) {
    match result {
        Ok(r) if r.success() => {
            info!(secret = %secret_name, target, "file secret injected");
        }
        Ok(r) => {
            warn!(secret = %secret_name, target, stderr = %r.stderr, "failed to inject file secret");
        }
        Err(e) => {
            warn!(secret = %secret_name, target, error = %e, "failed to inject file secret");
        }
    }
}

/// Write a single file-type secret into the container. Returns true on success.
async fn inject_single_file_secret(
    rt: &DindRuntime,
    container_id: &str,
    secret: &coast_secrets::keystore::StoredSecret,
) -> bool {
    let Ok(value) = String::from_utf8(secret.value.clone()) else {
        warn!(secret = %secret.secret_name, "skipping file secret with non-UTF-8 value");
        return false;
    };
    let cmd = build_file_secret_cmd(&secret.inject_target, &value);
    let result = rt.exec_in_coast(container_id, &["sh", "-c", &cmd]).await;
    let success = result
        .as_ref()
        .is_ok_and(coast_docker::runtime::ExecResult::success);
    log_file_secret_result(&secret.secret_name, &secret.inject_target, &result);
    success
}

async fn inject_file_secrets(
    docker: &bollard::Docker,
    container_id: &str,
    secrets: &[coast_secrets::keystore::StoredSecret],
) -> usize {
    let rt = DindRuntime::with_client(docker.clone());
    let mut injected = 0;
    for secret in secrets.iter().filter(|s| s.inject_type == "file") {
        if inject_single_file_secret(&rt, container_id, secret).await {
            injected += 1;
        }
    }
    injected
}

async fn start_compose_services(
    docker: &bollard::Docker,
    container_id: &str,
    has_shared_service_override: bool,
    project_dir: &str,
) -> Result<()> {
    let rt = DindRuntime::with_client(docker.clone());

    let compose_file = if has_shared_service_override {
        "/coast-artifact/compose.coast-shared.yml"
    } else {
        "/coast-artifact/compose.yml"
    };

    let compose_cmd = format!(
        "cd /workspace && docker compose -f {compose_file} --project-directory {project_dir} up -d --remove-orphans"
    );

    let result = rt
        .exec_in_coast(container_id, &["sh", "-c", &compose_cmd])
        .await?;
    if !result.success() {
        return Err(CoastError::state(format!(
            "compose up failed: {}",
            result.stderr
        )));
    }
    Ok(())
}

/// Read the compose path from the artifact's coastfile.toml and return
/// the project directory for `--project-directory`. This is the parent
/// directory of the compose file, relative to /workspace.
///
/// For example, `compose = "./infra/docker-compose.yml"` returns
/// `/workspace/infra`. Falls back to `/workspace` if no compose path
/// is found or the compose is at the project root.
pub(crate) fn read_compose_project_dir(artifact_dir: &Path) -> String {
    let coastfile_path = artifact_dir.join("coastfile.toml");
    let Ok(content) = std::fs::read_to_string(&coastfile_path) else {
        return "/workspace".to_string();
    };
    let Ok(toml_val) = content.parse::<toml::Value>() else {
        return "/workspace".to_string();
    };
    let compose_str = toml_val
        .get("coast")
        .and_then(|c| c.get("compose"))
        .and_then(|v| v.as_str());
    match compose_str {
        Some(path) => {
            let p = std::path::Path::new(path);
            match p.parent().and_then(|d| d.to_str()) {
                Some(dir) if !dir.is_empty() && dir != "." => {
                    let clean = dir.trim_start_matches("./");
                    format!("/workspace/{clean}")
                }
                _ => "/workspace".to_string(),
            }
        }
        None => "/workspace".to_string(),
    }
}

/// Generate a merged compose file that removes shared services, strips
/// `depends_on` references to them, and adds `extra_hosts` entries so
/// remaining services can reach the shared services via reverse SSH tunnel.
///
/// Written to the artifact directory **before** the DinD container is
/// created, so it's available via the read-only bind mount.
fn write_shared_service_compose_override(
    artifact_dir: &Path,
    compose_path: &Path,
    shared_service_ports: &[coast_core::protocol::SharedServicePortForward],
) -> Result<bool> {
    if shared_service_ports.is_empty() {
        return Ok(false);
    }

    let Ok(content) = std::fs::read_to_string(compose_path) else {
        return Ok(false);
    };
    let mut yaml: serde_yaml::Value = serde_yaml::from_str(&content)
        .map_err(|e| CoastError::state(format!("failed to parse compose YAML: {e}")))?;

    let shared_names: std::collections::HashSet<String> = shared_service_ports
        .iter()
        .map(|fwd| fwd.name.clone())
        .collect();

    remove_shared_services(&mut yaml, &shared_names);
    strip_shared_depends_on(&mut yaml, &shared_names);
    add_shared_extra_hosts(&mut yaml, &shared_names);
    make_env_files_optional(&mut yaml);
    remove_orphaned_top_level_volumes(&mut yaml, &shared_names, &content);

    let merged = serde_yaml::to_string(&yaml)
        .map_err(|e| CoastError::state(format!("failed to serialize merged compose: {e}")))?;

    let merged_path = artifact_dir.join("compose.coast-shared.yml");
    std::fs::write(&merged_path, &merged)
        .map_err(|e| CoastError::state(format!("failed to write merged compose: {e}")))?;

    info!(
        shared_services_removed = shared_names.len(),
        path = %merged_path.display(),
        "wrote merged compose with shared service routing"
    );

    Ok(true)
}

fn remove_shared_services(
    yaml: &mut serde_yaml::Value,
    shared_names: &std::collections::HashSet<String>,
) {
    let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) else {
        return;
    };
    for name in shared_names {
        services.remove(serde_yaml::Value::String(name.clone()));
    }
}

fn strip_shared_depends_on(
    yaml: &mut serde_yaml::Value,
    shared_names: &std::collections::HashSet<String>,
) {
    let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) else {
        return;
    };

    let keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
    for key in keys {
        let Some(svc) = services.get_mut(&key).and_then(|s| s.as_mapping_mut()) else {
            continue;
        };
        let depends_key = serde_yaml::Value::String("depends_on".into());
        let mut remove_depends = false;

        if let Some(deps) = svc.get_mut(&depends_key) {
            if let Some(seq) = deps.as_sequence_mut() {
                seq.retain(|v| {
                    v.as_str()
                        .map(|s| !shared_names.contains(s))
                        .unwrap_or(true)
                });
                remove_depends = seq.is_empty();
            } else if let Some(map) = deps.as_mapping_mut() {
                for name in shared_names {
                    map.remove(serde_yaml::Value::String(name.clone()));
                }
                remove_depends = map.is_empty();
            }
        }

        if remove_depends {
            svc.remove(&depends_key);
        }
    }
}

fn add_shared_extra_hosts(
    yaml: &mut serde_yaml::Value,
    shared_names: &std::collections::HashSet<String>,
) {
    let gateway_ip = resolve_docker_gateway_ip();

    let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) else {
        return;
    };

    let keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
    for key in keys {
        let Some(svc) = services.get_mut(&key).and_then(|s| s.as_mapping_mut()) else {
            continue;
        };
        let hosts_key = serde_yaml::Value::String("extra_hosts".into());
        let hosts_seq = svc
            .entry(hosts_key)
            .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
        let Some(seq) = hosts_seq.as_sequence_mut() else {
            continue;
        };

        let docker_internal = format!("host.docker.internal:{gateway_ip}");
        if !seq.iter().any(|v| {
            v.as_str()
                .is_some_and(|s| s.starts_with("host.docker.internal:"))
        }) {
            seq.push(serde_yaml::Value::String(docker_internal));
        }
        for name in shared_names {
            let entry = format!("{name}:{gateway_ip}");
            if !seq.iter().any(|v| {
                v.as_str()
                    .is_some_and(|s| s.starts_with(&format!("{name}:")))
            }) {
                seq.push(serde_yaml::Value::String(entry));
            }
        }
    }
}

/// Resolve the Docker bridge gateway IP from the daemon config.
///
/// Reads `/etc/docker/daemon.json` to find the `bip` setting (e.g.,
/// `"10.200.0.1/16"`), extracts the IP part. Falls back to `host-gateway`
/// if the config can't be read (e.g., on Docker Desktop where it's not needed).
/// Resolve the IP that inner compose services should use to reach the host
/// where shared service tunnels listen.
///
/// In the dev image (coast-service runs its own dockerd inside the container),
/// `daemon.json` has a custom `bip` -- inner services use that gateway IP
/// because the tunnel also terminates inside the same container.
///
/// In production (host Docker socket), DinD containers run on the host Docker.
/// Inner services need to reach the host, so we resolve the Docker bridge
/// gateway IP (`docker0` interface) which is the host IP from the DinD
/// container's perspective.
pub(crate) fn resolve_docker_gateway_ip() -> String {
    let config_path = std::path::Path::new("/etc/docker/daemon.json");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(bip) = json.get("bip").and_then(|v| v.as_str()) {
                if let Some(ip) = bip.split('/').next() {
                    info!(gateway_ip = %ip, "resolved Docker bridge gateway from daemon.json");
                    return ip.to_string();
                }
            }
        }
    }
    // In production (host Docker socket), query the bridge network gateway.
    // This is the host IP from the perspective of Docker containers.
    if let Ok(output) = std::process::Command::new("docker")
        .args([
            "network",
            "inspect",
            "bridge",
            "--format",
            "{{range .IPAM.Config}}{{.Gateway}}{{end}}",
        ])
        .output()
    {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() && ip.contains('.') {
            info!(gateway_ip = %ip, "resolved Docker bridge gateway from network inspect");
            return ip;
        }
    }
    "host-gateway".to_string()
}

/// Convert all `env_file` entries in compose services to the long form
/// with `required: false`. Remote workspaces often don't have `.env` files
/// that the project's compose references, and this prevents hard failures.
fn make_env_files_optional(yaml: &mut serde_yaml::Value) {
    let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) else {
        return;
    };

    let keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
    for key in keys {
        let Some(svc) = services.get_mut(&key).and_then(|s| s.as_mapping_mut()) else {
            continue;
        };
        let env_file_key = serde_yaml::Value::String("env_file".into());
        let Some(env_file) = svc.get_mut(&env_file_key) else {
            continue;
        };

        let new_entries: Vec<serde_yaml::Value> = match env_file {
            serde_yaml::Value::String(path) => {
                vec![env_file_entry_optional(path)]
            }
            serde_yaml::Value::Sequence(seq) => seq
                .iter()
                .map(|entry| match entry {
                    serde_yaml::Value::String(path) => env_file_entry_optional(path),
                    serde_yaml::Value::Mapping(m) => {
                        let mut m = m.clone();
                        m.insert(
                            serde_yaml::Value::String("required".into()),
                            serde_yaml::Value::Bool(false),
                        );
                        serde_yaml::Value::Mapping(m)
                    }
                    other => other.clone(),
                })
                .collect(),
            _ => continue,
        };

        *env_file = serde_yaml::Value::Sequence(new_entries);
    }
}

fn env_file_entry_optional(path: &str) -> serde_yaml::Value {
    let mut m = serde_yaml::Mapping::new();
    m.insert(
        serde_yaml::Value::String("path".into()),
        serde_yaml::Value::String(path.to_string()),
    );
    m.insert(
        serde_yaml::Value::String("required".into()),
        serde_yaml::Value::Bool(false),
    );
    serde_yaml::Value::Mapping(m)
}

fn remove_orphaned_top_level_volumes(
    yaml: &mut serde_yaml::Value,
    shared_names: &std::collections::HashSet<String>,
    original_content: &str,
) {
    let original: serde_yaml::Value = match serde_yaml::from_str(original_content) {
        Ok(v) => v,
        Err(_) => return,
    };

    let volume_names_from_shared: Vec<String> = shared_names
        .iter()
        .flat_map(|svc_name| {
            original
                .get("services")
                .and_then(|s| s.get(svc_name.as_str()))
                .and_then(|s| s.get("volumes"))
                .and_then(|v| v.as_sequence())
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    let s = v.as_str()?;
                    let source = s.split(':').next()?;
                    (!source.starts_with('.') && !source.starts_with('/') && !source.is_empty())
                        .then(|| source.to_string())
                })
        })
        .collect();

    if let Some(top_volumes) = yaml.get_mut("volumes").and_then(|v| v.as_mapping_mut()) {
        for vol in &volume_names_from_shared {
            top_volumes.remove(serde_yaml::Value::String(vol.clone()));
        }
    }
}

/// Build the DinD container configuration from run parameters.
fn build_container_config<'a>(
    req: &RunRequest,
    ws_path: &'a Path,
    coast_image: Option<&'a str>,
    artifact_dir: Option<&'a Path>,
    cache_path: &'a Path,
    env_secrets: &std::collections::HashMap<String, String>,
    ports: &[(String, u16)],
    dynamic_ports: &[u16],
) -> ContainerConfig {
    let mut dind_params = DindConfigParams::new(&req.project, &req.name, ws_path);
    dind_params.mount_workspace_directly = true;
    dind_params.coast_image = coast_image;
    dind_params.artifact_dir = artifact_dir;
    if cache_path.exists() {
        dind_params.image_cache_path = Some(cache_path);
    }
    for (key, val) in env_secrets {
        dind_params.env_vars.insert(key.clone(), val.clone());
    }

    if !req.shared_service_ports.is_empty() {
        dind_params
            .extra_hosts
            .push("host.docker.internal:host-gateway".to_string());
    }

    let mut config = coast_docker::dind::build_dind_config(dind_params);
    for ((_name, canonical), dynamic) in ports.iter().zip(dynamic_ports.iter()) {
        config.published_ports.push(PortPublish {
            host_port: *dynamic,
            container_port: *canonical,
        });
    }
    config
}

/// After the container is running, inject secrets and start compose if applicable.
async fn initialize_container_services(
    docker: &bollard::Docker,
    container_id: &str,
    secrets: &[coast_secrets::keystore::StoredSecret],
    has_artifact: bool,
    has_shared_service_override: bool,
    project_dir: &str,
) {
    let file_secrets_injected = inject_file_secrets(docker, container_id, secrets).await;
    if file_secrets_injected > 0 {
        info!(
            count = file_secrets_injected,
            "file secrets injected into container"
        );
    }

    if has_artifact {
        if let Err(e) = start_compose_services(
            docker,
            container_id,
            has_shared_service_override,
            project_dir,
        )
        .await
        {
            warn!(error = %e, "compose services failed to start");
        }
    }
}

/// Create and start the DinD container, load images, inject secrets, and start compose.
async fn provision_dind_container(
    docker: &bollard::Docker,
    req: &RunRequest,
    ws_path: &Path,
    coast_image: Option<&str>,
    artifact_dir: Option<&Path>,
    cache_path: &Path,
    env_secrets: &std::collections::HashMap<String, String>,
    ports: &[(String, u16)],
    dynamic_ports: &[u16],
    secrets: &[coast_secrets::keystore::StoredSecret],
) -> Result<String> {
    let config = build_container_config(
        req,
        ws_path,
        coast_image,
        artifact_dir,
        cache_path,
        env_secrets,
        ports,
        dynamic_ports,
    );

    let has_shared_service_override = if let Some(adir) = artifact_dir {
        let compose_path = adir.join("compose.yml");
        write_shared_service_compose_override(adir, &compose_path, &req.shared_service_ports)
            .unwrap_or(false)
    } else {
        false
    };

    let rt = DindRuntime::with_client(docker.clone());
    let container_name = config.container_name();
    if let Ok(info) = docker.inspect_container(&container_name, None).await {
        warn!(
            container_name = %container_name,
            container_id = ?info.id,
            "stale container found, force-removing before create"
        );
        let opts = bollard::container::RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        let _ = docker.remove_container(&container_name, Some(opts)).await;
    }
    let cid = rt.create_coast_container(&config).await?;
    rt.start_coast_container(&cid).await?;

    wait_for_inner_daemon(docker, &cid).await?;

    let loaded = load_cached_images(docker, &cid).await.unwrap_or(0);
    info!(
        loaded_images = loaded,
        "cached images loaded into inner daemon"
    );

    let project_dir = artifact_dir
        .map(read_compose_project_dir)
        .unwrap_or_else(|| "/workspace".to_string());

    initialize_container_services(
        docker,
        &cid,
        secrets,
        artifact_dir.is_some(),
        has_shared_service_override,
        &project_dir,
    )
    .await;

    Ok(cid)
}

pub async fn handle(req: RunRequest, state: &ServiceState) -> Result<RunResponse> {
    info!(name = %req.name, project = %req.project, "remote run request");

    let db = state.db.lock().await;
    if db.get_instance(&req.project, &req.name)?.is_some() {
        return Err(CoastError::state(format!(
            "instance '{}' already exists for project '{}'",
            req.name, req.project
        )));
    }

    let inst = crate::state::instances::RemoteInstance {
        name: req.name.clone(),
        project: req.project.clone(),
        status: "provisioning".to_string(),
        container_id: None,
        build_id: req.build_id.clone(),
        coastfile_type: req.coastfile_type.clone(),
        worktree: req.worktree.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    db.insert_instance(&inst)?;
    drop(db);

    let artifact_dir = resolve_artifact_dir(&req.project, req.coastfile_type.as_deref());
    let coast_image = artifact_dir.as_ref().and_then(|d| read_coast_image(d));
    let ports = artifact_dir
        .as_ref()
        .map(|d| read_ports_from_coastfile(d))
        .unwrap_or_default();

    let ws_path = workspace_path(&req.project, &req.name);
    std::fs::create_dir_all(&ws_path).map_err(|e| CoastError::Io {
        message: format!("failed to create workspace directory: {e}"),
        path: ws_path.clone(),
        source: Some(e),
    })?;
    make_world_writable(&ws_path);

    let cache_path = image_cache_path();
    let _ = std::fs::create_dir_all(&cache_path);

    let secrets = load_secrets_for_project(&req.project);
    let env_secrets: std::collections::HashMap<String, String> = secrets
        .iter()
        .filter(|s| s.inject_type == "env")
        .filter_map(|s| {
            String::from_utf8(s.value.clone())
                .ok()
                .map(|v| (s.inject_target.clone(), v))
        })
        .collect();

    let dynamic_ports = if state.docker.is_some() {
        crate::port_manager::allocate_dynamic_ports(ports.len())?
    } else {
        ports.iter().map(|(_, p)| *p).collect()
    };

    let container_id = if let Some(ref docker) = state.docker {
        Some(
            provision_dind_container(
                docker,
                &req,
                &ws_path,
                coast_image.as_deref(),
                artifact_dir.as_deref(),
                &cache_path,
                &env_secrets,
                &ports,
                &dynamic_ports,
                &secrets,
            )
            .await?,
        )
    } else {
        None
    };

    let db = state.db.lock().await;
    if let Some(ref cid) = container_id {
        db.update_instance_container_id(&req.project, &req.name, Some(cid))?;
    }
    db.update_instance_status(&req.project, &req.name, "running")?;

    let port_mappings: Vec<PortMapping> = ports
        .iter()
        .zip(dynamic_ports.iter())
        .map(|((name, canonical), dynamic)| PortMapping {
            logical_name: name.clone(),
            canonical_port: *canonical,
            dynamic_port: *dynamic,
            is_primary: false,
        })
        .collect();

    info!(
        name = %req.name,
        container_id = ?container_id,
        ports = port_mappings.len(),
        env_secrets = env_secrets.len(),
        "remote instance running"
    );

    Ok(RunResponse {
        name: req.name,
        container_id: container_id.unwrap_or_default(),
        ports: port_mappings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};
    use std::sync::Arc;

    fn test_state() -> Arc<ServiceState> {
        Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ))
    }

    fn run_req(name: &str, project: &str) -> RunRequest {
        RunRequest {
            name: name.to_string(),
            project: project.to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_run_creates_instance() {
        let state = test_state();
        let resp = handle(run_req("inst1", "proj"), &state).await.unwrap();
        assert_eq!(resp.name, "inst1");
        assert!(resp.container_id.is_empty());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "inst1").unwrap().unwrap();
        assert_eq!(inst.status, "running");
        assert!(inst.container_id.is_none());
    }

    #[tokio::test]
    async fn test_run_duplicate_errors() {
        let state = test_state();
        handle(run_req("dup", "proj"), &state).await.unwrap();
        let err = handle(run_req("dup", "proj"), &state).await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_run_stores_build_id() {
        let state = test_state();
        let mut req = run_req("bi", "proj");
        req.build_id = Some("build-42".to_string());
        handle(req, &state).await.unwrap();

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "bi").unwrap().unwrap();
        assert_eq!(inst.build_id.as_deref(), Some("build-42"));
    }

    #[test]
    fn test_resolve_artifact_dir_nonexistent() {
        let result = resolve_artifact_dir("nonexistent-project-xyz-42", None);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_artifact_dir_with_type() {
        let result = resolve_artifact_dir("nonexistent-project-xyz-42", Some("light"));
        assert!(result.is_none());
    }

    #[test]
    fn test_workspace_path_format() {
        let path = workspace_path("my-app", "dev-1");
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("workspaces"));
        assert!(path_str.contains("my-app"));
        assert!(path_str.contains("dev-1"));
    }

    #[test]
    fn test_read_coast_image_missing_dir() {
        let result = read_coast_image(Path::new("/nonexistent/dir"));
        assert!(result.is_none());
    }

    #[test]
    fn test_read_coast_image_valid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"coast_image": "my-custom:latest"}"#).unwrap();
        let result = read_coast_image(dir.path());
        assert_eq!(result, Some("my-custom:latest".to_string()));
    }

    #[test]
    fn test_read_coast_image_no_field() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"project_name": "test"}"#).unwrap();
        let result = read_coast_image(dir.path());
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_no_docker_fallback_updates_db() {
        let state = test_state();
        let resp = handle(run_req("fallback", "proj"), &state).await.unwrap();
        assert_eq!(resp.name, "fallback");
        assert!(resp.container_id.is_empty());

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "fallback").unwrap().unwrap();
        assert_eq!(inst.status, "running");
        assert!(inst.container_id.is_none());
    }

    #[tokio::test]
    async fn test_no_docker_fallback_returns_empty_ports() {
        let state = test_state();
        let resp = handle(run_req("noports", "proj"), &state).await.unwrap();
        assert!(resp.ports.is_empty());
    }

    #[test]
    fn test_resolve_artifact_dir_symlink_logic() {
        let dir = tempfile::tempdir().unwrap();
        let images_dir = dir.path().join("images").join("link-proj");
        std::fs::create_dir_all(&images_dir).unwrap();
        let build_dir = images_dir.join("build-42");
        std::fs::create_dir_all(&build_dir).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("build-42", images_dir.join("latest")).unwrap();

            let target = std::fs::read_link(images_dir.join("latest")).unwrap();
            let resolved = images_dir.join(target);
            assert!(resolved.ends_with("build-42"));
        }
    }

    #[test]
    fn test_resolve_artifact_dir_typed_link_naming() {
        let dir = tempfile::tempdir().unwrap();
        let images_dir = dir.path().join("images").join("typed-link");
        std::fs::create_dir_all(&images_dir).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("build-99", images_dir.join("latest-light")).unwrap();

            let target = std::fs::read_link(images_dir.join("latest-light")).unwrap();
            let resolved = images_dir.join(target);
            assert!(resolved.ends_with("build-99"));
        }
    }

    #[test]
    fn test_read_ports_from_coastfile_with_ports() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("coastfile.toml"),
            "[coast]\nname = \"test-app\"\n\n[ports]\nweb = 3000\napi = 8080\n",
        )
        .unwrap();

        let ports = read_ports_from_coastfile(dir.path());
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].1, 3000);
        assert_eq!(ports[1].1, 8080);
    }

    #[test]
    fn test_read_ports_from_coastfile_no_ports() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("coastfile.toml"),
            "[coast]\nname = \"no-ports\"\n",
        )
        .unwrap();

        let ports = read_ports_from_coastfile(dir.path());
        assert!(ports.is_empty());
    }

    #[test]
    fn test_read_ports_from_coastfile_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let ports = read_ports_from_coastfile(dir.path());
        assert!(ports.is_empty());
    }

    #[test]
    fn test_read_ports_from_coastfile_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("coastfile.toml"), "{{not valid toml}}").unwrap();
        let ports = read_ports_from_coastfile(dir.path());
        assert!(ports.is_empty());
    }

    #[test]
    fn test_read_ports_from_coastfile_sorted_output() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("coastfile.toml"),
            "[coast]\nname = \"sorted\"\n\n[ports]\ndb = 5432\nweb = 3000\nmetrics = 9090\n",
        )
        .unwrap();

        let ports = read_ports_from_coastfile(dir.path());
        assert_eq!(ports.len(), 3);
        assert!(ports[0].1 <= ports[1].1);
        assert!(ports[1].1 <= ports[2].1);
    }

    #[test]
    fn test_read_coast_image_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), "not json at all").unwrap();
        assert!(read_coast_image(dir.path()).is_none());
    }

    #[test]
    fn test_read_coast_image_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{}").unwrap();
        assert!(read_coast_image(dir.path()).is_none());
    }

    #[test]
    fn test_read_coast_image_null_field() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), r#"{"coast_image": null}"#).unwrap();
        assert!(read_coast_image(dir.path()).is_none());
    }

    #[test]
    fn test_read_coast_image_numeric_field() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), r#"{"coast_image": 42}"#).unwrap();
        assert!(read_coast_image(dir.path()).is_none());
    }

    #[test]
    fn test_build_file_secret_cmd_format() {
        let cmd = build_file_secret_cmd("/run/secrets/api-key", "my-secret-value");
        assert!(cmd.contains("mkdir -p"));
        assert!(cmd.contains("/run/secrets/api-key"));
        assert!(cmd.contains("my-secret-value"));
        assert!(cmd.contains("COAST_SECRET_EOF"));
    }

    #[test]
    fn test_build_file_secret_cmd_multiline_value() {
        let cmd = build_file_secret_cmd("/etc/cert.pem", "line1\nline2\nline3");
        assert!(cmd.contains("line1\nline2\nline3"));
        assert!(cmd.contains("/etc/cert.pem"));
    }

    #[test]
    fn test_log_file_secret_result_success_no_panic() {
        let result = Ok(coast_docker::runtime::ExecResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        });
        log_file_secret_result("test-secret", "/run/secrets/test", &result);
    }

    #[test]
    fn test_log_file_secret_result_exec_failure_no_panic() {
        let result = Ok(coast_docker::runtime::ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "permission denied".to_string(),
        });
        log_file_secret_result("test-secret", "/run/secrets/test", &result);
    }

    #[test]
    fn test_log_file_secret_result_error_no_panic() {
        let result: std::result::Result<
            coast_docker::runtime::ExecResult,
            coast_core::error::CoastError,
        > = Err(coast_core::error::CoastError::state("test error"));
        log_file_secret_result("test-secret", "/run/secrets/test", &result);
    }

    #[test]
    fn test_build_container_config_no_ports() {
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");
        let env_secrets = std::collections::HashMap::new();
        let ports: Vec<(String, u16)> = vec![];
        let dynamic_ports: Vec<u16> = vec![];

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &env_secrets,
            &ports,
            &dynamic_ports,
        );
        assert!(config.published_ports.is_empty());
        assert_eq!(config.project, "myproj");
        assert_eq!(config.instance_name, "dev");
    }

    #[test]
    fn test_build_container_config_with_ports() {
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");
        let env_secrets = std::collections::HashMap::new();
        let ports = vec![("web".to_string(), 3000u16), ("api".to_string(), 8080u16)];
        let dynamic_ports = vec![30001u16, 30002u16];

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &env_secrets,
            &ports,
            &dynamic_ports,
        );
        assert_eq!(config.published_ports.len(), 2);
        assert_eq!(config.published_ports[0].host_port, 30001);
        assert_eq!(config.published_ports[0].container_port, 3000);
        assert_eq!(config.published_ports[1].host_port, 30002);
        assert_eq!(config.published_ports[1].container_port, 8080);
    }

    #[test]
    fn test_build_container_config_with_env_secrets() {
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");
        let mut env_secrets = std::collections::HashMap::new();
        env_secrets.insert("API_KEY".to_string(), "secret123".to_string());
        env_secrets.insert("DB_URL".to_string(), "postgres://localhost".to_string());

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &env_secrets,
            &[],
            &[],
        );
        assert_eq!(config.env_vars.get("API_KEY").unwrap(), "secret123");
        assert_eq!(
            config.env_vars.get("DB_URL").unwrap(),
            "postgres://localhost"
        );
    }

    #[test]
    fn test_build_container_config_with_coast_image() {
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");

        let config = build_container_config(
            &req,
            &ws_path,
            Some("custom-coast:v2"),
            None,
            &cache_path,
            &std::collections::HashMap::new(),
            &[],
            &[],
        );
        assert_eq!(config.project, "myproj");
    }

    #[test]
    fn test_build_container_config_with_existing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().to_path_buf();
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &std::collections::HashMap::new(),
            &[],
            &[],
        );
        assert_eq!(config.project, "myproj");
    }

    #[test]
    fn test_build_container_config_with_artifact_dir() {
        let dir = tempfile::tempdir().unwrap();
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            Some(dir.path()),
            &cache_path,
            &std::collections::HashMap::new(),
            &[],
            &[],
        );
        assert_eq!(config.project, "myproj");
    }

    #[test]
    fn test_image_cache_path_format() {
        let path = image_cache_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("image-cache"));
    }

    #[test]
    fn test_workspace_path_segments() {
        let path = workspace_path("proj-a", "inst-1");
        let components: Vec<_> = path.components().collect();
        let last_three: Vec<String> = components
            .iter()
            .rev()
            .take(3)
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        assert_eq!(last_three[0], "inst-1");
        assert_eq!(last_three[1], "proj-a");
        assert_eq!(last_three[2], "workspaces");
    }

    #[tokio::test]
    async fn test_run_stores_coastfile_type() {
        let state = test_state();
        let mut req = run_req("ct", "proj");
        req.coastfile_type = Some("light".to_string());
        handle(req, &state).await.unwrap();

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "ct").unwrap().unwrap();
        assert_eq!(inst.coastfile_type.as_deref(), Some("light"));
    }

    #[tokio::test]
    async fn test_run_stores_worktree() {
        let state = test_state();
        let mut req = run_req("wt", "proj");
        req.worktree = Some("feature-x".to_string());
        handle(req, &state).await.unwrap();

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "wt").unwrap().unwrap();
        assert_eq!(inst.worktree.as_deref(), Some("feature-x"));
    }

    #[test]
    fn test_port_mapping_construction() {
        let ports = vec![("web".to_string(), 3000u16), ("api".to_string(), 8080u16)];
        let dynamic_ports = vec![30001u16, 30002u16];
        let port_mappings: Vec<PortMapping> = ports
            .iter()
            .zip(dynamic_ports.iter())
            .map(|((name, canonical), dynamic)| PortMapping {
                logical_name: name.clone(),
                canonical_port: *canonical,
                dynamic_port: *dynamic,
                is_primary: false,
            })
            .collect();

        assert_eq!(port_mappings.len(), 2);
        assert_eq!(port_mappings[0].logical_name, "web");
        assert_eq!(port_mappings[0].canonical_port, 3000);
        assert_eq!(port_mappings[0].dynamic_port, 30001);
        assert_eq!(port_mappings[1].logical_name, "api");
        assert_eq!(port_mappings[1].canonical_port, 8080);
        assert_eq!(port_mappings[1].dynamic_port, 30002);
    }

    #[test]
    fn test_port_mapping_empty() {
        let ports: Vec<(String, u16)> = vec![];
        let dynamic_ports: Vec<u16> = vec![];
        let port_mappings: Vec<PortMapping> = ports
            .iter()
            .zip(dynamic_ports.iter())
            .map(|((name, canonical), dynamic)| PortMapping {
                logical_name: name.clone(),
                canonical_port: *canonical,
                dynamic_port: *dynamic,
                is_primary: false,
            })
            .collect();
        assert!(port_mappings.is_empty());
    }

    #[test]
    fn test_load_secrets_nonexistent_project() {
        let secrets = load_secrets_for_project("nonexistent-project-xyz-42");
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_env_secret_filtering() {
        let ks = coast_secrets::keystore::Keystore::open(
            &std::env::temp_dir().join("coast-test-env-filter-keystore.db"),
            &std::env::temp_dir().join("coast-test-env-filter-keystore.key"),
        )
        .unwrap();
        let _ = ks.delete_secrets_for_image("filter-proj");
        ks.store_secret(&coast_secrets::keystore::StoreSecretParams {
            inject_target: "API_KEY",
            extractor: "env",
            ..coast_secrets::keystore::StoreSecretParams::new(
                "filter-proj",
                "api_key",
                b"secret123",
            )
        })
        .unwrap();
        ks.store_secret(&coast_secrets::keystore::StoreSecretParams {
            inject_type: "file",
            inject_target: "/run/secrets/cert",
            extractor: "file",
            ..coast_secrets::keystore::StoreSecretParams::new("filter-proj", "cert", b"cert-data")
        })
        .unwrap();

        let secrets = ks.get_all_secrets("filter-proj").unwrap();
        assert_eq!(secrets.len(), 2);

        let env_secrets: std::collections::HashMap<String, String> = secrets
            .iter()
            .filter(|s| s.inject_type == "env")
            .filter_map(|s| {
                String::from_utf8(s.value.clone())
                    .ok()
                    .map(|v| (s.inject_target.clone(), v))
            })
            .collect();
        assert_eq!(env_secrets.get("API_KEY").unwrap(), "secret123");

        let file_secrets: Vec<_> = secrets.iter().filter(|s| s.inject_type == "file").collect();
        assert_eq!(file_secrets.len(), 1);
        assert_eq!(file_secrets[0].inject_target, "/run/secrets/cert");

        let _ = ks.delete_secrets_for_image("filter-proj");
    }

    #[test]
    fn test_build_container_config_with_shared_service_ports_adds_extra_hosts() {
        let mut req = run_req("dev", "myproj");
        req.shared_service_ports = vec![coast_core::protocol::SharedServicePortForward {
            name: "postgres".to_string(),
            port: 5432,
        }];
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &std::collections::HashMap::new(),
            &[],
            &[],
        );
        assert!(
            config
                .extra_hosts
                .contains(&"host.docker.internal:host-gateway".to_string()),
            "DinD should have host.docker.internal when shared service ports are present"
        );
    }

    #[test]
    fn test_build_container_config_without_shared_service_ports_no_extra_hosts() {
        let req = run_req("dev", "myproj");
        let ws_path = PathBuf::from("/tmp/ws");
        let cache_path = PathBuf::from("/nonexistent/cache");

        let config = build_container_config(
            &req,
            &ws_path,
            None,
            None,
            &cache_path,
            &std::collections::HashMap::new(),
            &[],
            &[],
        );
        assert!(
            config.extra_hosts.is_empty(),
            "DinD should not have extra_hosts when no shared services"
        );
    }

    #[test]
    fn test_run_request_shared_service_ports_default() {
        let json = r#"{"name":"x","project":"p","branch":null}"#;
        let req: RunRequest = serde_json::from_str(json).unwrap();
        assert!(req.shared_service_ports.is_empty());
    }

    #[test]
    fn test_run_request_shared_service_ports_round_trip() {
        let req = RunRequest {
            name: "x".to_string(),
            project: "p".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: vec![
                coast_core::protocol::SharedServicePortForward {
                    name: "postgres".to_string(),
                    port: 5432,
                },
                coast_core::protocol::SharedServicePortForward {
                    name: "redis".to_string(),
                    port: 6379,
                },
            ],
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: RunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.shared_service_ports.len(), 2);
        assert_eq!(deserialized.shared_service_ports[0].name, "postgres");
        assert_eq!(deserialized.shared_service_ports[0].port, 5432);
        assert_eq!(deserialized.shared_service_ports[1].name, "redis");
        assert_eq!(deserialized.shared_service_ports[1].port, 6379);
    }
}
