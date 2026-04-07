/// Handler for the `coast build` command.
///
/// Parses the Coastfile, validates it, extracts secrets, prepares the image
/// artifact directory, caches OCI images, and copies injected host files.
mod artifact;
mod coast_image;
mod images;
mod manifest;
mod plan;
mod secrets;
pub(crate) mod utils;

use tracing::info;

use coast_core::artifact::coast_home;
use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, BuildRequest, BuildResponse};

use crate::server::AppState;

/// Send a progress event, ignoring send errors (CLI may have disconnected).
fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

/// Handle a build request.
///
/// Steps:
/// 1. Parse and validate the Coastfile.
/// 2. Extract secrets via configured extractors and store in keystore.
/// 3. Create the artifact directory at `$COAST_HOME/images/{project}/`.
/// 4. Cache OCI images referenced in the compose file.
/// 5. Build custom coast image (if configured).
/// 6. Write the manifest file.
pub async fn handle(
    req: BuildRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<BuildResponse> {
    info!(
        coastfile_path = %req.coastfile_path.display(),
        refresh = req.refresh,
        "handling build request"
    );

    let coastfile = Coastfile::from_file(&req.coastfile_path)?;

    let home = coast_home()?;
    std::fs::create_dir_all(&home).map_err(|error| CoastError::Io {
        message: format!("failed to create Coast home directory: {error}"),
        path: home.clone(),
        source: Some(error),
    })?;

    let compose_analysis = plan::ComposeAnalysis::from_coastfile(&coastfile);
    let build_plan = plan::BuildPlan::from_inputs(
        !coastfile.secrets.is_empty(),
        compose_analysis.has_build_directives(),
        compose_analysis.has_image_refs(),
        !coastfile.setup.is_empty(),
    );

    emit(&progress, build_plan.build_plan_event());
    emit(&progress, build_plan.started("Parsing Coastfile"));
    emit(
        &progress,
        BuildProgressEvent::done("Parsing Coastfile", "ok"),
    );

    let secret_output = secrets::extract_secrets(&coastfile, &home, &progress, &build_plan);
    let artifact_output =
        artifact::create_artifact(&req, &coastfile, &compose_analysis, &progress, &build_plan)?;
    let image_output = images::cache_images(
        &req,
        state,
        &coastfile,
        &compose_analysis,
        &progress,
        &build_plan,
    )
    .await?;
    let coast_image = coast_image::build_coast_image(
        &coastfile,
        &artifact_output.build_id,
        &progress,
        &build_plan,
    )
    .await?;

    let mut warnings = secret_output.warnings;
    warnings.extend(artifact_output.warnings.iter().cloned());
    warnings.extend(image_output.warnings.iter().cloned());

    manifest::write_manifest_and_finalize(manifest::ManifestInput {
        coastfile: &coastfile,
        artifact: &artifact_output,
        images: &image_output,
        coast_image: &coast_image,
        state,
        progress: &progress,
        plan: &build_plan,
    })
    .await?;

    info!(
        project = %coastfile.name,
        build_id = %artifact_output.build_id,
        artifact_path = %artifact_output.artifact_path.display(),
        images_cached = image_output.images_cached,
        images_built = image_output.images_built,
        secrets_extracted = secret_output.secrets_extracted,
        warnings_count = warnings.len(),
        "build completed"
    );

    Ok(BuildResponse {
        project: coastfile.name,
        artifact_path: artifact_output.artifact_path,
        images_cached: image_output.images_cached,
        images_built: image_output.images_built,
        secrets_extracted: secret_output.secrets_extracted,
        coast_image,
        warnings,
        coastfile_type: coastfile.coastfile_type,
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use super::*;
    use crate::state::StateDb;

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn test_progress_sender() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct HomeEnvGuard {
        _guard: MutexGuard<'static, ()>,
        previous_home: Option<OsString>,
        previous_coast_home: Option<OsString>,
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match &self.previous_coast_home {
                Some(coast_home) => unsafe {
                    std::env::set_var("COAST_HOME", coast_home);
                },
                None => unsafe {
                    std::env::remove_var("COAST_HOME");
                },
            }
            match &self.previous_home {
                Some(home) => unsafe {
                    std::env::set_var("HOME", home);
                },
                None => unsafe {
                    std::env::remove_var("HOME");
                },
            }
        }
    }

    fn set_test_home(path: &Path) -> HomeEnvGuard {
        let guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        std::fs::create_dir_all(path).unwrap();
        let previous_home = std::env::var_os("HOME");
        let previous_coast_home = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("HOME", path);
            std::env::remove_var("COAST_HOME");
        }
        HomeEnvGuard {
            _guard: guard,
            previous_home,
            previous_coast_home,
        }
    }

    fn set_test_home_and_coast_home(home: &Path, coast_home: &Path) -> HomeEnvGuard {
        let guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        std::fs::create_dir_all(home).unwrap();
        std::fs::create_dir_all(coast_home).unwrap();
        let previous_home = std::env::var_os("HOME");
        let previous_coast_home = std::env::var_os("COAST_HOME");
        unsafe {
            std::env::set_var("HOME", home);
            std::env::set_var("COAST_HOME", coast_home);
        }
        HomeEnvGuard {
            _guard: guard,
            previous_home,
            previous_coast_home,
        }
    }

    fn write_project_files(
        dir: &Path,
        coastfile_contents: &str,
        compose_contents: &str,
    ) -> PathBuf {
        let coastfile_path = dir.join("Coastfile");
        let compose_path = dir.join("docker-compose.yml");
        std::fs::write(&coastfile_path, coastfile_contents).unwrap();
        std::fs::write(&compose_path, compose_contents).unwrap();
        coastfile_path
    }

    fn drain_events(
        rx: &mut tokio::sync::mpsc::Receiver<BuildProgressEvent>,
    ) -> Vec<BuildProgressEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[tokio::test]
    async fn test_build_nonexistent_coastfile() {
        let state = test_state();
        let req = BuildRequest {
            coastfile_path: PathBuf::from("/tmp/nonexistent/Coastfile"),
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_valid_coastfile() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-build"
compose = "./docker-compose.yml"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let resp = handle(req, &state, test_progress_sender()).await.unwrap();
        assert_eq!(resp.project, "test-build");
        assert!(resp.artifact_path.exists());
        assert_eq!(resp.images_cached, 0);
        assert_eq!(resp.images_built, 0);
        assert_eq!(resp.secrets_extracted, 0);
        assert!(resp.coast_image.is_none());
        assert!(resp.warnings.is_empty());
        assert!(resp.coastfile_type.is_none());
    }

    #[tokio::test]
    async fn test_build_uses_active_coast_home_for_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let coast_home = home.path().join(".coast-dev");
        let _home_guard = set_test_home_and_coast_home(home.path(), &coast_home);
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-dev-home"
compose = "./docker-compose.yml"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let resp = handle(req, &state, test_progress_sender()).await.unwrap();

        assert!(resp.artifact_path.starts_with(coast_home.join("images")));
        assert!(!resp
            .artifact_path
            .starts_with(home.path().join(".coast").join("images")));
    }

    #[tokio::test]
    async fn test_build_shared_volume_warning() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-warn"
compose = "./docker-compose.yml"

[volumes.pg_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("shared"));
    }

    #[tokio::test]
    async fn test_build_with_missing_inject_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-inject"
compose = "./docker-compose.yml"

[inject]
files = ["/tmp/nonexistent_coast_test_file_12345"]
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("does not exist")));
    }

    #[tokio::test]
    async fn test_build_rewrites_artifact_compose() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-rewrite"
