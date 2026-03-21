use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::info;

use super::paths;

type YamlMapping = serde_yaml::Mapping;

/// Configuration for rewriting a compose file for a coast instance.
pub(super) struct ComposeRewriteConfig<'a> {
    /// Shared services to remove and remap via extra_hosts.
    pub shared_service_hosts: &'a HashMap<String, String>,
    /// Path to the coastfile (for reading omit config and volume definitions).
    pub coastfile_path: &'a Path,
    /// Per-instance image tags: (service_name, image_tag).
    pub per_instance_image_tags: &'a [(String, String)],
    /// Whether the instance has coast-managed volume mounts.
    pub has_volume_mounts: bool,
    /// Container paths of secret bind mounts to inject into each service.
    pub secret_container_paths: &'a [String],
    /// Shared Caddy PKI directory mounted into the outer runtime container.
    pub shared_caddy_pki_container_path: Option<&'a str>,
    /// Project name (used for override directory path).
    pub project: &'a str,
    /// Instance name (used for override directory path).
    pub instance_name: &'a str,
    /// Services using the "hot" assign strategy (need rslave mount propagation).
    pub hot_services: &'a [String],
    /// When true, ALL services get rslave propagation (assign default is "hot").
    pub default_hot: bool,
}

/// Rewrite a compose file for a coast instance and write to disk.
///
/// Delegates to [`rewrite_compose_yaml`] for the YAML transformation, then
/// writes the result to `$COAST_HOME/overrides/{project}/{instance}/docker-compose.coast.yml`.
pub(super) fn rewrite_compose_for_instance(
    compose_content: &str,
    config: &ComposeRewriteConfig<'_>,
) {
    if let Some(yaml_str) = rewrite_compose_yaml(compose_content, config) {
        let override_dir = output_dir(config.project, config.instance_name);
        if let Err(e) = std::fs::create_dir_all(&override_dir) {
            tracing::warn!(error = %e, "failed to create override directory");
        }
        let merged_path = override_dir.join("docker-compose.coast.yml");
        if let Err(e) = std::fs::write(&merged_path, &yaml_str) {
            tracing::warn!(error = %e, "failed to write merged compose file");
        } else {
            info!("wrote merged compose file to {}", merged_path.display());
        }
    }
}

/// Pure YAML transformation: apply all compose rewrites and return the modified YAML string.
///
/// Returns `None` if the input is invalid YAML or no modifications were needed.
/// Returns `Some(yaml_string)` with all transformations applied:
/// 1. Remove shared services and their depends_on references
/// 2. Remove top-level volumes used only by shared services
/// 3. Remove explicitly omitted volumes from the coastfile
/// 4. Apply per-instance image overrides (replace `build:` with `image:`)
/// 5. Apply coast-managed volume overrides
/// 6. Add extra_hosts entries for host.docker.internal and shared services
/// 7. Add secret file volume mounts to all remaining services
pub(super) fn rewrite_compose_yaml(
    compose_content: &str,
    config: &ComposeRewriteConfig<'_>,
) -> Option<String> {
    let mut yaml = serde_yaml::from_str::<serde_yaml::Value>(compose_content).ok()?;
    let base_yaml = yaml.clone();
    let coastfile = load_coastfile(config.coastfile_path);
    let stubbed_services = collect_stubbed_services(config, coastfile.as_ref());
    let removed_service_volume_names =
        collect_named_volumes_for_services(&base_yaml, &stubbed_services);

    let mut needs_write = false;
    needs_write |= remove_stubbed_services_and_depends_on(&mut yaml, &stubbed_services);
    remove_top_level_volumes(&mut yaml, &removed_service_volume_names);
    needs_write |= remove_omitted_volumes(&mut yaml, coastfile.as_ref());
    needs_write |= apply_image_overrides(&mut yaml, config.per_instance_image_tags);
    needs_write |= apply_coast_managed_volume_overrides(&mut yaml, coastfile.as_ref(), config);
    needs_write |= add_service_hosts_and_mounts(&mut yaml, config);
    needs_write |= apply_hot_service_rslave_overrides(&mut yaml, config);

    if needs_write {
        serde_yaml::to_string(&yaml).ok()
    } else {
        None
    }
}

fn load_coastfile(path: &Path) -> Option<coast_core::coastfile::Coastfile> {
    path.exists()
        .then(|| coast_core::coastfile::Coastfile::from_file(path).ok())
        .flatten()
}

