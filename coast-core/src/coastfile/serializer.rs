/// Serialize a resolved `Coastfile` to a standalone TOML string.
///
/// The output contains no `extends`, `includes`, or `unset` directives,
/// making it safe to store in a build artifact directory where parent
/// files are not available.
use std::fmt::Write;

use crate::types::{InjectType, RestartPolicy, SharedServicePort, VolumeStrategy};

use super::Coastfile;

/// Produce a TOML-safe quoted string value.
pub(super) fn toml_quote(s: &str) -> String {
    format!("{:?}", s)
}

fn format_shared_service_port(port: &SharedServicePort) -> String {
    if port.is_identity_mapping() {
        port.host_port.to_string()
    } else {
        toml_quote(&port.to_string())
    }
}

fn write_coast_section(coastfile: &Coastfile, out: &mut String) {
    writeln!(out, "[coast]").unwrap();
    writeln!(out, "name = {}", toml_quote(&coastfile.name)).unwrap();
    if let Some(ref compose) = coastfile.compose {
        let rel = compose
            .strip_prefix(&coastfile.project_root)
            .unwrap_or(compose)
            .display()
            .to_string();
        let rel = if !rel.starts_with('.') {
            format!("./{rel}")
        } else {
            rel
        };
        writeln!(out, "compose = {}", toml_quote(&rel)).unwrap();
    }
    writeln!(out, "runtime = {}", toml_quote(coastfile.runtime.as_str())).unwrap();
    if coastfile.worktree_dirs.len() == 1 {
        writeln!(
            out,
            "worktree_dir = {}",
            toml_quote(&coastfile.worktree_dirs[0])
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "worktree_dir = [{}]",
            coastfile
                .worktree_dirs
                .iter()
                .map(|d| toml_quote(d))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    if coastfile.default_worktree_dir
        != coastfile
            .worktree_dirs
            .first()
            .map(String::as_str)
            .unwrap_or(".worktrees")
    {
        writeln!(
            out,
            "default_worktree_dir = {}",
            toml_quote(&coastfile.default_worktree_dir)
        )
        .unwrap();
    }
    if !coastfile.autostart {
        writeln!(out, "autostart = false").unwrap();
    }
    if let Some(ref primary_port) = coastfile.primary_port {
        writeln!(out, "primary_port = {}", toml_quote(primary_port)).unwrap();
    }
}

fn write_setup_section(coastfile: &Coastfile, out: &mut String) {
    if coastfile.setup.packages.is_empty()
        && coastfile.setup.run.is_empty()
        && coastfile.setup.files.is_empty()
    {
        return;
    }

    writeln!(out, "\n[coast.setup]").unwrap();
    if !coastfile.setup.packages.is_empty() {
        writeln!(
            out,
            "packages = [{}]",
            coastfile
                .setup
                .packages
                .iter()
                .map(|package| toml_quote(package))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    if !coastfile.setup.run.is_empty() {
        writeln!(out, "run = [").unwrap();
        for command in &coastfile.setup.run {
            writeln!(out, "    {},", toml_quote(command)).unwrap();
        }
        writeln!(out, "]").unwrap();
    }
    for file in &coastfile.setup.files {
        writeln!(out, "\n[[coast.setup.files]]").unwrap();
        writeln!(out, "path = {}", toml_quote(&file.path)).unwrap();
        writeln!(out, "content = {}", toml_quote(&file.content)).unwrap();
        if let Some(ref mode) = file.mode {
            writeln!(out, "mode = {}", toml_quote(mode)).unwrap();
        }
    }
}

fn write_numeric_map_section(
    section: &str,
    values: &std::collections::HashMap<String, u16>,
    out: &mut String,
) {
    if values.is_empty() {
        return;
    }

    writeln!(out, "\n[{section}]").unwrap();
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_by_key(|(name, _)| *name);
    for (name, value) in entries {
        writeln!(out, "{name} = {value}").unwrap();
    }
}

fn write_healthcheck_section(coastfile: &Coastfile, out: &mut String) {
    if coastfile.healthcheck.is_empty() {
        return;
    }

    writeln!(out, "\n[healthcheck]").unwrap();
    let mut entries: Vec<_> = coastfile.healthcheck.iter().collect();
    entries.sort_by_key(|(name, _)| *name);
    for (name, path) in entries {
        writeln!(out, "{name} = {}", toml_quote(path)).unwrap();
    }
}

fn write_shared_services_section(coastfile: &Coastfile, out: &mut String) {
    for service in &coastfile.shared_services {
        writeln!(out, "\n[shared_services.{}]", service.name).unwrap();
        writeln!(out, "image = {}", toml_quote(&service.image)).unwrap();
        if !service.ports.is_empty() {
            writeln!(
                out,
                "ports = [{}]",
                service
                    .ports
                    .iter()
                    .map(format_shared_service_port)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
        if !service.volumes.is_empty() {
            writeln!(
                out,
                "volumes = [{}]",
                service
                    .volumes
                    .iter()
                    .map(|volume| toml_quote(volume))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
        if !service.env.is_empty() {
            write!(out, "env = {{ ").unwrap();
            let mut env_pairs: Vec<_> = service.env.iter().collect();
            env_pairs.sort_by_key(|(name, _)| *name);
            let pairs = env_pairs
                .iter()
                .map(|(name, value)| format!("{name} = {}", toml_quote(value)))
                .collect::<Vec<_>>();
            write!(out, "{}", pairs.join(", ")).unwrap();
            writeln!(out, " }}").unwrap();
        }
        if service.auto_create_db {
            writeln!(out, "auto_create_db = true").unwrap();
        }
    }
}

fn write_volumes_section(coastfile: &Coastfile, out: &mut String) {
    for volume in &coastfile.volumes {
        writeln!(out, "\n[volumes.{}]", volume.name).unwrap();
        let strategy = match volume.strategy {
            VolumeStrategy::Isolated => "isolated",
            VolumeStrategy::Shared => "shared",
        };
        writeln!(out, "strategy = {}", toml_quote(strategy)).unwrap();
        writeln!(out, "service = {}", toml_quote(&volume.service)).unwrap();
        writeln!(
            out,
            "mount = {}",
            toml_quote(&volume.mount.display().to_string())
        )
        .unwrap();
        if let Some(ref snapshot_source) = volume.snapshot_source {
            writeln!(out, "snapshot_source = {}", toml_quote(snapshot_source)).unwrap();
        }
    }
}

fn write_secrets_section(coastfile: &Coastfile, out: &mut String) {
    for secret in &coastfile.secrets {
        writeln!(out, "\n[secrets.{}]", secret.name).unwrap();
        writeln!(out, "extractor = {}", toml_quote(&secret.extractor)).unwrap();
        let mut params: Vec<_> = secret.params.iter().collect();
        params.sort_by_key(|(name, _)| *name);
        for (name, value) in params {
            writeln!(out, "{name} = {}", toml_quote(value)).unwrap();
        }
        let inject = match &secret.inject {
            InjectType::Env(var) => format!("env:{var}"),
            InjectType::File(path) => format!("file:{}", path.display()),
        };
        writeln!(out, "inject = {}", toml_quote(&inject)).unwrap();
        if let Some(ref ttl) = secret.ttl {
            writeln!(out, "ttl = {}", toml_quote(ttl)).unwrap();
        }
    }
}

fn write_omit_section(coastfile: &Coastfile, out: &mut String) {
    if coastfile.omit.services.is_empty() && coastfile.omit.volumes.is_empty() {
        return;
    }

    writeln!(out, "\n[omit]").unwrap();
    if !coastfile.omit.services.is_empty() {
        writeln!(
            out,
            "services = [{}]",
            coastfile
                .omit
                .services
                .iter()
                .map(|service| toml_quote(service))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    if !coastfile.omit.volumes.is_empty() {
        writeln!(
            out,
            "volumes = [{}]",
            coastfile
                .omit
                .volumes
                .iter()
                .map(|volume| toml_quote(volume))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
}

fn write_assign_section(coastfile: &Coastfile, out: &mut String) {
    writeln!(out, "\n[assign]").unwrap();
    writeln!(
        out,
        "default = {}",
        toml_quote(&coastfile.assign.default.to_string())
    )
    .unwrap();
    if !coastfile.assign.exclude_paths.is_empty() {
        let exclude_paths = coastfile
            .assign
            .exclude_paths
            .iter()
            .map(|path| toml_quote(path))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "exclude_paths = [{exclude_paths}]").unwrap();
    }
    if !coastfile.assign.services.is_empty() {
        writeln!(out, "[assign.services]").unwrap();
        let mut services: Vec<_> = coastfile.assign.services.iter().collect();
        services.sort_by_key(|(name, _)| *name);
        for (name, action) in services {
            writeln!(out, "{name} = {}", toml_quote(&action.to_string())).unwrap();
        }
    }
    if !coastfile.assign.rebuild_triggers.is_empty() {
        writeln!(out, "[assign.rebuild_triggers]").unwrap();
        let mut triggers: Vec<_> = coastfile.assign.rebuild_triggers.iter().collect();
        triggers.sort_by_key(|(name, _)| *name);
        for (name, paths) in triggers {
            let paths = paths
                .iter()
                .map(|path| toml_quote(path))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(out, "{name} = [{paths}]").unwrap();
        }
    }
}

fn write_mcp_servers_section(coastfile: &Coastfile, out: &mut String) {
    for server in &coastfile.mcp_servers {
        writeln!(out, "\n[mcp.{}]", server.name).unwrap();
        if let Some(ref command) = server.command {
            writeln!(out, "command = {}", toml_quote(command)).unwrap();
        }
        if !server.args.is_empty() {
            writeln!(
                out,
                "args = [{}]",
                server
                    .args
                    .iter()
                    .map(|arg| toml_quote(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
        if let Some(ref proxy) = server.proxy {
            writeln!(out, "proxy = {}", toml_quote(proxy.as_str())).unwrap();
        }
        for install in &server.install {
            writeln!(out, "install = {}", toml_quote(install)).unwrap();
        }
        if let Some(ref source) = server.source {
            writeln!(out, "source = {}", toml_quote(source)).unwrap();
        }
    }
}

fn write_services_section(coastfile: &Coastfile, out: &mut String) {
    for service in &coastfile.services {
        writeln!(out, "\n[services.{}]", service.name).unwrap();
        writeln!(out, "command = {}", toml_quote(&service.command)).unwrap();
        if let Some(port) = service.port {
            writeln!(out, "port = {port}").unwrap();
        }
        if service.restart != RestartPolicy::No {
            writeln!(out, "restart = {}", toml_quote(service.restart.as_str())).unwrap();
        }
        if !service.install.is_empty() {
            if service.install.len() == 1 {
                writeln!(out, "install = {}", toml_quote(&service.install[0])).unwrap();
            } else {
                writeln!(
                    out,
                    "install = [{}]",
                    service
                        .install
                        .iter()
                        .map(|command| toml_quote(command))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
        }
        if !service.cache.is_empty() {
            writeln!(
                out,
                "cache = [{}]",
                service
                    .cache
                    .iter()
                    .map(|entry| toml_quote(entry))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
    }
}

fn write_agent_shell_section(coastfile: &Coastfile, out: &mut String) {
    if let Some(ref agent) = coastfile.agent_shell {
        writeln!(out, "\n[agent_shell]").unwrap();
        writeln!(out, "command = {}", toml_quote(&agent.command)).unwrap();
    }
}

fn write_mcp_clients_section(coastfile: &Coastfile, out: &mut String) {
    for client in &coastfile.mcp_clients {
        writeln!(out, "\n[mcp_clients.{}]", client.name).unwrap();
        if let Some(ref format) = client.format {
            writeln!(out, "format = {}", toml_quote(format.as_str())).unwrap();
        }
        if let Some(ref config_path) = client.config_path {
            writeln!(out, "config_path = {}", toml_quote(config_path)).unwrap();
        }
        if let Some(ref run) = client.run {
            writeln!(out, "run = {}", toml_quote(run)).unwrap();
        }
    }
}

impl Coastfile {
    pub fn to_standalone_toml(&self) -> String {
        let mut out = String::new();
        write_coast_section(self, &mut out);
        write_setup_section(self, &mut out);
        write_numeric_map_section("ports", &self.ports, &mut out);
        write_healthcheck_section(self, &mut out);
        write_shared_services_section(self, &mut out);
        write_volumes_section(self, &mut out);
        write_secrets_section(self, &mut out);
        write_numeric_map_section("egress", &self.egress, &mut out);
        write_omit_section(self, &mut out);
        write_assign_section(self, &mut out);
        write_mcp_servers_section(self, &mut out);
        write_services_section(self, &mut out);
        write_agent_shell_section(self, &mut out);
        write_mcp_clients_section(self, &mut out);
        out
    }
}
