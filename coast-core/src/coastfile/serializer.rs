/// Serialize a resolved `Coastfile` to a standalone TOML string.
///
/// The output contains no `extends`, `includes`, or `unset` directives,
/// making it safe to store in a build artifact directory where parent
/// files are not available.
use std::fmt::Write;

use crate::types::{InjectType, RestartPolicy, VolumeStrategy};

use super::Coastfile;

/// Produce a TOML-safe quoted string value.
pub(super) fn toml_quote(s: &str) -> String {
    format!("{:?}", s)
}

impl Coastfile {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    pub fn to_standalone_toml(&self) -> String {
        let mut out = String::new();

        // [coast]
        writeln!(out, "[coast]").unwrap();
        writeln!(out, "name = {}", toml_quote(&self.name)).unwrap();
        if let Some(ref compose) = self.compose {
            let rel = compose
                .strip_prefix(&self.project_root)
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
        writeln!(out, "runtime = {}", toml_quote(self.runtime.as_str())).unwrap();
        writeln!(out, "worktree_dir = {}", toml_quote(&self.worktree_dir)).unwrap();
        if !self.autostart {
            writeln!(out, "autostart = false").unwrap();
        }
        if let Some(ref pp) = self.primary_port {
            writeln!(out, "primary_port = {}", toml_quote(pp)).unwrap();
        }
        if !self.setup.packages.is_empty()
            || !self.setup.run.is_empty()
            || !self.setup.files.is_empty()
        {
            writeln!(out, "\n[coast.setup]").unwrap();
            if !self.setup.packages.is_empty() {
                writeln!(
                    out,
                    "packages = [{}]",
                    self.setup
                        .packages
                        .iter()
                        .map(|s| toml_quote(s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
            if !self.setup.run.is_empty() {
                writeln!(out, "run = [").unwrap();
                for cmd in &self.setup.run {
                    writeln!(out, "    {},", toml_quote(cmd)).unwrap();
                }
                writeln!(out, "]").unwrap();
            }
            for file in &self.setup.files {
                writeln!(out, "\n[[coast.setup.files]]").unwrap();
                writeln!(out, "path = {}", toml_quote(&file.path)).unwrap();
                writeln!(out, "content = {}", toml_quote(&file.content)).unwrap();
                if let Some(ref mode) = file.mode {
                    writeln!(out, "mode = {}", toml_quote(mode)).unwrap();
                }
            }
        }

        // [ports]
        if !self.ports.is_empty() {
            writeln!(out, "\n[ports]").unwrap();
            let mut ports: Vec<_> = self.ports.iter().collect();
            ports.sort_by_key(|(k, _)| *k);
            for (name, port) in ports {
                writeln!(out, "{name} = {port}").unwrap();
            }
        }

        // [shared_services.*]
        for svc in &self.shared_services {
            writeln!(out, "\n[shared_services.{}]", svc.name).unwrap();
            writeln!(out, "image = {}", toml_quote(&svc.image)).unwrap();
            if !svc.ports.is_empty() {
                writeln!(
                    out,
                    "ports = [{}]",
                    svc.ports
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
            if !svc.volumes.is_empty() {
                writeln!(
                    out,
                    "volumes = [{}]",
                    svc.volumes
                        .iter()
                        .map(|v| toml_quote(v))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
            if !svc.env.is_empty() {
                write!(out, "env = {{ ").unwrap();
                let mut env_pairs: Vec<_> = svc.env.iter().collect();
                env_pairs.sort_by_key(|(k, _)| *k);
                let pairs: Vec<String> = env_pairs
                    .iter()
                    .map(|(k, v)| format!("{k} = {}", toml_quote(v)))
                    .collect();
                write!(out, "{}", pairs.join(", ")).unwrap();
                writeln!(out, " }}").unwrap();
            }
            if svc.auto_create_db {
                writeln!(out, "auto_create_db = true").unwrap();
            }
        }

        // [volumes.*]
        for vol in &self.volumes {
            writeln!(out, "\n[volumes.{}]", vol.name).unwrap();
            let strat = match vol.strategy {
                VolumeStrategy::Isolated => "isolated",
                VolumeStrategy::Shared => "shared",
            };
            writeln!(out, "strategy = {}", toml_quote(strat)).unwrap();
            writeln!(out, "service = {}", toml_quote(&vol.service)).unwrap();
            writeln!(
                out,
                "mount = {}",
                toml_quote(&vol.mount.display().to_string())
            )
            .unwrap();
            if let Some(ref src) = vol.snapshot_source {
                writeln!(out, "snapshot_source = {}", toml_quote(src)).unwrap();
            }
        }

        // [secrets.*]
        for secret in &self.secrets {
            writeln!(out, "\n[secrets.{}]", secret.name).unwrap();
            writeln!(out, "extractor = {}", toml_quote(&secret.extractor)).unwrap();
            let mut params: Vec<_> = secret.params.iter().collect();
            params.sort_by_key(|(k, _)| *k);
            for (k, v) in params {
                writeln!(out, "{k} = {}", toml_quote(v)).unwrap();
            }
            let inject_str = match &secret.inject {
                InjectType::Env(var) => format!("env:{var}"),
                InjectType::File(path) => format!("file:{}", path.display()),
            };
            writeln!(out, "inject = {}", toml_quote(&inject_str)).unwrap();
            if let Some(ref ttl) = secret.ttl {
                writeln!(out, "ttl = {}", toml_quote(ttl)).unwrap();
            }
        }

        // [egress]
        if !self.egress.is_empty() {
            writeln!(out, "\n[egress]").unwrap();
            let mut egress: Vec<_> = self.egress.iter().collect();
            egress.sort_by_key(|(k, _)| *k);
            for (name, port) in egress {
                writeln!(out, "{name} = {port}").unwrap();
            }
        }

        // [omit]
        if !self.omit.services.is_empty() || !self.omit.volumes.is_empty() {
            writeln!(out, "\n[omit]").unwrap();
            if !self.omit.services.is_empty() {
                writeln!(
                    out,
                    "services = [{}]",
                    self.omit
                        .services
                        .iter()
                        .map(|s| toml_quote(s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
            if !self.omit.volumes.is_empty() {
                writeln!(
                    out,
                    "volumes = [{}]",
                    self.omit
                        .volumes
                        .iter()
                        .map(|s| toml_quote(s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
        }

        // [assign]
        writeln!(out, "\n[assign]").unwrap();
        writeln!(
            out,
            "default = {}",
            toml_quote(&self.assign.default.to_string())
        )
        .unwrap();
        if !self.assign.services.is_empty() {
            writeln!(out, "[assign.services]").unwrap();
            let mut svcs: Vec<_> = self.assign.services.iter().collect();
            svcs.sort_by_key(|(k, _)| *k);
            for (name, action) in svcs {
                writeln!(out, "{name} = {}", toml_quote(&action.to_string())).unwrap();
            }
        }

        // [mcp.*]
        for mcp in &self.mcp_servers {
            writeln!(out, "\n[mcp.{}]", mcp.name).unwrap();
            if let Some(ref cmd) = mcp.command {
                writeln!(out, "command = {}", toml_quote(cmd)).unwrap();
            }
            if !mcp.args.is_empty() {
                writeln!(
                    out,
                    "args = [{}]",
                    mcp.args
                        .iter()
                        .map(|a| toml_quote(a))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .unwrap();
            }
            if let Some(ref proxy) = mcp.proxy {
                writeln!(out, "proxy = {}", toml_quote(proxy.as_str())).unwrap();
            }
            for install in &mcp.install {
                writeln!(out, "install = {}", toml_quote(install)).unwrap();
            }
            if let Some(ref source) = mcp.source {
                writeln!(out, "source = {}", toml_quote(source)).unwrap();
            }
        }

        // [services.*]
        for svc in &self.services {
            writeln!(out, "\n[services.{}]", svc.name).unwrap();
            writeln!(out, "command = {}", toml_quote(&svc.command)).unwrap();
            if let Some(port) = svc.port {
                writeln!(out, "port = {port}").unwrap();
            }
            if svc.restart != RestartPolicy::No {
                writeln!(out, "restart = {}", toml_quote(svc.restart.as_str())).unwrap();
            }
            if !svc.install.is_empty() {
                if svc.install.len() == 1 {
                    writeln!(out, "install = {}", toml_quote(&svc.install[0])).unwrap();
                } else {
                    writeln!(
                        out,
                        "install = [{}]",
                        svc.install
                            .iter()
                            .map(|s| toml_quote(s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .unwrap();
                }
            }
        }

        // [agent_shell]
        if let Some(ref agent) = self.agent_shell {
            writeln!(out, "\n[agent_shell]").unwrap();
            writeln!(out, "command = {}", toml_quote(&agent.command)).unwrap();
        }

        // [mcp_clients.*]
        for client in &self.mcp_clients {
            writeln!(out, "\n[mcp_clients.{}]", client.name).unwrap();
            if let Some(ref fmt) = client.format {
                writeln!(out, "format = {}", toml_quote(fmt.as_str())).unwrap();
            }
            if let Some(ref path) = client.config_path {
                writeln!(out, "config_path = {}", toml_quote(path)).unwrap();
            }
            if let Some(ref run) = client.run {
                writeln!(out, "run = {}", toml_quote(run)).unwrap();
            }
        }

        out
    }
}
