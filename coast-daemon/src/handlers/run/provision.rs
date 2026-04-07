use std::collections::{HashMap, HashSet};

use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, RunRequest};
use coast_docker::runtime::Runtime;

use crate::handlers::shared_service_routing::{
    ensure_shared_service_proxies, plan_shared_service_routing,
};
use crate::server::AppState;

use super::paths;
use super::service_start::{start_and_wait_for_services, StartServicesRequest};
use super::validate::ValidatedRun;
use super::{
    compose_rewrite, emit, host_builds, image_loading, mcp_setup, merge_dynamic_port_env_vars,
    secrets, shared_services_setup,
};

const MAX_CONTAINER_PORT_RETRY_ATTEMPTS: usize = 3;

pub(super) struct ProvisionResult {
    pub container_id: String,
    pub pre_allocated_ports: Vec<(String, u16, u16)>,
}

type PreAllocatedPort = (String, u16, u16);
type ExternalWorktreeDir = coast_core::coastfile::ResolvedExternalDir;
type DindContainerManager =
    coast_docker::container::ContainerManager<coast_docker::dind::DindRuntime>;

struct CoastfileResources {
    pre_allocated_ports: Vec<PreAllocatedPort>,
    volume_mounts: Vec<coast_docker::runtime::VolumeMount>,
    mcp_servers: Vec<coast_core::types::McpServerConfig>,
    mcp_clients: Vec<coast_core::types::McpClientConnectorConfig>,
    shared_services: Vec<coast_core::types::SharedServiceConfig>,
    shared_service_targets: HashMap<String, String>,
    shared_network: Option<String>,
}

/// Phase 2: Docker provisioning -- create container, load images, start services.
pub(super) async fn provision_instance(
    docker: &bollard::Docker,
    validated: &ValidatedRun,
    req: &RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<ProvisionResult> {
    let code_path = resolve_code_path(&req.project, validated.build_id.as_deref());

    let per_instance_image_tags = build_host_images(validated, &code_path, req, progress).await;

    let artifact_dir = resolve_artifact_dir(&req.project, validated.build_id.as_deref());
    let coastfile_path = artifact_dir.join("coastfile.toml");

    let resources = load_coastfile_resources(&coastfile_path, req, state, progress).await?;

    let secret_plan = secrets::load_secrets_for_instance(&coastfile_path, &req.name);
    let secret_container_paths = secret_plan.container_paths.clone();
    let secret_files_for_exec = secret_plan.files_for_exec.clone();
    let has_volume_mounts = !resources.volume_mounts.is_empty();

    let (container_id, pre_allocated_ports) = create_container(
        docker,
        validated,
        req,
        &code_path,
        &resources,
        secret_plan,
        resources.pre_allocated_ports.clone(),
        progress,
    )
    .await?;

    {
        let db = state.db.lock().await;
        db.update_instance_container_id(&req.project, &req.name, Some(&container_id))?;
    }

    connect_shared_network(state, &resources.shared_network, &container_id).await;

    let private_paths = coast_core::coastfile::Coastfile::from_file(&coastfile_path)
        .map(|cf| cf.private_paths)
        .unwrap_or_default();

    let ctx = InstanceConfig {
        docker,
        validated,
        req,
        code_path: &code_path,
        artifact_dir: &artifact_dir,
        coastfile_path: &coastfile_path,
        container_id: &container_id,
        resources: &resources,
        per_instance_image_tags: &per_instance_image_tags,
        has_volume_mounts,
        secret_container_paths: &secret_container_paths,
        secret_files_for_exec: &secret_files_for_exec,
        progress,
        private_paths,
    };

    normalize_inner_docker_socket_permissions(docker, &container_id).await;
    setup_shared_services(&ctx).await?;
    prepare_images(&ctx).await;
    prepare_runtime(&ctx).await?;
    start_services(&ctx).await?;

    Ok(ProvisionResult {
        container_id,
        pre_allocated_ports,
    })
}

struct InstanceConfig<'a> {
    docker: &'a bollard::Docker,
    validated: &'a ValidatedRun,
    req: &'a RunRequest,
    code_path: &'a std::path::Path,
    artifact_dir: &'a std::path::Path,
    coastfile_path: &'a std::path::Path,
    container_id: &'a str,
    resources: &'a CoastfileResources,
    per_instance_image_tags: &'a [(String, String)],
    has_volume_mounts: bool,
    secret_container_paths: &'a [String],
    secret_files_for_exec: &'a [(String, Vec<u8>)],
    progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
    private_paths: Vec<String>,
}

struct ContainerCreateContext<'a> {
    req: &'a RunRequest,
    code_path: &'a std::path::Path,
    resources: &'a CoastfileResources,
    secret_plan: &'a secrets::SecretInjectionPlan,
    image_cache_path: Option<&'a std::path::Path>,
    artifact_dir: Option<&'a std::path::Path>,
    coast_image: Option<&'a str>,
    override_dir: Option<&'a std::path::Path>,
    dind_extra_hosts: &'a [String],
    shared_caddy_pki_host_dir: &'a std::path::Path,
    external_worktree_dirs: &'a [ExternalWorktreeDir],
}

