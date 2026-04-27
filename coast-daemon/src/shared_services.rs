/// Shared service management for the Coast daemon.
///
/// Manages shared service containers that run on the host Docker daemon and
/// are shared across multiple coast instances within a project. Examples include
/// a shared PostgreSQL database that multiple instances connect to.
///
/// Shared service data outlives instance deletion -- `coast rm` never touches
/// shared service data. Only `coast shared-services rm` does.
use std::collections::HashMap;
use std::path::PathBuf;

use tracing::debug;

use coast_core::error::{CoastError, Result};
use coast_core::types::{InjectType, SharedServiceConfig};

/// Label key for identifying coast-managed shared service containers.
pub const COAST_SHARED_LABEL: &str = "coast.shared-service";

/// Generate the Docker bridge network name for a project's shared services.
///
/// Format: `coast-shared-{project}`
///
/// This network connects coast containers to shared services running on the
/// host Docker daemon.
pub fn shared_network_name(project: &str) -> String {
    format!("coast-shared-{project}")
}

/// Generate the Docker container name for a shared service.
///
/// Format: `{project}-shared-services-{service}`
///
/// Uses a separate compose project (`{project}-shared-services`) so Docker
/// Desktop shows shared services in their own group, distinct from coast
/// instances (`{project}-coasts`).
pub fn shared_container_name(project: &str, service: &str) -> String {
    format!("{project}-shared-services-{service}")
}

/// Generate the per-instance database name.
///
/// Format: `{instance}_{db_name}`
///
/// Used for auto-created databases in shared services like PostgreSQL,
/// giving each coast instance its own isolated database.
pub fn database_name(instance: &str, db_name: &str) -> String {
    format!("{instance}_{db_name}")
}

/// Generate the command to create a database in a shared database service.
///
/// Different database engines require different SQL syntax. Currently
/// supports:
/// - `postgres` / `postgresql`: Uses a PL/pgSQL `DO` block to conditionally
///   create the database (PostgreSQL does not support `CREATE DATABASE IF NOT EXISTS`).
///
/// # Arguments
///
/// * `db_type` - The database engine type (e.g., "postgres", "postgresql", "mysql").
/// * `db_name` - The name of the database to create.
///
/// # Returns
///
/// A vector of strings representing the command to execute inside the
/// shared service container.
pub fn create_db_command(db_type: &str, db_name: &str) -> Vec<String> {
    match db_type.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            // PostgreSQL does not support CREATE DATABASE IF NOT EXISTS,
            // and psql's `\gexec` meta-command is rejected by `-c` mode.
            // Emulate "CREATE IF NOT EXISTS" with a shell conditional:
            // first query `pg_database`, then create only if the row is
            // absent. `sh -c` is available in both postgres:*-alpine
            // and postgres:*-bookworm images.
            //
            // Container-quoting note: this command travels through
            // `docker exec ... sh -c "<shell>"` (for inline) and
            // `docker compose exec -T postgres sh -c "<shell>"` (for
            // SSG). Each intermediate hop passes argv elements as
            // separate strings — no extra escaping needed.
            let shell = format!(
                r#"if [ -z "$(psql -U postgres -tAc "SELECT 1 FROM pg_database WHERE datname = '{db_name}'")" ]; then psql -U postgres -c 'CREATE DATABASE "{db_name}"'; fi"#
            );
            vec!["sh".to_string(), "-c".to_string(), shell]
        }
        "mysql" | "mariadb" => {
            let sql = format!("CREATE DATABASE IF NOT EXISTS `{db_name}`;");
            vec![
                "mysql".to_string(),
                "-u".to_string(),
                "root".to_string(),
                "-e".to_string(),
                sql,
            ]
        }
        other => {
            // Unknown database type -- cannot construct a reliable CREATE DATABASE
            // command. Log and return a shell command that prints a warning.
            debug!(
                db_type = other,
                db_name = db_name,
                "Unknown database type, cannot auto-create database"
            );
            vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("echo 'Unsupported db_type: {other}. Cannot auto-create database.'"),
            ]
        }
    }
}