compose = "./docker-compose.yml"
"#,
            r#"services:
  app:
    build: .
    ports:
      - "3000:3000"
  db:
    image: postgres:16
"#,
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        let artifact_compose = result.artifact_path.join("compose.yml");
        let content = std::fs::read_to_string(&artifact_compose).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
        let app = doc.get("services").unwrap().get("app").unwrap();
        assert!(app.get("build").is_none());
        assert_eq!(
            app.get("image").unwrap().as_str().unwrap(),
            "coast-built/test-rewrite/app:latest"
        );
        let db = doc.get("services").unwrap().get("db").unwrap();
        assert_eq!(db.get("image").unwrap().as_str().unwrap(), "postgres:16");
    }

    #[tokio::test]
    async fn test_build_with_setup_no_docker() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-setup"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["curl"]
run = ["echo hello"]
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await;
        if let Ok(resp) = result {
            assert!(resp.coast_image.is_some());
            assert!(resp.coast_image.unwrap().contains("test-setup"));
        }
    }

    #[tokio::test]
    async fn test_build_without_setup_no_coast_image() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-no-setup"
compose = "./docker-compose.yml"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(result.coast_image.is_none());
    }

    #[tokio::test]
    async fn test_build_manifest_contains_project_root_and_response_fields() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-manifest"