enum ContainerCreateAttempt {
    Ready(String),
    Retry(CoastError),
}

async fn setup_shared_services(ctx: &InstanceConfig<'_>) -> Result<()> {
    info!(instance = %ctx.req.name, "provision: setting up shared services");
    let shared_service_routing = if ctx.resources.shared_services.is_empty() {
        None
    } else {
        Some(
            plan_shared_service_routing(
                ctx.docker,
                ctx.container_id,
                &ctx.resources.shared_services,
                &ctx.resources.shared_service_targets,
            )
            .await?,
        )
    };

    let shared_service_hosts = shared_service_routing.as_ref().map_or_else(
        HashMap::new,
        super::super::shared_service_routing::SharedServiceRoutingPlan::host_map,
    );

    rewrite_compose(
        ctx.artifact_dir,
        ctx.code_path,
        ctx.coastfile_path,
        &shared_service_hosts,
        ctx.per_instance_image_tags,
        ctx.has_volume_mounts,
        ctx.secret_container_paths,
        Some(paths::SHARED_CADDY_PKI_CONTAINER_PATH),
        &ctx.req.project,
        &ctx.req.name,
    );

    if let Some(ref routing) = shared_service_routing {
        ensure_shared_service_proxies(ctx.docker, ctx.container_id, routing).await?;
    }

    info!(instance = %ctx.req.name, "provision: shared services configured");
    Ok(())
}

async fn prepare_images(ctx: &InstanceConfig<'_>) {
    info!(instance = %ctx.req.name, "provision: preparing images");
    load_cached_images(
        ctx.docker,
        ctx.container_id,
        &ctx.req.project,
        ctx.validated,
        ctx.progress,
    )
    .await;
    image_loading::pipe_host_images_to_inner_daemon(ctx.per_instance_image_tags, ctx.container_id)
        .await;
    info!(instance = %ctx.req.name, "provision: images ready");
}

async fn prepare_runtime(ctx: &InstanceConfig<'_>) -> Result<()> {
    info!(instance = %ctx.req.name, "provision: preparing runtime");
    bind_workspace(
        ctx.docker,
        ctx.container_id,
        &ctx.req.name,
        &ctx.private_paths,
        &ctx.validated.bare_services,
    )
    .await;
    install_mcp_if_configured(ctx).await?;
    secrets::write_secret_files_via_exec(ctx.secret_files_for_exec, ctx.container_id, ctx.docker)
        .await;
    info!(instance = %ctx.req.name, "provision: runtime ready");
    Ok(())
}

async fn install_mcp_if_configured(ctx: &InstanceConfig<'_>) -> Result<()> {
    if ctx.resources.mcp_servers.is_empty() && ctx.resources.mcp_clients.is_empty() {
        return Ok(());
    }
    mcp_setup::install_mcp_servers(
        ctx.container_id,
        &ctx.resources.mcp_servers,
        &ctx.resources.mcp_clients,
        ctx.docker,
        ctx.progress,
    )
    .await
}