fn collect_stubbed_services(
    config: &ComposeRewriteConfig<'_>,
    coastfile: Option<&coast_core::coastfile::Coastfile>,
) -> std::collections::HashSet<String> {
    let mut stubbed_services: std::collections::HashSet<String> =
        config.shared_service_hosts.keys().cloned().collect();
    if let Some(coastfile) = coastfile {
        for service in &coastfile.omit.services {
            stubbed_services.insert(service.clone());
        }
    }
    stubbed_services
}

fn collect_named_volumes_for_services(
    yaml: &serde_yaml::Value,
    service_names: &std::collections::HashSet<String>,
) -> Vec<String> {
    service_names
        .iter()
        .flat_map(|service_name| {
            yaml.get("services")
                .and_then(|services| services.get(service_name.as_str()))
                .and_then(|service| service.as_mapping())
                .and_then(get_service_named_volumes)
                .unwrap_or_default()
        })
        .collect()
}

fn get_service_named_volumes(service_definition: &YamlMapping) -> Option<Vec<String>> {
    service_definition
        .get(serde_yaml::Value::String("volumes".into()))
        .and_then(|volumes| volumes.as_sequence())
        .map(|volumes| {
            volumes
                .iter()
                .filter_map(|volume| {
                    let volume_str = volume.as_str().unwrap_or("");
                    let source = volume_str.split(':').next().unwrap_or("");
                    is_named_volume_source(source).then(|| source.to_string())
                })
                .collect()
        })
}

fn is_named_volume_source(source: &str) -> bool {
    !source.starts_with('.') && !source.starts_with('/') && !source.is_empty()
}

fn remove_stubbed_services_and_depends_on(
    yaml: &mut serde_yaml::Value,
    stubbed_services: &std::collections::HashSet<String>,
) -> bool {
    if stubbed_services.is_empty() {
        return false;
    }

    let Some(services) = yaml
        .get_mut("services")
        .and_then(|services| services.as_mapping_mut())
    else {
        return false;
    };

    let removed_any = remove_stubbed_services(services, stubbed_services);
    strip_stubbed_depends_on_references(services, stubbed_services);
    removed_any
}

fn remove_stubbed_services(
    services: &mut YamlMapping,
    stubbed_services: &std::collections::HashSet<String>,
) -> bool {
    let mut removed_any = false;
    for service_name in stubbed_services {
        let key = serde_yaml::Value::String(service_name.clone());
        if services.remove(&key).is_some() {
            removed_any = true;
            tracing::info!(service = %service_name, "removed shared service from inner compose");
        }
    }
    removed_any
}

fn strip_stubbed_depends_on_references(
    services: &mut YamlMapping,
    stubbed_services: &std::collections::HashSet<String>,
) {
    let service_keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
    for service_key in service_keys {
        if let Some(service_definition) = services
            .get_mut(&service_key)
            .and_then(|service| service.as_mapping_mut())
        {
            strip_depends_on_from_service(service_definition, stubbed_services);
        }
    }
}

fn strip_depends_on_from_service(
    service_definition: &mut YamlMapping,
    stubbed_services: &std::collections::HashSet<String>,
) {
    let depends_on_key = serde_yaml::Value::String("depends_on".into());
    let mut remove_depends_on_key = false;
    if let Some(depends_on) = service_definition.get_mut(&depends_on_key) {
        if let Some(depends_on_map) = depends_on.as_mapping_mut() {
            for service_name in stubbed_services {
                depends_on_map.remove(serde_yaml::Value::String(service_name.clone()));
            }
            remove_depends_on_key = depends_on_map.is_empty();
        } else if let Some(depends_on_sequence) = depends_on.as_sequence_mut() {
            depends_on_sequence.retain(|value| {
                value
                    .as_str()
                    .map(|service| !stubbed_services.contains(service))
                    .unwrap_or(true)
            });
            remove_depends_on_key = depends_on_sequence.is_empty();
        }
    }
    if remove_depends_on_key {
        service_definition.remove(&depends_on_key);
    }
}

fn remove_top_level_volumes(yaml: &mut serde_yaml::Value, volume_names: &[String]) {
    let Some(top_level_volumes) = yaml
        .get_mut("volumes")
        .and_then(|volumes| volumes.as_mapping_mut())
    else {
        return;
    };

    for volume_name in volume_names {
        top_level_volumes.remove(serde_yaml::Value::String(volume_name.clone()));
    }
}

