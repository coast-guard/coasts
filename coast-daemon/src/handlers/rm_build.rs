/// Handler for the `coast rm-build` command.
///
/// Removes a project's build artifact directory and prunes associated
/// Docker resources: stopped containers, volumes, and images.
use tracing::{info, warn};

use coast_core::artifact::coast_home;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, CoastEvent, RmBuildRequest, RmBuildResponse};

use coast_core::types::CoastInstance;

use crate::server::AppState;
use crate::state::StateDb;

fn emit(
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
    event: BuildProgressEvent,
) {
    if let Some(tx) = progress {
        let _ = tx.try_send(event);
    }
}

/// Handle an rm-build request with optional streaming progress.
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
        validate_removable(&db, &req.project)?;
    }
    emit(&progress, BuildProgressEvent::ok("Validating", 1, total));

    let project = req.project.clone();

    state.emit_event(CoastEvent::BuildRemoving {
        project: project.clone(),
        build_ids: Vec::new(),
    });

    let (containers_removed, volumes_removed, images_removed) = {
        let docker = state.docker.as_ref();
        remove_docker_resources(docker.as_ref(), &project, &progress, total).await
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

    let project_dir = coast_home()
        .map(|home| home.join("images").join(project))
        .map_err(|_| CoastError::io("Could not determine Coast home directory", "rm-build"))?;

    let latest_target = std::fs::read_link(project_dir.join("latest"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()));

    let in_use = {
        let db = state.db.lock().await;
        let instances = db.list_instances_for_project(project).unwrap_or_default();
        build_in_use_map(&instances, latest_target.as_deref())
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
        if remove_build_dir(&project_dir, build_id, is_latest) {
            builds_removed += 1;
        }
        if let Some(docker) = state.docker.as_ref() {
            if remove_build_image(&docker, project, build_id).await {
                images_removed += 1;
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

/// Check that no instances or running shared services block build removal.
fn validate_removable(db: &StateDb, project: &str) -> Result<()> {
    let instances = db.list_instances_for_project(project)?;
    if !instances.is_empty() {
        return Err(CoastError::state(format!(
            "Cannot remove build for '{}': {} instance(s) still exist. \
             Run `coast rm --all --project {}` first.",
            project,
            instances.len(),
            project,
        )));
    }
    let shared = db.list_shared_services(Some(project))?;
    let running: Vec<_> = shared.iter().filter(|s| s.status == "running").collect();
    if !running.is_empty() {
        let names: Vec<&str> = running.iter().map(|s| s.service_name.as_str()).collect();
        return Err(CoastError::state(format!(
            "Cannot remove build for '{}': {} shared service(s) still running ({}). \
             Run `coast shared-services stop --all --project {}` first.",
            project,
            running.len(),
            names.join(", "),
            project,
        )));
    }
    Ok(())
}

/// Build a map from build ID to the instance names using that build.
///
/// Instances without a `build_id` are attributed to `latest_target` (the
/// symlink target of the "latest" alias) when present.
fn build_in_use_map(
    instances: &[CoastInstance],
    latest_target: Option<&str>,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut null_build_names: Vec<String> = Vec::new();
    for inst in instances {
        if let Some(ref bid) = inst.build_id {
            map.entry(bid.clone()).or_default().push(inst.name.clone());
        } else {
            null_build_names.push(inst.name.clone());
        }
    }
    if !null_build_names.is_empty() {
        if let Some(lt) = latest_target {
            map.entry(lt.to_string())
                .or_default()
                .extend(null_build_names);
        }
    }
    map
}

/// Remove Docker containers, volumes, and images for a project, with progress.
async fn remove_docker_resources(
    docker: Option<&bollard::Docker>,
    project: &str,
    progress: &Option<tokio::sync::mpsc::Sender<BuildProgressEvent>>,
    total: u32,
) -> (usize, usize, usize) {
    let Some(docker) = docker else {
        emit(
            progress,
            BuildProgressEvent::skip("Removing containers", 2, total),
        );
        emit(
            progress,
            BuildProgressEvent::skip("Removing volumes", 3, total),
        );
        emit(
            progress,
            BuildProgressEvent::skip("Removing images", 4, total),
        );
        warn!("Docker client not available, skipping resource cleanup");
        return (0, 0, 0);
    };

    emit(
        progress,
        BuildProgressEvent::started("Removing containers", 2, total),
    );
    let c = remove_project_containers(docker, project).await;
    emit(
        progress,
        BuildProgressEvent::ok_with_detail("Removing containers", 2, total, format!("{c} removed")),
    );

    emit(
        progress,
        BuildProgressEvent::started("Removing volumes", 3, total),
    );
    let v = remove_project_volumes(docker, project).await;
    emit(
        progress,
        BuildProgressEvent::ok_with_detail("Removing volumes", 3, total, format!("{v} removed")),
    );

    emit(
        progress,
        BuildProgressEvent::started("Removing images", 4, total),
    );
    let i = remove_project_images(docker, project).await;
    emit(
        progress,
        BuildProgressEvent::ok_with_detail("Removing images", 4, total, format!("{i} removed")),
    );

    (c, v, i)
}

/// Remove a single build directory and clean up the "latest" symlink if needed.
fn remove_build_dir(project_dir: &std::path::Path, build_id: &str, is_latest: bool) -> bool {
    let build_dir = project_dir.join(build_id);
    if !build_dir.exists() {
        return false;
    }
    match std::fs::remove_dir_all(&build_dir) {
        Ok(_) => {
            info!(build_id = %build_id, "removed build directory");
            if is_latest {
                let symlink_path = project_dir.join("latest");
                if symlink_path.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&symlink_path);
                    info!("removed stale 'latest' symlink after deleting latest build");
                }
            }
            true
        }
        Err(e) => {
            warn!(build_id = %build_id, error = %e, "failed to remove build directory");
            false
        }
    }
}

/// Remove the Docker image tag for a specific build.
async fn remove_build_image(docker: &bollard::Docker, project: &str, build_id: &str) -> bool {
    let tag = format!("coast-image/{}:{}", project, build_id);
    let rm_opts = bollard::image::RemoveImageOptions {
        force: false,
        noprune: false,
    };
    if docker.remove_image(&tag, Some(rm_opts), None).await.is_ok() {
        info!(tag = %tag, "removed Docker image tag");
        true
    } else {
        false
    }
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

/// Check whether a Docker volume belongs to the given project.
///
/// Three rules:
/// - **Shared volumes:** name starts with `coast-shared--{project}--`
/// - **Compose volumes:** name contains `{project}-coasts` or `{project}-shared-services`
/// - **Isolated volumes:** name starts with `coast--` AND the `coast.project` label matches
fn volume_belongs_to_project(
    name: &str,
    project: &str,
    labels: &std::collections::HashMap<String, String>,
) -> bool {
    let shared_prefix = format!("coast-shared--{project}--");
    let compose_prefix = format!("{project}-coasts");
    let shared_svc_prefix = format!("{project}-shared-services");

    if name.starts_with(&shared_prefix)
        || name.contains(&compose_prefix)
        || name.contains(&shared_svc_prefix)
    {
        return true;
    }

    if name.starts_with("coast--") {
        return labels
            .get("coast.project")
            .map(|p| p == project)
            .unwrap_or(false);
    }

    false
}

/// Check whether a Docker image matches the given project by its repo tags.
///
/// Matches when any tag starts with `coast-image/{project}:` or `{project}-coasts`.
fn image_matches_project(repo_tags: &[String], project: &str) -> bool {
    let prefix_a = format!("coast-image/{}:", project);
    let prefix_b = format!("{}-coasts", project);
    repo_tags
        .iter()
        .any(|tag| tag.starts_with(&prefix_a) || tag.starts_with(&prefix_b))
}

/// Remove Docker volumes matching project naming patterns.
async fn remove_project_volumes(docker: &bollard::Docker, project: &str) -> usize {
    use bollard::volume::ListVolumesOptions;
    use std::collections::HashMap;

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

        if !volume_belongs_to_project(name, project, &vol.labels) {
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

    let mut count = 0;
    for img in &images {
        if !image_matches_project(&img.repo_tags, project) {
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
    let Ok(home) = coast_home() else {
        return false;
    };
    let artifact_dir = home.join("images").join(project);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::state::StateDb;
    use coast_core::types::{CoastInstance, RuntimeType};

    fn make_instance(name: &str, project: &str, build_id: Option<&str>) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: coast_core::types::InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("container-123".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: build_id.map(String::from),
            coastfile_type: None,
            remote_host: None,
        }
    }

    // --- validate_removable tests ---

    #[test]
    fn test_validate_removable_no_instances_no_shared_services() {
        let db = StateDb::open_in_memory().unwrap();
        assert!(validate_removable(&db, "my-app").is_ok());
    }

    #[test]
    fn test_validate_removable_instances_exist() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_instance(&make_instance("feat-a", "my-app", None))
            .unwrap();
        let err = validate_removable(&db, "my-app").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("instance(s) still exist"), "got: {msg}");
    }

    #[test]
    fn test_validate_removable_running_shared_services() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_shared_service("my-app", "redis", Some("cid"), "running")
            .unwrap();
        let err = validate_removable(&db, "my-app").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("shared service(s) still running"),
            "got: {msg}"
        );
    }

    #[test]
    fn test_validate_removable_stopped_shared_services_ok() {
        let db = StateDb::open_in_memory().unwrap();
        db.insert_shared_service("my-app", "redis", None, "stopped")
            .unwrap();
        assert!(validate_removable(&db, "my-app").is_ok());
    }

    // --- build_in_use_map tests ---

    #[test]
    fn test_build_in_use_map_with_build_id() {
        let instances = vec![make_instance("inst-a", "proj", Some("abc"))];
        let map = build_in_use_map(&instances, None);
        assert_eq!(map.get("abc").unwrap(), &vec!["inst-a".to_string()]);
    }

    #[test]
    fn test_build_in_use_map_without_build_id_uses_latest() {
        let instances = vec![make_instance("inst-a", "proj", None)];
        let map = build_in_use_map(&instances, Some("xyz"));
        assert_eq!(map.get("xyz").unwrap(), &vec!["inst-a".to_string()]);
    }

    #[test]
    fn test_build_in_use_map_without_build_id_no_latest() {
        let instances = vec![make_instance("inst-a", "proj", None)];
        let map = build_in_use_map(&instances, None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_in_use_map_multiple_instances_same_build() {
        let instances = vec![
            make_instance("inst-a", "proj", Some("abc")),
            make_instance("inst-b", "proj", Some("abc")),
        ];
        let map = build_in_use_map(&instances, None);
        let mut names = map.get("abc").unwrap().clone();
        names.sort();
        assert_eq!(names, vec!["inst-a".to_string(), "inst-b".to_string()]);
    }

    #[test]
    fn test_build_in_use_map_empty_instances() {
        let map = build_in_use_map(&[], None);
        assert!(map.is_empty());
    }

    // --- volume_belongs_to_project tests ---

    #[test]
    fn test_shared_volume_matches() {
        let labels = HashMap::new();
        assert!(volume_belongs_to_project(
            "coast-shared--my-app--pg_data",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_shared_volume_different_project_does_not_match() {
        let labels = HashMap::new();
        assert!(!volume_belongs_to_project(
            "coast-shared--other-app--pg_data",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_compose_volume_matches() {
        let labels = HashMap::new();
        assert!(volume_belongs_to_project(
            "my-app-coasts-web_data",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_shared_services_volume_matches() {
        let labels = HashMap::new();
        assert!(volume_belongs_to_project(
            "my-app-shared-services-redis_data",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_isolated_volume_with_correct_label_matches() {
        let mut labels = HashMap::new();
        labels.insert("coast.project".to_string(), "my-app".to_string());
        assert!(volume_belongs_to_project(
            "coast--dev--pg",
            "my-app",
            &labels
        ));
    }

    #[test]
    fn test_isolated_volume_with_wrong_label_does_not_match() {
        let mut labels = HashMap::new();
        labels.insert("coast.project".to_string(), "other-app".to_string());
        assert!(!volume_belongs_to_project(
            "coast--dev--pg",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_isolated_volume_with_no_label_does_not_match() {
        let labels = HashMap::new();
        assert!(!volume_belongs_to_project(
            "coast--dev--pg",
            "my-app",
            &labels,
        ));
    }

    #[test]
    fn test_unrelated_volume_does_not_match() {
        let labels = HashMap::new();
        assert!(!volume_belongs_to_project(
            "postgres_data",
            "my-app",
            &labels
        ));
    }

    // --- image_matches_project tests ---

    #[test]
    fn test_coast_image_tag_matches() {
        let tags = vec!["coast-image/my-app:abc123".to_string()];
        assert!(image_matches_project(&tags, "my-app"));
    }

    #[test]
    fn test_coasts_compose_image_matches() {
        let tags = vec!["my-app-coasts-web:latest".to_string()];
        assert!(image_matches_project(&tags, "my-app"));
    }

    #[test]
    fn test_different_project_tag_does_not_match() {
        let tags = vec!["coast-image/other-app:abc123".to_string()];
        assert!(!image_matches_project(&tags, "my-app"));
    }

    #[test]
    fn test_empty_tags_does_not_match() {
        let tags: Vec<String> = vec![];
        assert!(!image_matches_project(&tags, "my-app"));
    }

    #[test]
    fn test_multiple_tags_one_matches() {
        let tags = vec![
            "postgres:15".to_string(),
            "coast-image/my-app:abc123".to_string(),
        ];
        assert!(image_matches_project(&tags, "my-app"));
    }

    // --- remove_build_dir tests ---

    #[test]
    fn test_remove_build_dir_removes_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path();
        let build_dir = project_dir.join("abc123");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(build_dir.join("file.txt"), "data").unwrap();

        assert!(remove_build_dir(project_dir, "abc123", false));
        assert!(!build_dir.exists());
    }

    #[test]
    fn test_remove_build_dir_nonexistent_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!remove_build_dir(tmp.path(), "no-such-build", false));
    }

    #[test]
    fn test_remove_build_dir_cleans_latest_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path();
        let build_dir = project_dir.join("abc123");
        std::fs::create_dir_all(&build_dir).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink("abc123", project_dir.join("latest")).unwrap();

        assert!(remove_build_dir(project_dir, "abc123", true));
        assert!(!build_dir.exists());
        #[cfg(unix)]
        assert!(
            !project_dir.join("latest").symlink_metadata().is_ok(),
            "latest symlink should be removed"
        );
    }

    #[test]
    fn test_remove_build_dir_not_latest_keeps_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path();
        let build_dir = project_dir.join("old-build");
        std::fs::create_dir_all(&build_dir).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink("current-build", project_dir.join("latest")).unwrap();

        assert!(remove_build_dir(project_dir, "old-build", false));
        #[cfg(unix)]
        assert!(
            project_dir.join("latest").symlink_metadata().is_ok(),
            "latest symlink should be kept for non-latest builds"
        );
    }
}
