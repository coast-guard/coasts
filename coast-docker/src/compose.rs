/// Docker Compose interaction inside coast containers.
///
/// Executes `docker compose` commands inside a coast container via `exec`.
/// When shared services are configured, generates a compose override file
/// that removes the local service and injects connection environment variables.
use std::collections::HashMap;

use tracing::{debug, info};

use coast_core::error::{CoastError, Result};
use coast_core::types::SharedServiceConfig;

use crate::runtime::{ExecResult, Runtime};

/// The compose command prefix used inside coast containers.
///
/// Uses the Docker Compose V2 plugin syntax.
pub const COMPOSE_CMD: &str = "docker";

/// The compose subcommand.
pub const COMPOSE_SUBCMD: &str = "compose";

/// The podman-compose command for Podman runtime.
pub const PODMAN_COMPOSE_CMD: &str = "podman-compose";

/// Manager for docker-compose operations inside coast containers.
///
/// Executes compose commands via `docker exec` into the coast container,
/// targeting the inner Docker daemon running inside.
pub struct ComposeManager<R: Runtime> {
    /// The underlying runtime for exec operations.
    runtime: R,
}

impl<R: Runtime> ComposeManager<R> {
    /// Create a new compose manager with the given runtime.
    pub fn new(runtime: R) -> Self {
        Self { runtime }
    }

    /// Get a reference to the underlying runtime.
    pub fn runtime(&self) -> &R {
        &self.runtime
    }

    /// Run `docker compose up -d` inside the coast container.
    ///
    /// Starts all compose services in detached mode. If an override file
    /// path is provided, it is included via `-f`.
    pub async fn up(
        &self,
        container_id: &str,
        compose_file: &str,
        override_file: Option<&str>,
    ) -> Result<ExecResult> {
        let mut args = self.compose_base_args(compose_file, override_file);
        args.push("up".to_string());
        args.push("-d".to_string());

        info!(
            container_id = %container_id,
            compose_file = %compose_file,
            "Running docker compose up"
        );

        let cmd_refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
        let result = self.runtime.exec_in_coast(container_id, &cmd_refs).await?;

        if !result.success() {
            return Err(CoastError::docker(format!(
                "docker compose up failed with exit code {}. \
                 stderr: {}. \
                 Check the compose file and service configurations.",
                result.exit_code, result.stderr
            )));
        }

        Ok(result)
    }

    /// Run `docker compose down` inside the coast container.
    pub async fn down(&self, container_id: &str, compose_file: &str) -> Result<ExecResult> {
        let mut args = self.compose_base_args(compose_file, None);
        args.push("down".to_string());

        info!(
            container_id = %container_id,
            "Running docker compose down"
        );

        let cmd_refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
        let result = self.runtime.exec_in_coast(container_id, &cmd_refs).await?;

        if !result.success() {
            return Err(CoastError::docker(format!(
                "docker compose down failed with exit code {}. stderr: {}",
                result.exit_code, result.stderr
            )));
        }

        Ok(result)
    }

    /// Run `docker compose ps` inside the coast container.
    pub async fn ps(&self, container_id: &str, compose_file: &str) -> Result<ExecResult> {
        let mut args = self.compose_base_args(compose_file, None);
        args.push("ps".to_string());

        debug!(
            container_id = %container_id,
            "Running docker compose ps"
        );

        let cmd_refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
        self.runtime.exec_in_coast(container_id, &cmd_refs).await
    }

    /// Run `docker compose logs` inside the coast container.
    ///
    /// Optionally filters to a specific service and/or follows the log stream.
    pub async fn logs(
        &self,
        container_id: &str,
        compose_file: &str,
        service: Option<&str>,
        follow: bool,
    ) -> Result<ExecResult> {
        let mut args = self.compose_base_args(compose_file, None);
        args.push("logs".to_string());

        if follow {
            args.push("--follow".to_string());
        }

        if let Some(svc) = service {
            args.push(svc.to_string());
        }

        debug!(
            container_id = %container_id,
            service = ?service,
            follow = follow,
            "Running docker compose logs"
        );

        let cmd_refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
        self.runtime.exec_in_coast(container_id, &cmd_refs).await
    }

    /// Build the base compose command arguments.
    ///
    /// Returns the command prefix and file arguments for a compose operation.
    fn compose_base_args(&self, compose_file: &str, override_file: Option<&str>) -> Vec<String> {
        let mut args = Vec::new();

        if self.runtime.name() == "podman" {
            args.push(PODMAN_COMPOSE_CMD.to_string());
        } else {
            args.push(COMPOSE_CMD.to_string());
            args.push(COMPOSE_SUBCMD.to_string());
        }

        args.push("-f".to_string());
        args.push(compose_file.to_string());

        if let Some(override_path) = override_file {
            args.push("-f".to_string());
            args.push(override_path.to_string());
        }

        args
    }
}