/// Extract the Docker named volume from a volume bind string.
///
/// Given `"pg_data:/var/lib/postgresql/data"`, returns `Some("pg_data")`.
/// Returns `None` for bind mounts (paths starting with `/` or `.`).
/// Used by `coast shared-services rm` to identify volumes to clean up.
pub fn extract_named_volume(volume_str: &str) -> Option<&str> {
    if let Some(colon_pos) = volume_str.find(':') {
        let source = &volume_str[..colon_pos];
        if source.starts_with('/') || source.starts_with('.') {
            None
        } else {
            Some(source)
        }
    } else if !volume_str.starts_with('/') && !volume_str.starts_with('.') {
        Some(volume_str)
    } else {
        None
    }
}

/// Configuration for creating a shared service container on the host daemon.
///
/// Translates a `SharedServiceConfig` from the Coastfile into concrete
/// Docker container creation parameters.
#[derive(Debug, Clone)]
pub struct SharedContainerConfig {
    /// Container name.
    pub name: String,
    /// Docker image.
    pub image: String,
    /// Environment variables.
    pub env: Vec<String>,
    /// Port bindings (host_port:container_port).
    pub ports: Vec<String>,
    /// Volume mounts.
    pub volumes: Vec<String>,
    /// Network to attach the container to.
    pub network: String,
    /// Labels for the container.
    pub labels: HashMap<String, String>,
}

/// Build a `SharedContainerConfig` from the Coastfile's shared service config.
///
/// # Arguments
///
/// * `project` - The project name.
/// * `config` - The shared service configuration from the Coastfile.
pub fn build_shared_container_config(
    project: &str,
    config: &SharedServiceConfig,
) -> SharedContainerConfig {
    let name = shared_container_name(project, &config.name);
    let network = shared_network_name(project);

    // Convert env HashMap to Docker-style "KEY=VALUE" strings
    let env: Vec<String> = config.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

    // Convert shared service mappings to "host:container" binding strings.
    let ports: Vec<String> = config
        .ports
        .iter()
        .map(|port| format!("{}:{}", port.forwarding_port, port.container_port))
        .collect();

    let mut labels = HashMap::new();
    labels.insert(COAST_SHARED_LABEL.to_string(), config.name.clone());
    labels.insert("coast.project".to_string(), project.to_string());
    labels.insert("coast.managed".to_string(), "true".to_string());
    labels.insert(
        "com.docker.compose.project".to_string(),
        format!("{}-shared-services", project),
    );
    labels.insert(
        "com.docker.compose.service".to_string(),
        config.name.clone(),
    );
    labels.insert(
        "com.docker.compose.container-number".to_string(),
        "1".to_string(),
    );
    labels.insert("com.docker.compose.oneoff".to_string(), "False".to_string());

    SharedContainerConfig {
        name,
        image: config.image.clone(),
        env,
        ports,
        volumes: config.volumes.clone(),
        network,
        labels,
    }
}

/// Generate the list of database names to auto-create for a set of instances.
///
/// For each instance, creates a database name using the `database_name` function.
///
/// # Arguments
///
/// * `instances` - Slice of instance names.
/// * `base_db_name` - The base database name from the service configuration.
pub fn auto_create_db_names(instances: &[&str], base_db_name: &str) -> Vec<String> {
    instances
        .iter()
        .map(|instance| database_name(instance, base_db_name))
        .collect()
}

/// Infer the database engine kind from a Docker image reference.
///
/// Returns the string accepted by [`create_db_command`] (`"postgres"`,
/// `"mysql"`) or `None` when the image is not a recognized DB engine —
/// non-DB services should skip `auto_create_db` rather than error.
///
/// See `coast-ssg/DESIGN.md §13`.
pub fn infer_db_type(image: &str) -> Option<&'static str> {
    let lower = image.to_lowercase();
    if lower.contains("postgres") || lower.contains("postgis") {
        Some("postgres")
    } else if lower.contains("mariadb") || lower.contains("mysql") {
        Some("mysql")
    } else {
        None
    }
}

