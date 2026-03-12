use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use bollard::{Docker, API_DEFAULT_VERSION};
use serde::Deserialize;

use coast_core::error::{CoastError, Result};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[cfg(unix)]
const DEFAULT_LOCAL_DOCKER_HOST: &str = "unix:///var/run/docker.sock";

#[cfg(windows)]
const DEFAULT_LOCAL_DOCKER_HOST: &str = "npipe:////./pipe/docker_engine";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerEndpointSource {
    EnvHost,
    EnvContext,
    ConfigContext,
    DefaultLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerEndpoint {
    pub host: String,
    pub source: DockerEndpointSource,
    pub context: Option<String>,
}

#[derive(Debug)]
pub struct HostDockerProbe {
    pub endpoint: Option<DockerEndpoint>,
    pub docker: Result<Docker>,
}

pub fn docker_endpoint_source_label(source: &DockerEndpointSource) -> &'static str {
    match source {
        DockerEndpointSource::EnvHost => "env_host",
        DockerEndpointSource::EnvContext => "env_context",
        DockerEndpointSource::ConfigContext => "config_context",
        DockerEndpointSource::DefaultLocal => "default_local",
    }
}

#[derive(Debug, Deserialize)]
struct DockerCliConfig {
    #[serde(rename = "currentContext")]
    current_context: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContextMeta {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Endpoints")]
    endpoints: std::collections::HashMap<String, ContextEndpoint>,
}

#[derive(Debug, Deserialize)]
struct ContextEndpoint {
    #[serde(rename = "Host")]
    host: Option<String>,
}

pub fn connect_to_host_docker() -> Result<Docker> {
    probe_host_docker().docker
}

pub fn probe_host_docker() -> HostDockerProbe {
    let docker_config_dir = env::var_os("DOCKER_CONFIG").map(PathBuf::from);
    let env_host = env::var("DOCKER_HOST").ok();
    let env_context = env::var("DOCKER_CONTEXT").ok();

    probe_host_docker_with(
        docker_config_dir.as_deref(),
        env_host.as_deref(),
        env_context.as_deref(),
    )
}

fn probe_host_docker_with(
    docker_config_dir: Option<&Path>,
    env_host: Option<&str>,
    env_context: Option<&str>,
) -> HostDockerProbe {
    let endpoint = match resolve_docker_endpoint(docker_config_dir, env_host, env_context) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return HostDockerProbe {
                endpoint: None,
                docker: Err(error),
            };
        }
    };

    let docker = match endpoint.source {
        DockerEndpointSource::EnvHost => Docker::connect_with_defaults().map_err(|e| {
            CoastError::docker(format!(
                "Failed to connect to Docker using DOCKER_HOST='{}'. Error: {e}",
                endpoint.host
            ))
        }),
        _ => connect_to_endpoint(&endpoint),
    };

    HostDockerProbe {
        endpoint: Some(endpoint),
        docker,
    }
}

pub fn resolve_docker_endpoint(
    docker_config_dir: Option<&Path>,
    env_host: Option<&str>,
    env_context: Option<&str>,
) -> Result<DockerEndpoint> {
    let config_dir = docker_config_dir
        .map(Path::to_path_buf)
        .or_else(default_docker_config_dir);

    if let Some(raw_context) = normalize_env_value(env_context) {
        if raw_context == "default" {
            return Ok(DockerEndpoint {
                host: DEFAULT_LOCAL_DOCKER_HOST.to_string(),
                source: DockerEndpointSource::DefaultLocal,
                context: None,
            });
        }

        let host = resolve_context_host(config_dir.as_deref(), raw_context)?;
        return Ok(DockerEndpoint {
            host,
            source: DockerEndpointSource::EnvContext,
            context: Some(raw_context.to_string()),
        });
    }

    if let Some(host) = normalize_env_value(env_host) {
        return Ok(DockerEndpoint {
            host: host.to_string(),
            source: DockerEndpointSource::EnvHost,
            context: None,
        });
    }

    if let Some(config_dir) = config_dir.as_deref() {
        if let Some(context) = current_context_from_config(config_dir)? {
            let host = resolve_context_host(Some(config_dir), &context)?;
            return Ok(DockerEndpoint {
                host,
                source: DockerEndpointSource::ConfigContext,
                context: Some(context),
            });
        }
    }

    Ok(DockerEndpoint {
        host: DEFAULT_LOCAL_DOCKER_HOST.to_string(),
        source: DockerEndpointSource::DefaultLocal,
        context: None,
    })
}

