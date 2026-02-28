/// Handler for the `coast rm-build` command.
///
/// Removes a project's build artifact directory and prunes associated
/// Docker resources: stopped containers, volumes, and images.
use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, RmBuildRequest, RmBuildResponse};

use crate::server::AppState;

fn emit(
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
    event: BuildProgressEvent,
) {
    if let Some(tx) = progress {
        let _ = tx.try_send(event);
    }
}

/// Handle an rm-build request with optional streaming progress.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: RmBuildRequest,
    state: &AppState,
    progress: Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<RmBuildResponse> {
    if !req.build_ids.is_empty() {
        return handle_remove_specific_builds(req, state, progress).await;
    }

    let total = 6u32;
    let steps = vec![
        "Validating".to_string(),
        "Removing containers".to_string(),
        "Removing volumes".to_string(),
        "Removing images".to_string(),
        "Removing artifact directory".to_string(),
        "Cleaning DB records".to_string(),
    ];
    emit(&progress, BuildProgressEvent::build_plan(steps));

    info!(project = %req.project, "handling rm-build request (full project removal)");

    emit(
        &progress,
        BuildProgressEvent::started("Validating", 1, total),
    );
    {
        let db = state.db.lock().await;
        let instances = db.list_instances_for_project(&req.project)?;
        if !instances.is_empty() {
            return Err(CoastError::state(format!(
                "Cannot remove build for '{}': {} instance(s) still exist. \
                 Run `coast rm --all --project {}` first.",
                req.project,
                instances.len(),
                req.project,
            )));
        }
        let shared = db.list_shared_services(Some(&req.project))?;
        let running: Vec<_> = shared.iter().filter(|s| s.status == "running").collect();
        if !running.is_empty() {
            let names: Vec<&str> = running.iter().map(|s| s.service_name.as_str()).collect();
            return Err(CoastError::state(format!(
                "Cannot remove build for '{}': {} shared service(s) still running ({}). \
                 Run `coast shared-services stop --all --project {}` first.",
                req.project,
                running.len(),
                names.join(", "),
                req.project,
            )));
        }
    }
    emit(&progress, BuildProgressEvent::ok("Validating", 1, total));

    let project = req.project.clone();

    state.emit_event(CoastEvent::BuildRemoving {
        project: project.clone(),
        build_ids: Vec::new(),
    });

    let (containers_removed, volumes_removed, images_removed) = match &state.docker {
        Some(docker) => {
            emit(
                &progress,
                BuildProgressEvent::started("Removing containers", 2, total),
            );
            let c = remove_project_containers(docker, &project).await;
            emit(
                &progress,
                BuildProgressEvent::ok_with_detail(
                    "Removing containers",
                    2,
                    total,
                    format!("{c} removed"),
                ),
            );

            emit(
                &progress,
                BuildProgressEvent::started("Removing volumes", 3, total),
            );
            let v = remove_project_volumes(docker, &project).await;
            emit(
                &progress,
                BuildProgressEvent::ok_with_detail(
                    "Removing volumes",
                    3,
                    total,
                    format!("{v} removed"),
                ),
            );

            emit(
                &progress,
                BuildProgressEvent::started("Removing images", 4, total),
            );
            let i = remove_project_images(docker, &project).await;
            emit(
                &progress,
                BuildProgressEvent::ok_with_detail(
                    "Removing images",
                    4,
                    total,
                    format!("{i} removed"),
                ),
            );

            (c, v, i)
        }
        None => {
            emit(
                &progress,
                BuildProgressEvent::skip("Removing containers", 2, total),
            );
            emit(
                &progress,
                BuildProgressEvent::skip("Removing volumes", 3, total),
            );
            emit(
                &progress,
                BuildProgressEvent::skip("Removing images", 4, total),
            );
            warn!("Docker client not available, skipping resource cleanup");
            (0, 0, 0)
        }
    };

    emit(
        &progress,
        BuildProgressEvent::started("Removing artifact directory", 5, total),
    );
    let artifact_removed = remove_artifact_dir(&project);
    emit(
        &progress,
        BuildProgressEvent::ok("Removing artifact directory", 5, total),
    );

    emit(
        &progress,
        BuildProgressEvent::started("Cleaning DB records", 6, total),
    );
    {
        let db = state.db.lock().await;
        if let Err(e) = db.delete_shared_services_for_project(&project) {
            warn!(project = %project, error = %e, "failed to clean shared service records");
        }
    }
    emit(
        &progress,
        BuildProgressEvent::ok("Cleaning DB records", 6, total),
    );

    info!(
        project = %project,
        containers = containers_removed,
        volumes = volumes_removed,
        images = images_removed,
        artifact = artifact_removed,
        "rm-build complete"
    );

    Ok(RmBuildResponse {
        project: req.project,
        containers_removed,
        volumes_removed,
        images_removed,
        artifact_removed,
        builds_removed: 0,
    })
}