async fn start_services(ctx: &InstanceConfig<'_>) -> Result<()> {
    let artifact_dir_opt = ctx.artifact_dir.exists().then_some(ctx.artifact_dir);
    start_and_wait_for_services(
        ctx.docker,
        StartServicesRequest {
            container_id: ctx.container_id,
            instance_name: &ctx.req.name,
            project: &ctx.req.project,
            has_compose: ctx.validated.has_compose,
            has_services: ctx.validated.has_services,
            compose_rel_dir: ctx.validated.compose_rel_dir.as_deref(),
            artifact_dir_opt,
            bare_services: &ctx.validated.bare_services,
            total_steps: ctx.validated.total_steps,
            progress: ctx.progress,
        },
    )
    .await?;

    normalize_shared_caddy_pki_permissions(ctx.docker, ctx.container_id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn resolve_code_path(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let project_dir = paths::project_images_dir(project);
    let manifest_path = build_id
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
}

fn resolve_artifact_dir(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let project_images_dir = paths::project_images_dir(project);
    if let Some(bid) = build_id {
        let resolved = project_images_dir.join(bid);
        if resolved.exists() {
            resolved
        } else {
            project_images_dir.join("latest")
        }
    } else {
        project_images_dir.join("latest")
    }
}

async fn build_host_images(
    validated: &ValidatedRun,
    code_path: &std::path::Path,
    req: &RunRequest,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Vec<(String, String)> {
    if !validated.has_compose {
        return Vec::new();
    }
    emit(
        progress,
        BuildProgressEvent::started("Building images", 2, validated.total_steps),
    );
    let tags = host_builds::build_per_instance_images_on_host(
        code_path,
        &req.project,
        &req.name,
        progress,
    )
    .await;
    emit(progress, BuildProgressEvent::done("Building images", "ok"));
    tags
}

async fn load_coastfile_resources(
    coastfile_path: &std::path::Path,
    req: &RunRequest,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<CoastfileResources> {
    let mut result = CoastfileResources {
        pre_allocated_ports: Vec::new(),
        volume_mounts: Vec::new(),
        mcp_servers: Vec::new(),
        mcp_clients: Vec::new(),
        shared_services: Vec::new(),
        shared_service_targets: HashMap::new(),
        shared_network: None,
    };

    if !coastfile_path.exists() {
        debug!(
            project = %req.project,
            instance = %req.name,
            path = %coastfile_path.display(),
            "artifact Coastfile missing while loading run resources"
        );
        return Ok(result);
    }
    let coastfile = match coast_core::coastfile::Coastfile::from_file(coastfile_path) {
        Ok(coastfile) => coastfile,
        Err(error) => {
            warn!(
                project = %req.project,
                instance = %req.name,
                path = %coastfile_path.display(),
                error = %error,
                "failed to parse artifact Coastfile while loading run resources"
            );
            return Ok(result);
        }
    };

    debug!(
        project = %req.project,
        instance = %req.name,
        path = %coastfile_path.display(),
        port_count = coastfile.ports.len(),
        volume_count = coastfile.volumes.len(),
        shared_service_count = coastfile.shared_services.len(),
        "loaded artifact Coastfile for run resources"
    );

    let shared_service_ports: HashSet<u16> = coastfile
        .shared_services
        .iter()
        .flat_map(|ss| ss.ports.iter().map(|p| p.container_port))
        .collect();

    let logical_ports: Vec<_> = coastfile
        .ports
        .iter()
        .filter(|(_, port_num)| !shared_service_ports.contains(port_num))
        .map(|(port_name, port_num)| (port_name.clone(), *port_num))
        .collect();
    result.pre_allocated_ports = allocate_pre_allocated_ports(&logical_ports)?;

    for vol_config in &coastfile.volumes {
        let resolved_name =
            coast_core::volume::resolve_volume_name(vol_config, &req.name, &req.project);
        result
            .volume_mounts
            .push(coast_docker::runtime::VolumeMount {
                volume_name: resolved_name,
                container_path: format!("/coast-volumes/{}", vol_config.name),
                read_only: false,
            });
    }

    copy_snapshot_volumes(&coastfile.volumes, &req.name, &req.project, progress).await?;

    result.mcp_servers = coastfile.mcp_servers.clone();
    result.mcp_clients = coastfile.mcp_clients.clone();
    result.shared_services = coastfile.shared_services.clone();

    if !coastfile.shared_services.is_empty() {
        if let Some(docker) = state.docker.as_ref() {
            let shared = shared_services_setup::start_shared_services(
                &req.project,
                &coastfile.shared_services,
                &docker,
                state,
            )
            .await?;
            result.shared_service_targets = shared.service_hosts;
            result.shared_network = shared.network_name;
        }
    }

    Ok(result)
}

fn rewrite_compose(
    artifact_dir: &std::path::Path,
    code_path: &std::path::Path,
    coastfile_path: &std::path::Path,
    shared_service_hosts: &HashMap<String, String>,
    per_instance_image_tags: &[(String, String)],
    has_volume_mounts: bool,
    secret_container_paths: &[String],
    shared_caddy_pki_container_path: Option<&str>,
    project: &str,
    instance_name: &str,
) {
    let compose_path = artifact_dir.join("compose.yml");
    let compose_content = if compose_path.exists() {
        std::fs::read_to_string(&compose_path).ok()
    } else {
        let ws_compose = code_path.join("docker-compose.yml");
        std::fs::read_to_string(&ws_compose).ok()
    };

    let Some(ref content) = compose_content else {
        return;
    };

    let assign_cfg = coast_core::coastfile::Coastfile::from_file(coastfile_path)
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
            shared_service_hosts,
            coastfile_path,
            per_instance_image_tags,
            has_volume_mounts,
            secret_container_paths,
            shared_caddy_pki_container_path,
            project,
            instance_name,
            hot_services: &hot_svcs,
            default_hot,
        },
    );
}

async fn create_container(
    docker: &bollard::Docker,
    validated: &ValidatedRun,
    req: &RunRequest,
    code_path: &std::path::Path,
    resources: &CoastfileResources,
    secret_plan: secrets::SecretInjectionPlan,
    initial_pre_allocated_ports: Vec<PreAllocatedPort>,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<(String, Vec<PreAllocatedPort>)> {
    let image_cache_dir = paths::image_cache_dir();
    let image_cache_path = if image_cache_dir.exists() {
        Some(image_cache_dir.as_path())
    } else {
        None
    };

    let artifact_dir_path = resolve_artifact_dir(&req.project, validated.build_id.as_deref());
    let artifact_dir_opt = if artifact_dir_path.exists() {
        Some(artifact_dir_path.as_path())
    } else {
        None
    };

    let coast_image = read_coast_image(&artifact_dir_path);

    let override_dir_path = paths::override_dir(&req.project, &req.name);
    std::fs::create_dir_all(&override_dir_path).map_err(|error| CoastError::Io {
        message: format!("failed to create override directory: {error}"),
        path: override_dir_path.clone(),
        source: Some(error),
    })?;
    let override_dir_opt = Some(override_dir_path.as_path());

    let shared_caddy_pki_host_dir = paths::shared_caddy_pki_host_dir();
    std::fs::create_dir_all(&shared_caddy_pki_host_dir).map_err(|error| CoastError::Io {
        message: format!("failed to create shared Caddy PKI directory: {error}"),
        path: shared_caddy_pki_host_dir.clone(),
        source: Some(error),
    })?;

    let dind_extra_hosts = vec!["host.docker.internal:host-gateway".to_string()];

    let creating_step = if validated.has_compose { 3 } else { 2 };
    emit(
        progress,
        BuildProgressEvent::started("Creating container", creating_step, validated.total_steps),
    );

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let manager = coast_docker::container::ContainerManager::new(runtime);
    let mut pre_allocated_ports = initial_pre_allocated_ports;
    let external_worktree_dirs = if let Ok(cf) =
        coast_core::coastfile::Coastfile::from_file(&artifact_dir_path.join("coastfile.toml"))
    {
        coast_core::coastfile::Coastfile::resolve_external_worktree_dirs_expanded(
            &cf.worktree_dirs,
            &cf.project_root,
        )
    } else {
        Vec::new()
    };
    let create_ctx = ContainerCreateContext {
        req,
        code_path,
        resources,
        secret_plan: &secret_plan,
        image_cache_path,
        artifact_dir: artifact_dir_opt,
        coast_image: coast_image.as_deref(),
        override_dir: override_dir_opt,
        dind_extra_hosts: &dind_extra_hosts,
        shared_caddy_pki_host_dir: &shared_caddy_pki_host_dir,
        external_worktree_dirs: &external_worktree_dirs,
    };

    let mut last_error: Option<CoastError> = None;

    for attempt in 1..=MAX_CONTAINER_PORT_RETRY_ATTEMPTS {
        let container_id = match create_and_start_container_attempt(
            &manager,
            &create_ctx,
            &pre_allocated_ports,
            attempt,
        )
        .await?
        {
            ContainerCreateAttempt::Ready(container_id) => container_id,
            ContainerCreateAttempt::Retry(error) => {
                pre_allocated_ports = reallocate_pre_allocated_ports(&pre_allocated_ports)?;
                last_error = Some(error);
                continue;
            }
        };

        match manager.wait_for_inner_daemon(&container_id).await {
            Ok(()) => {
                emit(
                    progress,
                    BuildProgressEvent::done("Creating container", "ok"),
                );
                return Ok((container_id, pre_allocated_ports));
            }
            Err(error) => {
                return Err(wrap_container_create_error(req, &error));
            }
        }
    }

    let error = last_error.unwrap_or_else(|| {
        CoastError::docker(format!(
            "Failed to create coast container for instance '{}' after retrying dynamic port allocation.",
            req.name
        ))
    });

    Err(wrap_container_create_error(req, &error))
}

fn build_container_config(
    ctx: &ContainerCreateContext<'_>,
    pre_allocated_ports: &[PreAllocatedPort],
) -> coast_docker::runtime::ContainerConfig {
    let mut env_vars = ctx.secret_plan.env_vars.clone();
    merge_dynamic_port_env_vars(&mut env_vars, pre_allocated_ports);

    let mut config = coast_docker::dind::build_dind_config(coast_docker::dind::DindConfigParams {
        env_vars,
        bind_mounts: ctx.secret_plan.bind_mounts.clone(),
        volume_mounts: ctx.resources.volume_mounts.clone(),
        image_cache_path: ctx.image_cache_path,
        artifact_dir: ctx.artifact_dir,
        coast_image: ctx.coast_image,
        override_dir: ctx.override_dir,
        extra_hosts: ctx.dind_extra_hosts.to_vec(),
        ..coast_docker::dind::DindConfigParams::new(&ctx.req.project, &ctx.req.name, ctx.code_path)
    });
    append_shared_caddy_pki_bind_mount(&mut config, ctx.shared_caddy_pki_host_dir);

    for ext_dir in ctx.external_worktree_dirs {
        config.bind_mounts.push(coast_docker::runtime::BindMount {
            host_path: ext_dir.resolved_path.clone(),
            container_path: coast_core::coastfile::Coastfile::external_mount_path(
                ext_dir.mount_index,
            ),
            read_only: false,
            propagation: None,
        });
    }

    for (_name, canonical, dynamic) in pre_allocated_ports {
        config
            .published_ports
            .push(coast_docker::runtime::PortPublish {
                host_port: *dynamic,
                container_port: *canonical,
            });
    }

    config
}

async fn create_and_start_container_attempt(
    manager: &DindContainerManager,
    ctx: &ContainerCreateContext<'_>,
    pre_allocated_ports: &[PreAllocatedPort],
    attempt: usize,
) -> Result<ContainerCreateAttempt> {
    let config = build_container_config(ctx, pre_allocated_ports);
    let container_id = match manager.create(&config).await {
        Ok(container_id) => container_id,
        Err(error) if should_retry_container_create_error(&error, attempt) => {
            warn!(
                instance = %ctx.req.name,
                attempt,
                error = %error,
                "docker rejected published ports during container creation; reallocating and retrying"
            );
            return Ok(ContainerCreateAttempt::Retry(error));
        }
        Err(error) => {
            return Err(wrap_container_create_error(ctx.req, &error));
        }
    };

    match manager.start(&container_id).await {
        Ok(()) => Ok(ContainerCreateAttempt::Ready(container_id)),
        Err(error) => {
            cleanup_failed_container_start(manager, ctx.req, &container_id).await;
            if should_retry_container_create_error(&error, attempt) {
                warn!(
                    instance = %ctx.req.name,
                    attempt,
                    error = %error,
                    "docker rejected published ports during container startup; reallocating and retrying"
                );
                return Ok(ContainerCreateAttempt::Retry(error));
            }

            Err(wrap_container_create_error(ctx.req, &error))
        }
    }
}

fn should_retry_container_create_error(error: &CoastError, attempt: usize) -> bool {
    attempt < MAX_CONTAINER_PORT_RETRY_ATTEMPTS && is_retryable_port_publish_error(error)
}

async fn cleanup_failed_container_start(
    manager: &DindContainerManager,
    req: &RunRequest,
    container_id: &str,
) {
    if let Err(cleanup_error) = manager.remove(container_id).await {
        warn!(
            instance = %req.name,
            container_id,
            error = %cleanup_error,
            "failed to remove partially created container after startup failure"
        );
    }
}

fn allocate_pre_allocated_ports(logical_ports: &[(String, u16)]) -> Result<Vec<PreAllocatedPort>> {
    allocate_pre_allocated_ports_excluding(logical_ports, &HashSet::new())
}

fn allocate_pre_allocated_ports_excluding(
    logical_ports: &[(String, u16)],
    excluded_dynamic_ports: &HashSet<u16>,
) -> Result<Vec<PreAllocatedPort>> {
    let mut used_dynamic_ports = excluded_dynamic_ports.clone();
    let mut pre_allocated_ports = Vec::with_capacity(logical_ports.len());

    for (logical_name, canonical_port) in logical_ports {
        let dynamic_port =
            crate::port_manager::allocate_dynamic_port_excluding(&used_dynamic_ports)?;
        used_dynamic_ports.insert(dynamic_port);
        pre_allocated_ports.push((logical_name.clone(), *canonical_port, dynamic_port));
    }

    Ok(pre_allocated_ports)
}

fn reallocate_pre_allocated_ports(
    pre_allocated_ports: &[PreAllocatedPort],
) -> Result<Vec<PreAllocatedPort>> {
    let logical_ports = pre_allocated_ports
        .iter()
        .map(|(logical_name, canonical_port, _dynamic_port)| {
            (logical_name.clone(), *canonical_port)
        })
        .collect::<Vec<_>>();
    let excluded_dynamic_ports = pre_allocated_ports
        .iter()
        .map(|(_, _, dynamic_port)| *dynamic_port)
        .collect::<HashSet<_>>();
    allocate_pre_allocated_ports_excluding(&logical_ports, &excluded_dynamic_ports)
}

fn is_retryable_port_publish_error(error: &CoastError) -> bool {
    let message = error.to_string();
    message.contains("ports are not available")
        || message.contains("/forwards/expose")
        || message.contains("port is already allocated")
        || message.contains("bind: address already in use")
}

fn wrap_container_create_error(req: &RunRequest, error: &CoastError) -> CoastError {
    CoastError::docker(format!(
        "Failed to create coast container for instance '{}': {}. \
         Ensure Docker is running and the docker:dind image is available.",
        req.name, error
    ))
}

fn append_shared_caddy_pki_bind_mount(
    config: &mut coast_docker::runtime::ContainerConfig,
    shared_caddy_pki_host_dir: &std::path::Path,
) {
    config.bind_mounts.push(coast_docker::runtime::BindMount {
        host_path: shared_caddy_pki_host_dir.to_path_buf(),
        container_path: paths::SHARED_CADDY_PKI_CONTAINER_PATH.to_string(),
        read_only: false,
        propagation: None,
    });
}

async fn normalize_shared_caddy_pki_permissions(docker: &bollard::Docker, container_id: &str) {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let command = shared_caddy_pki_permission_fixup_command();
    let command_refs: Vec<&str> = command.iter().map(std::string::String::as_str).collect();

    match runtime.exec_in_coast(container_id, &command_refs).await {
        Ok(result) if result.success() => {
            debug!(container_id, "normalized shared Caddy PKI permissions");
        }
        Ok(result) => {
            warn!(
                container_id,
                exit_code = result.exit_code,
                stderr = %result.stderr,
                "failed to normalize shared Caddy PKI permissions"
            );
        }
        Err(error) => {
            warn!(
                container_id,
                error = %error,
                "failed to exec shared Caddy PKI permission fixup"
            );
        }
    }
}

async fn normalize_inner_docker_socket_permissions(docker: &bollard::Docker, container_id: &str) {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let cmd = [
        "sh",
        "-c",
        "for _ in $(seq 1 20); do \
           if [ -S /var/run/docker.sock ]; then chmod 666 /var/run/docker.sock && exit 0; fi; \
           sleep 1; \
         done; \
         exit 1",
    ];
    match runtime.exec_in_coast(container_id, &cmd).await {
        Ok(result) if result.success() => {
            debug!(container_id, "normalized inner Docker socket permissions");
        }
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

fn shared_caddy_pki_permission_fixup_command() -> Vec<String> {
    let base = paths::SHARED_CADDY_PKI_CONTAINER_PATH;
    let script = format!(
        "set -eu; \
         base='{base}'; \
         if [ ! -d \"$base\" ]; then exit 0; fi; \
         chmod 755 \"$base\" 2>/dev/null || true; \
         for dir in \"$base/authorities\" \"$base/authorities/local\"; do \
             [ -d \"$dir\" ] && chmod 755 \"$dir\" 2>/dev/null || true; \
         done; \
         for cert in \"$base/authorities/local/root.crt\" \"$base/authorities/local/intermediate.crt\"; do \
             [ -f \"$cert\" ] && chmod 644 \"$cert\" 2>/dev/null || true; \
         done; \
         for key in \"$base/authorities/local/root.key\" \"$base/authorities/local/intermediate.key\"; do \
             [ -f \"$key\" ] && chmod 600 \"$key\" 2>/dev/null || true; \
         done"
    );

    vec!["sh".into(), "-lc".into(), script]
}

fn read_coast_image(artifact_dir: &std::path::Path) -> Option<String> {
    let manifest_path = artifact_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("coast_image")?.as_str().map(String::from))
}

async fn connect_shared_network(
    state: &AppState,
    shared_network: &Option<String>,
    container_id: &str,
) {
    let Some(ref net_name) = shared_network else {
        return;
    };
    let Some(docker) = state.docker.as_ref() else {
        return;
    };
    let nm = coast_docker::network::NetworkManager::with_client(docker.clone());
    if let Err(e) = nm.connect_container(net_name, container_id).await {
        tracing::warn!(error = %e, "failed to connect coast container to shared network (may already be connected)");
    } else {
        info!(network = %net_name, container = %container_id, "connected coast container to shared services network");
    }
}

async fn load_cached_images(
    docker: &bollard::Docker,
    container_id: &str,
    project: &str,
    validated: &ValidatedRun,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) {
    let loading_step = if validated.has_compose { 4 } else { 3 };
    emit(
        progress,
        BuildProgressEvent::started("Loading cached images", loading_step, validated.total_steps),
    );

    let cache_dir = paths::image_cache_dir();
    if cache_dir.exists() {
        let tarball_names = image_loading::collect_project_tarballs(&cache_dir, project);
        if !tarball_names.is_empty() {
            let existing_images = image_loading::query_existing_images(docker, container_id).await;
            let (tarballs_to_load, skipped) =
                image_loading::filter_tarballs_to_load(tarball_names, &existing_images);

            if skipped > 0 {
                emit(
                    progress,
                    BuildProgressEvent::item(
                        "Loading cached images",
                        format!("{skipped} already present (skipped)"),
                        "skip",
                    ),
                );
                info!(project = %project, skipped = skipped, "skipped loading images already present in inner daemon (persistent volume)");
            }

            info!(project = %project, tarball_count = tarballs_to_load.len(), "loading project-relevant cached images");
            image_loading::load_tarballs_into_inner_daemon(
                &tarballs_to_load,
                docker,
                container_id,
                progress,
            )
            .await;
        }
    }
    emit(
        progress,
        BuildProgressEvent::done("Loading cached images", "ok"),
    );
}

async fn bind_workspace(
    docker: &bollard::Docker,
    container_id: &str,
    instance_name: &str,
    private_paths: &[String],
    bare_services: &[coast_core::types::BareServiceConfig],
) {
    let private_cmds =
        coast_core::coastfile::Coastfile::build_private_paths_mount_commands(private_paths);
    let cache_cmds = coast_core::coastfile::Coastfile::build_cache_mount_commands(bare_services);
    let cmd = format!(
        "mkdir -p /workspace && mount --bind /host-project /workspace && mount --make-rshared /workspace{private_cmds}{cache_cmds}"
    );
    let mount_rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let mount_result = mount_rt
        .exec_in_coast(container_id, &["sh", "-c", &cmd])
        .await;
    match mount_result {
        Ok(r) if r.success() => {
            info!(instance = %instance_name, "bound /host-project -> /workspace")
        }
        Ok(r) => warn!(instance = %instance_name, stderr = %r.stderr, "failed to bind /workspace"),
        Err(e) => warn!(instance = %instance_name, error = %e, "failed to bind /workspace"),
    }
}

// ---------------------------------------------------------------------------
// Snapshot volume copying
// ---------------------------------------------------------------------------

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
        copy_single_snapshot(src, &dest, &vol_config.name, progress).await?;
    }
    Ok(())
}

async fn copy_single_snapshot(
    src: &str,
    dest: &str,
    volume_name: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    info!(source = %src, dest = %dest, "copying snapshot_source volume");
    emit(
        progress,
        BuildProgressEvent::done(format!("Copying volume {src} \u{2192} {dest}"), "started"),
    );

    let stopped_ids = stop_containers_using_volume(src).await?;

    let cmd = coast_core::volume::snapshot_copy_command(src, dest);
    let output = tokio::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to run snapshot copy for volume '{volume_name}': {e}"
            ))
        })?;

    restart_stopped_containers(&stopped_ids).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::docker(format!(
            "snapshot copy failed for volume '{volume_name}' (source: '{src}'): {stderr}. \
             Verify the source volume exists with: docker volume ls | grep {src}"
        )));
    }
    Ok(())
}

