/// Volume strategy logic for coast instances.
///
/// Handles the two volume strategies (isolated and shared) plus the optional
/// `snapshot_source` field on isolated volumes for copy-on-run seeding.
use crate::types::{VolumeConfig, VolumeStrategy};

/// Database-like service names that trigger warnings for shared volumes.
const DATABASE_SERVICE_NAMES: &[&str] = &[
    "postgres",
    "postgresql",
    "mysql",
    "mariadb",
    "mongo",
    "mongodb",
    "redis",
    "memcached",
    "cassandra",
    "couchdb",
    "cockroachdb",
    "db",
    "database",
];

/// Generate the Docker volume name for an isolated volume.
///
/// Format: `coast--{instance}--{volume_name}`
///
/// Also used for isolated volumes with `snapshot_source` since they are
/// per-instance and follow the same naming convention.
pub fn isolated_volume_name(instance: &str, volume_name: &str) -> String {
    format!("coast--{instance}--{volume_name}")
}

/// Generate the Docker volume name for a shared volume.
///
/// Format: `coast-shared--{project}--{volume_name}`
pub fn shared_volume_name(project: &str, volume_name: &str) -> String {
    format!("coast-shared--{project}--{volume_name}")
}

/// Resolve the volume name for a given config, instance, and project.
pub fn resolve_volume_name(config: &VolumeConfig, instance: &str, project: &str) -> String {
    match config.strategy {
        VolumeStrategy::Isolated => isolated_volume_name(instance, &config.name),
        VolumeStrategy::Shared => shared_volume_name(project, &config.name),
    }
}

/// Check if a service name looks like a database service.
fn is_database_service(service: &str) -> bool {
    let lower = service.to_lowercase();
    DATABASE_SERVICE_NAMES.iter().any(|db| lower.contains(db))
}

/// Generate warnings for volume configurations at build time.
///
/// Returns a list of warning messages. Currently warns when a shared
/// volume is attached to a database-like service.
pub fn generate_volume_warnings(volumes: &[VolumeConfig]) -> Vec<String> {
    let mut warnings = Vec::new();

    for vol in volumes {
        if vol.strategy == VolumeStrategy::Shared && is_database_service(&vol.service) {
            warnings.push(format!(
                "Warning: volume '{}' uses strategy 'shared'. \
                 If multiple instances run simultaneously, service '{}' \
                 may experience data corruption. Consider 'shared_services' \
                 for databases that need cross-instance access.",
                vol.name, vol.service
            ));
        }
    }

    warnings
}

/// Construct the Docker command to copy data from a source volume to a destination volume.
///
/// Uses: `docker run --rm -v {src}:/src -v {dst}:/dst alpine cp -a /src/. /dst/`
pub fn snapshot_copy_command(source_volume: &str, dest_volume: &str) -> Vec<String> {
    vec![
        "docker".to_string(),
        "run".to_string(),
        "--rm".to_string(),
        "-v".to_string(),
        format!("{source_volume}:/src"),
        "-v".to_string(),
        format!("{dest_volume}:/dst"),
        "alpine".to_string(),
        "cp".to_string(),
        "-a".to_string(),
        "/src/.".to_string(),
        "/dst/".to_string(),
    ]
}