/// Generate a docker-compose override YAML that removes shared services
/// and injects connection environment variables.
///
/// When shared services are configured (e.g., a shared postgres running on the
/// host daemon), the inner compose stack should not start its own copy of that
/// service. This function generates an override file that:
///
/// 1. Sets `profiles: ["disabled"]` on the shared service to effectively disable it.
/// 2. Adds connection environment variables to dependent services.
///
/// Returns the override YAML content as a string.
pub fn generate_shared_service_override(
    shared_services: &[SharedServiceConfig],
    shared_service_hosts: &HashMap<String, String>,
    instance_name: &str,
    project_name: &str,
) -> Result<String> {
    if shared_services.is_empty() {
        return Ok(String::new());
    }

    let mut yaml = String::from("# Auto-generated by Coast - do not edit\n");
    yaml.push_str("# Overrides for shared services\n");
    yaml.push_str("services:\n");

    for service in shared_services {
        // Disable the local service by assigning it to a profile that won't be activated
        yaml.push_str(&format!("  {}:\n", service.name));
        yaml.push_str("    profiles:\n");
        yaml.push_str("      - disabled\n");

        // If the service has an inject configuration, add env vars to
        // other services that might depend on it.
        if let Some(inject) = &service.inject {
            let inject_str = inject.to_inject_string();
            if let Some(env_var) = inject_str.strip_prefix("env:") {
                let host = shared_service_hosts
                    .get(&service.name)
                    .map(std::string::String::as_str)
                    .unwrap_or("localhost");

                let port = service.ports.first().copied().unwrap_or(5432);
                let db_name = format!("{instance_name}_{project_name}");

                let connection_url = build_connection_url(&service.image, host, port, &db_name);

                yaml.push_str(&format!("  # Connection env for {}: ", service.name));
                yaml.push_str(&format!("{env_var}={connection_url}\n"));
            }
        }
    }

    Ok(yaml)
}

/// Build a connection URL based on the service image type.
///
/// For postgres images, generates a `postgres://` URL.
/// For redis images, generates a `redis://` URL.
/// For unknown images, generates a generic `host:port` string.
pub fn build_connection_url(image: &str, host: &str, port: u16, db_name: &str) -> String {
    if image.contains("postgres") {
        format!("postgres://postgres:dev@{host}:{port}/{db_name}")
    } else if image.contains("redis") {
        format!("redis://{host}:{port}")
    } else if image.contains("mysql") || image.contains("mariadb") {
        format!("mysql://root:dev@{host}:{port}/{db_name}")
    } else {
        format!("{host}:{port}")
    }
}

/// Determine which services in a compose file should be removed because
/// they are handled as shared services.
///
/// Returns a list of service names that should be disabled in the override.
pub fn services_to_remove(shared_services: &[SharedServiceConfig]) -> Vec<String> {
    shared_services.iter().map(|s| s.name.clone()).collect()
}

/// Generate compose-override YAML that adds `extra_hosts` to every listed
/// service so inner containers can reach the host machine via
/// `host.docker.internal`.
///
/// `host_ip` is the resolved Docker bridge gateway (e.g. `"172.17.0.1"`).
pub fn generate_egress_extra_hosts_yaml(services: &[String], host_ip: &str) -> String {
    if services.is_empty() {
        return String::new();
    }
    let mut yaml = String::new();
    for svc in services {
        yaml.push_str(&format!("  {}:\n", svc));
        yaml.push_str("    extra_hosts:\n");
        yaml.push_str(&format!("      - \"host.docker.internal:{}\"\n", host_ip));
    }
    yaml
}