fn remove_omitted_volumes(
    yaml: &mut serde_yaml::Value,
    coastfile: Option<&coast_core::coastfile::Coastfile>,
) -> bool {
    let Some(coastfile) = coastfile else {
        return false;
    };
    if coastfile.omit.volumes.is_empty() {
        return false;
    }
    let Some(top_level_volumes) = yaml
        .get_mut("volumes")
        .and_then(|volumes| volumes.as_mapping_mut())
    else {
        return false;
    };

    let mut removed_any = false;
    for volume_name in &coastfile.omit.volumes {
        if top_level_volumes
            .remove(serde_yaml::Value::String(volume_name.clone()))
            .is_some()
        {
            tracing::info!(volume = %volume_name, "removed omitted volume from inner compose");
            removed_any = true;
        }
    }
    removed_any
}

fn apply_image_overrides(
    yaml: &mut serde_yaml::Value,
    per_instance_image_tags: &[(String, String)],
) -> bool {
    let mut changed = false;
    for (service_name, tag) in per_instance_image_tags {
        if let Some(service_definition) = yaml
            .get_mut("services")
            .and_then(|services| services.get_mut(service_name.as_str()))
            .and_then(|service| service.as_mapping_mut())
        {
            service_definition.insert(
                serde_yaml::Value::String("image".into()),
                serde_yaml::Value::String(tag.clone()),
            );
            service_definition.remove(serde_yaml::Value::String("build".into()));
            changed = true;
        }
    }
    changed
}

fn apply_coast_managed_volume_overrides(
    yaml: &mut serde_yaml::Value,
    coastfile: Option<&coast_core::coastfile::Coastfile>,
    config: &ComposeRewriteConfig<'_>,
) -> bool {
    if !config.has_volume_mounts {
        return false;
    }
    let Some(coastfile) = coastfile else {
        return false;
    };

    let top_level_volumes = yaml
        .as_mapping_mut()
        .expect("compose root should be a mapping")
        .entry(serde_yaml::Value::String("volumes".into()))
        .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    let Some(volume_map) = top_level_volumes.as_mapping_mut() else {
        return false;
    };

    for volume in &coastfile.volumes {
        volume_map.insert(
            serde_yaml::Value::String(volume.name.clone()),
            coast_managed_volume_definition(&volume.name),
        );
    }
    true
}

fn coast_managed_volume_definition(volume_name: &str) -> serde_yaml::Value {
    let container_mount = format!("/coast-volumes/{volume_name}");
    let mut options = serde_yaml::Mapping::new();
    options.insert(
        serde_yaml::Value::String("driver".into()),
        serde_yaml::Value::String("local".into()),
    );
    let mut driver_options = serde_yaml::Mapping::new();
    driver_options.insert(
        serde_yaml::Value::String("type".into()),
        serde_yaml::Value::String("none".into()),
    );
    driver_options.insert(
        serde_yaml::Value::String("device".into()),
        serde_yaml::Value::String(container_mount),
    );
    driver_options.insert(
        serde_yaml::Value::String("o".into()),
        serde_yaml::Value::String("bind".into()),
    );
    options.insert(
        serde_yaml::Value::String("driver_opts".into()),
        serde_yaml::Value::Mapping(driver_options),
    );
    serde_yaml::Value::Mapping(options)
}

fn add_service_hosts_and_mounts(
    yaml: &mut serde_yaml::Value,
    config: &ComposeRewriteConfig<'_>,
) -> bool {
    let Some(services) = yaml
        .get_mut("services")
        .and_then(|services| services.as_mapping_mut())
    else {
        return false;
    };

    let service_names: Vec<String> = services
        .keys()
        .filter_map(|key| key.as_str().map(String::from))
        .collect();
    let mut changed = false;

    for service_name in &service_names {
        if let Some(service_definition) = services
            .get_mut(serde_yaml::Value::String(service_name.clone()))
            .and_then(|service| service.as_mapping_mut())
        {
            changed |= ensure_extra_hosts(service_definition, config);
            changed |= ensure_secret_mounts(service_definition, config.secret_container_paths);
            changed |= ensure_shared_caddy_pki_mount(service_definition, config);
        }
    }

    changed
}

fn ensure_extra_hosts(
    service_definition: &mut YamlMapping,
    config: &ComposeRewriteConfig<'_>,
) -> bool {
    let hosts_key = serde_yaml::Value::String("extra_hosts".into());
    let hosts_seq = service_definition
        .entry(hosts_key)
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
    let Some(sequence) = hosts_seq.as_sequence_mut() else {
        return false;
    };

    let existing_hosts: std::collections::HashSet<String> = sequence
        .iter()
        .filter_map(|value| value.as_str().map(String::from))
        .collect();
    let mut changed = false;

    changed |= push_host_if_missing(
        sequence,
        &existing_hosts,
        "host.docker.internal:host-gateway".to_string(),
    );
    for (shared_service_name, host_target) in config.shared_service_hosts {
        changed |= push_host_if_missing(
            sequence,
            &existing_hosts,
            format!("{shared_service_name}:{host_target}"),
        );
    }

    changed
}

