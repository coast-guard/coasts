/// Individual field parsing functions for converting Raw* types into validated domain types.
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::error::{CoastError, Result};
use crate::types::{
    AssignAction, AssignConfig, BareServiceConfig, InjectType, McpClientConnectorConfig,
    McpClientFormat, McpProxyMode, McpServerConfig, RestartPolicy, SecretConfig, SetupFileConfig,
    SharedServiceConfig, VolumeConfig, VolumeStrategy,
};

use super::raw_types::*;
use super::Coastfile;

impl Coastfile {
    pub(super) fn parse_secrets(
        raw_secrets: HashMap<String, RawSecretConfig>,
    ) -> Result<Vec<SecretConfig>> {
        let mut secrets = Vec::new();

        for (name, raw) in raw_secrets {
            let inject = InjectType::parse(&raw.inject)
                .map_err(|e| CoastError::coastfile(format!("secret '{name}': {e}")))?;

            let mut params = HashMap::new();
            for (key, value) in raw.params {
                if key == "extractor" || key == "inject" || key == "ttl" {
                    continue;
                }
                let string_value = match value {
                    toml::Value::String(s) => s,
                    other => other.to_string(),
                };
                params.insert(key, string_value);
            }

            secrets.push(SecretConfig {
                name,
                extractor: raw.extractor,
                params,
                inject,
                ttl: raw.ttl,
            });
        }

        Ok(secrets)
    }

    pub(super) fn parse_setup_files(
        raw_files: Vec<RawSetupFileConfig>,
    ) -> Result<Vec<SetupFileConfig>> {
        raw_files
            .into_iter()
            .map(Self::parse_setup_file)
            .collect::<Result<Vec<_>>>()
    }

    pub(super) fn parse_setup_file(raw: RawSetupFileConfig) -> Result<SetupFileConfig> {
        let path = raw.path.trim().to_string();
        if path.is_empty() {
            return Err(CoastError::coastfile(
                "coast.setup.files.path cannot be empty".to_string(),
            ));
        }

        let p = Path::new(&path);
        if !p.is_absolute() {
            return Err(CoastError::coastfile(format!(
                "coast.setup.files.path '{}' must be an absolute container path",
                path
            )));
        }
        if path.ends_with('/') {
            return Err(CoastError::coastfile(format!(
                "coast.setup.files.path '{}' must point to a file, not a directory",
                path
            )));
        }
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(CoastError::coastfile(format!(
                "coast.setup.files.path '{}' must not contain '..'",
                path
            )));
        }

        if let Some(mode) = raw.mode.as_deref() {
            let is_octal = (mode.len() == 3 || mode.len() == 4)
                && mode.chars().all(|c| matches!(c, '0'..='7'));
            if !is_octal {
                return Err(CoastError::coastfile(format!(
                    "coast.setup.files.mode '{}' must be a 3-4 digit octal string (e.g. '600' or '0644')",
                    mode
                )));
            }
        }

