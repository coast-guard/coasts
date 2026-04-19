//! Synthesize the inner `compose.yml` that the SSG DinD runs.
//!
//! Phase: ssg-phase-2 (build-time) and ssg-phase-3 (run-time consumer).
//! See `DESIGN.md §9.2`.
//!
//! Pure function: takes a parsed [`SsgCoastfile`] and emits a
//! `docker compose`-compatible YAML document that:
//!
//! - Defines one service per `[shared_services.*]` entry using the
//!   declared image + env.
//! - Publishes each service's declared container port as
//!   `{port}:{port}` inside the DinD. The outer DinD's
//!   `{dynamic}:{canonical}` publication is set up separately in
//!   Phase 3's `runtime::lifecycle`.
//! - Emits volume strings verbatim per the symmetric-path plan
//!   (`DESIGN.md §10.2`): host bind mounts use the same path on both
//!   mount hops; inner named volumes get a top-level `volumes:` entry
//!   in addition to the service-level mount reference.
//! - Sets `restart: unless-stopped` on every service so they recover
//!   through inner dockerd restarts.

use std::collections::{BTreeMap, BTreeSet};

use serde_yaml::{Mapping, Value};

use crate::coastfile::{SsgCoastfile, SsgSharedServiceConfig, SsgVolumeEntry};

/// Synthesize the inner compose YAML for this SSG build.
pub fn synth_inner_compose(cf: &SsgCoastfile) -> String {
    let mut root = Mapping::new();

    let mut services = Mapping::new();
    let mut named_volumes: BTreeSet<String> = BTreeSet::new();

    for svc in &cf.services {
        let (name_key, service_value, volumes) = service_entry(svc);
        services.insert(name_key, service_value);
        named_volumes.extend(volumes);
    }

    root.insert(Value::String("services".into()), Value::Mapping(services));

    if !named_volumes.is_empty() {
        let mut volumes_section = Mapping::new();
        for name in named_volumes {
            // docker compose accepts `name: {}` to declare a volume
            // with all defaults.
            volumes_section.insert(Value::String(name), Value::Mapping(Mapping::new()));
        }
        root.insert(
            Value::String("volumes".into()),
            Value::Mapping(volumes_section),
        );
    }

    serde_yaml::to_string(&Value::Mapping(root))
        .expect("serde_yaml can always serialize a Mapping of valid values")
}

