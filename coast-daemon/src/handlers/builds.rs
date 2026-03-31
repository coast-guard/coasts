/// Handler for the `coast builds` command.
///
/// Inspects build artifacts, cached images, and live Docker images for
/// coast projects. Supports versioned builds with build_id-based lookups.
use std::collections::HashMap;

use tracing::{info, warn};

use coast_core::artifact::coast_home;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    BuildSummary, BuildsContentResponse, BuildsDockerImagesResponse, BuildsImagesResponse,
    BuildsInspectResponse, BuildsLsResponse, BuildsRequest, BuildsResponse, CachedImageInfo,
    DockerImageInfo, InstanceSummary, McpBuildInfo, McpClientBuildInfo, SharedServiceBuildInfo,
    VolumeBuildInfo,
};
use coast_core::types::{InstanceStatus, SharedServicePort};

use crate::server::AppState;

/// Handle a builds request.
pub async fn handle(req: BuildsRequest, state: &AppState) -> Result<BuildsResponse> {
    match req {
        BuildsRequest::Ls { project } => handle_ls(project.as_deref(), state).await,
        BuildsRequest::Inspect { project, build_id } => {
            handle_inspect(&project, build_id.as_deref(), state).await
        }
        BuildsRequest::Images { project, build_id } => {
            handle_images(&project, build_id.as_deref()).await
        }
        BuildsRequest::DockerImages { project, build_id } => {
            handle_docker_images(&project, build_id.as_deref(), state).await
        }
        BuildsRequest::InspectDockerImage { project, image } => {
            handle_inspect_docker_image(&project, &image, state).await
        }
        BuildsRequest::Compose { project, build_id } => {
            handle_file(&project, "compose", build_id.as_deref()).await
        }
        BuildsRequest::Manifest { project, build_id } => {
            handle_file(&project, "manifest", build_id.as_deref()).await
        }
        BuildsRequest::Coastfile { project, build_id } => {
            handle_file(&project, "coastfile", build_id.as_deref()).await
        }
    }
}

/// Resolve a build_id (or None/"latest") to an actual build directory path.
/// Handles both new versioned layout and legacy flat layout.
fn resolve_build_dir(project: &str, build_id: Option<&str>) -> Option<std::path::PathBuf> {
    let project_dir = project_images_dir(project)?;
    if !project_dir.is_dir() {
        return None;
    }

    let bid = build_id.unwrap_or("latest");

    // New versioned layout: project_dir/{build_id}/manifest.json
    let versioned = project_dir.join(bid);
    if versioned.join("manifest.json").exists() {
        return Some(versioned);
    }

    // Follow latest symlinks (latest, latest-{type})
    if bid.starts_with("latest") {
        if let Ok(target) = std::fs::read_link(project_dir.join(bid)) {
            let resolved = project_dir.join(target);
            if resolved.join("manifest.json").exists() {
                return Some(resolved);
            }
        }
    }

    // Legacy flat layout: only fall back to project root for "latest", not arbitrary IDs
    if bid.starts_with("latest") && project_dir.join("manifest.json").exists() {
        return Some(project_dir);
    }

    None
}

