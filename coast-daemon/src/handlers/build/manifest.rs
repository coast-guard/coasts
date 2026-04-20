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
    /// Original coastfile path used for the build (None for coastfile-less builds).
    pub coastfile_path: Option<&'a std::path::Path>,
}

pub(super) async fn write_manifest_and_finalize(input: ManifestInput<'_>) -> Result<()> {
    emit(input.progress, input.plan.started("Writing manifest"));

    // Phase 7: when the consumer Coastfile references SSG services,
    // snapshot the active SSG's build_id + image refs so `coast run`
    // can detect drift. See `coast-ssg/DESIGN.md §6.1`.
    let ssg_block = build_ssg_manifest_block(input.coastfile);

    let mut manifest = serde_json::json!({
        "build_id": &input.artifact.build_id,
        "project": &input.coastfile.name,
        "coastfile_type": &input.coastfile.coastfile_type,
        "arch": std::env::consts::ARCH,
        "project_root": input.coastfile.project_root.display().to_string(),
        "coastfile_path": input.coastfile_path.map(|p| p.display().to_string()),
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
    if let Some(block) = ssg_block {
        manifest["ssg"] = block;
    }
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

/// Compute the Phase 7 `ssg` manifest block for this coast build, or
/// `None` when drift detection doesn't apply.
///
/// Returns `None` when the Coastfile has no `from_group = true`
/// references (non-consumer build) or when no active SSG build
/// exists (coast build shouldn't block on a missing SSG; the
/// run-side check will auto-start / error using the §11.1 path).
///
/// When present, the block records only the services the consumer
/// actually references — keeps the snapshot minimal and makes
/// "service missing" a clean check.
fn build_ssg_manifest_block(coastfile: &Coastfile) -> Option<serde_json::Value> {
    if coastfile.shared_service_group_refs.is_empty() {
        return None;
    }

    let build_id = coast_ssg::paths::resolve_latest_build_id()?;
    let build_dir = coast_ssg::paths::ssg_build_dir(&build_id).ok()?;
    let manifest_path = build_dir.join("manifest.json");
    let manifest_contents = std::fs::read_to_string(&manifest_path).ok()?;
    let active: coast_ssg::build::artifact::SsgManifest =
        serde_json::from_str(&manifest_contents).ok()?;

    let referenced: Vec<String> = coastfile
        .shared_service_group_refs
        .iter()
        .map(|r| r.name.clone())
        .collect();

    let mut images = std::collections::BTreeMap::new();
    for name in &referenced {
        if let Some(svc) = active.services.iter().find(|s| s.name == *name) {
            images.insert(name.clone(), svc.image.clone());
        }
    }

    Some(serde_json::json!({
        "build_id": active.build_id,
        "services": referenced,
        "images": images,
    }))
}

#[cfg(test)]
mod ssg_block_tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    /// Serialize tests that mutate COAST_HOME across files.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_coast_home<F: FnOnce(&Path)>(f: F) {
        let guard = ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("COAST_HOME", tmp.path());
        }
        f(tmp.path());
        unsafe {
            match prev {
                Some(v) => std::env::set_var("COAST_HOME", v),
                None => std::env::remove_var("COAST_HOME"),
            }
        }
        drop(guard);
    }

    fn consumer_coastfile(with_ssg_ref: bool) -> Coastfile {
        let body = if with_ssg_ref {
            r#"
[coast]
name = "consumer"
compose = "./docker-compose.yml"

[shared_services.postgres]
from_group = true
"#
        } else {
            r#"
[coast]
name = "non-consumer"
compose = "./docker-compose.yml"
"#
        };
        Coastfile::parse(body, Path::new("/tmp/phase7-manifest-test")).unwrap()
    }

    #[test]
    fn block_is_none_without_ssg_refs() {
        with_coast_home(|_home| {
            let cf = consumer_coastfile(false);
            assert!(build_ssg_manifest_block(&cf).is_none());
        });
    }

    #[test]
    fn block_is_none_when_no_active_ssg_build() {
        with_coast_home(|_home| {
            // COAST_HOME is a fresh tempdir with no SSG artifacts; the
            // helper must silently return None so `coast build` doesn't
            // block users who haven't built their SSG yet.
            let cf = consumer_coastfile(true);
            assert!(build_ssg_manifest_block(&cf).is_none());
        });
    }

    #[test]
    fn block_populated_when_active_ssg_has_referenced_service() {
        with_coast_home(|home| {
            // Hand-roll a minimal SSG artifact tree matching
            // `coast_ssg::paths`:
            //   ~/.coast/ssg/builds/{bid}/manifest.json
            //   ~/.coast/ssg/latest        -> builds/{bid}
            let build_id = "fake-hash_20260101010101";
            let ssg_home = home.join("ssg");
            let build_dir = ssg_home.join("builds").join(build_id);
            std::fs::create_dir_all(&build_dir).unwrap();
            let manifest = serde_json::json!({
                "build_id": build_id,
                "built_at": "2026-04-20T00:00:00Z",
                "coastfile_hash": "fake-hash",
                "services": [{
                    "name": "postgres",
                    "image": "postgres:16-alpine",
                    "ports": [5432],
                    "env_keys": ["POSTGRES_PASSWORD"],
                    "volumes": [],
                    "auto_create_db": false,
                }],
            });
            std::fs::write(
                build_dir.join("manifest.json"),
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            // latest symlink lives at ~/.coast/ssg/latest.
            let latest = ssg_home.join("latest");
            let _ = std::fs::remove_file(&latest);
            #[cfg(unix)]
            std::os::unix::fs::symlink(Path::new("builds").join(build_id), &latest).unwrap();

            let cf = consumer_coastfile(true);
            let block = build_ssg_manifest_block(&cf).expect("expected block");
            assert_eq!(block["build_id"], build_id);
            assert_eq!(block["services"][0], "postgres");
            assert_eq!(block["images"]["postgres"], "postgres:16-alpine");
        });
    }
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
