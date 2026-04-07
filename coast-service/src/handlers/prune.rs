use tracing::info;

use coast_core::error::Result;
use coast_core::protocol::api_types::{PruneItem, PruneRequest, PruneResponse};

use crate::state::ServiceState;

pub async fn handle(req: PruneRequest, state: &ServiceState) -> Result<PruneResponse> {
    info!(dry_run = req.dry_run, "prune request");

    let active_keys = load_active_keys(state).await;
    let mut items = Vec::new();

    collect_orphaned_volumes(state, &active_keys, req.dry_run, &mut items).await;
    collect_orphaned_workspaces(&active_keys, req.dry_run, &mut items);

    let total_freed_bytes = if req.dry_run {
        0
    } else {
        items.iter().map(|i| i.size_bytes).sum()
    };

    Ok(PruneResponse {
        items,
        total_freed_bytes,
        dry_run: req.dry_run,
    })
}

async fn load_active_keys(state: &ServiceState) -> std::collections::HashSet<(String, String)> {
    let db = state.db.lock().await;
    db.list_all_instances()
        .unwrap_or_default()
        .iter()
        .map(|i| (i.project.clone(), i.name.clone()))
        .collect()
}

async fn collect_orphaned_volumes(
    state: &ServiceState,
    active_keys: &std::collections::HashSet<(String, String)>,
    dry_run: bool,
    items: &mut Vec<PruneItem>,
) {
    let Some(ref docker) = state.docker else {
        return;
    };
    let Ok(volumes) = docker.list_volumes::<String>(None).await else {
        return;
    };
    let Some(vols) = volumes.volumes else {
        return;
    };

    for vol in vols {
        let Some((project, instance)) = parse_dind_volume_name(&vol.name) else {
            continue;
        };
        if active_keys.contains(&(project, instance)) {
            continue;
        }
        let size = vol
            .usage_data
            .as_ref()
            .map(|u| u.size.max(0) as u64)
            .unwrap_or(0);
        items.push(PruneItem {
            kind: "volume".to_string(),
            name: vol.name.clone(),
            size_bytes: size,
        });
        if !dry_run {
            match docker.remove_volume(&vol.name, None).await {
                Ok(()) => info!(volume = %vol.name, "pruned orphaned volume"),
                Err(e) => info!(volume = %vol.name, error = %e, "failed to prune volume"),
            }
        }
    }
}

fn collect_orphaned_workspaces(
    active_keys: &std::collections::HashSet<(String, String)>,
    dry_run: bool,
    items: &mut Vec<PruneItem>,
) {
    let ws_root = crate::state::service_home().join("workspaces");
    let Ok(projects) = std::fs::read_dir(&ws_root) else {
        return;
    };

    for project_entry in projects.flatten() {
        let project = project_entry.file_name().to_string_lossy().to_string();
        let Ok(instances) = std::fs::read_dir(project_entry.path()) else {
            continue;
        };
        for inst_entry in instances.flatten() {
            let name = inst_entry.file_name().to_string_lossy().to_string();
            if name == "build" {
                continue;
            }
            if active_keys.contains(&(project.clone(), name)) {
                continue;
            }
            let size = dir_size(&inst_entry.path());
            let path_str = inst_entry.path().display().to_string();
            items.push(PruneItem {
                kind: "workspace".to_string(),
                name: path_str.clone(),
                size_bytes: size,
            });
            if !dry_run {
                let _ = std::fs::remove_dir_all(&path_str);
                info!(path = %path_str, "pruned orphaned workspace");
            }
        }
    }
}

fn parse_dind_volume_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("coast-dind--")?;
    let parts: Vec<&str> = rest.splitn(2, "--").collect();
    if parts.len() == 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                total += dir_size(&entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dind_volume_name() {
        assert_eq!(
            parse_dind_volume_name("coast-dind--cg--x5"),
            Some(("cg".to_string(), "x5".to_string()))
        );
        assert_eq!(
            parse_dind_volume_name("coast-dind--my-app--feature-1"),
            Some(("my-app".to_string(), "feature-1".to_string()))
        );
        assert_eq!(parse_dind_volume_name("some-other-volume"), None);
        assert_eq!(parse_dind_volume_name("coast-dind--noinstance"), None);
    }

    #[test]
    fn test_active_keys_skip_orphan_detection() {
        let mut active = std::collections::HashSet::new();
        active.insert(("cg".to_string(), "dev-1".to_string()));

        assert!(parse_dind_volume_name("coast-dind--cg--dev-1")
            .map(|k| active.contains(&k))
            .unwrap_or(false),);
        assert!(parse_dind_volume_name("coast-dind--cg--x5")
            .map(|k| !active.contains(&k))
            .unwrap_or(false),);
    }

    #[test]
    fn test_build_directory_skipped() {
        let active = std::collections::HashSet::new();
        let mut items = Vec::new();
        collect_orphaned_workspaces(&active, true, &mut items);
        for item in &items {
            assert!(
                !item.name.ends_with("/build"),
                "build directory should never be pruned"
            );
        }
    }
}
