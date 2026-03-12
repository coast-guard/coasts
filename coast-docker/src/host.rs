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
    pub tls: Option<DockerTlsMaterial>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerTlsMaterial {
    pub ca_path: PathBuf,
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
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
    #[serde(rename = "SkipTLSVerify")]
    skip_tls_verify: Option<bool>,
}

pub fn connect_to_host_docker() -> Result<Docker> {
    let docker_config_dir = env::var_os("DOCKER_CONFIG").map(PathBuf::from);
    let env_host = env::var("DOCKER_HOST").ok();
    let env_context = env::var("DOCKER_CONTEXT").ok();

    connect_to_host_docker_with(
        docker_config_dir.as_deref(),
        env_host.as_deref(),
        env_context.as_deref(),
    )
}

fn connect_to_host_docker_with(
    docker_config_dir: Option<&Path>,
    env_host: Option<&str>,
    env_context: Option<&str>,
) -> Result<Docker> {
    let endpoint = resolve_docker_endpoint(docker_config_dir, env_host, env_context)?;

    match endpoint.source {
        DockerEndpointSource::EnvHost => Docker::connect_with_defaults().map_err(|e| {
            CoastError::docker(format!(
                "Failed to connect to Docker using DOCKER_HOST='{}'. Error: {e}",
                endpoint.host
            ))
        }),
        _ => connect_to_endpoint(&endpoint),
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
                tls: None,
            });
        }

        let resolved = resolve_context_endpoint(config_dir.as_deref(), raw_context)?;
        return Ok(DockerEndpoint {
            host: resolved.host,
            source: DockerEndpointSource::EnvContext,
            context: Some(raw_context.to_string()),
            tls: resolved.tls,
        });
    }

    if let Some(host) = normalize_env_value(env_host) {
        return Ok(DockerEndpoint {
            host: host.to_string(),
            source: DockerEndpointSource::EnvHost,
            context: None,
            tls: None,
        });
    }

    if let Some(config_dir) = config_dir.as_deref() {
        if let Some(context) = current_context_from_config(config_dir)? {
            let resolved = resolve_context_endpoint(Some(config_dir), &context)?;
            return Ok(DockerEndpoint {
                host: resolved.host,
                source: DockerEndpointSource::ConfigContext,
                context: Some(context),
                tls: resolved.tls,
            });
        }
    }

    Ok(DockerEndpoint {
        host: DEFAULT_LOCAL_DOCKER_HOST.to_string(),
        source: DockerEndpointSource::DefaultLocal,
        context: None,
        tls: None,
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

    if host.starts_with("ssh://") {
        return Err(CoastError::docker(format!(
            "Unsupported Docker endpoint '{}' from {context_msg}. \
             SSH Docker contexts are out of scope for this resolver; set DOCKER_HOST explicitly to a supported transport.",
            endpoint.host
        )));
    }

    if let Some(ref tls) = endpoint.tls {
        return Docker::connect_with_ssl(
            host,
            &tls.key_path,
            &tls.cert_path,
            &tls.ca_path,
            DEFAULT_TIMEOUT_SECS,
            API_DEFAULT_VERSION,
        )
        .map_err(|e| {
            CoastError::docker(format!(
                "Failed to connect to {context_msg} at '{}' using TLS material from '{}'. Error: {e}",
                endpoint.host,
                tls.ca_path
                    .parent()
                    .map(Path::display)
                    .map(|path| path.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            ))
        });
    }

    if host.starts_with("https://") {
        return Err(CoastError::docker(format!(
            "Docker endpoint '{}' from {context_msg} requires TLS material, but none was found in the Docker context storage.",
            endpoint.host
        )));
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

struct ResolvedContextEndpoint {
    host: String,
    tls: Option<DockerTlsMaterial>,
}

fn resolve_context_endpoint(
    config_dir: Option<&Path>,
    context_name: &str,
) -> Result<ResolvedContextEndpoint> {
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

        let endpoint = meta.endpoints.get("docker").ok_or_else(|| {
            CoastError::docker(format!(
                "Docker context '{context_name}' has no docker endpoint metadata in '{}'.",
                meta_path.display()
            ))
        })?;

        let host = endpoint
            .host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                CoastError::docker(format!(
                    "Docker context '{context_name}' has no docker endpoint host in '{}'.",
                    meta_path.display()
                ))
            })?;

        let tls = resolve_context_tls_material(config_dir, &meta_path, host, endpoint)?;

        return Ok(ResolvedContextEndpoint {
            host: host.to_string(),
            tls,
        });
    }

    Err(CoastError::docker(format!(
        "Docker context '{context_name}' was not found under '{}'.",
        meta_root.display()
    )))
}

fn resolve_context_tls_material(
    config_dir: &Path,
    meta_path: &Path,
    host: &str,
    endpoint: &ContextEndpoint,
) -> Result<Option<DockerTlsMaterial>> {
    if host.starts_with("unix://") || host.starts_with("npipe://") || host.starts_with("ssh://") {
        return Ok(None);
    }

    let Some(hash_dir) = meta_path.parent().and_then(Path::file_name) else {
        return Err(CoastError::docker(format!(
            "Could not resolve the Docker context storage directory for '{}'.",
            meta_path.display()
        )));
    };

    let tls_root = config_dir.join("contexts").join("tls").join(hash_dir);
    let search_roots = [tls_root.join("docker"), tls_root.clone()];
    let pem_names = ["ca.pem", "cert.pem", "key.pem"];

    for root in &search_roots {
        let found: Vec<PathBuf> = pem_names.iter().map(|name| root.join(name)).collect();
        let existing_count = found.iter().filter(|path| path.exists()).count();

        if existing_count == 0 {
            continue;
        }

        if existing_count != pem_names.len() {
            return Err(CoastError::docker(format!(
                "Docker context '{}' has partial TLS material in '{}'. Expected ca.pem, cert.pem, and key.pem.",
                host,
                root.display()
            )));
        }

        return Ok(Some(DockerTlsMaterial {
            ca_path: found[0].clone(),
            cert_path: found[1].clone(),
            key_path: found[2].clone(),
        }));
    }

    if host.starts_with("https://")
        || (host.starts_with("tcp://") && endpoint.skip_tls_verify.unwrap_or(false))
    {
        return Err(CoastError::docker(format!(
            "Docker context '{}' requires TLS, but no TLS material was found under '{}'.",
            host,
            tls_root.display()
        )));
    }

    Ok(None)
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
        assert_eq!(endpoint.tls, None);
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
        assert_eq!(endpoint.tls, None);
    }

    #[test]
    fn explicit_default_context_falls_back_to_default_socket() {
        let endpoint =
            resolve_docker_endpoint(None, Some("unix:///tmp/docker.sock"), Some("default"))
                .unwrap();

        assert_eq!(endpoint.source, DockerEndpointSource::DefaultLocal);
        assert_eq!(endpoint.host, DEFAULT_LOCAL_DOCKER_HOST);
        assert_eq!(endpoint.tls, None);
    }

    #[test]
    fn missing_context_is_actionable() {
        let temp = TempDir::new().unwrap();
        let error = resolve_docker_endpoint(Some(temp.path()), None, Some("missing")).unwrap_err();

        assert!(error.to_string().contains("Docker context 'missing'"));
    }

    #[test]
    fn resolves_tcp_context_without_tls_material_as_plain_host() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"remote","Endpoints":{"docker":{"Host":"tcp://docker.example:2375","SkipTLSVerify":false}}}"#,
        );

        let endpoint = resolve_docker_endpoint(Some(temp.path()), None, Some("remote")).unwrap();

        assert_eq!(endpoint.host, "tcp://docker.example:2375");
        assert_eq!(endpoint.tls, None);
    }

    #[test]
    fn resolves_https_context_with_tls_material() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"secure","Endpoints":{"docker":{"Host":"https://docker.example:2376","SkipTLSVerify":false}}}"#,
        );
        write_json(&temp.path().join("contexts/tls/hash/docker/ca.pem"), "ca");
        write_json(
            &temp.path().join("contexts/tls/hash/docker/cert.pem"),
            "cert",
        );
        write_json(&temp.path().join("contexts/tls/hash/docker/key.pem"), "key");

        let endpoint = resolve_docker_endpoint(Some(temp.path()), None, Some("secure")).unwrap();

        assert_eq!(endpoint.host, "https://docker.example:2376");
        assert_eq!(
            endpoint.tls,
            Some(DockerTlsMaterial {
                ca_path: temp.path().join("contexts/tls/hash/docker/ca.pem"),
                cert_path: temp.path().join("contexts/tls/hash/docker/cert.pem"),
                key_path: temp.path().join("contexts/tls/hash/docker/key.pem"),
            })
        );
    }

    #[test]
    fn ssh_context_is_rejected_explicitly() {
        let temp = TempDir::new().unwrap();
        write_json(
            &temp.path().join("contexts/meta/hash/meta.json"),
            r#"{"Name":"ssh-ctx","Endpoints":{"docker":{"Host":"ssh://docker.example","SkipTLSVerify":false}}}"#,
        );

        let endpoint = resolve_docker_endpoint(Some(temp.path()), None, Some("ssh-ctx")).unwrap();
        let error = connect_to_endpoint(&endpoint).unwrap_err();

        assert!(error
            .to_string()
            .contains("SSH Docker contexts are out of scope"));
    }
}