/// Determine which volumes should be deleted when an instance is removed.
///
/// Only isolated volumes (including those with `snapshot_source`) are deleted;
/// shared volumes are preserved.
pub fn volumes_to_delete(volumes: &[VolumeConfig], instance: &str) -> Vec<String> {
    volumes
        .iter()
        .filter(|v| v.strategy != VolumeStrategy::Shared)
        .map(|v| isolated_volume_name(instance, &v.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_volume(name: &str, strategy: VolumeStrategy, service: &str) -> VolumeConfig {
        VolumeConfig {
            name: name.to_string(),
            strategy,
            service: service.to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: None,
        }
    }

    #[test]
    fn test_isolated_volume_name() {
        assert_eq!(
            isolated_volume_name("feature-oauth", "postgres_data"),
            "coast--feature-oauth--postgres_data"
        );
    }

    #[test]
    fn test_shared_volume_name() {
        assert_eq!(
            shared_volume_name("my-app", "redis_data"),
            "coast-shared--my-app--redis_data"
        );
    }

    #[test]
    fn test_resolve_volume_name_isolated() {
        let vol = make_volume("pg_data", VolumeStrategy::Isolated, "db");
        assert_eq!(
            resolve_volume_name(&vol, "inst1", "proj"),
            "coast--inst1--pg_data"
        );
    }

    #[test]
    fn test_resolve_volume_name_shared() {
        let vol = make_volume("redis_data", VolumeStrategy::Shared, "redis");
        assert_eq!(
            resolve_volume_name(&vol, "inst1", "my-app"),
            "coast-shared--my-app--redis_data"
        );
    }

    #[test]
    fn test_resolve_volume_name_isolated_with_snapshot_source() {
        let mut vol = make_volume("seed", VolumeStrategy::Isolated, "db");
        vol.snapshot_source = Some("coast_seed_pg".to_string());
        assert_eq!(
            resolve_volume_name(&vol, "inst1", "proj"),
            "coast--inst1--seed"
        );
    }

    #[test]
    fn test_warning_shared_database() {
        let volumes = vec![
            make_volume("pg_data", VolumeStrategy::Shared, "postgres"),
            make_volume("app_data", VolumeStrategy::Shared, "app"),
        ];

        let warnings = generate_volume_warnings(&volumes);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("pg_data"));
        assert!(warnings[0].contains("postgres"));
        assert!(warnings[0].contains("data corruption"));
    }

    #[test]
    fn test_warning_shared_redis() {
        let volumes = vec![make_volume("cache", VolumeStrategy::Shared, "redis")];
        let warnings = generate_volume_warnings(&volumes);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_warning_shared_mysql() {
        let volumes = vec![make_volume("data", VolumeStrategy::Shared, "mysql-primary")];
        let warnings = generate_volume_warnings(&volumes);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_no_warning_isolated_database() {
        let volumes = vec![make_volume("pg_data", VolumeStrategy::Isolated, "postgres")];
        let warnings = generate_volume_warnings(&volumes);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_no_warning_shared_non_database() {
        let volumes = vec![
            make_volume("models", VolumeStrategy::Shared, "app"),
            make_volume("cache", VolumeStrategy::Shared, "frontend"),
        ];
        let warnings = generate_volume_warnings(&volumes);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_no_warning_isolated_with_snapshot_source() {
        let mut vol = make_volume("seed", VolumeStrategy::Isolated, "db");
        vol.snapshot_source = Some("src".to_string());
        let warnings = generate_volume_warnings(&[vol]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_snapshot_copy_command() {
        let cmd = snapshot_copy_command("coast_seed_pg_data", "coast--inst1--seed_data");
        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "run");
        assert_eq!(cmd[2], "--rm");
        assert!(cmd.contains(&"coast_seed_pg_data:/src".to_string()));
        assert!(cmd.contains(&"coast--inst1--seed_data:/dst".to_string()));
        assert!(cmd.contains(&"alpine".to_string()));
        assert!(cmd.contains(&"cp".to_string()));
    }

    #[test]
    fn test_volumes_to_delete_only_non_shared() {
        let volumes = vec![
            make_volume("pg_data", VolumeStrategy::Isolated, "db"),
            make_volume("redis_data", VolumeStrategy::Shared, "redis"),
            {
                let mut v = make_volume("seed", VolumeStrategy::Isolated, "db");
                v.snapshot_source = Some("src".to_string());
                v
            },
        ];

        let to_delete = volumes_to_delete(&volumes, "feature-oauth");
        assert_eq!(to_delete.len(), 2);
        assert!(to_delete.contains(&"coast--feature-oauth--pg_data".to_string()));
        assert!(to_delete.contains(&"coast--feature-oauth--seed".to_string()));
        assert!(!to_delete.iter().any(|v| v.contains("redis_data")));
    }

    #[test]
    fn test_volumes_to_delete_empty() {
        let to_delete = volumes_to_delete(&[], "inst1");
        assert!(to_delete.is_empty());
    }

    #[test]
    fn test_volumes_to_delete_all_shared() {
        let volumes = vec![
            make_volume("a", VolumeStrategy::Shared, "svc1"),
            make_volume("b", VolumeStrategy::Shared, "svc2"),
        ];
        let to_delete = volumes_to_delete(&volumes, "inst1");
        assert!(to_delete.is_empty());
    }

    #[test]
    fn test_is_database_service_variations() {
        assert!(is_database_service("postgres"));
        assert!(is_database_service("PostgreSQL"));
        assert!(is_database_service("my-postgres-service"));
        assert!(is_database_service("mysql"));
        assert!(is_database_service("MongoDB"));
        assert!(is_database_service("redis"));
        assert!(is_database_service("db"));
        assert!(!is_database_service("app"));
        assert!(!is_database_service("frontend"));
        assert!(!is_database_service("worker"));
    }
}
