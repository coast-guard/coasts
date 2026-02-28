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
    /// Directory for git worktrees, relative to project root (default: ".coasts").
    pub worktree_dir: String,
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
            worktree_dir: ".coasts".to_string(),
            omit: OmitConfig::default(),
            mcp_servers: vec![],
            mcp_clients: vec![],
            coastfile_type: None,
            autostart: true,
            services: vec![],
            agent_shell: None,
        }
    }

    /// Merge a raw TOML layer on top of an existing Coastfile.
    ///
    /// Fields present in `raw` override the base. Absent fields are inherited.
    /// Maps and named collections are merged (layer overrides same-name items).
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
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

        let compose = match raw.coast.compose {
            Some(c) => {
                let p = Path::new(&c);
                Some(if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    project_root.join(p)
                })
            }
            None => base.compose,
        };

        let runtime = match raw.coast.runtime {
            Some(ref r) => RuntimeType::from_str_value(r).ok_or_else(|| {
                CoastError::coastfile(format!(
                    "invalid runtime '{}'. Expected one of: dind, sysbox, podman",
                    r
                ))
            })?,
            None => base.runtime,
        };

        // Ports: merge maps, validate
        let mut ports = base.ports;
        for (pname, port) in &raw.ports {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "port '{pname}' has value 0, which is not a valid port number"
                )));
            }
        }
        ports.extend(raw.ports);

        // Egress: merge maps, validate
        let mut egress = base.egress;
        for (ename, port) in &raw.egress {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "egress '{ename}' has value 0, which is not a valid port number"
                )));
            }
        }
        egress.extend(raw.egress);

        // Secrets: merge by name
        let layer_secrets = Self::parse_secrets(raw.secrets)?;
        let mut secrets = base.secrets;
        for ls in layer_secrets {
            if let Some(pos) = secrets.iter().position(|s| s.name == ls.name) {
                secrets[pos] = ls;
            } else {
                secrets.push(ls);
            }
        }

        // Inject: concatenate
        let inject = match raw.inject {
            Some(raw_inject) => {
                let mut env = base.inject.env;
                env.extend(raw_inject.env);
                let mut files = base.inject.files;
                files.extend(raw_inject.files);
                HostInjectConfig { env, files }
            }
            None => base.inject,
        };

        // Volumes: merge by name
        let layer_volumes = Self::parse_volumes(raw.volumes)?;
        let mut volumes = base.volumes;
        for lv in layer_volumes {
            if let Some(pos) = volumes.iter().position(|v| v.name == lv.name) {
                volumes[pos] = lv;
            } else {
                volumes.push(lv);
            }
        }

        // Shared services: merge by name
        let layer_shared = Self::parse_shared_services(raw.shared_services)?;
        let mut shared_services = base.shared_services;
        for ls in layer_shared {
            if let Some(pos) = shared_services.iter().position(|s| s.name == ls.name) {
                shared_services[pos] = ls;
            } else {
                shared_services.push(ls);
            }
        }

        // Setup: concatenate (packages deduped, run appended)
        let setup = match raw.coast.setup {
            Some(raw_setup) => {
                let RawSetupConfig {
                    packages: raw_packages,
                    run: raw_run,
                    files: raw_files,
                } = raw_setup;
                let mut packages = base.setup.packages;
                for pkg in raw_packages {
                    if !packages.contains(&pkg) {
                        packages.push(pkg);
                    }
                }
                let mut run = base.setup.run;
                run.extend(raw_run);
                let mut files = base.setup.files;
                for raw_file in raw_files {
                    let parsed = Self::parse_setup_file(raw_file)?;
                    if let Some(pos) = files.iter().position(|f| f.path == parsed.path) {
                        files[pos] = parsed;
                    } else {
                        files.push(parsed);
                    }
                }
                SetupConfig {
                    packages,
                    run,
                    files,
                }
            }
            None => base.setup,
        };

        // Project root: layer overrides if set
        let resolved_root = match raw.coast.root {
            Some(ref root_str) => {
                let root_path = Path::new(root_str);
                if root_path.is_absolute() {
                    root_path.to_path_buf()
                } else {
                    project_root.join(root_path)
                }
            }
            None => base.project_root,
        };

        // Assign: layer overrides entirely if present
        let assign = match raw.assign {
            Some(raw_assign) => Self::parse_assign_config(Some(raw_assign))?,
            None => base.assign,
        };

        let worktree_dir = raw.coast.worktree_dir.unwrap_or(base.worktree_dir);

        // Omit: concatenate
        let omit = match raw.omit {
            Some(raw_omit) => {
                let mut services = base.omit.services;
                services.extend(raw_omit.services);
                let mut vols = base.omit.volumes;
                vols.extend(raw_omit.volumes);
                OmitConfig {
                    services,
                    volumes: vols,
                }
            }
            None => base.omit,
        };

        // MCP servers: merge by name
        let layer_mcp = Self::parse_mcp_servers(raw.mcp)?;
        let mut mcp_servers = base.mcp_servers;
        for lm in layer_mcp {
            if let Some(pos) = mcp_servers.iter().position(|m| m.name == lm.name) {
                mcp_servers[pos] = lm;
            } else {
                mcp_servers.push(lm);
            }
        }

        // MCP clients: merge by name
        let layer_clients = Self::parse_mcp_clients(raw.mcp_clients)?;
        let mut mcp_clients = base.mcp_clients;
        for lc in layer_clients {
            if let Some(pos) = mcp_clients.iter().position(|c| c.name == lc.name) {
                mcp_clients[pos] = lc;
            } else {
                mcp_clients.push(lc);
            }
        }

        // Bare services: merge by name
        let layer_services = Self::parse_bare_services(raw.services)?;
        let mut services = base.services;
        for ls in layer_services {
            if let Some(pos) = services.iter().position(|s| s.name == ls.name) {
                services[pos] = ls;
            } else {
                services.push(ls);
            }
        }

        let agent_shell = match raw.agent_shell {
            Some(raw_agent) => Some(AgentShellConfig {
                command: raw_agent.command,
            }),
            None => base.agent_shell,
        };

        // Validate mutual exclusion: compose and services cannot coexist
        if compose.is_some() && !services.is_empty() {
            return Err(CoastError::coastfile(
                "a Coastfile cannot define both 'compose' and '[services]'. \
                 Use compose for Docker Compose workflows, or [services] for bare process services."
                    .to_string(),
            ));
        }

        let primary_port = raw.coast.primary_port.or(base.primary_port);

        if let Some(ref pp) = primary_port {
            if !ports.contains_key(pp) {
                return Err(CoastError::coastfile(format!(
                    "primary_port '{}' does not match any declared port. \
                     Available ports: {}",
                    pp,
                    ports.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }

        Ok(Coastfile {
            name,
            compose,
            runtime,
            ports,
            primary_port,
            secrets,
            inject,
            volumes,
            shared_services,
            setup,
            project_root: resolved_root,
            assign,
            egress,
            worktree_dir,
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
        let compose = raw.coast.compose.map(|c| {
            let compose_path = Path::new(&c);
            if compose_path.is_absolute() {
                compose_path.to_path_buf()
            } else {
                project_root.join(compose_path)
            }
        });

        // Validate runtime
        let runtime_str = raw.coast.runtime.unwrap_or_else(|| "dind".to_string());
        let runtime = RuntimeType::from_str_value(&runtime_str).ok_or_else(|| {
            CoastError::coastfile(format!(
                "invalid runtime '{}'. Expected one of: dind, sysbox, podman",
                runtime_str
            ))
        })?;

        // Validate ports
        for (name, port) in &raw.ports {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "port '{name}' has value 0, which is not a valid port number"
                )));
            }
        }

        // Validate egress ports
        for (name, port) in &raw.egress {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "egress '{name}' has value 0, which is not a valid port number"
                )));
            }
        }

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
            Some(ref root_str) => {
                let root_path = Path::new(root_str);
                if root_path.is_absolute() {
                    root_path.to_path_buf()
                } else {
                    project_root.join(root_path)
                }
            }
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

        // Validate mutual exclusion: compose and services cannot coexist
        if compose.is_some() && !services.is_empty() {
            return Err(CoastError::coastfile(
                "a Coastfile cannot define both 'compose' and '[services]'. \
                 Use compose for Docker Compose workflows, or [services] for bare process services."
                    .to_string(),
            ));
        }

        let primary_port = raw.coast.primary_port;
        if let Some(ref pp) = primary_port {
            if !raw.ports.contains_key(pp) {
                return Err(CoastError::coastfile(format!(
                    "primary_port '{}' does not match any declared port. \
                     Available ports: {}",
                    pp,
                    raw.ports.keys().cloned().collect::<Vec<_>>().join(", ")
                )));
            }
        }

        Ok(Coastfile {
            name,
            compose,
            runtime,
            ports: raw.ports,
            primary_port,
            secrets,
            inject,
            volumes,
            shared_services,
            setup,
            project_root: resolved_root,
            assign,
            egress: raw.egress,
            worktree_dir: raw
                .coast
                .worktree_dir
                .unwrap_or_else(|| ".coasts".to_string()),
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