/// Extract the top-level service names from a Docker Compose YAML string.
///
/// Parses the YAML and returns the keys under the `services:` mapping.
/// Returns an empty vec if the file has no `services` section.
pub fn extract_compose_services(yaml_content: &str) -> Vec<String> {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml_content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(serde_yaml::Value::Mapping(services)) = value.get("services") else {
        return Vec::new();
    };
    services
        .keys()
        .filter_map(|k| k.as_str().map(String::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::types::InjectType;

    #[test]
    fn test_compose_base_args_docker() {
        // Create a minimal mock to test arg construction
        let args = build_compose_args_for_test("dind", "/workspace/docker-compose.yml", None);
        assert_eq!(args[0], "docker");
        assert_eq!(args[1], "compose");
        assert_eq!(args[2], "-f");
        assert_eq!(args[3], "/workspace/docker-compose.yml");
        assert_eq!(args.len(), 4);
    }

    #[test]
    fn test_compose_base_args_with_override() {
        let args = build_compose_args_for_test(
            "dind",
            "/workspace/docker-compose.yml",
            Some("/workspace/override.yml"),
        );
        assert_eq!(args.len(), 6);
        assert_eq!(args[4], "-f");
        assert_eq!(args[5], "/workspace/override.yml");
    }

    #[test]
    fn test_compose_base_args_podman() {
        let args = build_compose_args_for_test("podman", "/workspace/docker-compose.yml", None);
        assert_eq!(args[0], "podman-compose");
        assert_eq!(args[1], "-f");
        assert_eq!(args[2], "/workspace/docker-compose.yml");
        assert_eq!(args.len(), 3);
    }

    /// Helper to test compose arg building without needing a real Runtime.
    fn build_compose_args_for_test(
        runtime_name: &str,
        compose_file: &str,
        override_file: Option<&str>,
    ) -> Vec<String> {
        let mut args = Vec::new();
        if runtime_name == "podman" {
            args.push(PODMAN_COMPOSE_CMD.to_string());
        } else {
            args.push(COMPOSE_CMD.to_string());
            args.push(COMPOSE_SUBCMD.to_string());
        }
        args.push("-f".to_string());
        args.push(compose_file.to_string());
        if let Some(override_path) = override_file {
            args.push("-f".to_string());
            args.push(override_path.to_string());
        }
        args
    }

    #[test]
    fn test_build_connection_url_postgres() {
        let url = build_connection_url("postgres:16", "shared-pg", 5432, "feature_oauth_myapp");
        assert_eq!(
            url,
            "postgres://postgres:dev@shared-pg:5432/feature_oauth_myapp"
        );
    }

    #[test]
    fn test_build_connection_url_redis() {
        let url = build_connection_url("redis:7", "shared-redis", 6379, "ignored");
        assert_eq!(url, "redis://shared-redis:6379");
    }

    #[test]
    fn test_build_connection_url_mysql() {
        let url = build_connection_url("mysql:8", "shared-mysql", 3306, "mydb");
        assert_eq!(url, "mysql://root:dev@shared-mysql:3306/mydb");
    }

    #[test]
    fn test_build_connection_url_mariadb() {
        let url = build_connection_url("mariadb:10", "shared-maria", 3306, "mydb");
        assert_eq!(url, "mysql://root:dev@shared-maria:3306/mydb");
    }

    #[test]
    fn test_build_connection_url_unknown() {
        let url = build_connection_url("custom-service:latest", "host", 8080, "db");
        assert_eq!(url, "host:8080");
    }

    #[test]
    fn test_services_to_remove() {
        let shared = vec![
            SharedServiceConfig {
                name: "postgres".to_string(),
                image: "postgres:16".to_string(),
                ports: vec![5432],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: true,
                inject: Some(InjectType::Env("DATABASE_URL".to_string())),
            },
            SharedServiceConfig {
                name: "redis".to_string(),
                image: "redis:7".to_string(),
                ports: vec![6379],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: false,
                inject: Some(InjectType::Env("REDIS_URL".to_string())),
            },
        ];

        let to_remove = services_to_remove(&shared);
        assert_eq!(to_remove.len(), 2);
        assert!(to_remove.contains(&"postgres".to_string()));
        assert!(to_remove.contains(&"redis".to_string()));
    }

    #[test]
    fn test_services_to_remove_empty() {
        let to_remove = services_to_remove(&[]);
        assert!(to_remove.is_empty());
    }

    #[test]
    fn test_generate_shared_service_override_empty() {
        let result = generate_shared_service_override(&[], &HashMap::new(), "test", "app").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_generate_shared_service_override_postgres() {
        let shared = vec![SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![5432],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: true,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        }];

        let mut hosts = HashMap::new();
        hosts.insert("postgres".to_string(), "coast-shared-pg".to_string());

        let override_yaml =
            generate_shared_service_override(&shared, &hosts, "feature-oauth", "my-app").unwrap();

        assert!(override_yaml.contains("services:"));
        assert!(override_yaml.contains("postgres:"));
        assert!(override_yaml.contains("profiles:"));
        assert!(override_yaml.contains("disabled"));
        assert!(override_yaml.contains("DATABASE_URL"));
        assert!(override_yaml.contains("coast-shared-pg"));
    }

    #[test]
    fn test_generate_shared_service_override_no_inject() {
        let shared = vec![SharedServiceConfig {
            name: "redis".to_string(),
            image: "redis:7".to_string(),
            ports: vec![6379],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: None,
        }];

        let override_yaml =
            generate_shared_service_override(&shared, &HashMap::new(), "test", "app").unwrap();

        assert!(override_yaml.contains("redis:"));
        assert!(override_yaml.contains("disabled"));
        // No connection URL since inject is None
        assert!(!override_yaml.contains("REDIS_URL"));
    }

    #[test]
    fn test_generate_shared_service_override_file_inject_skipped() {
        let shared = vec![SharedServiceConfig {
            name: "secret-svc".to_string(),
            image: "custom:latest".to_string(),
            ports: vec![8080],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::File(std::path::PathBuf::from(
                "/run/secrets/key",
            ))),
        }];

        let override_yaml =
            generate_shared_service_override(&shared, &HashMap::new(), "test", "app").unwrap();

        // File injection doesn't generate env var override
        assert!(override_yaml.contains("secret-svc:"));
        assert!(override_yaml.contains("disabled"));
    }

    #[test]
    fn test_generate_shared_service_override_default_host() {
        let shared = vec![SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![5432],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: true,
            inject: Some(InjectType::Env("DB_URL".to_string())),
        }];

        // Empty hosts map - should fall back to "localhost"
        let override_yaml =
            generate_shared_service_override(&shared, &HashMap::new(), "test", "app").unwrap();

        assert!(override_yaml.contains("localhost"));
    }

    #[test]
    fn test_compose_cmd_constant() {
        assert_eq!(COMPOSE_CMD, "docker");
    }

    #[test]
    fn test_compose_subcmd_constant() {
        assert_eq!(COMPOSE_SUBCMD, "compose");
    }

    #[test]
    fn test_podman_compose_cmd_constant() {
        assert_eq!(PODMAN_COMPOSE_CMD, "podman-compose");
    }

    #[test]
    fn test_generate_override_multiple_services() {
        let shared = vec![
            SharedServiceConfig {
                name: "postgres".to_string(),
                image: "postgres:16".to_string(),
                ports: vec![5432],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: true,
                inject: Some(InjectType::Env("DATABASE_URL".to_string())),
            },
            SharedServiceConfig {
                name: "redis".to_string(),
                image: "redis:7".to_string(),
                ports: vec![6379],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: false,
                inject: Some(InjectType::Env("REDIS_URL".to_string())),
            },
        ];

        let mut hosts = HashMap::new();
        hosts.insert("postgres".to_string(), "pg-host".to_string());
        hosts.insert("redis".to_string(), "redis-host".to_string());

        let yaml =
            generate_shared_service_override(&shared, &hosts, "test-inst", "my-app").unwrap();

        assert!(yaml.contains("postgres:"));
        assert!(yaml.contains("redis:"));
        assert!(yaml.contains("DATABASE_URL"));
        assert!(yaml.contains("REDIS_URL"));
    }

    // --- egress extra_hosts tests ---

    #[test]
    fn test_generate_egress_extra_hosts_yaml_single_service() {
        let services = vec!["app".to_string()];
        let yaml = generate_egress_extra_hosts_yaml(&services, "172.17.0.1");
        assert!(yaml.contains("app:"));
        assert!(yaml.contains("extra_hosts:"));
        assert!(yaml.contains("host.docker.internal:172.17.0.1"));
    }

    #[test]
    fn test_generate_egress_extra_hosts_yaml_multiple_services() {
        let services = vec!["app".to_string(), "worker".to_string(), "db".to_string()];
        let yaml = generate_egress_extra_hosts_yaml(&services, "192.168.65.1");
        assert!(yaml.contains("app:"));
        assert!(yaml.contains("worker:"));
        assert!(yaml.contains("db:"));
        let count = yaml.matches("host.docker.internal:192.168.65.1").count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_generate_egress_extra_hosts_yaml_empty() {
        let yaml = generate_egress_extra_hosts_yaml(&[], "172.17.0.1");
        assert!(yaml.is_empty());
    }

    // --- extract_compose_services tests ---

    #[test]
    fn test_extract_compose_services_basic() {
        let yaml = r#"
services:
  app:
    image: node:18
  db:
    image: postgres:16
  cache:
    image: redis:7
"#;
        let mut services = extract_compose_services(yaml);
        services.sort();
        assert_eq!(services, vec!["app", "cache", "db"]);
    }

    #[test]
    fn test_extract_compose_services_single() {
        let yaml = r#"
services:
  web:
    build: .
    ports:
      - "3000:3000"
"#;
        let services = extract_compose_services(yaml);
        assert_eq!(services, vec!["web"]);
    }

    #[test]
    fn test_extract_compose_services_no_services() {
        let yaml = r#"
version: "3.8"
volumes:
  data:
"#;
        let services = extract_compose_services(yaml);
        assert!(services.is_empty());
    }

    #[test]
    fn test_extract_compose_services_invalid_yaml() {
        let services = extract_compose_services("{{not valid yaml");
        assert!(services.is_empty());
    }

    #[test]
    fn test_extract_compose_services_empty_string() {
        let services = extract_compose_services("");
        assert!(services.is_empty());
    }
}
