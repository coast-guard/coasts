use std::path::{Path, PathBuf};

use tracing::info;

/// Configuration for rewriting a compose file for a coast instance.
pub(super) struct ComposeRewriteConfig<'a> {
    /// Services to remove (shared services + explicitly omitted).
    pub shared_service_names: &'a [String],
    /// Path to the coastfile (for reading omit config and volume definitions).
    pub coastfile_path: &'a Path,
    /// Per-instance image tags: (service_name, image_tag).
    pub per_instance_image_tags: &'a [(String, String)],
    /// Whether the instance has coast-managed volume mounts.
    pub has_volume_mounts: bool,
    /// Bridge gateway IP for extra_hosts entries.
    pub bridge_gateway_ip: Option<&'a str>,
    /// Container paths of secret bind mounts to inject into each service.
    pub secret_container_paths: &'a [String],
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
/// writes the result to `~/.coast/overrides/{project}/{instance}/docker-compose.coast.yml`.
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
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub(super) fn rewrite_compose_yaml(
    compose_content: &str,
    config: &ComposeRewriteConfig<'_>,
) -> Option<String> {
    let mut yaml = serde_yaml::from_str::<serde_yaml::Value>(compose_content).ok()?;

    let mut needs_write = false;

    let mut svc_stub: std::collections::HashSet<String> =
        config.shared_service_names.iter().cloned().collect();

    if config.coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(config.coastfile_path) {
            for svc in &cf.omit.services {
                svc_stub.insert(svc.clone());
            }
        }
    }

    // --- Remove shared services from the services section ---
    if !svc_stub.is_empty() {
        if let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) {
            for svc_name in &svc_stub {
                let key = serde_yaml::Value::String(svc_name.clone());
                if services.remove(&key).is_some() {
                    needs_write = true;
                    tracing::info!(service = %svc_name, "removed shared service from inner compose");
                }
            }

            // Strip depends_on references to removed shared services
            let svc_keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
            for svc_key in svc_keys {
                if let Some(svc_def) = services.get_mut(&svc_key).and_then(|v| v.as_mapping_mut()) {
                    let dep_key = serde_yaml::Value::String("depends_on".into());
                    let mut remove_depends = false;
                    if let Some(deps) = svc_def.get_mut(&dep_key) {
                        if let Some(dep_map) = deps.as_mapping_mut() {
                            for svc_name in &svc_stub {
                                dep_map.remove(serde_yaml::Value::String(svc_name.clone()));
                            }
                            if dep_map.is_empty() {
                                remove_depends = true;
                            }
                        } else if let Some(dep_seq) = deps.as_sequence_mut() {
                            dep_seq.retain(|v| {
                                v.as_str().map(|s| !svc_stub.contains(s)).unwrap_or(true)
                            });
                            if dep_seq.is_empty() {
                                remove_depends = true;
                            }
                        }
                    }
                    if remove_depends {
                        svc_def.remove(&dep_key);
                    }
                }
            }
        }

        // Remove top-level volume definitions used only by shared services
        if let Some(top_volumes) = yaml.get_mut("volumes").and_then(|v| v.as_mapping_mut()) {
            let shared_vol_names: Vec<String> = svc_stub
                .iter()
                .flat_map(|svc| {
                    if let Ok(base) = serde_yaml::from_str::<serde_yaml::Value>(compose_content) {
                        if let Some(svc_def) = base
                            .get("services")
                            .and_then(|s| s.get(svc.as_str()))
                            .and_then(|v| v.as_mapping())
                        {
                            if let Some(vols) = svc_def
                                .get(serde_yaml::Value::String("volumes".into()))
                                .and_then(|v| v.as_sequence())
                            {
                                return vols
                                    .iter()
                                    .filter_map(|v| {
                                        let s = v.as_str().unwrap_or("");
                                        let src = s.split(':').next().unwrap_or("");
                                        if !src.starts_with('.')
                                            && !src.starts_with('/')
                                            && !src.is_empty()
                                        {
                                            Some(src.to_string())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>();
                            }
                        }
                    }
                    vec![]
                })
                .collect();
            for vol_name in &shared_vol_names {
                top_volumes.remove(serde_yaml::Value::String(vol_name.clone()));
            }
        }
    }

    // --- Remove explicitly omitted volumes ---
    if config.coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(config.coastfile_path) {
            if !cf.omit.volumes.is_empty() {
                if let Some(top_volumes) = yaml.get_mut("volumes").and_then(|v| v.as_mapping_mut())
                {
                    for vol_name in &cf.omit.volumes {
                        if top_volumes
                            .remove(serde_yaml::Value::String(vol_name.clone()))
                            .is_some()
                        {
                            tracing::info!(volume = %vol_name, "removed omitted volume from inner compose");
                            needs_write = true;
                        }
                    }
                }
            }
        }
    }

    // --- Apply per-instance image overrides ---
    for (service_name, tag) in config.per_instance_image_tags {
        if let Some(svc_def) = yaml
            .get_mut("services")
            .and_then(|s| s.get_mut(service_name.as_str()))
            .and_then(|v| v.as_mapping_mut())
        {
            svc_def.insert(
                serde_yaml::Value::String("image".into()),
                serde_yaml::Value::String(tag.clone()),
            );
            svc_def.remove(serde_yaml::Value::String("build".into()));
            needs_write = true;
        }
    }

    // --- Apply volume overrides (coast-managed named volumes) ---
    if config.has_volume_mounts && config.coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(config.coastfile_path) {
            let top_vols = yaml
                .as_mapping_mut()
                .unwrap()
                .entry(serde_yaml::Value::String("volumes".into()))
                .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
            if let Some(vol_map) = top_vols.as_mapping_mut() {
                for vol_config in &cf.volumes {
                    let container_mount = format!("/coast-volumes/{}", vol_config.name);
                    let mut opts = serde_yaml::Mapping::new();
                    opts.insert(
                        serde_yaml::Value::String("driver".into()),
                        serde_yaml::Value::String("local".into()),
                    );
                    let mut driver_opts = serde_yaml::Mapping::new();
                    driver_opts.insert(
                        serde_yaml::Value::String("type".into()),
                        serde_yaml::Value::String("none".into()),
                    );
                    driver_opts.insert(
                        serde_yaml::Value::String("device".into()),
                        serde_yaml::Value::String(container_mount),
                    );
                    driver_opts.insert(
                        serde_yaml::Value::String("o".into()),
                        serde_yaml::Value::String("bind".into()),
                    );
                    opts.insert(
                        serde_yaml::Value::String("driver_opts".into()),
                        serde_yaml::Value::Mapping(driver_opts),
                    );
                    vol_map.insert(
                        serde_yaml::Value::String(vol_config.name.clone()),
                        serde_yaml::Value::Mapping(opts),
                    );
                }
                needs_write = true;
            }
        }
    }

    // --- Add extra_hosts and secret volume mounts to remaining services ---
    if let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) {
        let svc_keys: Vec<String> = services
            .keys()
            .filter_map(|k| k.as_str().map(String::from))
            .collect();

        for svc_name in &svc_keys {
            if let Some(svc_def) = services
                .get_mut(serde_yaml::Value::String(svc_name.clone()))
                .and_then(|v| v.as_mapping_mut())
            {
                // extra_hosts: host.docker.internal + shared service hostnames
                let hosts_key = serde_yaml::Value::String("extra_hosts".into());
                let hosts_seq = svc_def
                    .entry(hosts_key)
                    .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
                if let Some(seq) = hosts_seq.as_sequence_mut() {
                    let existing: std::collections::HashSet<String> = seq
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    let entry = "host.docker.internal:host-gateway".to_string();
                    if !existing.contains(&entry) {
                        seq.push(serde_yaml::Value::String(entry));
                    }
                    let host_target = config.bridge_gateway_ip.unwrap_or("host-gateway");
                    for shared_name in config.shared_service_names {
                        let e = format!("{shared_name}:{host_target}");
                        if !existing.contains(&e) {
                            seq.push(serde_yaml::Value::String(e));
                        }
                    }
                    needs_write = true;
                }

                // Secret file mounts
                if !config.secret_container_paths.is_empty() {
                    let vols_key = serde_yaml::Value::String("volumes".into());
                    let vols_seq = svc_def
                        .entry(vols_key)
                        .or_insert_with(|| serde_yaml::Value::Sequence(vec![]));
                    if let Some(seq) = vols_seq.as_sequence_mut() {
                        for cp in config.secret_container_paths {
                            let mount = format!("{cp}:{cp}:ro");
                            seq.push(serde_yaml::Value::String(mount));
                        }
                        needs_write = true;
                    }
                }
            }
        }
    }

    // --- Add rslave propagation to bind-mount volumes for hot services ---
    if config.default_hot || !config.hot_services.is_empty() {
        if let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) {
            let svc_names: Vec<String> = services
                .keys()
                .filter_map(|k| k.as_str().map(String::from))
                .collect();
            for svc_name in &svc_names {
                let is_hot =
                    config.default_hot || config.hot_services.iter().any(|s| s == svc_name);
                if !is_hot {
                    continue;
                }
                if let Some(svc_def) = services
                    .get_mut(serde_yaml::Value::String(svc_name.clone()))
                    .and_then(|v| v.as_mapping_mut())
                {
                    let vols_key = serde_yaml::Value::String("volumes".into());
                    if let Some(vols_seq) =
                        svc_def.get_mut(&vols_key).and_then(|v| v.as_sequence_mut())
                    {
                        for vol in vols_seq.iter_mut() {
                            rewrite_volume_with_rslave(vol);
                        }
                        needs_write = true;
                    }
                }
            }
        }
    }

    if needs_write {
        serde_yaml::to_string(&yaml).ok()
    } else {
        None
    }
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
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".coast")
        .join("overrides")
        .join(project)
        .join(instance_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config<'a>() -> ComposeRewriteConfig<'a> {
        ComposeRewriteConfig {
            shared_service_names: &[],
            coastfile_path: Path::new("/nonexistent-coastfile"),
            per_instance_image_tags: &[],
            has_volume_mounts: false,
            bridge_gateway_ip: None,
            secret_container_paths: &[],
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
        let shared = vec!["postgres".to_string()];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
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
        let shared = vec!["postgres".to_string()];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
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
        let shared = vec!["postgres".to_string()];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
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
        let shared = vec!["postgres".to_string()];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
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
    fn test_shared_service_hostname_uses_bridge_gateway_ip() {
        let compose = r#"
services:
  web:
    image: nginx
"#;
        let shared = vec!["postgres".to_string()];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
            bridge_gateway_ip: Some("172.17.0.1"),
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
        assert!(host_strs.contains(&"postgres:172.17.0.1"));
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
        let shared = vec!["postgres".to_string()];
        let tags = vec![("web".to_string(), "my-web:coast-xyz".to_string())];
        let config = ComposeRewriteConfig {
            shared_service_names: &shared,
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
}