/// Remove specific builds by ID (just their directories and image tags).
#[allow(clippy::cognitive_complexity)]
async fn handle_remove_specific_builds(
    req: RmBuildRequest,
    state: &AppState,
    progress: Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
) -> Result<RmBuildResponse> {
    let project = &req.project;
    info!(project = %project, build_ids = ?req.build_ids, "handling rm-build for specific builds");

    let build_count = req.build_ids.len() as u32;
    let total = 1 + build_count;
    let mut steps = vec!["Validating builds".to_string()];
    for bid in &req.build_ids {
        steps.push(format!("Removing {bid}"));
    }
    emit(&progress, BuildProgressEvent::build_plan(steps));

    emit(
        &progress,
        BuildProgressEvent::started("Validating builds", 1, total),
    );

    let Some(home) = dirs::home_dir() else {
        return Err(CoastError::io(
            "Could not determine home directory",
            "rm-build",
        ));
    };
    let project_dir = home.join(".coast").join("images").join(project);

    let latest_target = std::fs::read_link(project_dir.join("latest"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));

    let in_use: std::collections::HashMap<String, Vec<String>> = {
        let db = state.db.lock().await;
        let instances = db.list_instances_for_project(project).unwrap_or_default();
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut null_build_names: Vec<String> = Vec::new();
        for inst in instances {
            if let Some(bid) = inst.build_id {
                map.entry(bid).or_default().push(inst.name);
            } else {
                null_build_names.push(inst.name);
            }
        }
        if !null_build_names.is_empty() {
            if let Some(ref lt) = latest_target {
                map.entry(lt.clone()).or_default().extend(null_build_names);
            }
        }
        map
    };
    emit(
        &progress,
        BuildProgressEvent::ok("Validating builds", 1, total),
    );

    state.emit_event(CoastEvent::BuildRemoving {
        project: project.clone(),
        build_ids: req.build_ids.clone(),
    });

    let mut builds_removed = 0usize;
    let mut images_removed = 0usize;

    for (idx, build_id) in req.build_ids.iter().enumerate() {
        let step_num = 2 + idx as u32;
        let step_name = format!("Removing {build_id}");

        if let Some(instance_names) = in_use.get(build_id.as_str()) {
            warn!(
                build_id = %build_id,
                instances = ?instance_names,
                "skipping removal of build — in use by running instance(s)"
            );
            emit(
                &progress,
                BuildProgressEvent::skip(&step_name, step_num, total),
            );
            continue;
        }

        emit(
            &progress,
            BuildProgressEvent::started(&step_name, step_num, total),
        );

        let is_latest = latest_target.as_deref() == Some(build_id.as_str());

        let build_dir = project_dir.join(build_id);
        if build_dir.exists() {
            match std::fs::remove_dir_all(&build_dir) {
                Ok(_) => {
                    info!(build_id = %build_id, "removed build directory");
                    builds_removed += 1;

                    if is_latest {
                        let symlink_path = project_dir.join("latest");
                        if symlink_path.symlink_metadata().is_ok() {
                            let _ = std::fs::remove_file(&symlink_path);
                            info!("removed stale 'latest' symlink after deleting latest build");
                        }
                    }
                }
                Err(e) => {
                    warn!(build_id = %build_id, error = %e, "failed to remove build directory");
                }
            }
        }

        if let Some(docker) = &state.docker {
            let tag = format!("coast-image/{}:{}", project, build_id);
            let rm_opts = bollard::image::RemoveImageOptions {
                force: false,
                noprune: false,
            };
            if docker.remove_image(&tag, Some(rm_opts), None).await.is_ok() {
                images_removed += 1;
                info!(tag = %tag, "removed Docker image tag");
            }
        }

        emit(
            &progress,
            BuildProgressEvent::ok(&step_name, step_num, total),
        );
    }

    state.emit_event(CoastEvent::BuildRemoved {
        project: project.clone(),
        build_ids: req.build_ids.clone(),
    });

    Ok(RmBuildResponse {
        project: req.project,
        containers_removed: 0,
        volumes_removed: 0,
        images_removed,
        artifact_removed: false,
        builds_removed,
    })
}