compose = "./docker-compose.yml"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        let manifest_path = result.artifact_path.join("manifest.json");
        let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_str).unwrap();
        assert!(manifest.get("project_root").is_some());
        assert_eq!(
            manifest.get("build_id").unwrap().as_str().unwrap(),
            result.artifact_path.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(
            manifest.get("project").unwrap().as_str().unwrap(),
            "test-manifest"
        );

        let project_root = manifest["project_root"].as_str().unwrap();
        assert!(project_root.contains(dir.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn test_build_emits_progress_events_for_minimal_build() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-progress"
compose = "./docker-compose.yml"
"#,
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let _result = handle(req, &state, tx).await.unwrap();

        let events = drain_events(&mut rx);
        assert_eq!(
            events[0].plan.as_ref().unwrap(),
            &vec![
                "Parsing Coastfile".to_string(),
                "Creating artifact".to_string(),
                "Writing manifest".to_string(),
            ]
        );
        assert_eq!(events[0].status, "plan");
        assert_eq!(events[0].total_steps, Some(3));

        let summary: Vec<_> = events
            .iter()
            .skip(1)
            .map(|event| {
                (
                    event.step.clone(),
                    event.detail.clone(),
                    event.status.clone(),
                    event.step_number,
                    event.total_steps,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "Parsing Coastfile".to_string(),
                    None,
                    "started".to_string(),
                    Some(1),
                    Some(3),
                ),
                (
                    "Parsing Coastfile".to_string(),
                    None,
                    "ok".to_string(),
                    None,
                    None,
                ),
                (
                    "Creating artifact".to_string(),
                    None,
                    "started".to_string(),
                    Some(2),
                    Some(3),
                ),
                (
                    "Creating artifact".to_string(),
                    None,
                    "ok".to_string(),
                    None,
                    None,
                ),
                (
                    "Writing manifest".to_string(),
                    None,
                    "started".to_string(),
                    Some(3),
                    Some(3),
                ),
                (
                    "Writing manifest".to_string(),
                    None,
                    "ok".to_string(),
                    None,
                    None,
                ),
            ]
        );
    }

    #[tokio::test]
    async fn test_build_with_secret_emits_secret_item_without_done_ok() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let var_name = "COAST_BUILD_SECRET_ITEM_TEST";
        unsafe {
            std::env::set_var(var_name, "shh");
        }
        let coastfile_path = write_project_files(
            dir.path(),
            &format!(
                r#"
[coast]
name = "test-secret-progress"
compose = "./docker-compose.yml"

[secrets.api_key]
extractor = "env"
var = "{var_name}"
inject = "env:API_KEY"
ttl = "1h"
"#
            ),
            "version: '3'\nservices: {}",
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await.unwrap();
        unsafe {
            std::env::remove_var(var_name);
        }

        assert_eq!(result.secrets_extracted, 1);
        let events = drain_events(&mut rx);
        assert_eq!(
            events[0].plan.as_ref().unwrap(),
            &vec![
                "Parsing Coastfile".to_string(),
                "Extracting secrets".to_string(),
                "Creating artifact".to_string(),
                "Writing manifest".to_string(),
            ]
        );
        assert!(events.iter().any(|event| {
            event.step == "Extracting secrets"
                && event.status == "started"
                && event.step_number == Some(2)
                && event.total_steps == Some(4)
        }));
        assert!(events.iter().any(|event| {
            event.step == "Extracting secrets"
                && event.detail.as_deref() == Some("env -> API_KEY")
                && event.status == "ok"
        }));
        assert!(!events.iter().any(|event| {
            event.step == "Extracting secrets" && event.detail.is_none() && event.status == "ok"
        }));
    }

    #[tokio::test]
    async fn test_build_without_docker_skips_pulling_image_refs() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-pull-skip"
compose = "./docker-compose.yml"
"#,
            r#"services:
  db:
    image: postgres:16
"#,
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await.unwrap();

        assert_eq!(
            result.warnings,
            vec![
                "Docker is not available — skipping OCI image pulling. Images will be pulled at runtime."
                    .to_string()
            ]
        );

        let events = drain_events(&mut rx);
        assert_eq!(
            events[0].plan.as_ref().unwrap(),
            &vec![
                "Parsing Coastfile".to_string(),
                "Creating artifact".to_string(),
                "Pulling images".to_string(),
                "Writing manifest".to_string(),
            ]
        );
        assert!(events.iter().any(|event| {
            event.step == "Pulling images"
                && event.status == "started"
                && event.step_number == Some(3)
                && event.total_steps == Some(4)
        }));
        assert!(events.iter().any(|event| {
            event.step == "Pulling images"
                && event.status == "skip"
                && event.verbose_detail.as_deref() == Some("Docker not available")
        }));
    }

    #[tokio::test]
    async fn test_build_omit_strips_services_from_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home_guard = set_test_home(home.path());
        let coastfile_path = write_project_files(
            dir.path(),
            r#"
[coast]
name = "test-omit"
compose = "./docker-compose.yml"

[omit]
services = ["keycloak", "redash"]
volumes = ["keycloak-db-data"]
"#,
            r#"services:
  app:
    image: myapp:latest
    depends_on:
      - keycloak
      - db
  keycloak:
    image: quay.io/keycloak/keycloak
    depends_on:
      - db
  redash:
    image: redash/redash
  db:
    image: postgres:16
volumes:
  keycloak-db-data:
  app-data:
"#,
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
            remote: None,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        let artifact_compose = result.artifact_path.join("compose.yml");
        let content = std::fs::read_to_string(&artifact_compose).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();

        let services = doc.get("services").unwrap().as_mapping().unwrap();
        assert!(services.contains_key(&serde_yaml::Value::String("app".into())));
        assert!(services.contains_key(&serde_yaml::Value::String("db".into())));
        assert!(!services.contains_key(&serde_yaml::Value::String("keycloak".into())));
        assert!(!services.contains_key(&serde_yaml::Value::String("redash".into())));

        let app = services
            .get(&serde_yaml::Value::String("app".into()))
            .unwrap();
        if let Some(deps) = app.get("depends_on") {
            let dep_list: Vec<&str> = if let Some(seq) = deps.as_sequence() {
                seq.iter().filter_map(|value| value.as_str()).collect()
            } else if let Some(map) = deps.as_mapping() {
                map.keys().filter_map(|key| key.as_str()).collect()
            } else {
                vec![]
            };
            assert!(!dep_list.contains(&"keycloak"));
            assert!(dep_list.contains(&"db"));
        }

        if let Some(volumes) = doc.get("volumes").and_then(|value| value.as_mapping()) {
            assert!(!volumes.contains_key(&serde_yaml::Value::String("keycloak-db-data".into())));
            assert!(volumes.contains_key(&serde_yaml::Value::String("app-data".into())));
        }
    }

    #[tokio::test]
    async fn test_build_omit_skips_building_omitted_images() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &compose_path,
            r#"services:
  app:
    build: .
  redash:
    build: ./redash
  db:
    image: postgres:16
"#,
        )
        .unwrap();

        let content = std::fs::read_to_string(&compose_path).unwrap();
        let unfiltered =
            coast_docker::compose_build::parse_compose_file(&content, "test-omit-build").unwrap();
        assert_eq!(unfiltered.build_directives.len(), 2);

        let filtered = coast_docker::compose_build::parse_compose_file_filtered(
            &content,
            "test-omit-build",
            &["redash".to_string()],
        )
        .unwrap();
        assert_eq!(filtered.build_directives.len(), 1);
        assert_eq!(filtered.build_directives[0].service_name, "app");
    }
}
