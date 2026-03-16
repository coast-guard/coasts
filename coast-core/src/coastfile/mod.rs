/// Coastfile parsing and validation.
///
/// Parses the TOML-based Coastfile schema defined in SPEC.md,
/// validates all fields, and resolves relative paths.
///
/// Submodules:
/// - [`raw_types`]: Raw TOML serde deserialization structs
mod field_parsers;
mod raw_types;
mod serializer;
#[cfg(test)]
mod tests_inheritance;
#[cfg(test)]
mod tests_parsing;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{CoastError, Result};
use crate::types::{
    AssignConfig, BareServiceConfig, HostInjectConfig, McpClientConnectorConfig, McpServerConfig,
    OmitConfig, RuntimeType, SecretConfig, SetupConfig, SharedServiceConfig, VolumeConfig,
};

use raw_types::*;

/// A fully parsed and validated Coastfile.
#[derive(Debug, Clone)]
pub struct Coastfile {
    /// Project name.
    pub name: String,
    /// Path to the docker-compose file (resolved to absolute), if present.
    pub compose: Option<PathBuf>,
    /// Container runtime.
    pub runtime: RuntimeType,
    /// Port mappings (logical_name -> port).
    pub ports: HashMap<String, u16>,
    /// HTTP healthcheck paths per port (logical_name -> path like "/").
    /// Ports with a healthcheck use HTTP GET instead of TCP for health probes.
    pub healthcheck: HashMap<String, String>,
    /// Default primary port service name (shown starred in UI/CLI).
    pub primary_port: Option<String>,
    /// Secret configurations.
    pub secrets: Vec<SecretConfig>,
    /// Host injection config.
    pub inject: HostInjectConfig,
    /// Volume configurations.
    pub volumes: Vec<VolumeConfig>,
    /// Shared service configurations.
    pub shared_services: Vec<SharedServiceConfig>,
    /// Coast container setup configuration.
    pub setup: SetupConfig,
    /// The directory containing the Coastfile (project root).
    pub project_root: PathBuf,
    /// Configuration for `coast assign` behavior.
    pub assign: AssignConfig,
    /// Egress port declarations (logical_name -> host port).
    /// When non-empty, enables host connectivity from inner compose services.
    pub egress: HashMap<String, u16>,
    /// Directories for git worktrees, relative to project root (default: [".worktrees"]).
    pub worktree_dirs: Vec<String>,
    /// Directory to create new worktrees in (default: first entry of `worktree_dirs`).
    pub default_worktree_dir: String,
    /// Services and volumes to omit from the compose file.
    pub omit: OmitConfig,
    /// MCP server configurations.
    pub mcp_servers: Vec<McpServerConfig>,
    /// MCP client connector configurations (where to write MCP configs for AI tools).
    pub mcp_clients: Vec<McpClientConnectorConfig>,
    /// Coastfile type derived from filename (None = "default", Some("light") = Coastfile.light).
    pub coastfile_type: Option<String>,
    /// Whether to auto-start compose services during `coast run` (default: true).
    /// Set to `false` for configs where the user's workflow handles service startup.
    pub autostart: bool,
    /// Bare process services (alternative to compose).
    pub services: Vec<BareServiceConfig>,
    /// Agent shell configuration (command auto-started when a coast runs).
    pub agent_shell: Option<AgentShellConfig>,
}

/// Configuration for the `[agent_shell]` Coastfile section.
#[derive(Debug, Clone)]
pub struct AgentShellConfig {
    /// Command to execute inside the DinD container (e.g. `"claude --dangerously-skip-permissions"`).
    pub command: String,
}

impl Coastfile {
    /// Parse a Coastfile from a TOML string (standalone, no inheritance).
    ///
    /// The `project_root` is the directory containing the Coastfile,
    /// used to resolve relative paths. This method does not support
    /// `extends` or `includes` — use `from_file()` for inheritance.
    pub fn parse(content: &str, project_root: &Path) -> Result<Self> {
        let raw: RawCoastfile = toml::from_str(content)?;
        if raw.coast.extends.is_some() || raw.coast.includes.is_some() {
            return Err(CoastError::coastfile(
                "extends and includes require file-based parsing. \
                 Use Coastfile::from_file() instead.",
            ));
        }
        let mut cf = Self::validate_and_build(raw, project_root)?;
        cf.coastfile_type = None;
        Ok(cf)
    }

    /// Parse a Coastfile from a file path, resolving inheritance chains.
    pub fn from_file(path: &Path) -> Result<Self> {
        Self::from_file_with_ancestry(path, &mut HashSet::new())
    }