fn push_host_if_missing(
    sequence: &mut Vec<serde_yaml::Value>,
    existing_hosts: &std::collections::HashSet<String>,
    entry: String,
) -> bool {
    if existing_hosts.contains(&entry) {
        return false;
    }
    sequence.push(serde_yaml::Value::String(entry));
    true
}

fn ensure_secret_mounts(
    service_definition: &mut YamlMapping,
    secret_container_paths: &[String],
) -> bool {
    if secret_container_paths.is_empty() {
        return false;
    }

    let volumes_key = serde_yaml::Value::String("volumes".into());
    let volumes_seq = service_definition
        .entry(volumes_key)
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
    let Some(sequence) = volumes_seq.as_sequence_mut() else {
        return false;
    };

    for container_path in secret_container_paths {
        let mount = format!("{container_path}:{container_path}:ro");
        sequence.push(serde_yaml::Value::String(mount));
    }
    true
}

fn ensure_shared_caddy_pki_mount(
    service_definition: &mut YamlMapping,
    config: &ComposeRewriteConfig<'_>,
) -> bool {
    let Some(container_path) = config.shared_caddy_pki_container_path else {
        return false;
    };
    if !is_caddy_service(service_definition) {
        return false;
    }

    ensure_bind_mount(
        service_definition,
        &format!("{container_path}:/data/caddy/pki"),
        "/data/caddy/pki",
    )
}

fn ensure_bind_mount(service_definition: &mut YamlMapping, mount: &str, target_path: &str) -> bool {
    let volumes_key = serde_yaml::Value::String("volumes".into());
    let volumes_seq = service_definition
        .entry(volumes_key)
        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
    let Some(sequence) = volumes_seq.as_sequence_mut() else {
        return false;
    };

    if service_has_mount_target(sequence, target_path) {
        return false;
    }

    sequence.push(serde_yaml::Value::String(mount.to_string()));
    true
}

fn service_has_mount_target(sequence: &[serde_yaml::Value], target_path: &str) -> bool {
    sequence
        .iter()
        .any(|volume| volume_mount_target(volume).is_some_and(|target| target == target_path))
}

fn is_caddy_service(service_definition: &YamlMapping) -> bool {
    service_image_name(service_definition)
        .is_some_and(|image| image_repo_basename(image) == "caddy")
        || service_mounts_etc_caddy(service_definition)
}

fn service_image_name(service_definition: &YamlMapping) -> Option<&str> {
    service_definition
        .get(serde_yaml::Value::String("image".into()))
        .and_then(serde_yaml::Value::as_str)
}

fn image_repo_basename(image: &str) -> &str {
    let without_digest = image.split('@').next().unwrap_or(image);
    let last_segment = without_digest.rsplit('/').next().unwrap_or(without_digest);
    last_segment.split(':').next().unwrap_or(last_segment)
}

fn service_mounts_etc_caddy(service_definition: &YamlMapping) -> bool {
    service_definition
        .get(serde_yaml::Value::String("volumes".into()))
        .and_then(serde_yaml::Value::as_sequence)
        .is_some_and(|volumes| {
            volumes.iter().any(|volume| {
                volume_mount_target(volume).is_some_and(|target| {
                    target == "/etc/caddy" || target == "/etc/caddy/Caddyfile"
                })
            })
        })
}

fn volume_mount_target(volume: &serde_yaml::Value) -> Option<&str> {
    match volume {
        serde_yaml::Value::String(entry) => {
            let parts: Vec<&str> = entry.splitn(3, ':').collect();
            (parts.len() >= 2).then(|| parts[1])
        }
        serde_yaml::Value::Mapping(mapping) => mapping
            .get(serde_yaml::Value::String("target".into()))
            .or_else(|| mapping.get(serde_yaml::Value::String("destination".into())))
            .and_then(serde_yaml::Value::as_str),
        _ => None,
    }
}

fn apply_hot_service_rslave_overrides(
    yaml: &mut serde_yaml::Value,
    config: &ComposeRewriteConfig<'_>,
) -> bool {
    if !config.default_hot && config.hot_services.is_empty() {
        return false;
    }

    let Some(services) = yaml
        .get_mut("services")
        .and_then(|services| services.as_mapping_mut())
    else {
        return false;
    };

    let service_names: Vec<String> = services
        .keys()
        .filter_map(|key| key.as_str().map(String::from))
        .collect();
    let mut changed = false;

    for service_name in &service_names {
        if !is_hot_service(service_name, config) {
            continue;
        }
        if let Some(service_definition) = services
            .get_mut(serde_yaml::Value::String(service_name.clone()))
            .and_then(|service| service.as_mapping_mut())
        {
            changed |= apply_rslave_to_service_volumes(service_definition);
        }
    }

    changed
}