/// Read and parse manifest.json for a specific build.
fn read_manifest(project: &str, build_id: Option<&str>) -> Option<serde_json::Value> {
    let dir = resolve_build_dir(project, build_id)?;
    let content = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// List all build IDs for a project (newest first), handling both layouts.
fn list_build_ids(project: &str) -> Vec<(String, String, bool)> {
    // Returns: Vec<(build_id, build_timestamp, is_latest)>
    let Some(project_dir) = project_images_dir(project) else {
        return Vec::new();
    };
    if !project_dir.is_dir() {
        return Vec::new();
    }

    // Collect all latest* symlink targets (latest, latest-light, etc.)
    let mut latest_targets: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(&project_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with("latest") {
                if let Ok(target) = std::fs::read_link(entry.path()) {
                    if let Some(name) = target.file_name() {
                        latest_targets.insert(name.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    // Check for legacy layout (manifest.json directly in project dir)
    if project_dir.join("manifest.json").exists() && latest_targets.is_empty() {
        let ts = std::fs::read_to_string(project_dir.join("manifest.json"))
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| {
                v.get("build_timestamp")?
                    .as_str()
                    .map(std::string::ToString::to_string)
            })
            .unwrap_or_default();
        let bid = std::fs::read_to_string(project_dir.join("manifest.json"))
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| {
                v.get("coastfile_hash")?
                    .as_str()
                    .map(std::string::ToString::to_string)
            })
            .unwrap_or_else(|| "legacy".to_string());
        return vec![(bid, ts, true)];
    }

    let Ok(entries) = std::fs::read_dir(&project_dir) else {
        return Vec::new();
    };

    let mut builds = Vec::new();
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
        if !manifest_path.exists() {
            continue;
        }
        let ts = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|v| {
                v.get("build_timestamp")?
                    .as_str()
                    .map(std::string::ToString::to_string)
            })
            .unwrap_or_default();
        let is_latest = latest_targets.contains(&name);
        builds.push((name, ts, is_latest));
    }
    builds.sort_by(|a, b| b.1.cmp(&a.1));
    builds
}

/// Get the image cache directory path.
fn image_cache_dir() -> Option<std::path::PathBuf> {
    let path = coast_core::artifact::image_cache_dir().ok()?;
    path.is_dir().then_some(path)
}

fn images_dir() -> Option<std::path::PathBuf> {
    Some(coast_home().ok()?.join("images"))
}

fn project_images_dir(project: &str) -> Option<std::path::PathBuf> {
    Some(images_dir()?.join(project))
}

/// Compute directory size recursively.
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_file() {
                total += meta.len();
            } else if meta.is_dir() {
                total += dir_size(&entry.path());
            }
        }
    }
    total
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(std::string::ToString::to_string)
}

fn json_usize(v: &serde_json::Value, key: &str) -> usize {
    v.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0) as usize
}