/// Remove all containers labelled with `coast.project={project}`.
async fn remove_project_containers(docker: &bollard::Docker, project: &str) -> usize {
    use bollard::container::{ListContainersOptions, RemoveContainerOptions};
    use std::collections::HashMap;

    let label_filter = format!("coast.project={project}");
    let mut filters = HashMap::new();
    filters.insert("label", vec![label_filter.as_str()]);

    let opts = ListContainersOptions {
        all: true,
        filters,
        ..Default::default()
    };

    let containers = match docker.list_containers(Some(opts)).await {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to list containers for rm-build");
            return 0;
        }
    };

    let mut count = 0;
    for container in &containers {
        let id = match &container.id {
            Some(id) => id.clone(),
            None => continue,
        };
        let rm_opts = RemoveContainerOptions {
            force: true,
            v: true,
            ..Default::default()
        };
        match docker.remove_container(&id, Some(rm_opts)).await {
            Ok(_) => count += 1,
            Err(e) => warn!(container = %id, error = %e, "failed to remove container"),
        }
    }
    count
}

/// Remove Docker volumes matching project naming patterns.
async fn remove_project_volumes(docker: &bollard::Docker, project: &str) -> usize {
    use bollard::volume::ListVolumesOptions;
    use std::collections::HashMap;

    let shared_prefix = format!("coast-shared--{project}--");
    let compose_prefix = format!("{project}-coasts");
    let shared_svc_prefix = format!("{project}-shared-services");

    let opts = ListVolumesOptions::<String> {
        filters: HashMap::new(),
    };

    let volumes = match docker.list_volumes(Some(opts)).await {
        Ok(v) => v.volumes.unwrap_or_default(),
        Err(e) => {
            warn!(error = %e, "failed to list volumes for rm-build");
            return 0;
        }
    };

    let mut count = 0;
    for vol in &volumes {
        let name = &vol.name;

        if name.starts_with(&shared_prefix)
            || name.contains(&compose_prefix)
            || name.contains(&shared_svc_prefix)
        {
            // Shared volumes and compose-project volumes are always ours
        } else if name.starts_with("coast--") {
            // Isolated volumes — check labels to verify project ownership
            let project_match = vol
                .labels
                .get("coast.project")
                .map(|p| p == project)
                .unwrap_or(false);
            if !project_match {
                continue;
            }
        } else {
            continue;
        }

        match docker.remove_volume(name, None).await {
            Ok(_) => count += 1,
            Err(e) => warn!(volume = %name, error = %e, "failed to remove volume"),
        }
    }
    count
}

/// Remove Docker images matching `coast-image/{project}:*` or `{project}-coasts-*`.
async fn remove_project_images(docker: &bollard::Docker, project: &str) -> usize {
    use bollard::image::{ListImagesOptions, RemoveImageOptions};
    use std::collections::HashMap;

    let opts = ListImagesOptions::<String> {
        all: false,
        filters: HashMap::new(),
        ..Default::default()
    };

    let images = match docker.list_images(Some(opts)).await {
        Ok(imgs) => imgs,
        Err(e) => {
            warn!(error = %e, "failed to list images for rm-build");
            return 0;
        }
    };

    let prefix_a = format!("coast-image/{}:", project);
    let prefix_b = format!("{}-coasts", project);

    let mut count = 0;
    for img in &images {
        let matches = img
            .repo_tags
            .iter()
            .any(|tag| tag.starts_with(&prefix_a) || tag.starts_with(&prefix_b));
        if !matches {
            continue;
        }
        let rm_opts = RemoveImageOptions {
            force: true,
            noprune: false,
        };
        match docker.remove_image(&img.id, Some(rm_opts), None).await {
            Ok(_) => count += 1,
            Err(e) => warn!(image = %img.id, error = %e, "failed to remove image"),
        }
    }
    count
}

/// Remove the build artifact directory at ~/.coast/images/{project}/.
fn remove_artifact_dir(project: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let artifact_dir = home.join(".coast").join("images").join(project);
    if !artifact_dir.exists() {
        return false;
    }
    match std::fs::remove_dir_all(&artifact_dir) {
        Ok(_) => {
            info!(path = %artifact_dir.display(), "removed artifact directory");
            true
        }
        Err(e) => {
            warn!(path = %artifact_dir.display(), error = %e, "failed to remove artifact directory");
            false
        }
    }
}
