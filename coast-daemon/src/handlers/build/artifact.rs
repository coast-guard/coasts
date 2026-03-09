use std::path::{Path, PathBuf};

use tracing::info;

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, BuildRequest};
use coast_core::types::{VolumeConfig, VolumeStrategy};

use super::emit;
use super::plan::{BuildPlan, ComposeAnalysis};

pub(super) struct ArtifactOutput {
    pub build_id: String,
    pub build_timestamp: chrono::DateTime<chrono::Utc>,
    pub coastfile_hash: String,
    pub project_dir: PathBuf,
    pub artifact_path: PathBuf,
    pub warnings: Vec<String>,
}

pub(super) fn create_artifact(
    req: &BuildRequest,
    coastfile: &Coastfile,
    compose_analysis: &ComposeAnalysis,
    home: &Path,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    plan: &BuildPlan,
) -> Result<ArtifactOutput> {
    emit(progress, plan.started("Creating artifact"));

    let coastfile_raw = std::fs::read_to_string(&req.coastfile_path).unwrap_or_default();
    let build_timestamp = chrono::Utc::now();
    let coastfile_hash = compute_coastfile_hash(&coastfile_raw, coastfile);
    let build_id = format!(
        "{}_{}",
        &coastfile_hash,
        build_timestamp.format("%Y%m%d%H%M%S")
    );

    let project_dir = home.join(".coast").join("images").join(&coastfile.name);
    let artifact_path = project_dir.join(&build_id);
    std::fs::create_dir_all(&artifact_path).map_err(|error| CoastError::Io {
        message: format!("failed to create artifact directory: {error}"),
        path: artifact_path.clone(),
        source: Some(error),
    })?;

    write_artifact_coastfile(coastfile, &artifact_path)?;

    if let Some(content) = compose_analysis.content.as_deref() {
        write_artifact_compose(coastfile, &artifact_path, content)?;
    }

    let mut warnings = shared_volume_warnings(&coastfile.volumes);
    warnings.extend(copy_injected_files(
        &coastfile.inject.files,
        &artifact_path,
    )?);
    create_artifact_secrets_dir(&artifact_path)?;

    emit(
        progress,
        BuildProgressEvent::done("Creating artifact", "ok"),
    );

    Ok(ArtifactOutput {
        build_id,
        build_timestamp,
        coastfile_hash,
        project_dir,
        artifact_path,
        warnings,
    })
}

fn compute_coastfile_hash(coastfile_raw: &str, coastfile: &Coastfile) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    coastfile_raw.hash(&mut hasher);
    format!("{:?}", coastfile.ports).hash(&mut hasher);
    format!("{:?}", coastfile.secrets).hash(&mut hasher);
    format!("{:?}", coastfile.shared_services).hash(&mut hasher);
    format!("{:?}", coastfile.volumes).hash(&mut hasher);
    format!("{:?}", coastfile.setup).hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn write_artifact_coastfile(coastfile: &Coastfile, artifact_path: &Path) -> Result<()> {
    let artifact_coastfile = artifact_path.join("coastfile.toml");
    let standalone_toml = coastfile.to_standalone_toml();
    std::fs::write(&artifact_coastfile, standalone_toml.as_bytes()).map_err(|error| {
        CoastError::Io {
            message: format!("failed to write resolved Coastfile to artifact: {error}"),
            path: artifact_coastfile.clone(),
            source: Some(error),
        }
    })?;
    Ok(())
}

fn write_artifact_compose(
    coastfile: &Coastfile,
    artifact_path: &Path,
    content: &str,
) -> Result<()> {
    let artifact_compose = artifact_path.join("compose.yml");
    let rewritten =
        coast_docker::compose_build::rewrite_compose_for_artifact(content, &coastfile.name)?;
    let rewritten = strip_omitted_services_and_volumes(coastfile, rewritten);

    std::fs::write(&artifact_compose, &rewritten).map_err(|error| CoastError::Io {
        message: format!("failed to write rewritten compose file to artifact: {error}"),
        path: artifact_compose,
        source: Some(error),
    })?;
    Ok(())
}

fn strip_omitted_services_and_volumes(coastfile: &Coastfile, rewritten: String) -> String {
    if coastfile.omit.is_empty() {
        return rewritten;
    }

    let Ok(mut yaml) = serde_yaml::from_str::<serde_yaml::Value>(&rewritten) else {
        return rewritten;
    };

    let mut changed = false;

    if let Some(services) = yaml
        .get_mut("services")
        .and_then(|value| value.as_mapping_mut())
    {
        for service_name in &coastfile.omit.services {
            let key = serde_yaml::Value::String(service_name.clone());
            if services.remove(&key).is_some() {
                info!(service = %service_name, "stripped omitted service from artifact compose");
                changed = true;
            }
        }

        let omit_set: std::collections::HashSet<&str> = coastfile
            .omit
            .services
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let service_keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
        for service_key in service_keys {
            if let Some(service_def) = services
                .get_mut(&service_key)
                .and_then(|value| value.as_mapping_mut())
            {
                strip_omitted_depends_on(service_def, &coastfile.omit.services, &omit_set);
            }
        }
    }

    if let Some(top_volumes) = yaml
        .get_mut("volumes")
        .and_then(|value| value.as_mapping_mut())
    {
        for volume_name in &coastfile.omit.volumes {
            if top_volumes
                .remove(serde_yaml::Value::String(volume_name.clone()))
                .is_some()
            {
                info!(volume = %volume_name, "stripped omitted volume from artifact compose");
                changed = true;
            }
        }
    }

    if !changed {
        return rewritten;
    }

    serde_yaml::to_string(&yaml).unwrap_or(rewritten)
}