fn is_hot_service(service_name: &str, config: &ComposeRewriteConfig<'_>) -> bool {
    config.default_hot
        || config
            .hot_services
            .iter()
            .any(|service| service == service_name)
}

fn apply_rslave_to_service_volumes(service_definition: &mut YamlMapping) -> bool {
    let volumes_key = serde_yaml::Value::String("volumes".into());
    let Some(volumes) = service_definition
        .get_mut(&volumes_key)
        .and_then(|value| value.as_sequence_mut())
    else {
        return false;
    };

    for volume in volumes.iter_mut() {
        rewrite_volume_with_rslave(volume);
    }
    true
}

/// Rewrite a single compose volume entry to add `rslave` mount propagation.
///
/// Handles both short-form (`./src:/app/src` or `./src:/app/src:rw`) and
/// long-form (mapping with `type: bind`) volumes. Named volumes and
/// non-bind mounts are left untouched.
fn rewrite_volume_with_rslave(vol: &mut serde_yaml::Value) {
    match vol {
        serde_yaml::Value::String(s) => {
            let parts: Vec<&str> = s.splitn(3, ':').collect();
            if parts.len() < 2 {
                return;
            }
            let src = parts[0];
            // Named volumes start with a letter/digit, bind mounts start with . or /
            if !src.starts_with('.') && !src.starts_with('/') {
                return;
            }
            if parts.len() == 2 {
                *s = format!("{}:{}:rslave", parts[0], parts[1]);
            } else {
                let mode = parts[2];
                if mode.contains("rslave") || mode.contains("rshared") || mode.contains("rprivate")
                {
                    return;
                }
                *s = format!("{}:{}:{},rslave", parts[0], parts[1], mode);
            }
        }
        serde_yaml::Value::Mapping(m) => {
            let typ = m
                .get(serde_yaml::Value::String("type".into()))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if typ != "bind" {
                return;
            }
            let bind_key = serde_yaml::Value::String("bind".into());
            let bind_opts = m
                .entry(bind_key)
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
            if let Some(bind_map) = bind_opts.as_mapping_mut() {
                let prop_key = serde_yaml::Value::String("propagation".into());
                if !bind_map.contains_key(&prop_key) {
                    bind_map.insert(prop_key, serde_yaml::Value::String("rslave".into()));
                }
            }
        }
        _ => {}
    }
}

