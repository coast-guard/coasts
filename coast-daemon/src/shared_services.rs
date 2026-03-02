/// Shared service management for the Coast daemon.
///
/// Manages shared service containers that run on the host Docker daemon and
/// are shared across multiple coast instances within a project. Examples include
/// a shared PostgreSQL database that multiple instances connect to.
///
/// Shared service data outlives instance deletion -- `coast rm` never touches
/// shared service data. Only `coast shared-services rm` does.
use std::collections::HashMap;

use tracing::debug;

use coast_core::types::SharedServiceConfig;

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
            // PostgreSQL does not support CREATE DATABASE IF NOT EXISTS.
            // Use the psql \gexec trick: SELECT the CREATE DATABASE statement
            // conditionally, then execute it. This avoids requiring the dblink
            // extension.
            let sql = format!(
                "SELECT 'CREATE DATABASE \"{db_name}\"' WHERE NOT EXISTS \
                 (SELECT FROM pg_database WHERE datname = '{db_name}')\\gexec"
            );
            vec![
                "psql".to_string(),
                "-U".to_string(),
                "postgres".to_string(),
                "-c".to_string(),
                sql,
            ]
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

    // Convert port numbers to "port:port" binding strings
    let ports: Vec<String> = config.ports.iter().map(|p| format!("{p}:{p}")).collect();

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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(cmd[0], "psql");
        assert_eq!(cmd[1], "-U");
        assert_eq!(cmd[2], "postgres");
        assert_eq!(cmd[3], "-c");
        assert!(cmd[4].contains("mydb"));
        assert!(cmd[4].contains("CREATE DATABASE"));
        assert!(cmd[4].contains("NOT EXISTS"));
    }

    #[test]
    fn test_create_db_command_postgresql() {
        let cmd = create_db_command("postgresql", "testdb");
        assert_eq!(cmd[0], "psql");
        assert!(cmd[4].contains("testdb"));
    }

    #[test]
    fn test_create_db_command_postgres_case_insensitive() {
        let cmd = create_db_command("POSTGRES", "mydb");
        assert_eq!(cmd[0], "psql");
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
        assert!(cmd[4].contains("feature-oauth_dev"));
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
            ports: vec![5432],
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
            ports: vec![6379],
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
    fn test_build_shared_container_config_multiple_ports() {
        let service_config = SharedServiceConfig {
            name: "multi-port".to_string(),
            image: "some-image:latest".to_string(),
            ports: vec![5432, 8080, 9090],
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
}