fn strip_omitted_depends_on(
    service_def: &mut serde_yaml::Mapping,
    omitted_services: &[String],
    omit_set: &std::collections::HashSet<&str>,
) {
    let dep_key = serde_yaml::Value::String("depends_on".into());
    let mut remove_depends = false;
    if let Some(deps) = service_def.get_mut(&dep_key) {
        if let Some(dep_map) = deps.as_mapping_mut() {
            for service_name in omitted_services {
                dep_map.remove(serde_yaml::Value::String(service_name.clone()));
            }
            if dep_map.is_empty() {
                remove_depends = true;
            }
        } else if let Some(dep_seq) = deps.as_sequence_mut() {
            dep_seq.retain(|value| {
                value
                    .as_str()
                    .map(|name| !omit_set.contains(name))
                    .unwrap_or(true)
            });
            if dep_seq.is_empty() {
                remove_depends = true;
            }
        }
    }
    if remove_depends {
        service_def.remove(&dep_key);
    }
}

fn shared_volume_warnings(volumes: &[VolumeConfig]) -> Vec<String> {
    let mut warnings = Vec::new();
    for volume in volumes {
        if volume.strategy == VolumeStrategy::Shared && looks_database_related(&volume.service) {
            warnings.push(format!(
                "Volume '{}' uses 'shared' strategy on service '{}' which looks database-related. \
                 Multiple instances writing to the same database volume can cause data corruption. \
                 Consider using 'isolated' strategy or 'shared_services' instead.",
                volume.name, volume.service
            ));
        }
    }
    warnings
}

fn looks_database_related(service: &str) -> bool {
    let service_lower = service.to_lowercase();
    service_lower.contains("postgres")
        || service_lower.contains("mysql")
        || service_lower.contains("mongo")
        || service_lower.contains("redis")
        || service_lower.contains("db")
}

fn copy_injected_files(files: &[String], artifact_path: &Path) -> Result<Vec<String>> {
    let inject_dir = artifact_path.join("inject");
    std::fs::create_dir_all(&inject_dir).map_err(|error| CoastError::Io {
        message: format!("failed to create inject directory: {error}"),
        path: inject_dir.clone(),
        source: Some(error),
    })?;

    let mut warnings = Vec::new();
    for file_path_str in files {
        let expanded = shellexpand::tilde(file_path_str);
        let host_path = PathBuf::from(expanded.as_ref());
        if host_path.exists() {
            if let Some(filename) = host_path.file_name() {
                let dest = inject_dir.join(filename);
                std::fs::copy(&host_path, &dest).map_err(|error| CoastError::Io {
                    message: format!(
                        "failed to copy injected file '{}' to artifact: {error}",
                        host_path.display()
                    ),
                    path: dest,
                    source: Some(error),
                })?;
            }
        } else {
            warnings.push(format!(
                "Injected host file '{}' does not exist, skipping.",
                file_path_str
            ));
        }
    }

    Ok(warnings)
}

fn create_artifact_secrets_dir(artifact_path: &Path) -> Result<()> {
    let secrets_dir = artifact_path.join("secrets");
    std::fs::create_dir_all(&secrets_dir).map_err(|error| CoastError::Io {
        message: format!("failed to create secrets directory: {error}"),
        path: secrets_dir,
        source: Some(error),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_omitted_services_and_volumes_removes_references() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile = Coastfile::parse(
            r#"
[coast]
name = "artifact-test"
compose = "./docker-compose.yml"

[omit]
services = ["keycloak", "redash"]
volumes = ["keycloak-db-data"]
"#,
            dir.path(),
        )
        .unwrap();

        let rewritten = strip_omitted_services_and_volumes(
            &coastfile,
            r#"services:
  app:
    image: myapp:latest
    depends_on:
      - keycloak
      - db
  keycloak:
    image: quay.io/keycloak/keycloak
  redash:
    image: redash/redash
  db:
    image: postgres:16
volumes:
  keycloak-db-data:
  app-data:
"#
            .to_string(),
        );

        let doc: serde_yaml::Value = serde_yaml::from_str(&rewritten).unwrap();
        let services = doc.get("services").unwrap().as_mapping().unwrap();
        assert!(!services.contains_key(serde_yaml::Value::String("keycloak".into())));
        assert!(!services.contains_key(serde_yaml::Value::String("redash".into())));

        let app = services
            .get(serde_yaml::Value::String("app".into()))
            .unwrap();
        let deps = app
            .get("depends_on")
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        assert!(!deps.contains(&"keycloak"));
        assert!(deps.contains(&"db"));

        let volumes = doc.get("volumes").unwrap().as_mapping().unwrap();
        assert!(!volumes.contains_key(serde_yaml::Value::String("keycloak-db-data".into())));
        assert!(volumes.contains_key(serde_yaml::Value::String("app-data".into())));
    }

    #[test]
    fn test_shared_volume_warnings_only_flags_database_services() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile = Coastfile::parse(
            r#"
[coast]
name = "artifact-warnings"
compose = "./docker-compose.yml"

[volumes.pg_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"

[volumes.uploads]
strategy = "shared"
service = "web"
mount = "/uploads"
"#,
            dir.path(),
        )
        .unwrap();

        let warnings = shared_volume_warnings(&coastfile.volumes);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("pg_data"));
        assert!(warnings[0].contains("postgres"));
    }
}