fn connect_to_endpoint(endpoint: &DockerEndpoint) -> Result<Docker> {
    let host = endpoint.host.as_str();
    let context_msg = endpoint
        .context
        .as_ref()
        .map(|name| format!("Docker context '{name}'"))
        .unwrap_or_else(|| "resolved Docker host".to_string());

    #[cfg(any(unix, windows))]
    if host.starts_with("unix://") || host.starts_with("npipe://") {
        return Docker::connect_with_socket(host, DEFAULT_TIMEOUT_SECS, API_DEFAULT_VERSION)
            .map_err(|e| {
                CoastError::docker(format!(
                    "Failed to connect to {context_msg} at '{}'. Error: {e}",
                    endpoint.host
                ))
            });
    }

    if host.starts_with("tcp://") || host.starts_with("http://") {
        return Docker::connect_with_http(host, DEFAULT_TIMEOUT_SECS, API_DEFAULT_VERSION).map_err(
            |e| {
                CoastError::docker(format!(
                    "Failed to connect to {context_msg} at '{}'. Error: {e}",
                    endpoint.host
                ))
            },
        );
    }

    Err(CoastError::docker(format!(
        "Unsupported Docker endpoint '{}' from {context_msg}. \
         Set DOCKER_HOST explicitly if this engine requires a transport Coasts does not yet auto-resolve.",
        endpoint.host
    )))
}

fn normalize_env_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalize_context_name(value: Option<&str>) -> Option<String> {
    match normalize_env_value(value) {
        Some("default") | None => None,
        Some(value) => Some(value.to_string()),
    }
}

fn default_docker_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".docker"))
}

fn current_context_from_config(config_dir: &Path) -> Result<Option<String>> {
    let config_path = config_dir.join("config.json");
    if !config_path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&config_path).map_err(|e| CoastError::Docker {
        message: format!(
            "Failed to read Docker config '{}'. Error: {e}",
            config_path.display()
        ),
        source: Some(Box::new(e)),
    })?;

    let config: DockerCliConfig =
        serde_json::from_str(&contents).map_err(|e| CoastError::Docker {
            message: format!(
                "Failed to parse Docker config '{}'. Error: {e}",
                config_path.display()
            ),
            source: Some(Box::new(e)),
        })?;

    Ok(normalize_context_name(config.current_context.as_deref()))
}