/// Per-instance database name for a consumer coast: `{instance}_{project}`.
///
/// This convention is shared by `auto_create_db` (the DB we create
/// inside the shared service) and `inject` (the DB name embedded in
/// the connection URL). Keeping them identical means the consumer's
/// env var always points at the DB we actually created. See
/// `coast-ssg/DESIGN.md §13` and the matching URL builder in
/// [`coast-docker/src/compose.rs::build_connection_url`].
pub fn consumer_db_name(instance: &str, project: &str) -> String {
    format!("{instance}_{project}")
}

/// Compute the env vars to inject into a consumer coast for every
/// shared service with `inject = Some(InjectType::Env(name))`.
///
/// Resolution rules (DESIGN.md §14):
/// - Host: the service name (DNS-routable inside the coast via the
///   socat-alias network).
/// - Port: the canonical container port (first declared entry).
///   Explicitly NOT the dynamic host port.
/// - DB name: [`consumer_db_name`].
///
/// Services with `inject = Some(InjectType::File(_))` are handled by
/// the sibling function [`shared_service_inject_file_writes`] — this
/// function only covers the `env:` variant. Services without a
/// declared port fall back to the image's default (postgres 5432,
/// mysql 3306, redis 6379) via
/// [`coast_docker::compose::build_connection_url`].
pub fn shared_service_inject_env_vars(
    services: &[SharedServiceConfig],
    project: &str,
    instance: &str,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let db_name = consumer_db_name(instance, project);

    for svc in services {
        let Some(inject) = &svc.inject else { continue };
        let var_name = match inject {
            InjectType::Env(name) => name,
            // File injects produce file bodies, not env vars. The
            // runtime path for those is
            // `shared_service_inject_file_writes`.
            InjectType::File(_) => continue,
        };
        let url = build_inject_url(svc, &db_name);
        out.insert(var_name.clone(), url);
    }
    out
}

/// Compute the file writes to inject into a consumer coast for every
/// shared service with `inject = Some(InjectType::File(path))`.
///
/// Each entry pairs the inner container path declared by the
/// consumer with the connection URL that would be written into the
/// file. The URL is byte-identical to what
/// [`shared_service_inject_env_vars`] would set for the same
/// service, so either inject variant lands the consumer at the same
/// database.
///
/// Errors when any `inject = "file:<path>"` is relative. Docker bind
/// mounts require absolute paths and the parser (intentionally)
/// accepts any string after `file:`; validating here keeps the error
/// close to the user's Coastfile rather than surfacing as a
/// late-stage Docker mount failure.
pub fn shared_service_inject_file_writes(
    services: &[SharedServiceConfig],
    project: &str,
    instance: &str,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut out = Vec::new();
    let db_name = consumer_db_name(instance, project);

    for svc in services {
        let Some(inject) = &svc.inject else { continue };
        let container_path = match inject {
            InjectType::File(path) => path,
            // Env injects are handled by the sibling function.
            InjectType::Env(_) => continue,
        };
        if !container_path.is_absolute() {
            return Err(CoastError::coastfile(format!(
                "shared service '{service}' declares `inject = \"file:{path}\"` but the path must \
                 be absolute (Docker bind mounts cannot target relative paths). Use e.g. \
                 `file:/run/secrets/{service}_url`.",
                service = svc.name,
                path = container_path.display(),
            )));
        }
        let url = build_inject_url(svc, &db_name);
        out.push((container_path.clone(), url.into_bytes()));
    }
    Ok(out)
}

/// Canonical inject connection URL for a shared service + consumer
/// (instance, project). Shared by the env and file inject paths so
/// both variants produce byte-identical bodies. See DESIGN.md §14.
fn build_inject_url(svc: &SharedServiceConfig, db_name: &str) -> String {
    let host = svc.name.as_str();
    let port = svc
        .ports
        .first()
        .map(|p| p.container_port)
        .unwrap_or_else(|| default_port_for_image(&svc.image));
    coast_docker::compose::build_connection_url(&svc.image, host, port, db_name)
}

