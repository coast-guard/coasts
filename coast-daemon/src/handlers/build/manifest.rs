use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::VolumeStrategy;

use crate::server::AppState;

use super::artifact::ArtifactOutput;
use super::emit;
use super::images::ImageBuildOutput;
use super::plan::BuildPlan;
use super::utils::auto_prune_builds;

pub(super) struct ManifestInput<'a> {
    pub coastfile: &'a Coastfile,
    pub artifact: &'a ArtifactOutput,
    pub images: &'a ImageBuildOutput,
    pub coast_image: &'a Option<String>,
    pub state: &'a AppState,
    pub progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
    pub plan: &'a BuildPlan,
}

pub(super) async fn write_manifest_and_finalize(input: ManifestInput<'_>) -> Result<()> {
    emit(input.progress, input.plan.started("Writing manifest"));

    let manifest = serde_json::json!({
        "build_id": &input.artifact.build_id,
        "project": &input.coastfile.name,
        "coastfile_type": &input.coastfile.coastfile_type,
        "arch": std::env::consts::ARCH,
        "project_root": input.coastfile.project_root.display().to_string(),
        "build_timestamp": input.artifact.build_timestamp.to_rfc3339(),
        "coastfile_hash": input.artifact.coastfile_hash,
        "images_cached": input.images.images_cached,
        "images_built": input.images.images_built,
        "coast_image": input.coast_image,
        "secrets": input
            .coastfile
            .secrets
            .iter()
            .map(|secret| &secret.name)
            .collect::<Vec<_>>(),
        "built_services": &input.images.built_services,
        "pulled_images": &input.images.pulled_images,
        "base_images": &input.images.base_images,
        "omitted_services": &input.coastfile.omit.services,
        "omitted_volumes": &input.coastfile.omit.volumes,
        "mcp_servers": input.coastfile.mcp_servers.iter().map(|mcp| {
            serde_json::json!({
                "name": mcp.name,
                "proxy": mcp.proxy.as_ref().map(coast_core::types::McpProxyMode::as_str),
                "command": mcp.command,
                "args": mcp.args,
            })
        }).collect::<Vec<_>>(),
        "mcp_clients": input.coastfile.mcp_clients.iter().map(|client| {
            serde_json::json!({
                "name": client.name,
                "format": client.format.as_ref().map(coast_core::types::McpClientFormat::as_str),
                "config_path": client.resolved_config_path(),
            })
        }).collect::<Vec<_>>(),
        "shared_services": input.coastfile.shared_services.iter().map(|service| {
            serde_json::json!({
                "name": service.name,
                "image": service.image,
                "ports": service.ports,
                "auto_create_db": service.auto_create_db,
            })
        }).collect::<Vec<_>>(),
        "volumes": input.coastfile.volumes.iter().map(|volume| {
            serde_json::json!({
                "name": volume.name,
                "strategy": match volume.strategy {
                    VolumeStrategy::Isolated => "isolated",
                    VolumeStrategy::Shared => "shared",
                },
                "service": volume.service,
                "mount": volume.mount.display().to_string(),
                "snapshot_source": volume.snapshot_source,
            })
        }).collect::<Vec<_>>(),
        "agent_shell": input.coastfile.agent_shell.as_ref().map(|agent_shell| {
            serde_json::json!({ "command": agent_shell.command })
        }),
        "primary_port": &input.coastfile.primary_port,
    });
    let manifest_path = input.artifact.artifact_path.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| CoastError::protocol(format!("failed to serialize manifest: {error}")))?;
    std::fs::write(&manifest_path, manifest_json).map_err(|error| CoastError::Io {
        message: format!("failed to write manifest.json: {error}"),
        path: manifest_path,
        source: Some(error),
    })?;

    store_primary_port_setting(&input).await?;
    update_latest_symlink(&input)?;
    prune_old_builds(&input).await;

    emit(
        input.progress,
        BuildProgressEvent::done("Writing manifest", "ok"),
    );

    Ok(())
}

async fn store_primary_port_setting(input: &ManifestInput<'_>) -> Result<()> {
    let primary = input.coastfile.primary_port.clone().or_else(|| {
        if input.coastfile.ports.len() == 1 {
            input.coastfile.ports.keys().next().cloned()
        } else {
            None
        }
    });
    if let Some(ref service) = primary {
        let db = input.state.db.lock().await;
        let key = format!(
            "primary_port:{}:{}",
            input.coastfile.name, input.artifact.build_id
        );
        db.set_setting(&key, service)?;
    }
    Ok(())
}

fn update_latest_symlink(input: &ManifestInput<'_>) -> Result<()> {
    let latest_name = match &input.coastfile.coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = input.artifact.project_dir.join(&latest_name);
    let _ = std::fs::remove_file(&latest_link);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&input.artifact.build_id, &latest_link).map_err(|error| {
            CoastError::Io {
                message: format!("failed to create '{}' symlink: {error}", latest_name),
                path: latest_link.clone(),
                source: Some(error),
            }
        })?;
    }
    Ok(())
}

async fn prune_old_builds(input: &ManifestInput<'_>) {
    let in_use_build_ids: std::collections::HashSet<String> = {
        let db = input.state.db.lock().await;
        let instances = db
            .list_instances_for_project(&input.coastfile.name)
            .unwrap_or_default();
        let has_null_build_id = instances.iter().any(|instance| instance.build_id.is_none());
        let mut ids: std::collections::HashSet<String> = instances
            .into_iter()
            .filter_map(|instance| instance.build_id)
            .collect();
        if has_null_build_id {
            if let Ok(target) = std::fs::read_link(input.artifact.project_dir.join("latest")) {
                if let Some(name) = target.file_name() {
                    ids.insert(name.to_string_lossy().into_owned());
                }
            }
        }
        ids
    };
    auto_prune_builds(
        &input.artifact.project_dir,
        5,
        &in_use_build_ids,
        input.coastfile.coastfile_type.as_deref(),
    );
}