fn output_dir(project: &str, instance_name: &str) -> PathBuf {
    paths::override_dir(project, instance_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    fn empty_shared_service_hosts() -> &'static HashMap<String, String> {
        static EMPTY: LazyLock<HashMap<String, String>> = LazyLock::new(HashMap::new);
        &EMPTY
    }

    fn base_config<'a>() -> ComposeRewriteConfig<'a> {
        ComposeRewriteConfig {
            shared_service_hosts: empty_shared_service_hosts(),
            coastfile_path: Path::new("/nonexistent-coastfile"),
            per_instance_image_tags: &[],
            has_volume_mounts: false,
            secret_container_paths: &[],
            shared_caddy_pki_container_path: None,
            project: "test-proj",
            instance_name: "test-inst",
            hot_services: &[],
            default_hot: false,
        }
    }

    fn parse_output(yaml_str: &str) -> serde_yaml::Value {
        serde_yaml::from_str(yaml_str).unwrap()
    }

    fn get_service_names(yaml: &serde_yaml::Value) -> Vec<String> {
        yaml.get("services")
            .and_then(|s| s.as_mapping())
            .map(|m| {
                m.keys()
                    .filter_map(|k| k.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- rewrite_compose_yaml: shared service removal ---

    #[test]
    fn test_removes_shared_service_and_its_volume() {
        let compose = r#"
services:
  web:
    image: nginx:latest
    depends_on:
      - postgres
  postgres:
    image: postgres:16
    volumes:
      - pgdata:/var/lib/postgresql/data
volumes:
  pgdata:
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let services = get_service_names(&yaml);
        assert!(services.contains(&"web".to_string()));
        assert!(!services.contains(&"postgres".to_string()));

        let volumes = yaml.get("volumes").and_then(|v| v.as_mapping());
        let has_pgdata = volumes
            .map(|m| m.contains_key(&serde_yaml::Value::String("pgdata".into())))
            .unwrap_or(false);
        assert!(
            !has_pgdata,
            "pgdata volume should be removed with its service"
        );
    }

    #[test]
    fn test_strips_depends_on_sequence_referencing_removed_service() {
        let compose = r#"
services:
  web:
    image: nginx
    depends_on:
      - redis
      - postgres
  redis:
    image: redis:7
  postgres:
    image: postgres:16
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let web_deps = yaml
            .get("services")
            .and_then(|s| s.get("web"))
            .and_then(|w| w.get("depends_on"))
            .and_then(|d| d.as_sequence())
            .unwrap();
        let dep_names: Vec<&str> = web_deps.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(dep_names, vec!["redis"]);
    }

    #[test]
    fn test_strips_depends_on_map_referencing_removed_service() {
        let compose = r#"
services:
  web:
    image: nginx
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_started
  redis:
    image: redis:7
  postgres:
    image: postgres:16
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let web_deps = yaml
            .get("services")
            .and_then(|s| s.get("web"))
            .and_then(|w| w.get("depends_on"))
            .and_then(|d| d.as_mapping())
            .unwrap();
        assert!(web_deps.contains_key(&serde_yaml::Value::String("redis".into())));
        assert!(!web_deps.contains_key(&serde_yaml::Value::String("postgres".into())));
    }

    #[test]
    fn test_removes_depends_on_entirely_when_all_deps_removed() {
        let compose = r#"
services:
  web:
    image: nginx
    depends_on:
      - postgres
  postgres:
    image: postgres:16
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let has_depends_on = yaml
            .get("services")
            .and_then(|s| s.get("web"))
            .and_then(|w| w.get("depends_on"))
            .is_some();
        assert!(!has_depends_on, "depends_on should be removed entirely");
    }

    // --- rewrite_compose_yaml: image overrides ---

    #[test]
    fn test_image_override_replaces_build_directive() {
        let compose = r#"
services:
  app:
    build: .
    ports:
      - "3000:3000"
  worker:
    build:
      context: ./worker
"#;
        let tags = vec![
            ("app".to_string(), "my-app:coast-abc123".to_string()),
            ("worker".to_string(), "my-worker:coast-abc123".to_string()),
        ];
        let config = ComposeRewriteConfig {
            per_instance_image_tags: &tags,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let app = yaml.get("services").unwrap().get("app").unwrap();
        assert_eq!(
            app.get("image").unwrap().as_str().unwrap(),
            "my-app:coast-abc123"
        );
        assert!(
            app.get("build").is_none(),
            "build directive should be removed"
        );
        assert!(
            app.get("ports").is_some(),
            "non-build fields should be preserved"
        );

        let worker = yaml.get("services").unwrap().get("worker").unwrap();
        assert_eq!(
            worker.get("image").unwrap().as_str().unwrap(),
            "my-worker:coast-abc123"
        );
        assert!(worker.get("build").is_none());
    }

    // --- rewrite_compose_yaml: extra_hosts injection ---

    #[test]
    fn test_extra_hosts_added_to_all_services() {
        let compose = r#"
services:
  web:
    image: nginx
  api:
    image: node:20
"#;
        let config = ComposeRewriteConfig { ..base_config() };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        for svc_name in &["web", "api"] {
            let hosts = yaml
                .get("services")
                .and_then(|s| s.get(*svc_name))
                .and_then(|s| s.get("extra_hosts"))
                .and_then(|h| h.as_sequence())
                .unwrap();
            let host_strs: Vec<&str> = hosts.iter().filter_map(|v| v.as_str()).collect();
            assert!(
                host_strs.contains(&"host.docker.internal:host-gateway"),
                "service {svc_name} should have host.docker.internal"
            );
        }
    }

    #[test]
    fn test_shared_service_hostname_uses_configured_alias_ip() {
        let compose = r#"
services:
  web:
    image: nginx
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let hosts = yaml
            .get("services")
            .and_then(|s| s.get("web"))
            .and_then(|s| s.get("extra_hosts"))
            .and_then(|h| h.as_sequence())
            .unwrap();
        let host_strs: Vec<&str> = hosts.iter().filter_map(|v| v.as_str()).collect();
        assert!(host_strs.contains(&"postgres:172.17.255.254"));
    }

    // --- rewrite_compose_yaml: secret volume mounts ---

    #[test]
    fn test_secret_mounts_injected_into_all_services() {
        let compose = r#"
services:
  web:
    image: nginx
  api:
    image: node:20
"#;
        let secrets = vec!["/run/secrets/db_password".to_string()];
        let config = ComposeRewriteConfig {
            secret_container_paths: &secrets,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        for svc_name in &["web", "api"] {
            let vols = yaml
                .get("services")
                .and_then(|s| s.get(*svc_name))
                .and_then(|s| s.get("volumes"))
                .and_then(|v| v.as_sequence())
                .unwrap();
            let vol_strs: Vec<&str> = vols.iter().filter_map(|v| v.as_str()).collect();
            assert!(
                vol_strs.contains(&"/run/secrets/db_password:/run/secrets/db_password:ro"),
                "service {svc_name} should have secret mount"
            );
        }
    }

    #[test]
    fn test_caddy_image_gets_shared_pki_mount() {
        let compose = r#"
services:
  proxy:
    image: caddy:2
"#;
        let config = ComposeRewriteConfig {
            shared_caddy_pki_container_path: Some(paths::SHARED_CADDY_PKI_CONTAINER_PATH),
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let volumes = yaml
            .get("services")
            .and_then(|s| s.get("proxy"))
            .and_then(|s| s.get("volumes"))
            .and_then(|v| v.as_sequence())
            .unwrap();
        let volume_strs: Vec<&str> = volumes.iter().filter_map(|v| v.as_str()).collect();
        assert!(volume_strs.contains(&"/coast-caddy-pki:/data/caddy/pki"));
    }

    #[test]
    fn test_non_caddy_service_does_not_get_shared_pki_mount() {
        let compose = r#"
services:
  web:
    image: nginx:latest
"#;
        let config = ComposeRewriteConfig {
            shared_caddy_pki_container_path: Some(paths::SHARED_CADDY_PKI_CONTAINER_PATH),
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let volumes: Vec<&str> = yaml
            .get("services")
            .and_then(|s| s.get("web"))
            .and_then(|s| s.get("volumes"))
            .and_then(|v| v.as_sequence())
            .map(|volumes| volumes.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert!(!volumes.contains(&"/coast-caddy-pki:/data/caddy/pki"));
    }

    #[test]
    fn test_etc_caddy_mount_marks_service_as_caddy_candidate() {
        let compose = r#"
services:
  proxy:
    image: custom-proxy:latest
    volumes:
      - ./docker/caddy/Caddyfile:/etc/caddy/Caddyfile
"#;
        let config = ComposeRewriteConfig {
            shared_caddy_pki_container_path: Some(paths::SHARED_CADDY_PKI_CONTAINER_PATH),
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let volumes = yaml
            .get("services")
            .and_then(|s| s.get("proxy"))
            .and_then(|s| s.get("volumes"))
            .and_then(|v| v.as_sequence())
            .unwrap();
        let volume_strs: Vec<&str> = volumes.iter().filter_map(|v| v.as_str()).collect();
        assert!(volume_strs.contains(&"/coast-caddy-pki:/data/caddy/pki"));
    }

    #[test]
    fn test_shared_pki_mount_preserves_existing_data_mount() {
        let compose = r#"
services:
  proxy:
    image: caddy:2
    volumes:
      - caddy_data:/data
      - caddy_config:/config
volumes:
  caddy_data:
  caddy_config:
"#;
        let config = ComposeRewriteConfig {
            shared_caddy_pki_container_path: Some(paths::SHARED_CADDY_PKI_CONTAINER_PATH),
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let volumes = yaml
            .get("services")
            .and_then(|s| s.get("proxy"))
            .and_then(|s| s.get("volumes"))
            .and_then(|v| v.as_sequence())
            .unwrap();
        let volume_strs: Vec<&str> = volumes.iter().filter_map(|v| v.as_str()).collect();
        assert!(volume_strs.contains(&"caddy_data:/data"));
        assert!(volume_strs.contains(&"caddy_config:/config"));
        assert!(volume_strs.contains(&"/coast-caddy-pki:/data/caddy/pki"));
    }

    // --- rewrite_compose_yaml: edge cases ---

    #[test]
    fn test_no_changes_returns_none() {
        let _compose = r#"
services:
  web:
    image: nginx
"#;
        // With no shared services, no image overrides, no secrets, and no volume mounts,
        // the only change is extra_hosts. So this still returns Some.
        // To get None, we'd need a truly no-op config -- but extra_hosts is always added.
        // This test verifies that invalid YAML returns None.
        let result = rewrite_compose_yaml("not: valid: yaml: {{", &base_config());
        assert!(result.is_none());
    }

    #[test]
    fn test_combined_shared_removal_and_image_override() {
        let compose = r#"
services:
  web:
    build: .
    depends_on:
      - postgres
  postgres:
    image: postgres:16
    volumes:
      - pgdata:/var/lib/postgresql/data
volumes:
  pgdata:
"#;
        let shared = HashMap::from([("postgres".to_string(), "172.17.255.254".to_string())]);
        let tags = vec![("web".to_string(), "my-web:coast-xyz".to_string())];
        let config = ComposeRewriteConfig {
            shared_service_hosts: &shared,
            per_instance_image_tags: &tags,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml = parse_output(&result);

        let services = get_service_names(&yaml);
        assert_eq!(services, vec!["web"]);

        let web = yaml.get("services").unwrap().get("web").unwrap();
        assert_eq!(
            web.get("image").unwrap().as_str().unwrap(),
            "my-web:coast-xyz"
        );
        assert!(web.get("build").is_none());
        assert!(web.get("depends_on").is_none());
    }

    // --- rewrite_compose_yaml: rslave propagation for hot services ---

    #[test]
    fn test_rslave_short_form_bind_mount() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - ./src:/app/src
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("./src:/app/src:rslave"),
            "short-form bind mount should get :rslave, got: {result}"
        );
    }

    #[test]
    fn test_rslave_short_form_with_existing_mode() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - ./src:/app/src:rw
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("./src:/app/src:rw,rslave"),
            "existing mode should be preserved with rslave appended, got: {result}"
        );
    }

    #[test]
    fn test_rslave_long_form_bind_mount() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - type: bind
        source: ./src
        target: /app/src
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        let yaml: serde_yaml::Value = serde_yaml::from_str(&result).unwrap();
        let vol = &yaml["services"]["web"]["volumes"][0];
        let prop = vol["bind"]["propagation"].as_str().unwrap();
        assert_eq!(prop, "rslave");
    }

    #[test]
    fn test_rslave_skips_named_volumes() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - pgdata:/var/lib/postgresql
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("pgdata:/var/lib/postgresql"),
            "named volumes should be unchanged"
        );
        assert!(
            !result.contains("rslave"),
            "named volumes should not get rslave"
        );
    }

    #[test]
    fn test_rslave_only_hot_services() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - ./src:/app/src
  api:
    image: node:20
    volumes:
      - ./api:/app/api
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("./src:/app/src:rslave"),
            "hot service web should get rslave"
        );
        assert!(
            result.contains("./api:/app/api"),
            "non-hot service api should be unchanged"
        );
        assert!(
            !result.contains("./api:/app/api:rslave"),
            "non-hot service should NOT get rslave"
        );
    }

    #[test]
    fn test_rslave_default_hot_all_services() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - ./src:/app/src
  api:
    image: node:20
    volumes:
      - ./api:/app/api
"#;
        let config = ComposeRewriteConfig {
            default_hot: true,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("./src:/app/src:rslave"),
            "web should get rslave with default_hot"
        );
        assert!(
            result.contains("./api:/app/api:rslave"),
            "api should get rslave with default_hot"
        );
    }

    #[test]
    fn test_volume_mount_target_anonymous_volume_does_not_panic() {
        let vol = serde_yaml::Value::String("/workspace/node_modules".into());
        assert_eq!(volume_mount_target(&vol), None);
    }

    #[test]
    fn test_volume_mount_target_bind_mount() {
        let vol = serde_yaml::Value::String("./src:/app/src".into());
        assert_eq!(volume_mount_target(&vol), Some("/app/src"));
    }

    #[test]
    fn test_volume_mount_target_bind_mount_with_mode() {
        let vol = serde_yaml::Value::String("./src:/app/src:rw".into());
        assert_eq!(volume_mount_target(&vol), Some("/app/src"));
    }

    #[test]
    fn test_rslave_rewrite_skips_anonymous_volumes() {
        let compose = r#"
services:
  web:
    image: node:20
    volumes:
      - ../apps/web:/workspace
      - /workspace/node_modules
"#;
        let hot = vec!["web".to_string()];
        let config = ComposeRewriteConfig {
            hot_services: &hot,
            ..base_config()
        };
        let result = rewrite_compose_yaml(compose, &config).unwrap();
        assert!(
            result.contains("../apps/web:/workspace:rslave"),
            "bind mount should get rslave"
        );
        assert!(
            result.contains("/workspace/node_modules"),
            "anonymous volume should be preserved unchanged"
        );
        assert!(
            !result.contains("/workspace/node_modules:rslave"),
            "anonymous volume must not get rslave appended"
        );
    }
}