/// Canonical port fallback when a shared service declares no
/// `ports = [...]` list. Matches the image-kind heuristics in
/// [`coast_docker::compose::build_connection_url`] so the fallback and
/// URL-shape stay consistent.
fn default_port_for_image(image: &str) -> u16 {
    let lower = image.to_lowercase();
    if lower.contains("postgres") {
        5432
    } else if lower.contains("mysql") || lower.contains("mariadb") {
        3306
    } else if lower.contains("redis") {
        6379
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::types::SharedServicePort;

    // -----------------------------------------------------------
    // shared_network_name tests
    // -----------------------------------------------------------

    #[test]
    fn test_shared_network_name_basic() {
        assert_eq!(shared_network_name("my-app"), "coast-shared-my-app");
    }

    #[test]
    fn test_shared_network_name_with_underscores() {
        assert_eq!(shared_network_name("my_app"), "coast-shared-my_app");
    }

    #[test]
    fn test_shared_network_name_simple() {
        assert_eq!(shared_network_name("app"), "coast-shared-app");
    }

    #[test]
    fn test_shared_network_name_with_numbers() {
        assert_eq!(shared_network_name("app123"), "coast-shared-app123");
    }

    #[test]
    fn test_shared_network_name_empty() {
        assert_eq!(shared_network_name(""), "coast-shared-");
    }

    #[test]
    fn test_shared_network_name_complex() {
        assert_eq!(
            shared_network_name("my-cool-project"),
            "coast-shared-my-cool-project"
        );
    }

    // -----------------------------------------------------------
    // shared_container_name tests
    // -----------------------------------------------------------

    #[test]
    fn test_shared_container_name_basic() {
        assert_eq!(
            shared_container_name("my-app", "postgres"),
            "my-app-shared-services-postgres"
        );
    }

    #[test]
    fn test_shared_container_name_redis() {
        assert_eq!(
            shared_container_name("my-app", "redis"),
            "my-app-shared-services-redis"
        );
    }

    #[test]
    fn test_shared_container_name_with_hyphens() {
        assert_eq!(
            shared_container_name("my-cool-app", "my-db"),
            "my-cool-app-shared-services-my-db"
        );
    }

    #[test]
    fn test_shared_container_name_empty_project() {
        assert_eq!(
            shared_container_name("", "postgres"),
            "-shared-services-postgres"
        );
    }

    #[test]
    fn test_shared_container_name_empty_service() {
        assert_eq!(
            shared_container_name("my-app", ""),
            "my-app-shared-services-"
        );
    }

    // -----------------------------------------------------------
    // database_name tests
    // -----------------------------------------------------------

    #[test]
    fn test_database_name_basic() {
        assert_eq!(database_name("feature-oauth", "mydb"), "feature-oauth_mydb");
    }

    #[test]
    fn test_database_name_with_underscores() {
        assert_eq!(database_name("my_instance", "app_db"), "my_instance_app_db");
    }

    #[test]
    fn test_database_name_main() {
        assert_eq!(database_name("main", "postgres"), "main_postgres");
    }

    #[test]
    fn test_database_name_empty_instance() {
        assert_eq!(database_name("", "mydb"), "_mydb");
    }

    #[test]
    fn test_database_name_empty_db() {
        assert_eq!(database_name("instance", ""), "instance_");
    }

    #[test]
    fn test_database_name_complex() {
        assert_eq!(
            database_name("feature-billing-v2", "app_development"),
            "feature-billing-v2_app_development"
        );
    }

    // -----------------------------------------------------------
    // create_db_command tests
    // -----------------------------------------------------------

    #[test]
    fn test_create_db_command_postgres() {
        let cmd = create_db_command("postgres", "mydb");
        // Uses sh -c for the "create-if-not-exists" emulation; see
        // create_db_command doc for why \gexec can't be used with -c.
        assert_eq!(cmd[0], "sh");
        assert_eq!(cmd[1], "-c");
        let shell = &cmd[2];
        assert!(shell.contains("mydb"));
        assert!(shell.contains("CREATE DATABASE"));
        assert!(shell.contains("pg_database"));
        assert!(
            !shell.contains("\\gexec"),
            "shell command must not use \\gexec (broken with psql -c)"
        );
    }

    #[test]
    fn test_create_db_command_postgresql() {
        let cmd = create_db_command("postgresql", "testdb");
        assert_eq!(cmd[0], "sh");
        assert!(cmd[2].contains("testdb"));
    }

    #[test]
    fn test_create_db_command_postgres_case_insensitive() {
        let cmd = create_db_command("POSTGRES", "mydb");
        assert_eq!(cmd[0], "sh");
    }

    #[test]
    fn test_create_db_command_mysql() {
        let cmd = create_db_command("mysql", "mydb");
        assert_eq!(cmd[0], "mysql");
        assert_eq!(cmd[1], "-u");
        assert_eq!(cmd[2], "root");
        assert_eq!(cmd[3], "-e");
        assert!(cmd[4].contains("CREATE DATABASE IF NOT EXISTS"));
        assert!(cmd[4].contains("mydb"));
    }

    #[test]
    fn test_create_db_command_mariadb() {
        let cmd = create_db_command("mariadb", "testdb");
        assert_eq!(cmd[0], "mysql");
        assert!(cmd[4].contains("testdb"));
    }

    #[test]
    fn test_create_db_command_unknown_type() {
        let cmd = create_db_command("cockroachdb", "mydb");
        assert_eq!(cmd[0], "sh");
        assert_eq!(cmd[1], "-c");
        assert!(cmd[2].contains("Unsupported"));
    }

    #[test]
    fn test_create_db_command_postgres_special_chars_in_name() {
        let cmd = create_db_command("postgres", "feature-oauth_dev");
        assert!(cmd[2].contains("feature-oauth_dev"));
    }

    // -----------------------------------------------------------
    // build_shared_container_config tests
    // -----------------------------------------------------------

    #[test]
    fn test_build_shared_container_config_basic() {
        let mut env = HashMap::new();
        env.insert("POSTGRES_PASSWORD".to_string(), "dev".to_string());
        env.insert("POSTGRES_USER".to_string(), "postgres".to_string());

        let service_config = SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![SharedServicePort::same(5432)],
            volumes: vec!["coast_shared_pg:/var/lib/postgresql/data".to_string()],
            env,
            auto_create_db: true,
            inject: None,
        };

        let config = build_shared_container_config("my-app", &service_config);

        assert_eq!(config.name, "my-app-shared-services-postgres");
        assert_eq!(config.image, "postgres:16");
        assert_eq!(config.network, "coast-shared-my-app");
        assert_eq!(config.ports, vec!["5432:5432"]);
        assert_eq!(
            config.volumes,
            vec!["coast_shared_pg:/var/lib/postgresql/data"]
        );
        assert!(config.env.contains(&"POSTGRES_PASSWORD=dev".to_string()));
        assert!(config.env.contains(&"POSTGRES_USER=postgres".to_string()));
    }

    #[test]
    fn test_build_shared_container_config_labels() {
        let service_config = SharedServiceConfig {
            name: "redis".to_string(),
            image: "redis:7".to_string(),
            ports: vec![SharedServicePort::same(6379)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: None,
        };

        let config = build_shared_container_config("my-app", &service_config);

        assert_eq!(
            config.labels.get(COAST_SHARED_LABEL),
            Some(&"redis".to_string())
        );
        assert_eq!(
            config.labels.get("coast.project"),
            Some(&"my-app".to_string())
        );
        assert_eq!(
            config.labels.get("coast.managed"),
            Some(&"true".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.project"),
            Some(&"my-app-shared-services".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.service"),
            Some(&"redis".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.container-number"),
            Some(&"1".to_string())
        );
        assert_eq!(
            config.labels.get("com.docker.compose.oneoff"),
            Some(&"False".to_string())
        );
    }

    #[test]
    fn test_build_shared_container_config_mapped_ports() {
        let service_config = SharedServiceConfig {
            name: "postgis-db".to_string(),
            image: "ghcr.io/baosystems/postgis:12-3.3".to_string(),
            ports: vec![SharedServicePort::new(5433, 5432)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: None,
        };

        let config = build_shared_container_config("yc", &service_config);

        assert_eq!(config.ports, vec!["5433:5432"]);
    }

    #[test]
    fn test_build_shared_container_config_multiple_ports() {
        let service_config = SharedServiceConfig {
            name: "multi-port".to_string(),
            image: "some-image:latest".to_string(),
            ports: vec![
                SharedServicePort::same(5432),
                SharedServicePort::same(8080),
                SharedServicePort::same(9090),
            ],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: None,
        };

        let config = build_shared_container_config("proj", &service_config);

        assert_eq!(config.ports.len(), 3);
        assert!(config.ports.contains(&"5432:5432".to_string()));
        assert!(config.ports.contains(&"8080:8080".to_string()));
        assert!(config.ports.contains(&"9090:9090".to_string()));
    }

    #[test]
    fn test_build_shared_container_config_no_ports_no_volumes() {
        let service_config = SharedServiceConfig {
            name: "minimal".to_string(),
            image: "alpine:latest".to_string(),
            ports: vec![],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: None,
        };

        let config = build_shared_container_config("proj", &service_config);

        assert!(config.ports.is_empty());
        assert!(config.volumes.is_empty());
        assert!(config.env.is_empty());
    }

    // -----------------------------------------------------------
    // auto_create_db_names tests
    // -----------------------------------------------------------

    #[test]
    fn test_auto_create_db_names_single() {
        let names = auto_create_db_names(&["feature-oauth"], "mydb");
        assert_eq!(names, vec!["feature-oauth_mydb"]);
    }

    #[test]
    fn test_auto_create_db_names_multiple() {
        let names = auto_create_db_names(&["main", "feature-a", "feature-b"], "app_dev");
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "main_app_dev");
        assert_eq!(names[1], "feature-a_app_dev");
        assert_eq!(names[2], "feature-b_app_dev");
    }

    #[test]
    fn test_auto_create_db_names_empty() {
        let names = auto_create_db_names(&[], "mydb");
        assert!(names.is_empty());
    }

    // -----------------------------------------------------------
    // extract_named_volume tests
    // -----------------------------------------------------------

    #[test]
    fn test_extract_named_volume_basic() {
        assert_eq!(
            extract_named_volume("pg_data:/var/lib/postgresql/data"),
            Some("pg_data")
        );
    }

    #[test]
    fn test_extract_named_volume_bind_mount_returns_none() {
        assert_eq!(extract_named_volume("/host/path:/container/path"), None);
    }

    #[test]
    fn test_extract_named_volume_relative_returns_none() {
        assert_eq!(extract_named_volume("./data:/container/path"), None);
    }

    #[test]
    fn test_extract_named_volume_bare() {
        assert_eq!(extract_named_volume("myvolume"), Some("myvolume"));
    }

    #[test]
    fn test_extract_named_volume_with_opts() {
        assert_eq!(
            extract_named_volume("redis_data:/data:ro"),
            Some("redis_data")
        );
    }

    #[test]
    fn test_extract_named_volume_infra_postgres() {
        assert_eq!(
            extract_named_volume("infra_postgres_data:/var/lib/postgresql/data"),
            Some("infra_postgres_data")
        );
    }

    // -----------------------------------------------------------
    // Constants tests
    // -----------------------------------------------------------

    #[test]
    fn test_coast_shared_label() {
        assert_eq!(COAST_SHARED_LABEL, "coast.shared-service");
    }

    // -----------------------------------------------------------
    // infer_db_type tests (Phase 5)
    // -----------------------------------------------------------

    #[test]
    fn test_infer_db_type_postgres() {
        assert_eq!(infer_db_type("postgres:16"), Some("postgres"));
        assert_eq!(infer_db_type("postgres:16-alpine"), Some("postgres"));
        assert_eq!(
            infer_db_type("ghcr.io/baosystems/postgis:12-3.3"),
            Some("postgres")
        );
    }

    #[test]
    fn test_infer_db_type_mysql_and_mariadb() {
        assert_eq!(infer_db_type("mysql:8"), Some("mysql"));
        assert_eq!(infer_db_type("MYSQL:latest"), Some("mysql"));
        assert_eq!(infer_db_type("mariadb:11"), Some("mysql"));
    }

    #[test]
    fn test_infer_db_type_non_db_images() {
        assert_eq!(infer_db_type("redis:7"), None);
        assert_eq!(infer_db_type("nginx:1.25"), None);
        assert_eq!(infer_db_type("rabbitmq:3-management"), None);
    }

    // -----------------------------------------------------------
    // consumer_db_name tests (Phase 5)
    // -----------------------------------------------------------

    #[test]
    fn test_consumer_db_name_basic() {
        assert_eq!(consumer_db_name("dev-1", "my-app"), "dev-1_my-app");
    }

    #[test]
    fn test_consumer_db_name_with_underscores_preserved() {
        assert_eq!(
            consumer_db_name("feature_x", "web_app"),
            "feature_x_web_app"
        );
    }

    // -----------------------------------------------------------
    // shared_service_inject_env_vars tests (Phase 5)
    // -----------------------------------------------------------

    fn postgres_cfg_with_inject(inject: Option<InjectType>) -> SharedServiceConfig {
        SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![SharedServicePort::same(5432)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject,
        }
    }

    #[test]
    fn test_inject_env_vars_postgres_builds_connection_url() {
        let svc = postgres_cfg_with_inject(Some(InjectType::Env("DATABASE_URL".to_string())));
        let vars = shared_service_inject_env_vars(&[svc], "my-app", "dev-1");
        assert_eq!(vars.len(), 1);
        assert_eq!(
            vars.get("DATABASE_URL").unwrap(),
            "postgres://postgres:dev@postgres:5432/dev-1_my-app"
        );
    }

    #[test]
    fn test_inject_env_vars_skips_services_without_inject() {
        let svc = postgres_cfg_with_inject(None);
        let vars = shared_service_inject_env_vars(&[svc], "my-app", "dev-1");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_inject_env_vars_skips_file_inject() {
        let svc = postgres_cfg_with_inject(Some(InjectType::File(std::path::PathBuf::from(
            "/run/secrets/db",
        ))));
        let vars = shared_service_inject_env_vars(&[svc], "my-app", "dev-1");
        assert!(vars.is_empty());
    }

    #[test]
    fn test_inject_env_vars_multiple_services_produce_one_var_each() {
        let pg = SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![SharedServicePort::same(5432)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        };
        let redis = SharedServiceConfig {
            name: "redis".to_string(),
            image: "redis:7".to_string(),
            ports: vec![SharedServicePort::same(6379)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::Env("REDIS_URL".to_string())),
        };
        let vars = shared_service_inject_env_vars(&[pg, redis], "my-app", "dev-1");
        assert_eq!(vars.len(), 2);
        assert!(vars.get("DATABASE_URL").unwrap().contains("postgres://"));
        assert_eq!(vars.get("REDIS_URL").unwrap(), "redis://redis:6379");
    }

    #[test]
    fn test_inject_env_vars_falls_back_to_default_port_when_ports_empty() {
        let svc = SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        };
        let vars = shared_service_inject_env_vars(&[svc], "my-app", "dev-1");
        // Falls back to 5432 (default postgres port) rather than 0.
        assert_eq!(
            vars.get("DATABASE_URL").unwrap(),
            "postgres://postgres:dev@postgres:5432/dev-1_my-app"
        );
    }

    #[test]
    fn test_inject_env_vars_uses_canonical_port_not_dynamic() {
        // DESIGN.md §14: port must be the canonical container port,
        // never the dynamic host-published port. The URL builder takes
        // whatever we give it, so we must pass container_port, not
        // host_port.
        let svc = SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![SharedServicePort {
                forwarding_port: 61234,
                container_port: 5432,
            }],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::Env("DATABASE_URL".to_string())),
        };
        let vars = shared_service_inject_env_vars(&[svc], "my-app", "dev-1");
        let url = vars.get("DATABASE_URL").unwrap();
        assert!(url.contains(":5432/"), "expected canonical 5432, got {url}");
        assert!(
            !url.contains("61234"),
            "must not embed dynamic host port, got {url}"
        );
    }

    // -----------------------------------------------------------
    // shared_service_inject_file_writes tests (Phase 13)
    // -----------------------------------------------------------

    #[test]
    fn test_inject_file_writes_env_only_produces_no_file_writes() {
        let svc = postgres_cfg_with_inject(Some(InjectType::Env("DATABASE_URL".to_string())));
        let writes = shared_service_inject_file_writes(&[svc], "my-app", "dev-1").unwrap();
        assert!(writes.is_empty());
    }

    #[test]
    fn test_inject_file_writes_file_only_produces_one_write() {
        let svc = postgres_cfg_with_inject(Some(InjectType::File(std::path::PathBuf::from(
            "/run/secrets/db_url",
        ))));
        let writes = shared_service_inject_file_writes(&[svc], "my-app", "dev-1").unwrap();
        assert_eq!(writes.len(), 1);
        let (path, bytes) = &writes[0];
        assert_eq!(path, &std::path::PathBuf::from("/run/secrets/db_url"));
        assert_eq!(
            std::str::from_utf8(bytes).unwrap(),
            "postgres://postgres:dev@postgres:5432/dev-1_my-app"
        );
    }

    #[test]
    fn test_inject_file_writes_mixed_partitions_env_and_file() {
        let pg = postgres_cfg_with_inject(Some(InjectType::File(std::path::PathBuf::from(
            "/run/secrets/db_url",
        ))));
        let redis = SharedServiceConfig {
            name: "redis".to_string(),
            image: "redis:7".to_string(),
            ports: vec![SharedServicePort::same(6379)],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::Env("REDIS_URL".to_string())),
        };
        let file_writes =
            shared_service_inject_file_writes(&[pg.clone(), redis.clone()], "my-app", "dev-1")
                .unwrap();
        assert_eq!(file_writes.len(), 1);
        assert_eq!(
            file_writes[0].0,
            std::path::PathBuf::from("/run/secrets/db_url")
        );

        let env_vars = shared_service_inject_env_vars(&[pg, redis], "my-app", "dev-1");
        assert_eq!(env_vars.len(), 1);
        assert!(env_vars.contains_key("REDIS_URL"));
    }

    #[test]
    fn test_inject_file_writes_url_bytes_match_env_inject_bytes() {
        // The file body must be byte-identical to the env var value
        // for the same service + consumer so switching inject types
        // doesn't surprise users.
        let svc_env = postgres_cfg_with_inject(Some(InjectType::Env("DATABASE_URL".to_string())));
        let svc_file = postgres_cfg_with_inject(Some(InjectType::File(std::path::PathBuf::from(
            "/run/secrets/db_url",
        ))));
        let env_url = shared_service_inject_env_vars(&[svc_env], "my-app", "dev-1");
        let file_url = shared_service_inject_file_writes(&[svc_file], "my-app", "dev-1").unwrap();

        let env_bytes = env_url.get("DATABASE_URL").unwrap().as_bytes().to_vec();
        let file_bytes = file_url[0].1.clone();
        assert_eq!(env_bytes, file_bytes);
    }

    #[test]
    fn test_inject_file_writes_rejects_relative_path() {
        let svc = postgres_cfg_with_inject(Some(InjectType::File(std::path::PathBuf::from(
            "run/secrets/db_url",
        ))));
        let err = shared_service_inject_file_writes(&[svc], "my-app", "dev-1").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("must be absolute"), "got: {msg}");
        assert!(msg.contains("postgres"), "mentions service name: {msg}");
    }

    #[test]
    fn test_inject_file_writes_skips_services_without_inject() {
        let svc = postgres_cfg_with_inject(None);
        let writes = shared_service_inject_file_writes(&[svc], "my-app", "dev-1").unwrap();
        assert!(writes.is_empty());
    }

    #[test]
    fn test_inject_file_writes_falls_back_to_default_port_when_ports_empty() {
        let svc = SharedServiceConfig {
            name: "postgres".to_string(),
            image: "postgres:16".to_string(),
            ports: vec![],
            volumes: vec![],
            env: HashMap::new(),
            auto_create_db: false,
            inject: Some(InjectType::File(std::path::PathBuf::from(
                "/run/secrets/db_url",
            ))),
        };
        let writes = shared_service_inject_file_writes(&[svc], "my-app", "dev-1").unwrap();
        let body = std::str::from_utf8(&writes[0].1).unwrap();
        assert!(
            body.contains(":5432/"),
            "expected canonical 5432, got {body}"
        );
    }
}