    /// Derive the Coastfile "type" from a filename.
    ///
    /// - `Coastfile` -> `None` (the default type, displayed as "default")
    /// - `Coastfile.light` -> `Some("light")`
    /// - `Coastfile.default` -> error (reserved name)
    pub fn coastfile_type_from_path(path: &Path) -> Result<Option<String>> {
        let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

        if !filename.starts_with("Coastfile") {
            return Ok(None);
        }

        if filename == "Coastfile" {
            return Ok(None);
        }

        if let Some(suffix) = filename.strip_prefix("Coastfile.") {
            if suffix.is_empty() {
                return Err(CoastError::coastfile(
                    "Coastfile type cannot be empty (trailing dot). \
                     Use 'Coastfile' for the default type.",
                ));
            }
            if suffix == "default" {
                return Err(CoastError::coastfile(
                    "'Coastfile.default' is not allowed. \
                     The base 'Coastfile' is the default type.",
                ));
            }
            return Ok(Some(suffix.to_string()));
        }

        Ok(None)
    }

    /// Recursively parse a Coastfile, resolving `extends` and `includes`.
    fn from_file_with_ancestry(path: &Path, ancestors: &mut HashSet<PathBuf>) -> Result<Self> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        if !ancestors.insert(canonical.clone()) {
            return Err(CoastError::coastfile(format!(
                "circular extends/includes dependency detected: '{}'",
                path.display()
            )));
        }

        let coastfile_type = Self::coastfile_type_from_path(path)?;

        let project_root = path
            .parent()
            .ok_or_else(|| CoastError::coastfile("Coastfile path has no parent directory"))?;

        let content = std::fs::read_to_string(path).map_err(|e| CoastError::Io {
            message: format!("Failed to read Coastfile: {e}"),
            path: path.to_path_buf(),
            source: Some(e),
        })?;

        let raw: RawCoastfile = toml::from_str(&content)?;

        let has_extends = raw.coast.extends.is_some();
        let has_includes = raw.coast.includes.is_some();

        let mut result = if has_extends || has_includes {
            let extends_ref = raw.coast.extends.clone();
            let includes_ref = raw.coast.includes.clone();
            let unset = raw.unset.clone();

            let mut base = if let Some(ref extends_path_str) = extends_ref {
                let extends_path = project_root.join(extends_path_str);
                Self::from_file_with_ancestry(&extends_path, ancestors)?
            } else {
                Self::empty(project_root)
            };

            if let Some(ref includes) = includes_ref {
                for include_path_str in includes {
                    let include_path = project_root.join(include_path_str);
                    let include_content =
                        std::fs::read_to_string(&include_path).map_err(|e| CoastError::Io {
                            message: format!(
                                "Failed to read include file '{}': {e}",
                                include_path.display()
                            ),
                            path: include_path.clone(),
                            source: Some(e),
                        })?;
                    let include_raw: RawCoastfile = toml::from_str(&include_content)?;
                    if include_raw.coast.extends.is_some() || include_raw.coast.includes.is_some() {
                        return Err(CoastError::coastfile(format!(
                            "included file '{}' cannot use extends or includes. \
                             Use extends in the main Coastfile for inheritance chains.",
                            include_path.display()
                        )));
                    }
                    let include_root = include_path.parent().unwrap_or(project_root);
                    base = Self::merge_raw_onto(base, include_raw, include_root)?;
                }
            }

            let mut merged = Self::merge_raw_onto(base, raw, project_root)?;
            Self::apply_unset(&mut merged, unset);

            if merged.name.is_empty() {
                return Err(CoastError::coastfile(
                    "coast.name is required and cannot be empty. \
                     Set it in this file or in the parent via extends.",
                ));
            }

            merged
        } else {
            Self::validate_and_build(raw, project_root)?
        };

        result.coastfile_type = coastfile_type;