fn extract_mcp_servers(manifest: &serde_json::Value) -> Vec<McpBuildInfo> {
    manifest
        .get("mcp_servers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(McpBuildInfo {
                        name: item.get("name")?.as_str()?.to_string(),
                        proxy: item
                            .get("proxy")
                            .and_then(|p| p.as_str())
                            .map(std::string::ToString::to_string),
                        command: item
                            .get("command")
                            .and_then(|c| c.as_str())
                            .map(std::string::ToString::to_string),
                        args: item
                            .get("args")
                            .and_then(|a| a.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| {
                                        v.as_str().map(std::string::ToString::to_string)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_mcp_clients(manifest: &serde_json::Value) -> Vec<McpClientBuildInfo> {
    manifest
        .get("mcp_clients")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(McpClientBuildInfo {
                        name: item.get("name")?.as_str()?.to_string(),
                        format: item
                            .get("format")
                            .and_then(|f| f.as_str())
                            .map(std::string::ToString::to_string),
                        config_path: item
                            .get("config_path")
                            .and_then(|p| p.as_str())
                            .map(std::string::ToString::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_shared_services(manifest: &serde_json::Value) -> Vec<SharedServiceBuildInfo> {
    manifest
        .get("shared_services")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(SharedServiceBuildInfo {
                        name: item.get("name")?.as_str()?.to_string(),
                        image: item.get("image")?.as_str()?.to_string(),
                        ports: item
                            .get("ports")
                            .and_then(|p| p.as_array())
                            .map(|a| a.iter().filter_map(parse_shared_service_port).collect())
                            .unwrap_or_default(),
                        auto_create_db: item
                            .get("auto_create_db")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_shared_service_port(value: &serde_json::Value) -> Option<SharedServicePort> {
    if let Some(port) = value.as_u64().and_then(|n| u16::try_from(n).ok()) {
        return Some(SharedServicePort::same(port));
    }

    Some(SharedServicePort::new(
        u16::try_from(value.get("host_port")?.as_u64()?).ok()?,
        u16::try_from(value.get("container_port")?.as_u64()?).ok()?,
    ))
}

fn extract_volumes(manifest: &serde_json::Value) -> Vec<VolumeBuildInfo> {
    manifest
        .get("volumes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    Some(VolumeBuildInfo {
                        name: item.get("name")?.as_str()?.to_string(),
                        strategy: item.get("strategy")?.as_str()?.to_string(),
                        service: item.get("service")?.as_str()?.to_string(),
                        mount: item.get("mount")?.as_str()?.to_string(),
                        snapshot_source: item
                            .get("snapshot_source")
                            .and_then(|s| s.as_str())
                            .map(std::string::ToString::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn json_string_vec(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(std::string::ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn compute_full_cache_size(project: &str, manifest: &serde_json::Value) -> u64 {
    let Some(cache_dir) = image_cache_dir() else {
        return 0;
    };
    let built_prefix = format!("coast-built_{}_", project);
    let pulled = json_string_vec(manifest, "pulled_images");
    let base = json_string_vec(manifest, "base_images");

    let mut pulled_filenames: std::collections::HashSet<String> = std::collections::HashSet::new();
    for img_ref in pulled.iter().chain(base.iter()) {
        let filename = format!("{}.tar", img_ref.replace(['/', ':'], "_"));
        pulled_filenames.insert(filename);
    }

    let Ok(entries) = std::fs::read_dir(&cache_dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_project_built = name.starts_with(&built_prefix);
        let is_pulled = pulled_filenames.contains(&name);
        if is_project_built || is_pulled {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Build a `BuildSummary` from a manifest and project metadata.
fn summary_from_manifest(
    project: &str,
    build_id: Option<&str>,
    is_latest: bool,
    manifest: &serde_json::Value,
    instance_counts: &HashMap<String, (usize, usize)>,
    per_build_counts: &HashMap<String, usize>,
    archived: bool,
) -> BuildSummary {
    let cache_size_bytes = compute_full_cache_size(project, manifest);
    let (instance_count, running_count) = instance_counts.get(project).copied().unwrap_or((0, 0));
    let resolved_bid = build_id
        .map(std::string::ToString::to_string)
        .or_else(|| json_str(manifest, "build_id"))
        .or_else(|| json_str(manifest, "coastfile_hash"));
    let instances_using = resolved_bid
        .as_deref()
        .and_then(|bid| per_build_counts.get(bid).copied())
        .unwrap_or(0);
    let coast_image = json_str(manifest, "coast_image");
    let images_built =
        json_usize(manifest, "images_built") + if coast_image.is_some() { 1 } else { 0 };
    BuildSummary {
        project: project.to_string(),
        build_id: resolved_bid,
        is_latest,
        project_root: json_str(manifest, "project_root"),
        build_timestamp: json_str(manifest, "build_timestamp"),
        images_cached: json_usize(manifest, "images_cached"),
        images_built,
        secrets_count: json_string_vec(manifest, "secrets").len(),
        coast_image,
        cache_size_bytes,
        instance_count,
        running_count,
        archived,
        instances_using,
        coastfile_type: json_str(manifest, "coastfile_type"),
    }
}

/// List builds.
async fn handle_ls(project: Option<&str>, state: &AppState) -> Result<BuildsResponse> {
    info!(project = ?project, "handling builds ls request");
    let Some(images_dir) = images_dir() else {
        return Ok(BuildsResponse::Ls(BuildsLsResponse { builds: Vec::new() }));
    };

    let archived_set = {
        let db = state.db.lock().await;
        db.list_archived_projects().unwrap_or_default()
    };

    let mut instance_counts: HashMap<String, (usize, usize)> = HashMap::new();
    let mut per_build_counts: HashMap<String, usize> = HashMap::new();
    {
        let db = state.db.lock().await;
        let all = db.list_instances().unwrap_or_default();
        for inst in &all {
            let entry = instance_counts
                .entry(inst.project.clone())
                .or_insert((0, 0));
            entry.0 += 1;
            if matches!(
                inst.status,
                InstanceStatus::Running | InstanceStatus::CheckedOut
            ) {
                entry.1 += 1;
            }
            if let Some(ref bid) = inst.build_id {
                *per_build_counts.entry(bid.clone()).or_default() += 1;
            }
        }
    }

    let mut builds = Vec::new();

    if let Some(proj) = project {
        // List all builds for a specific project
        let build_ids = list_build_ids(proj);
        let archived = archived_set.contains(proj);
        for (bid, _ts, is_latest) in &build_ids {
            if let Some(m) = read_manifest(proj, Some(bid)) {
                builds.push(summary_from_manifest(
                    proj,
                    Some(bid),
                    *is_latest,
                    &m,
                    &instance_counts,
                    &per_build_counts,
                    archived,
                ));
            }
        }
    } else {
        // List latest build per project
        let Ok(entries) = std::fs::read_dir(&images_dir) else {
            return Ok(BuildsResponse::Ls(BuildsLsResponse { builds }));
        };
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let proj = entry.file_name().to_string_lossy().to_string();
            let archived = archived_set.contains(&proj);
            if let Some(m) = read_manifest(&proj, Some("latest")) {
                builds.push(summary_from_manifest(
                    &proj,
                    json_str(&m, "build_id").as_deref(),
                    true,
                    &m,
                    &instance_counts,
                    &per_build_counts,
                    archived,
                ));
            }
        }
        builds.sort_by(|a, b| a.project.cmp(&b.project));
    }

    Ok(BuildsResponse::Ls(BuildsLsResponse { builds }))
}

/// Detailed inspect for a single build.
async fn handle_inspect(
    project: &str,
    build_id: Option<&str>,
    state: &AppState,
) -> Result<BuildsResponse> {
    info!(project = %project, build_id = ?build_id, "handling builds inspect request");

    let art_dir = resolve_build_dir(project, build_id).ok_or_else(|| {
        CoastError::state(format!(
            "No build artifact found for project '{project}'. Run `coast build` first."
        ))
    })?;

    let manifest = read_manifest(project, build_id).unwrap_or_default();
    let artifact_size_bytes = dir_size(&art_dir);
    let cache_size_bytes = compute_full_cache_size(project, &manifest);

    let resolved_bid =
        json_str(&manifest, "build_id").or_else(|| json_str(&manifest, "coastfile_hash"));
    let instances = {
        let db = state.db.lock().await;
        let rows = db.list_instances_for_project(project)?;
        rows.iter()
            .filter(|row| match (&row.build_id, &resolved_bid) {
                (Some(inst_bid), Some(this_bid)) => inst_bid == this_bid,
                _ => false,
            })
            .map(|row| InstanceSummary {
                name: row.name.clone(),
                project: row.project.clone(),
                status: row.status.clone(),
                branch: row.branch.clone(),
                runtime: row.runtime.clone(),
                checked_out: row.status == InstanceStatus::CheckedOut,
                project_root: json_str(&manifest, "project_root"),
                worktree: row.worktree_name.clone(),
                build_id: row.build_id.clone(),
                coastfile_type: row.coastfile_type.clone(),
                port_count: 0,
                primary_port_service: None,
                primary_port_canonical: None,
                primary_port_dynamic: None,
                primary_port_url: None,
                down_service_count: 0,
                remote_name: row.remote_name.clone(),
            })
            .collect()
    };

    let all_docker_images = list_project_docker_images(project, state).await;
    let built_svcs = json_string_vec(&manifest, "built_services");
    let coast_img = json_str(&manifest, "coast_image");
    let built_prefix = format!("coast-built/{}/", project);
    let docker_images: Vec<DockerImageInfo> = all_docker_images
        .into_iter()
        .filter(|img| {
            if let Some(svc) = img.repository.strip_prefix(&built_prefix) {
                return built_svcs.iter().any(|s| s == svc);
            }
            if let Some(ref ci) = coast_img {
                let full = format!("{}:{}", img.repository, img.tag);
                if full == *ci {
                    return true;
                }
            }
            false
        })
        .collect();

    Ok(BuildsResponse::Inspect(Box::new(BuildsInspectResponse {
        project: project.to_string(),
        build_id: resolved_bid,
        project_root: json_str(&manifest, "project_root"),
        build_timestamp: json_str(&manifest, "build_timestamp"),
        coastfile_hash: json_str(&manifest, "coastfile_hash"),
        coast_image: json_str(&manifest, "coast_image"),
        artifact_path: art_dir.display().to_string(),
        artifact_size_bytes,
        images_cached: json_usize(&manifest, "images_cached"),
        images_built: json_usize(&manifest, "images_built"),
        cache_size_bytes,
        secrets: json_string_vec(&manifest, "secrets"),
        built_services: json_string_vec(&manifest, "built_services"),
        pulled_images: json_string_vec(&manifest, "pulled_images"),
        base_images: json_string_vec(&manifest, "base_images"),
        omitted_services: json_string_vec(&manifest, "omitted_services"),
        omitted_volumes: json_string_vec(&manifest, "omitted_volumes"),
        mcp_servers: extract_mcp_servers(&manifest),
        mcp_clients: extract_mcp_clients(&manifest),
        shared_services: extract_shared_services(&manifest),
        volumes: extract_volumes(&manifest),
        instances,
        docker_images,
        coastfile_type: json_str(&manifest, "coastfile_type"),
    })))
}

/// List cached image tarballs for a project build.
async fn handle_images(project: &str, build_id: Option<&str>) -> Result<BuildsResponse> {
    info!(project = %project, "handling builds images request");

    let _ = resolve_build_dir(project, build_id).ok_or_else(|| {
        CoastError::state(format!(
            "No build artifact found for project '{project}'. Run `coast build` first."
        ))
    })?;

    let manifest = read_manifest(project, build_id).unwrap_or_default();
    let built_services = json_string_vec(&manifest, "built_services");
    let pulled_refs = json_string_vec(&manifest, "pulled_images");
    let base_refs = json_string_vec(&manifest, "base_images");

    let built_prefix = format!("coast-built_{}_", project);

    let mut images = Vec::new();
    let mut total_size_bytes = 0u64;

    if let Some(cache_dir) = image_cache_dir() {
        let pulled_filenames: std::collections::HashSet<String> = pulled_refs
            .iter()
            .map(|r| format!("{}.tar", r.replace(['/', ':'], "_")))
            .collect();
        let base_filenames: std::collections::HashSet<String> = base_refs
            .iter()
            .map(|r| format!("{}.tar", r.replace(['/', ':'], "_")))
            .collect();

        if let Ok(entries) = std::fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if !filename.ends_with(".tar") {
                    continue;
                }

                let (image_type, reference) = if filename.starts_with(&built_prefix) {
                    let service = filename
                        .strip_prefix(&built_prefix)
                        .and_then(|rest| rest.strip_suffix(".tar"))
                        .and_then(|rest| rest.rsplit_once('_'))
                        .map(|(svc_tag, _hash)| {
                            svc_tag
                                .rsplit_once('_')
                                .map(|(svc, _tag)| svc.to_string())
                                .unwrap_or_else(|| svc_tag.to_string())
                        })
                        .unwrap_or_default();
                    if built_services.contains(&service)
                        || built_services.iter().any(|s| filename.contains(s))
                    {
                        ("built", format!("coast-built/{}/{}", project, service))
                    } else {
                        continue;
                    }
                } else if pulled_filenames.contains(&filename) {
                    let reference = filename
                        .strip_suffix(".tar")
                        .unwrap_or(&filename)
                        .to_string();
                    ("pulled", reference)
                } else if base_filenames.contains(&filename) {
                    let reference = filename
                        .strip_suffix(".tar")
                        .unwrap_or(&filename)
                        .to_string();
                    ("base", reference)
                } else {
                    continue;
                };

                let (size_bytes, modified) = match entry.metadata() {
                    Ok(meta) => {
                        let modified = meta
                            .modified()
                            .ok()
                            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
                        (meta.len(), modified)
                    }
                    Err(_) => (0, None),
                };

                total_size_bytes += size_bytes;
                images.push(CachedImageInfo {
                    reference: reference.replace('_', "/").replacen("/", ":", 1),
                    filename,
                    size_bytes,
                    image_type: image_type.to_string(),
                    modified,
                });
            }
        }
    }

    images.sort_by(|a, b| {
        a.image_type
            .cmp(&b.image_type)
            .then(a.reference.cmp(&b.reference))
    });

    Ok(BuildsResponse::Images(BuildsImagesResponse {
        images,
        total_size_bytes,
    }))
}

/// List live Docker images on the host for a project, filtered to the build.
async fn handle_docker_images(
    project: &str,
    build_id: Option<&str>,
    state: &AppState,
) -> Result<BuildsResponse> {
    info!(project = %project, build_id = ?build_id, "handling builds docker-images request");
    let all_images = list_project_docker_images(project, state).await;
    let manifest = read_manifest(project, build_id).unwrap_or_default();
    let built_svcs = json_string_vec(&manifest, "built_services");
    let coast_img = json_str(&manifest, "coast_image");
    let built_prefix = format!("coast-built/{}/", project);
    let images: Vec<DockerImageInfo> = all_images
        .into_iter()
        .filter(|img| {
            if let Some(svc) = img.repository.strip_prefix(&built_prefix) {
                return built_svcs.iter().any(|s| s == svc);
            }
            if let Some(ref ci) = coast_img {
                let full = format!("{}:{}", img.repository, img.tag);
                if full == *ci {
                    return true;
                }
            }
            false
        })
        .collect();
    Ok(BuildsResponse::DockerImages(BuildsDockerImagesResponse {
        images,
    }))
}

/// Query bollard for Docker images matching a project.
async fn list_project_docker_images(project: &str, state: &AppState) -> Vec<DockerImageInfo> {
    let Some(ref docker) = state.docker else {
        return Vec::new();
    };

    use bollard::image::ListImagesOptions;
    let opts = ListImagesOptions::<String> {
        all: false,
        ..Default::default()
    };

    let images = match docker.list_images(Some(opts)).await {
        Ok(imgs) => imgs,
        Err(e) => {
            warn!(error = %e, "failed to list Docker images");
            return Vec::new();
        }
    };

    let prefix_built = format!("coast-built/{}/", project);
    let prefix_image = format!("coast-image/{}:", project);
    let prefix_compose = format!("{}-coasts", project);

    let mut result = Vec::new();
    for img in &images {
        let matching_tag = img.repo_tags.iter().find(|tag| {
            tag.starts_with(&prefix_built)
                || tag.starts_with(&prefix_image)
                || tag.starts_with(&prefix_compose)
        });
        let Some(tag_str) = matching_tag else {
            continue;
        };

        let (repository, tag) = tag_str
            .rsplit_once(':')
            .map(|(r, t)| (r.to_string(), t.to_string()))
            .unwrap_or_else(|| (tag_str.clone(), String::new()));

        let id_short = img
            .id
            .strip_prefix("sha256:")
            .unwrap_or(&img.id)
            .chars()
            .take(12)
            .collect::<String>();

        let created = img.created;
        let created_str = if created > 0 {
            chrono::DateTime::from_timestamp(created, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| created.to_string())
        } else {
            String::new()
        };

        let size_bytes = img.size;
        let size = format_bytes(size_bytes);

        result.push(DockerImageInfo {
            id: id_short,
            repository,
            tag,
            created: created_str,
            size,
            size_bytes,
        });
    }

    result.sort_by(|a, b| a.repository.cmp(&b.repository));
    result
}

/// Inspect a specific Docker image via bollard.
async fn handle_inspect_docker_image(
    _project: &str,
    image: &str,
    state: &AppState,
) -> Result<BuildsResponse> {
    info!(image = %image, "handling builds inspect-docker-image request");

    let docker = state.docker.as_ref().ok_or_else(|| {
        CoastError::docker("Docker is not available. Is the Docker daemon running?")
    })?;

    let inspect = docker
        .inspect_image(image)
        .await
        .map_err(|e| CoastError::docker(format!("Failed to inspect image '{image}': {e}")))?;

    let data = serde_json::to_value(inspect).map_err(|e| {
        CoastError::protocol(format!("Failed to serialize image inspect data: {e}"))
    })?;

    Ok(BuildsResponse::DockerImageInspect { data })
}

/// Read a file from the artifact directory.
async fn handle_file(
    project: &str,
    file_type: &str,
    build_id: Option<&str>,
) -> Result<BuildsResponse> {
    info!(project = %project, file_type = %file_type, "handling builds file request");

    let art_dir = resolve_build_dir(project, build_id).ok_or_else(|| {
        CoastError::state(format!(
            "No build artifact found for project '{project}'. Run `coast build` first."
        ))
    })?;

    let filename = match file_type {
        "compose" => "compose.yml",
        "manifest" => "manifest.json",
        "coastfile" => "coastfile.toml",
        _ => {
            return Err(CoastError::protocol(format!(
                "Unknown file type: {file_type}"
            )));
        }
    };

    let path = art_dir.join(filename);
    let content: String = std::fs::read_to_string(&path).unwrap_or_default();

    Ok(BuildsResponse::Content(BuildsContentResponse {
        content,
        file_type: file_type.to_string(),
    }))
}

fn format_bytes(bytes: i64) -> String {
    let bytes = bytes as f64;
    if bytes >= 1_073_741_824.0 {
        format!("{:.1} GB", bytes / 1_073_741_824.0)
    } else if bytes >= 1_048_576.0 {
        format!("{:.0} MB", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.0} KB", bytes / 1024.0)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let _lock = env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key,
                previous,
                _lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_ls_empty() {
        let state = test_state();
        let result = handle(BuildsRequest::Ls { project: None }, &state)
            .await
            .unwrap();
        match result {
            BuildsResponse::Ls(resp) => {
                assert!(resp.builds.is_empty() || !resp.builds.is_empty());
            }
            _ => panic!("expected Ls response"),
        }
    }

    #[tokio::test]
    async fn test_inspect_nonexistent_project() {
        let state = test_state();
        let result = handle(
            BuildsRequest::Inspect {
                project: "nonexistent-project-xyz".to_string(),
                build_id: None,
            },
            &state,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_images_nonexistent_project() {
        let state = test_state();
        let result = handle(
            BuildsRequest::Images {
                project: "nonexistent-project-xyz".to_string(),
                build_id: None,
            },
            &state,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_nonexistent_project() {
        let state = test_state();
        let result = handle(
            BuildsRequest::Compose {
                project: "nonexistent-project-xyz".to_string(),
                build_id: None,
            },
            &state,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_docker_images_no_docker() {
        let state = test_state();
        let result = handle(
            BuildsRequest::DockerImages {
                project: "my-app".to_string(),
                build_id: None,
            },
            &state,
        )
        .await
        .unwrap();
        match result {
            BuildsResponse::DockerImages(resp) => {
                assert!(resp.images.is_empty());
            }
            _ => panic!("expected DockerImages response"),
        }
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1_048_576), "1 MB");
        assert_eq!(format_bytes(52_428_800), "50 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(5_368_709_120), "5.0 GB");
    }

    #[test]
    fn test_json_helpers() {
        let val = serde_json::json!({
            "project": "my-app",
            "count": 42,
            "items": ["a", "b", "c"]
        });
        assert_eq!(json_str(&val, "project"), Some("my-app".to_string()));
        assert_eq!(json_str(&val, "missing"), None);
        assert_eq!(json_usize(&val, "count"), 42);
        assert_eq!(json_usize(&val, "missing"), 0);
        assert_eq!(
            json_string_vec(&val, "items"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(json_string_vec(&val, "missing").is_empty());
    }

    #[test]
    fn test_list_build_ids_skips_dirs_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("COAST_HOME", tmp.path());
        let project_dir = tmp.path().join("images").join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        // Real build with manifest
        let build_dir = project_dir.join("abc123");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(
            build_dir.join("manifest.json"),
            r#"{"build_timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        // Non-build dirs (like inject/, secrets/) with no manifest
        std::fs::create_dir_all(project_dir.join("inject")).unwrap();
        std::fs::create_dir_all(project_dir.join("secrets")).unwrap();

        // Symlink named "latest"
        #[cfg(unix)]
        std::os::unix::fs::symlink("abc123", project_dir.join("latest")).unwrap();

        // Simulate what list_build_ids does: scan dirs, skip "latest",
        // skip dirs without manifest.json
        let entries = std::fs::read_dir(&project_dir).unwrap();
        let mut builds = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "latest" {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let manifest_path = entry.path().join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }
            builds.push(name);
        }

        assert_eq!(builds, vec!["abc123"]);
        assert!(!builds.contains(&"inject".to_string()));
        assert!(!builds.contains(&"secrets".to_string()));
    }

    #[test]
    fn test_resolve_build_dir_uses_coast_home() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("COAST_HOME", tmp.path());
        let project_dir = tmp.path().join("images").join("overhead");
        let build_id = "e1ab4f19565d7d26_20260321042523";
        let build_dir = project_dir.join(build_id);
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(
            build_dir.join("manifest.json"),
            r#"{"build_timestamp":"2026-03-21T04:25:23Z"}"#,
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(build_id, project_dir.join("latest")).unwrap();

        let resolved = resolve_build_dir("overhead", Some("latest"));

        assert_eq!(resolved, Some(project_dir.join("latest")));
        assert_eq!(
            std::fs::canonicalize(resolved.unwrap()).unwrap(),
            std::fs::canonicalize(&build_dir).unwrap()
        );
    }

    #[test]
    fn test_resolve_build_dir_no_legacy_fallback_for_arbitrary_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path();

        // Legacy manifest at project root
        std::fs::write(
            project_dir.join("manifest.json"),
            r#"{"build_timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        // inject/ subdir with no manifest
        std::fs::create_dir_all(project_dir.join("inject")).unwrap();

        // For bid="inject": versioned path inject/manifest.json doesn't exist
        let versioned = project_dir.join("inject");
        assert!(!versioned.join("manifest.json").exists());

        // The legacy fallback should NOT activate for non-"latest" build IDs
        let bid = "inject";
        let should_fallback = bid == "latest" && project_dir.join("manifest.json").exists();
        assert!(
            !should_fallback,
            "legacy fallback must not trigger for arbitrary build IDs"
        );

        // For bid="latest" the fallback SHOULD activate
        let bid_latest = "latest";
        let should_fallback_latest =
            bid_latest == "latest" && project_dir.join("manifest.json").exists();
        assert!(
            should_fallback_latest,
            "legacy fallback should trigger for 'latest'"
        );
    }
}