        Ok(SetupFileConfig {
            path,
            content: raw.content,
            mode: raw.mode,
        })
    }

    pub(super) fn parse_volumes(
        raw_volumes: HashMap<String, RawVolumeConfig>,
    ) -> Result<Vec<VolumeConfig>> {
        let mut volumes = Vec::new();

        for (name, raw) in raw_volumes {
            let strategy = VolumeStrategy::from_str_value(&raw.strategy).ok_or_else(|| {
                CoastError::coastfile(format!(
                    "volume '{name}': invalid strategy '{}'. Expected one of: isolated, shared",
                    raw.strategy
                ))
            })?;

            if strategy == VolumeStrategy::Shared && raw.snapshot_source.is_some() {
                return Err(CoastError::coastfile(format!(
                    "volume '{name}': snapshot_source is only valid with strategy 'isolated'"
                )));
            }

            volumes.push(VolumeConfig {
                name,
                strategy,
                service: raw.service,
                mount: PathBuf::from(raw.mount),
                snapshot_source: raw.snapshot_source,
            });
        }

        Ok(volumes)
    }

    pub(super) fn parse_shared_services(
        raw_services: HashMap<String, RawSharedServiceConfig>,
    ) -> Result<Vec<SharedServiceConfig>> {
        let mut services = Vec::new();

        for (name, raw) in raw_services {
            let inject = match raw.inject {
                Some(inject_str) => {
                    let parsed = InjectType::parse(&inject_str).map_err(|e| {
                        CoastError::coastfile(format!("shared_service '{name}': {e}"))
                    })?;
                    Some(parsed)
                }
                None => None,
            };

            for port in &raw.ports {
                if *port == 0 {
                    return Err(CoastError::coastfile(format!(
                        "shared_service '{name}': port 0 is not valid"
                    )));
                }
            }

            services.push(SharedServiceConfig {
                name,
                image: raw.image,
                ports: raw.ports,
                volumes: raw.volumes,
                env: raw.env,
                auto_create_db: raw.auto_create_db,
                inject,
            });
        }

        Ok(services)
    }

    pub(super) fn parse_mcp_servers(
        raw_mcp: HashMap<String, RawMcpConfig>,
    ) -> Result<Vec<McpServerConfig>> {
        let mut servers = Vec::new();

        for (name, raw) in raw_mcp {
            let proxy = match raw.proxy {
                Some(ref proxy_str) => {
                    let mode = McpProxyMode::from_str_value(proxy_str).ok_or_else(|| {
                        CoastError::coastfile(format!(
                            "mcp '{}': invalid proxy '{}'. Expected: host",
                            name, proxy_str
                        ))
                    })?;
                    Some(mode)
                }
                None => None,
            };

            let is_host = proxy.is_some();

            if is_host && !raw.install.is_empty() {
                return Err(CoastError::coastfile(format!(
                    "mcp '{}': 'install' is not allowed when proxy = \"host\". \
                     Host-proxied MCPs run on the host machine, not inside the coast container. \
                     Remove the install field or remove proxy = \"host\" to make it internal.",
                    name
                )));
            }

            if is_host && raw.source.is_some() {
                return Err(CoastError::coastfile(format!(
                    "mcp '{}': 'source' is not allowed when proxy = \"host\". \
                     Host-proxied MCPs run on the host machine and don't need source files \
                     copied into the container. Remove the source field or remove proxy = \"host\" \
                     to make it internal.",
                    name
                )));
            }

            if !is_host && raw.command.is_none() {
                return Err(CoastError::coastfile(format!(
                    "mcp '{}': 'command' is required for internal MCPs. \
                     Specify the command to run the MCP server (e.g., command = \"node\"), \
                     or add proxy = \"host\" if this MCP should run on the host machine.",
                    name
                )));
            }

            servers.push(McpServerConfig {
                name,
                proxy,
                command: raw.command,
                args: raw.args,
                env: raw.env,
                install: raw.install,
                source: raw.source,
            });
        }

        Ok(servers)
    }

    pub(super) fn parse_bare_services(
        raw_services: HashMap<String, RawBareServiceConfig>,
    ) -> Result<Vec<BareServiceConfig>> {
        let mut services = Vec::new();
        for (name, raw) in raw_services {
            if raw.command.trim().is_empty() {
                return Err(CoastError::coastfile(format!(
                    "services.{name}: 'command' cannot be empty"
                )));
            }
            let restart = match raw.restart {
                Some(ref s) => RestartPolicy::from_str_value(s).ok_or_else(|| {
                    CoastError::coastfile(format!(
                        "services.{name}: invalid restart policy '{s}'. \
                         Expected one of: no, on-failure, always"
                    ))
                })?,
                None => RestartPolicy::default(),
            };
            if let Some(port) = raw.port {
                if port == 0 {
                    return Err(CoastError::coastfile(format!(
                        "services.{name}: port cannot be 0"
                    )));
                }
            }
            services.push(BareServiceConfig {
                name,
                command: raw.command,
                port: raw.port,
                restart,
                install: raw.install,
            });
        }
        Ok(services)
    }

    pub(super) fn parse_mcp_clients(
        raw_clients: HashMap<String, RawMcpClientConfig>,
    ) -> Result<Vec<McpClientConnectorConfig>> {
        let mut clients = Vec::new();
        let builtin_names = ["claude-code", "cursor"];

        for (name, raw) in raw_clients {
            let has_run = raw.run.is_some();
            let has_format = raw.format.is_some();
            let has_config_path = raw.config_path.is_some();

            if has_run && (has_format || has_config_path) {
                return Err(CoastError::coastfile(format!(
                    "mcp_clients '{}': 'run' cannot be combined with 'format' or 'config_path'. \
                     Use either a shell command (run) or a format-based connector, not both.",
                    name
                )));
            }

            let is_builtin = builtin_names.contains(&name.as_str());

            let format = if let Some(ref fmt_str) = raw.format {
                let fmt = McpClientFormat::from_str_value(fmt_str).ok_or_else(|| {
                    CoastError::coastfile(format!(
                        "mcp_clients '{}': unknown format '{}'. Expected one of: claude-code, cursor",
                        name, fmt_str
                    ))
                })?;
                Some(fmt)
            } else if is_builtin && !has_run {
                Some(McpClientFormat::from_str_value(&name).unwrap())
            } else {
                None
            };

            if has_format && !has_config_path && !is_builtin {
                return Err(CoastError::coastfile(format!(
                    "mcp_clients '{}': 'config_path' is required when using 'format' on a \
                     custom connector. Specify the path where the config file should be written.",
                    name
                )));
            }

            clients.push(McpClientConnectorConfig {
                name,
                format,
                config_path: raw.config_path,
                run: raw.run,
            });
        }

        Ok(clients)
    }

    pub(super) fn parse_assign_config(raw: Option<RawAssignConfig>) -> Result<AssignConfig> {
        let Some(raw) = raw else {
            return Ok(AssignConfig::default());
        };

        let default = match raw.default {
            Some(ref s) => AssignAction::from_str_value(s).ok_or_else(|| {
                CoastError::coastfile(format!(
                    "assign.default: invalid action '{}'. Expected one of: none, hot, restart, rebuild",
                    s
                ))
            })?,
            None => AssignAction::default(),
        };

        let mut services = HashMap::new();
        for (name, action_str) in raw.services {
            let action = AssignAction::from_str_value(&action_str).ok_or_else(|| {
                CoastError::coastfile(format!(
                    "assign.services.{}: invalid action '{}'. Expected one of: none, hot, restart, rebuild",
                    name, action_str
                ))
            })?;
            services.insert(name, action);
        }

        Ok(AssignConfig {
            default,
            services,
            rebuild_triggers: raw.rebuild_triggers,
            exclude_paths: raw.exclude_paths,
        })
    }
}