fn resolve_context_host(config_dir: Option<&Path>, context_name: &str) -> Result<String> {
    let Some(config_dir) = config_dir else {
        return Err(CoastError::docker(format!(
            "Docker context '{context_name}' was requested, but no Docker config directory could be found."
        )));
    };

    let meta_root = config_dir.join("contexts").join("meta");
    if !meta_root.exists() {
        return Err(CoastError::docker(format!(
            "Docker context '{context_name}' was requested, but '{}' does not exist.",
            meta_root.display()
        )));
    }

    for entry in fs::read_dir(&meta_root).map_err(|e| CoastError::Docker {
        message: format!(
            "Failed to read Docker contexts in '{}'. Error: {e}",
            meta_root.display()
        ),
        source: Some(Box::new(e)),
    })? {
        let entry = entry.map_err(|e| CoastError::Docker {
            message: format!(
                "Failed to inspect Docker context metadata in '{}'. Error: {e}",
                meta_root.display()
            ),
            source: Some(Box::new(e)),
        })?;

        let meta_path = entry.path().join("meta.json");
        if !meta_path.exists() {
            continue;
        }

        let contents = fs::read_to_string(&meta_path).map_err(|e| CoastError::Docker {
            message: format!(
                "Failed to read Docker context metadata '{}'. Error: {e}",
                meta_path.display()
            ),
            source: Some(Box::new(e)),
        })?;
        let meta: ContextMeta =
            serde_json::from_str(&contents).map_err(|e| CoastError::Docker {
                message: format!(
                    "Failed to parse Docker context metadata '{}'. Error: {e}",
                    meta_path.display()
                ),
                source: Some(Box::new(e)),
            })?;

        if meta.name != context_name {
            continue;
        }

        let host = meta
            .endpoints
            .get("docker")
            .and_then(|endpoint| endpoint.host.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CoastError::docker(format!(
                    "Docker context '{context_name}' has no docker endpoint host in '{}'.",
                    meta_path.display()
                ))
            })?;

        return Ok(host.to_string());
    }

    Err(CoastError::docker(format!(
        "Docker context '{context_name}' was not found under '{}'.",
        meta_root.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn write_json(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn resolves_env_host_before_config_context() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("config.json"),
            r#"{"currentContext":"orbstack"}"#,
        );

        let endpoint =
            resolve_docker_endpoint(Some(temp.path()), Some("unix:///tmp/docker.sock"), None)
                .unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::EnvHost);
        assert_eq!(endpoint.host, "unix:///tmp/docker.sock");
    }

    #[test]
    fn resolves_explicit_context_from_meta_store() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"orbstack","Endpoints":{"docker":{"Host":"unix:///Users/test/.orbstack/run/docker.sock"}}}"#,
        );

        let endpoint = resolve_docker_endpoint(Some(temp.path()), None, Some("orbstack")).unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::EnvContext);
        assert_eq!(
            endpoint.host,
            "unix:///Users/test/.orbstack/run/docker.sock"
        );
        assert_eq!(endpoint.context.as_deref(), Some("orbstack"));
    }

    #[test]
    fn explicit_context_overrides_docker_host() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"orbstack","Endpoints":{"docker":{"Host":"unix:///Users/test/.orbstack/run/docker.sock"}}}"#,
        );

        let endpoint = resolve_docker_endpoint(
            Some(temp.path()),
            Some("unix:///tmp/docker.sock"),
            Some("orbstack"),
        )
        .unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::EnvContext);
        assert_eq!(
            endpoint.host,
            "unix:///Users/test/.orbstack/run/docker.sock"
        );
    }

    #[test]
    fn resolves_current_context_from_config_when_env_is_unset() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("config.json"),
            r#"{"currentContext":"orbstack"}"#,
        );
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"orbstack","Endpoints":{"docker":{"Host":"unix:///Users/test/.orbstack/run/docker.sock"}}}"#,
        );

        let endpoint = resolve_docker_endpoint(Some(temp.path()), None, None).unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::ConfigContext);
        assert_eq!(endpoint.context.as_deref(), Some("orbstack"));
    }

    #[test]
    fn explicit_default_context_falls_back_to_default_socket() {
        let endpoint =
            resolve_docker_endpoint(None, Some("unix:///tmp/docker.sock"), Some("default"))
                .unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::DefaultLocal);
        assert_eq!(endpoint.host, DEFAULT_LOCAL_DOCKER_HOST);
    }

    #[test]
    fn missing_context_is_actionable() {
        let temp = TempDir::new().unwrap();
        let error = resolve_docker_endpoint(Some(temp.path()), None, Some("missing")).unwrap_err();

        assert!(error.to_string().contains("Docker context 'missing'"));
    }

    #[test]
    fn probe_captures_endpoint_on_connection_failure() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"orbstack","Endpoints":{"docker":{"Host":"unix:///tmp/does-not-exist.sock"}}}"#,
        );

        let probe = probe_host_docker_with(Some(temp.path()), None, Some("orbstack"));

        assert!(probe.endpoint.is_some());
        assert!(probe.docker.is_err());
    }
}