        ancestors.remove(&canonical);
        Ok(result)
    }

    /// Create an empty Coastfile used as the base when no `extends` is set.
    fn empty(project_root: &Path) -> Self {
        Self {
            name: String::new(),
            compose: None,
            runtime: RuntimeType::Dind,
            ports: HashMap::new(),
            healthcheck: HashMap::new(),
            primary_port: None,
            secrets: vec![],
            inject: HostInjectConfig {
                env: vec![],
                files: vec![],
            },
            volumes: vec![],
            shared_services: vec![],
            setup: SetupConfig::default(),
            project_root: project_root.to_path_buf(),
            assign: AssignConfig::default(),
            egress: HashMap::new(),
            worktree_dirs: vec![".worktrees".to_string()],
            default_worktree_dir: ".worktrees".to_string(),
            omit: OmitConfig::default(),
            mcp_servers: vec![],
            mcp_clients: vec![],
            coastfile_type: None,
            autostart: true,
            services: vec![],
            agent_shell: None,
        }
    }

    fn resolve_path(path: &str, project_root: &Path) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        }
    }

    fn parse_runtime(runtime: Option<&str>) -> Result<Option<RuntimeType>> {
        runtime
            .map(|runtime| {
                RuntimeType::from_str_value(runtime).ok_or_else(|| {
                    CoastError::coastfile(format!(
                        "invalid runtime '{}'. Expected one of: dind, sysbox, podman",
                        runtime
                    ))
                })
            })
            .transpose()
    }

    fn validate_named_ports(ports: &HashMap<String, u16>, kind: &str) -> Result<()> {
        for (name, port) in ports {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "{kind} '{name}' has value 0, which is not a valid port number"
                )));
            }
        }

        Ok(())
    }

    fn merge_validated_ports(
        mut base: HashMap<String, u16>,
        layer: HashMap<String, u16>,
        kind: &str,
    ) -> Result<HashMap<String, u16>> {
        Self::validate_named_ports(&layer, kind)?;
        base.extend(layer);
        Ok(base)
    }

    fn merge_named_items<T, F>(mut base: Vec<T>, layer: Vec<T>, key: F) -> Vec<T>
    where
        F: Fn(&T) -> &str,
    {
        for item in layer {
            if let Some(pos) = base.iter().position(|existing| key(existing) == key(&item)) {
                base[pos] = item;
            } else {
                base.push(item);
            }
        }

        base
    }

    fn merge_inject(base: HostInjectConfig, raw: Option<RawInjectConfig>) -> HostInjectConfig {
        match raw {
            Some(raw_inject) => {
                let mut env = base.env;
                env.extend(raw_inject.env);
                let mut files = base.files;
                files.extend(raw_inject.files);
                HostInjectConfig { env, files }
            }
            None => base,
        }
    }

    fn merge_setup(base: SetupConfig, raw: Option<RawSetupConfig>) -> Result<SetupConfig> {
        match raw {
            Some(raw_setup) => {
                let RawSetupConfig {
                    packages: raw_packages,
                    run: raw_run,
                    files: raw_files,
                } = raw_setup;
                let mut packages = base.packages;
                for pkg in raw_packages {
                    if !packages.contains(&pkg) {
                        packages.push(pkg);
                    }
                }
                let mut run = base.run;
                run.extend(raw_run);
                let files = Self::merge_named_items(
                    base.files,
                    Self::parse_setup_files(raw_files)?,
                    |file| file.path.as_str(),
                );
                Ok(SetupConfig {
                    packages,
                    run,
                    files,
                })
            }
            None => Ok(base),
        }
    }

    fn merge_omit(base: OmitConfig, raw: Option<RawOmitConfig>) -> OmitConfig {
        match raw {
            Some(raw_omit) => {
                let mut services = base.services;
                services.extend(raw_omit.services);
                let mut volumes = base.volumes;
                volumes.extend(raw_omit.volumes);
                OmitConfig { services, volumes }
            }
            None => base,
        }
    }

    fn validate_primary_port(
        primary_port: &Option<String>,
        ports: &HashMap<String, u16>,
    ) -> Result<()> {
        if let Some(primary_port) = primary_port {
            if !ports.contains_key(primary_port) {
                return Err(CoastError::coastfile(format!(
                    "primary_port '{}' does not match any declared port. \
                     Available ports: {}",
                    primary_port,
                    ports.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }

        Ok(())
    }

    /// Merge a raw TOML layer on top of an existing Coastfile.
    ///
    /// Fields present in `raw` override the base. Absent fields are inherited.
    /// Maps and named collections are merged (layer overrides same-name items).
    fn merge_raw_onto(
        base: Coastfile,
        raw: RawCoastfile,
        project_root: &Path,
    ) -> Result<Coastfile> {
        let name = match raw.coast.name {
            Some(ref n) if n.is_empty() => {
                return Err(CoastError::coastfile("coast.name cannot be empty"));
            }
            Some(n) => n,
            None => base.name,
        };

        let compose = raw
            .coast
            .compose
            .map(|compose| Self::resolve_path(&compose, project_root))
            .or(base.compose);
        let runtime = Self::parse_runtime(raw.coast.runtime.as_deref())?.unwrap_or(base.runtime);
        let ports = Self::merge_validated_ports(base.ports, raw.ports, "port")?;
        let egress = Self::merge_validated_ports(base.egress, raw.egress, "egress")?;
        let secrets =
            Self::merge_named_items(base.secrets, Self::parse_secrets(raw.secrets)?, |secret| {
                secret.name.as_str()
            });
        let inject = Self::merge_inject(base.inject, raw.inject);
        let volumes =
            Self::merge_named_items(base.volumes, Self::parse_volumes(raw.volumes)?, |volume| {
                volume.name.as_str()
            });
        let shared_services = Self::merge_named_items(
            base.shared_services,
            Self::parse_shared_services(raw.shared_services)?,
            |service| service.name.as_str(),
        );
        let setup = Self::merge_setup(base.setup, raw.coast.setup)?;
        let resolved_root = raw
            .coast
            .root
            .map(|root| Self::resolve_path(&root, project_root))
            .unwrap_or(base.project_root);
        let assign = raw
            .assign
            .map(|assign| Self::parse_assign_config(Some(assign)))
            .transpose()?
            .unwrap_or(base.assign);
        let worktree_dirs = raw.coast.worktree_dir.unwrap_or(base.worktree_dirs);
        let default_worktree_dir = raw
            .coast
            .default_worktree_dir
            .or(Some(base.default_worktree_dir))
            .unwrap_or_else(|| {
                worktree_dirs
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ".worktrees".to_string())
            });
        let omit = Self::merge_omit(base.omit, raw.omit);
        let mcp_servers = Self::merge_named_items(
            base.mcp_servers,
            Self::parse_mcp_servers(raw.mcp)?,
            |server| server.name.as_str(),
        );
        let mcp_clients = Self::merge_named_items(
            base.mcp_clients,
            Self::parse_mcp_clients(raw.mcp_clients)?,
            |client| client.name.as_str(),
        );
        let services = Self::merge_named_items(
            base.services,
            Self::parse_bare_services(raw.services)?,
            |service| service.name.as_str(),
        );

        let agent_shell = match raw.agent_shell {
            Some(raw_agent) => Some(AgentShellConfig {
                command: raw_agent.command,
            }),
            None => base.agent_shell,
        };

        let primary_port = raw.coast.primary_port.or(base.primary_port);
        Self::validate_primary_port(&primary_port, &ports)?;

        Ok(Coastfile {
            name,
            compose,
            runtime,
            ports,
            healthcheck: raw.healthcheck,
            primary_port,
            secrets,
            inject,
            volumes,
            shared_services,
            setup,
            project_root: resolved_root,
            assign,
            egress,
            worktree_dirs,
            default_worktree_dir,
            omit,
            mcp_servers,
            mcp_clients,
            coastfile_type: None,
            autostart: raw.coast.autostart.unwrap_or(base.autostart),
            services,
            agent_shell,
        })
    }

    /// Remove items listed in `[unset]` from the resolved config.
    fn apply_unset(coastfile: &mut Coastfile, unset: Option<RawUnsetConfig>) {
        let Some(unset) = unset else { return };

        for name in &unset.secrets {
            coastfile.secrets.retain(|s| s.name != *name);
        }
        for name in &unset.ports {
            coastfile.ports.remove(name);
        }
        for name in &unset.shared_services {
            coastfile.shared_services.retain(|s| s.name != *name);
        }
        for name in &unset.volumes {
            coastfile.volumes.retain(|v| v.name != *name);
        }
        for name in &unset.mcp {
            coastfile.mcp_servers.retain(|m| m.name != *name);
        }
        for name in &unset.mcp_clients {
            coastfile.mcp_clients.retain(|c| c.name != *name);
        }
        for name in &unset.egress {
            coastfile.egress.remove(name);
        }
        for name in &unset.services {
            coastfile.services.retain(|s| s.name != *name);
        }
    }

    fn validate_and_build(raw: RawCoastfile, project_root: &Path) -> Result<Self> {
        // Validate project name (required for standalone files)
        let name = raw.coast.name.unwrap_or_default();
        if name.is_empty() {
            return Err(CoastError::coastfile(
                "coast.name is required and cannot be empty",
            ));
        }

        // Validate and resolve compose path (optional)
        let compose = raw
            .coast
            .compose
            .map(|compose| Self::resolve_path(&compose, project_root));

        // Validate runtime
        let runtime =
            Self::parse_runtime(raw.coast.runtime.as_deref())?.unwrap_or(RuntimeType::Dind);

        // Validate ports
        Self::validate_named_ports(&raw.ports, "port")?;

        // Validate egress ports
        Self::validate_named_ports(&raw.egress, "egress")?;

        // Parse secrets
        let secrets = Self::parse_secrets(raw.secrets)?;

        // Parse inject config
        let inject = match raw.inject {
            Some(raw_inject) => HostInjectConfig {
                env: raw_inject.env,
                files: raw_inject.files,
            },
            None => HostInjectConfig {
                env: vec![],
                files: vec![],
            },
        };

        // Parse volumes
        let volumes = Self::parse_volumes(raw.volumes)?;

        // Parse shared services
        let shared_services = Self::parse_shared_services(raw.shared_services)?;

        // Parse setup config
        let setup = match raw.coast.setup {
            Some(raw_setup) => {
                let RawSetupConfig {
                    packages,
                    run,
                    files: raw_files,
                } = raw_setup;
                SetupConfig {
                    packages,
                    run,
                    files: Self::parse_setup_files(raw_files)?,
                }
            }
            None => SetupConfig::default(),
        };

        // Resolve project root: if `root` is set, resolve relative to Coastfile dir
        let resolved_root = match raw.coast.root {
            Some(root_str) => Self::resolve_path(&root_str, project_root),
            None => project_root.to_path_buf(),
        };

        // Parse [assign] config
        let assign = Self::parse_assign_config(raw.assign)?;

        // Parse [omit] config
        let omit = match raw.omit {
            Some(raw_omit) => OmitConfig {
                services: raw_omit.services,
                volumes: raw_omit.volumes,
            },
            None => OmitConfig::default(),
        };

        // Parse MCP servers
        let mcp_servers = Self::parse_mcp_servers(raw.mcp)?;

        // Parse MCP client connectors
        let mcp_clients = Self::parse_mcp_clients(raw.mcp_clients)?;

        // Parse bare services
        let services = Self::parse_bare_services(raw.services)?;

        let agent_shell = raw
            .agent_shell
            .map(|r| AgentShellConfig { command: r.command });

        let primary_port = raw.coast.primary_port;
        Self::validate_primary_port(&primary_port, &raw.ports)?;

        Ok(Coastfile {
            name,
            compose,
            runtime,
            ports: raw.ports,
            healthcheck: raw.healthcheck,
            primary_port,
            secrets,
            inject,
            volumes,
            shared_services,
            setup,
            project_root: resolved_root,
            assign,
            egress: raw.egress,
            worktree_dirs: raw
                .coast
                .worktree_dir
                .clone()
                .unwrap_or_else(|| vec![".worktrees".to_string()]),
            default_worktree_dir: raw.coast.default_worktree_dir.unwrap_or_else(|| {
                raw.coast
                    .worktree_dir
                    .as_ref()
                    .and_then(|v| v.first().cloned())
                    .unwrap_or_else(|| ".worktrees".to_string())
            }),
            omit,
            mcp_servers,
            mcp_clients,
            coastfile_type: None,
            autostart: raw.coast.autostart.unwrap_or(true),
            services,
            agent_shell,
        })
    }
}

/// Container mount path prefix for external worktree directories.
pub const EXTERNAL_WORKTREE_MOUNT_PREFIX: &str = "/host-external-wt";

impl Coastfile {
    /// Returns `true` if a worktree dir path is external (absolute or home-relative).
    ///
    /// External dirs start with `~/` or `/`. Relative dirs (like `.worktrees`)
    /// are resolved against the project root and are considered local.
    pub fn is_external_worktree_dir(dir: &str) -> bool {
        dir.starts_with("~/") || dir.starts_with('/')
    }

    /// Resolve a worktree dir to an absolute path.
    ///
    /// - `~/foo` expands `~` to the user's home directory
    /// - `/absolute/path` is returned as-is
    /// - `relative/path` is joined to `project_root`
    pub fn resolve_worktree_dir(project_root: &Path, dir: &str) -> PathBuf {
        if let Some(rest) = dir.strip_prefix("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(rest)
        } else if dir.starts_with('/') {
            PathBuf::from(dir)
        } else {
            project_root.join(dir)
        }
    }

    /// Returns `(index, resolved_path)` pairs for all external worktree dirs.
    ///
    /// The index corresponds to the position in `self.worktree_dirs`.
    pub fn external_worktree_dirs(&self) -> Vec<(usize, PathBuf)> {
        self.worktree_dirs
            .iter()
            .enumerate()
            .filter(|(_, dir)| Self::is_external_worktree_dir(dir))
            .map(|(idx, dir)| (idx, Self::resolve_worktree_dir(&self.project_root, dir)))
            .collect()
    }

    /// Compute the container mount path for an external worktree dir by its index.
    pub fn external_mount_path(index: usize) -> String {
        format!("{EXTERNAL_WORKTREE_MOUNT_PREFIX}/{index}")
    }
}
