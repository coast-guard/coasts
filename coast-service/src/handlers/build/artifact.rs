use std::path::{Path, PathBuf};

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use tracing::info;

pub struct ArtifactOutput {
    pub artifact_path: PathBuf,
    pub warnings: Vec<String>,
}

pub fn create_artifact(
    coastfile: &Coastfile,
    build_id: &str,
    home: &Path,
) -> Result<ArtifactOutput> {
    let artifact_path = home.join("images").join(&coastfile.name).join(build_id);

    std::fs::create_dir_all(&artifact_path).map_err(|e| CoastError::Io {
        message: format!("failed to create artifact dir: {e}"),
        path: artifact_path.clone(),
        source: Some(e),
    })?;

    let mut warnings = Vec::new();

    let toml_content = coastfile.to_standalone_toml();
    let toml_path = artifact_path.join("coastfile.toml");
    std::fs::write(&toml_path, &toml_content).map_err(|e| CoastError::Io {
        message: format!("failed to write coastfile.toml: {e}"),
        path: toml_path.clone(),
        source: Some(e),
    })?;
    info!(path = %toml_path.display(), "wrote coastfile.toml");

    if let Some(ref compose_path) = coastfile.compose {
        match std::fs::read_to_string(compose_path) {
            Ok(compose_content) => {
                let rewritten = coast_docker::compose_build::rewrite_compose_for_artifact(
                    &compose_content,
                    &coastfile.name,
                )?;
                let stripped = strip_omitted(&rewritten, &coastfile.omit)?;
                let dest = artifact_path.join("compose.yml");
                std::fs::write(&dest, &stripped).map_err(|e| CoastError::Io {
                    message: format!("failed to write compose.yml: {e}"),
                    path: dest.clone(),
                    source: Some(e),
                })?;
                info!(path = %dest.display(), "wrote compose.yml");
            }
            Err(e) => {
                warnings.push(format!(
                    "compose file '{}' not readable: {e}",
                    compose_path.display()
                ));
            }
        }
    }

    let secrets_dir = artifact_path.join("secrets");
    std::fs::create_dir_all(&secrets_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create secrets dir: {e}"),
        path: secrets_dir.clone(),
        source: Some(e),
    })?;

    if !coastfile.inject.files.is_empty() {
        let inject_dir = artifact_path.join("inject");
        std::fs::create_dir_all(&inject_dir).map_err(|e| CoastError::Io {
            message: format!("failed to create inject dir: {e}"),
            path: inject_dir.clone(),
            source: Some(e),
        })?;

        for file_path in &coastfile.inject.files {
            let src = Path::new(file_path);
            if !src.exists() {
                warnings.push(format!("inject file '{}' not found, skipping", file_path));
                continue;
            }
            let filename = src
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));
            let dest = inject_dir.join(filename);
            std::fs::copy(src, &dest).map_err(|e| CoastError::Io {
                message: format!("failed to copy inject file '{}': {e}", file_path),
                path: dest.clone(),
                source: Some(e),
            })?;
            info!(file = %file_path, "copied inject file");
        }
    }

    Ok(ArtifactOutput {
        artifact_path,
        warnings,
    })
}

fn strip_omitted(compose_content: &str, omit: &coast_core::types::OmitConfig) -> Result<String> {
    if omit.is_empty() {
        return Ok(compose_content.to_string());
    }

    let mut doc: serde_yaml::Value = serde_yaml::from_str(compose_content)
        .map_err(|e| CoastError::coastfile(format!("failed to parse compose YAML: {e}")))?;

    if let Some(serde_yaml::Value::Mapping(services)) = doc.get_mut("services") {
        for svc in &omit.services {
            services.remove(serde_yaml::Value::String(svc.clone()));
        }
    }

    if let Some(serde_yaml::Value::Mapping(volumes)) = doc.get_mut("volumes") {
        for vol in &omit.volumes {
            volumes.remove(serde_yaml::Value::String(vol.clone()));
        }
    }

    serde_yaml::to_string(&doc)
        .map_err(|e| CoastError::coastfile(format!("failed to serialize stripped compose: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::coastfile::Coastfile;
    use coast_core::types::OmitConfig;

    fn minimal_coastfile(dir: &Path) -> Coastfile {
        let toml = "[coast]\nname = \"test-project\"\n";
        Coastfile::parse(toml, dir).unwrap()
    }

    #[test]
    fn test_artifact_directory_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cf = minimal_coastfile(tmp.path());

        let out = create_artifact(&cf, "abc123_20260101", &home).unwrap();

        assert!(out.artifact_path.exists());
        assert!(out.artifact_path.join("coastfile.toml").exists());
        assert!(out.artifact_path.join("secrets").is_dir());
        assert_eq!(
            out.artifact_path,
            home.join("images/test-project/abc123_20260101")
        );
    }

    #[test]
    fn test_coastfile_toml_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cf = minimal_coastfile(tmp.path());

        create_artifact(&cf, "rt_001", &home).unwrap();

        let written =
            std::fs::read_to_string(home.join("images/test-project/rt_001/coastfile.toml"))
                .unwrap();
        let reparsed = Coastfile::parse(&written, tmp.path()).unwrap();
        assert_eq!(reparsed.name, "test-project");
    }

    #[test]
    fn test_compose_rewrite() {
        let tmp = tempfile::tempdir().unwrap();
        let compose_content = r#"
services:
  app:
    image: node:18
    ports:
      - "3000:3000"
"#;
        let compose_path = tmp.path().join("docker-compose.yml");
        std::fs::write(&compose_path, compose_content).unwrap();

        let toml = format!(
            "[coast]\nname = \"myproj\"\ncompose = \"{}\"\n",
            compose_path.display()
        );
        let cf = Coastfile::parse(&toml, tmp.path()).unwrap();
        let home = tmp.path().join("home");

        let out = create_artifact(&cf, "c_001", &home).unwrap();
        assert!(out.artifact_path.join("compose.yml").exists());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn test_omit_stripping() {
        let yaml = r#"
services:
  app:
    image: node:18
  db:
    image: postgres:16
  redis:
    image: redis:7
volumes:
  pgdata:
    driver: local
  cache:
    driver: local
"#;
        let omit = OmitConfig {
            services: vec!["redis".to_string()],
            volumes: vec!["cache".to_string()],
        };
        let result = strip_omitted(yaml, &omit).unwrap();
        assert!(!result.contains("redis"));
        assert!(result.contains("app"));
        assert!(result.contains("db"));
        assert!(!result.contains("cache"));
        assert!(result.contains("pgdata"));
    }

    #[test]
    fn test_strip_empty_omit_is_noop() {
        let yaml = "services:\n  app:\n    image: node:18\n";
        let omit = OmitConfig::default();
        let result = strip_omitted(yaml, &omit).unwrap();
        assert!(result.contains("app"));
    }
}