/// Build one `services.<name>:` mapping and collect any inner named
/// volumes that the top-level `volumes:` section must declare.
fn service_entry(svc: &SsgSharedServiceConfig) -> (Value, Value, Vec<String>) {
    let mut m = Mapping::new();
    m.insert(
        Value::String("image".into()),
        Value::String(svc.image.clone()),
    );
    m.insert(
        Value::String("restart".into()),
        Value::String("unless-stopped".into()),
    );

    if !svc.ports.is_empty() {
        let ports = svc
            .ports
            .iter()
            .map(|p| Value::String(format!("{p}:{p}")))
            .collect();
        m.insert(Value::String("ports".into()), Value::Sequence(ports));
    }

    if !svc.env.is_empty() {
        let mut env_map = Mapping::new();
        // BTreeMap for deterministic ordering.
        let sorted: BTreeMap<_, _> = svc.env.iter().collect();
        for (k, v) in sorted {
            env_map.insert(Value::String(k.clone()), Value::String(v.clone()));
        }
        m.insert(Value::String("environment".into()), Value::Mapping(env_map));
    }

    let mut named = Vec::new();
    if !svc.volumes.is_empty() {
        let mut seq = Vec::with_capacity(svc.volumes.len());
        for entry in &svc.volumes {
            match entry {
                SsgVolumeEntry::HostBindMount {
                    host_path,
                    container_path,
                } => {
                    // Symmetric-path: host path is used verbatim as
                    // the source on this hop. See DESIGN.md §10.2.
                    seq.push(Value::String(format!(
                        "{}:{}",
                        host_path.display(),
                        container_path.display()
                    )));
                }
                SsgVolumeEntry::InnerNamedVolume {
                    name,
                    container_path,
                } => {
                    seq.push(Value::String(format!(
                        "{}:{}",
                        name,
                        container_path.display()
                    )));
                    named.push(name.clone());
                }
            }
        }
        m.insert(Value::String("volumes".into()), Value::Sequence(seq));
    }

    (Value::String(svc.name.clone()), Value::Mapping(m), named)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(toml: &str) -> SsgCoastfile {
        SsgCoastfile::parse(toml, Path::new("/tmp/ssg-test")).unwrap()
    }

    fn synth(toml: &str) -> String {
        synth_inner_compose(&parse(toml))
    }

    fn as_yaml(s: &str) -> Value {
        serde_yaml::from_str::<Value>(s).unwrap_or_else(|e| panic!("invalid YAML: {e}\n---\n{s}"))
    }

    #[test]
    fn synth_minimal_single_service() {
        let yaml = synth(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
"#,
        );
        let value = as_yaml(&yaml);
        let svc = value
            .get("services")
            .and_then(Value::as_mapping)
            .and_then(|m| m.get(Value::String("postgres".into())))
            .and_then(Value::as_mapping)
            .expect("services.postgres mapping");
        assert_eq!(
            svc.get(Value::String("image".into())),
            Some(&Value::String("postgres:16".into()))
        );
        assert_eq!(
            svc.get(Value::String("restart".into())),
            Some(&Value::String("unless-stopped".into()))
        );
        let ports = svc
            .get(Value::String("ports".into()))
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(ports, &vec![Value::String("5432:5432".into())]);
    }

    #[test]
    fn synth_host_bind_uses_symmetric_path() {
        let yaml = synth(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/pg:/var/lib/postgresql/data"]
"#,
        );
        let value = as_yaml(&yaml);
        let volumes = value
            .get("services")
            .and_then(|s| s.get("postgres"))
            .and_then(|p| p.get("volumes"))
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(
            volumes,
            &vec![Value::String(
                "/var/coast-data/pg:/var/lib/postgresql/data".into()
            )]
        );
        // Host bind mounts don't create a top-level volumes entry.
        assert!(value.get("volumes").is_none());
    }

    #[test]
    fn synth_inner_named_volume_adds_top_level_entry() {
        let yaml = synth(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["pg_wal:/var/lib/postgresql/wal"]
"#,
        );
        let value = as_yaml(&yaml);
        let top_vols = value
            .get("volumes")
            .and_then(Value::as_mapping)
            .expect("top-level volumes mapping");
        assert!(top_vols
            .get(Value::String("pg_wal".into()))
            .is_some_and(|v| v.as_mapping().map(|m| m.is_empty()).unwrap_or(false)));
    }

    #[test]
    fn synth_env_is_preserved_as_mapping() {
        let yaml = synth(
            r#"
[shared_services.postgres]
image = "postgres:16"
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "secret" }
"#,
        );
        let value = as_yaml(&yaml);
        let env = value
            .get("services")
            .and_then(|s| s.get("postgres"))
            .and_then(|p| p.get("environment"))
            .and_then(Value::as_mapping)
            .unwrap();
        assert_eq!(
            env.get(Value::String("POSTGRES_USER".into())),
            Some(&Value::String("coast".into()))
        );
        assert_eq!(
            env.get(Value::String("POSTGRES_PASSWORD".into())),
            Some(&Value::String("secret".into()))
        );
    }

    #[test]
    fn synth_multi_service_output_is_valid_yaml() {
        let yaml = synth(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/coast-data/pg:/var/lib/postgresql/data",
    "pg_wal:/var/lib/postgresql/wal",
]

[shared_services.redis]
image = "redis:7"
ports = [6379]
"#,
        );
        let value = as_yaml(&yaml);
        let services = value.get("services").and_then(Value::as_mapping).unwrap();
        assert!(services.contains_key(Value::String("postgres".into())));
        assert!(services.contains_key(Value::String("redis".into())));
        assert!(value
            .get("volumes")
            .and_then(Value::as_mapping)
            .unwrap()
            .contains_key(Value::String("pg_wal".into())));
    }

    #[test]
    fn synth_bare_service_has_restart_policy() {
        // No ports, no volumes, no env.
        let yaml = synth(
            r#"
[shared_services.daemon]
image = "busybox:latest"
"#,
        );
        let value = as_yaml(&yaml);
        let svc = value.get("services").and_then(|s| s.get("daemon")).unwrap();
        assert_eq!(
            svc.get("restart"),
            Some(&Value::String("unless-stopped".into()))
        );
    }
}