async fn stop_containers_using_volume(volume: &str) -> Result<Vec<String>> {
    let using_output = tokio::process::Command::new("docker")
        .args(["ps", "-q", "--filter", &format!("volume={volume}")])
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to check containers using volume '{volume}': {e}"
            ))
        })?;

    let ids: Vec<String> = String::from_utf8_lossy(&using_output.stdout)
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
        .collect();

    if ids.is_empty() {
        return Ok(ids);
    }

    info!(volume = %volume, containers = ?ids, "stopping containers for consistent snapshot copy");
    let mut stop_args = vec!["stop".to_string()];
    stop_args.extend(ids.clone());
    let stop_out = tokio::process::Command::new("docker")
        .args(&stop_args)
        .output()
        .await
        .map_err(|e| {
            CoastError::docker(format!(
                "failed to stop containers using volume '{volume}': {e}"
            ))
        })?;
    if !stop_out.status.success() {
        warn!(
            "failed to stop containers on volume '{}': {}",
            volume,
            String::from_utf8_lossy(&stop_out.stderr)
        );
    }
    Ok(ids)
}

async fn restart_stopped_containers(stopped_ids: &[String]) {
    if stopped_ids.is_empty() {
        return;
    }
    info!(containers = ?stopped_ids, "restarting containers after snapshot copy");
    let mut start_args = vec!["start".to_string()];
    start_args.extend(stopped_ids.iter().cloned());
    let _ = tokio::process::Command::new("docker")
        .args(&start_args)
        .output()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn discard_progress() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    fn sample_run_request() -> RunRequest {
        RunRequest {
            name: "dev-1".to_string(),
            project: "proj".to_string(),
            branch: None,
            worktree: None,
            build_id: None,
            commit_sha: None,
            coastfile_type: None,
            force_remove_dangling: false,
            remote: None,
            shared_service_ports: Vec::new(),
        }
    }

    #[test]
    fn test_resolve_artifact_dir_with_build_id_missing_falls_back_to_latest() {
        let path = resolve_artifact_dir("myproj", Some("nonexistent-build-id"));
        assert!(
            path.to_string_lossy().contains("latest"),
            "should fall back to latest when build_id dir doesn't exist"
        );
    }

    #[test]
    fn test_resolve_artifact_dir_without_build_id_uses_latest() {
        let path = resolve_artifact_dir("myproj", None);
        assert!(
            path.to_string_lossy().contains("latest"),
            "should use latest when no build_id"
        );
        assert!(path.to_string_lossy().contains("myproj"));
    }

    #[test]
    fn test_resolve_code_path_no_manifest_uses_cwd() {
        let path = resolve_code_path("nonexistent-project", None);
        let cwd = std::env::current_dir().unwrap_or_default();
        assert_eq!(path, cwd, "should fall back to CWD when no manifest exists");
    }

    #[test]
    fn test_read_coast_image_missing_dir() {
        let result = read_coast_image(std::path::Path::new("/nonexistent/dir"));
        assert!(result.is_none(), "should return None for missing dir");
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
    fn test_read_coast_image_no_coast_image_field() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("manifest.json");
        std::fs::write(&manifest, r#"{"project_root": "/some/path"}"#).unwrap();
        let result = read_coast_image(dir.path());
        assert!(
            result.is_none(),
            "should return None when coast_image field is missing"
        );
    }

    #[test]
    fn test_append_shared_caddy_pki_bind_mount_adds_outer_runtime_mount() {
        let mut config =
            coast_docker::runtime::ContainerConfig::new("proj", "dev-1", "docker:dind");
        let host_dir = std::path::Path::new("/tmp/coast-home/caddy/pki");

        append_shared_caddy_pki_bind_mount(&mut config, host_dir);

        assert!(config.bind_mounts.iter().any(|mount| {
            mount.host_path == host_dir
                && mount.container_path == paths::SHARED_CADDY_PKI_CONTAINER_PATH
                && !mount.read_only
        }));
    }

    #[test]
    fn test_shared_caddy_pki_permission_fixup_command_exposes_public_cert_only() {
        let command = shared_caddy_pki_permission_fixup_command();
        assert_eq!(command[0], "sh");
        assert_eq!(command[1], "-lc");
        let script = &command[2];

        assert!(script.contains("base='/coast-caddy-pki'"));
        assert!(script.contains("\"$base/authorities/local/root.crt\""));
        assert!(script.contains("\"$base/authorities/local/intermediate.crt\""));
        assert!(script.contains("chmod 644"));
        assert!(script.contains("\"$base/authorities/local/root.key\""));
        assert!(script.contains("\"$base/authorities/local/intermediate.key\""));
        assert!(script.contains("chmod 600"));
    }

    #[test]
    fn test_allocate_pre_allocated_ports_uses_unique_dynamic_ports() {
        let mappings = allocate_pre_allocated_ports(&[
            ("web".to_string(), 3000),
            ("postgres".to_string(), 5432),
            ("redis".to_string(), 6379),
            ("mailpit".to_string(), 8025),
        ])
        .unwrap();

        assert_eq!(mappings.len(), 4);

        let unique_dynamic_ports = mappings
            .iter()
            .map(|(_, _, dynamic)| *dynamic)
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(unique_dynamic_ports.len(), mappings.len());
    }

    #[test]
    fn test_reallocate_pre_allocated_ports_excludes_previous_attempt_ports() {
        let initial = vec![
            (
                "web".to_string(),
                3000,
                crate::port_manager::allocate_dynamic_port().unwrap(),
            ),
            (
                "postgres".to_string(),
                5432,
                crate::port_manager::allocate_dynamic_port().unwrap(),
            ),
        ];

        let remapped = reallocate_pre_allocated_ports(&initial).unwrap();
        let initial_ports = initial
            .iter()
            .map(|(_, _, dynamic_port)| *dynamic_port)
            .collect::<HashSet<_>>();

        assert!(remapped
            .iter()
            .all(|(_, _, dynamic_port)| !initial_ports.contains(dynamic_port)));
    }

    #[test]
    fn test_retryable_port_publish_error_matches_wsl_forwarding_failure() {
        let error = CoastError::docker(
            "Failed to start coast container 'abc'. Is Docker running? Error: Docker responded \
             with status code 500: ports are not available: exposing port TCP \
             127.0.0.1:53987 -> 127.0.0.1:0: /forwards/expose returned unexpected status: 500",
        );

        assert!(is_retryable_port_publish_error(&error));
    }

    #[test]
    fn test_retryable_port_publish_error_ignores_unrelated_docker_failures() {
        let error = CoastError::docker("Failed to pull image 'docker:dind': unauthorized");
        assert!(!is_retryable_port_publish_error(&error));
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_reads_ports_and_volume_mounts() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"
compose = "./docker-compose.yml"

[ports]
web = 3000

[volumes.cache]
strategy = "shared"
service = "redis"
mount = "/data"
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert_eq!(resources.pre_allocated_ports.len(), 1);
        assert_eq!(resources.pre_allocated_ports[0].0, "web");
        assert_eq!(resources.pre_allocated_ports[0].1, 3000);
        assert!(resources.pre_allocated_ports[0].2 > 0);

        assert_eq!(resources.volume_mounts.len(), 1);
        assert_eq!(
            resources.volume_mounts[0].volume_name,
            "coast-shared--proj--cache"
        );
        assert_eq!(
            resources.volume_mounts[0].container_path,
            "/coast-volumes/cache"
        );
        assert!(!resources.volume_mounts[0].read_only);

        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_missing_file_returns_empty_resources() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("missing-coastfile.toml");

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert!(resources.pre_allocated_ports.is_empty());
        assert!(resources.volume_mounts.is_empty());
        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_invalid_coastfile_returns_empty_resources() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"

[volumes.bad]
strategy = "shared"
service = "db"
mount = "/data"
snapshot_source = "seed-volume"
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert!(resources.pre_allocated_ports.is_empty());
        assert!(resources.volume_mounts.is_empty());
        assert!(resources.mcp_servers.is_empty());
        assert!(resources.mcp_clients.is_empty());
        assert!(resources.shared_services.is_empty());
        assert!(resources.shared_service_targets.is_empty());
        assert!(resources.shared_network.is_none());
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_excludes_shared_service_ports() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"
compose = "./docker-compose.yml"

[ports]
web = 3000
postgres = 5432
redis = 6379

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert_eq!(
            resources.pre_allocated_ports.len(),
            1,
            "only non-shared-service ports should be pre-allocated"
        );
        assert_eq!(resources.pre_allocated_ports[0].0, "web");
        assert_eq!(resources.pre_allocated_ports[0].1, 3000);
        assert!(resources.pre_allocated_ports[0].2 > 0);

        assert_eq!(resources.shared_services.len(), 2);
    }

    #[tokio::test]
    async fn test_load_coastfile_resources_no_shared_services_keeps_all_ports() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("coastfile.toml");
        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "proj"
compose = "./docker-compose.yml"

[ports]
web = 3000
postgres = 5432
redis = 6379
"#,
        )
        .unwrap();

        let state = AppState::new_for_testing(StateDb::open_in_memory().unwrap());
        let progress = discard_progress();
        let resources =
            load_coastfile_resources(&coastfile_path, &sample_run_request(), &state, &progress)
                .await
                .unwrap();

        assert_eq!(
            resources.pre_allocated_ports.len(),
            3,
            "all ports allocated when no shared services overlap"
        );
    }
}
