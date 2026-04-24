//! Shared-service routing primitives for DinD-backed coasts.
//!
//! This module is shared between `coast-daemon` (local consumer path) and
//! `coast-service` (Phase 18 symmetric remote consumer path). Both sides
//! allocate docker0 alias IPs inside a target DinD container and spawn
//! socat forwarders that bridge a canonical container port to
//! `host.docker.internal:{port}`, where the `{port}` means:
//!
//! - **Daemon local path**: the host Docker publish of the shared-service
//!   container (typically the canonical port).
//! - **Daemon SSG local path**: the SSG's dynamic host port.
//! - **Remote path (Phase 18)**: the per-forward dynamic `remote_port` that
//!   coast-daemon allocated and told `coast-service` about via
//!   `SharedServicePortForward.remote_port`. sshd on the remote VM binds
//!   that `remote_port` via a reverse SSH tunnel.
//!
//! Callers decide what upstream port each route targets by setting
//! `SharedServicePort.forwarding_port` on the input
//! `SharedServiceConfig` values; the generated socat upstream is
//! always `TCP:host.docker.internal:{forwarding_port}`. After
//! Phase 28 the forwarding port is the daemon-managed virtual port
//! supervised by `coast-daemon::handlers::ssg::host_socat`, so the
//! consumer side is stable across SSG rebuilds.
//!
//! See [`coast-ssg/DESIGN.md §11`](../../coast-ssg/DESIGN.md) for the
//! local topology and [`§20`](../../coast-ssg/DESIGN.md) for the Phase 18
//! symmetric remote topology.

use std::collections::HashMap;
use std::fmt::Write;
use std::net::Ipv4Addr;

use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::types::{SharedServiceConfig, SharedServicePort};

use crate::runtime::Runtime;

/// Upstream host every shared-service socat forwards to. Both the daemon
/// (inside a consumer DinD) and coast-service (inside the remote DinD)
/// rely on `host.docker.internal:host-gateway` being registered on the
/// outer DinD's `extra_hosts`, which routes to "the host above me."
pub const SOCAT_UPSTREAM_HOST: &str = "host.docker.internal";

/// One shared service's routing plan: an alias IP on the DinD's docker0
/// plus the set of (canonical, upstream) ports the socat processes will
/// serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedServiceRoute {
    pub service_name: String,
    pub alias_ip: Ipv4Addr,
    pub target_container: String,
    pub ports: Vec<SharedServicePort>,
}

/// Aggregate plan for all shared services targeting a single DinD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedServiceRoutingPlan {
    pub docker0_prefix_len: u8,
    pub routes: Vec<SharedServiceRoute>,
}

impl SharedServiceRoutingPlan {
    /// Map of `service_name -> alias_ip` for compose `extra_hosts`
    /// injection. Callers use this to rewrite `extra_hosts: postgres:{ip}`
    /// inside the consumer's compose project.
    pub fn host_map(&self) -> HashMap<String, String> {
        self.routes
            .iter()
            .map(|route| (route.service_name.clone(), route.alias_ip.to_string()))
            .collect()
    }
}

/// Inspect the target DinD's docker0 subnet and build a routing plan.
///
/// Returns an empty plan if `shared_services` is empty (no inspection
/// performed). Errors if the DinD has no docker0 or if any referenced
/// service is missing from `target_containers`.
pub async fn plan_shared_service_routing(
    docker: &bollard::Docker,
    container_id: &str,
    shared_services: &[SharedServiceConfig],
    target_containers: &HashMap<String, String>,
) -> Result<SharedServiceRoutingPlan> {
    if shared_services.is_empty() {
        return Ok(SharedServiceRoutingPlan {
            docker0_prefix_len: 0,
            routes: Vec::new(),
        });
    }

    let (docker0_ip, docker0_prefix_len) = resolve_docker0_cidr(docker, container_id).await?;
    build_routing_plan(
        shared_services,
        target_containers,
        docker0_ip,
        docker0_prefix_len,
    )
}

/// Spawn (or refresh) the alias-IP + socat processes inside the target
/// DinD for every route in `plan`. Idempotent: re-running will kill stale
/// socats (by recorded pid file) before spawning new ones.
pub async fn ensure_shared_service_proxies(
    docker: &bollard::Docker,
    container_id: &str,
    plan: &SharedServiceRoutingPlan,
) -> Result<()> {
    if plan.routes.is_empty() {
        return Ok(());
    }

    let runtime = crate::dind::DindRuntime::with_client(docker.clone());
    let script = build_proxy_setup_script(plan);
    let result = runtime
        .exec_in_coast(container_id, &["sh", "-lc", &script])
        .await
        .map_err(|error| {
            CoastError::docker(format!(
                "failed to configure shared-service proxies: {error}"
            ))
        })?;

    if !result.success() {
        let stdout = result.stdout.trim();
        let stderr = result.stderr.trim();
        let output = match (stdout.is_empty(), stderr.is_empty()) {
            (false, false) => format!("stdout:\n{stdout}\n\nstderr:\n{stderr}"),
            (false, true) => format!("stdout:\n{stdout}"),
            (true, false) => format!("stderr:\n{stderr}"),
            (true, true) => "no stdout/stderr captured".to_string(),
        };
        return Err(CoastError::docker(format!(
            "failed to configure shared-service proxies (exit {}): {output}",
            result.exit_code
        )));
    }

    info!(
        container_id = %container_id,
        shared_service_count = plan.routes.len(),
        "configured shared-service proxies inside dind"
    );

    Ok(())
}

fn resolve_docker0_cidr_output(stdout: &str) -> Result<(Ipv4Addr, u8)> {
    let cidr = stdout
        .split_whitespace()
        .skip_while(|token| *token != "inet")
        .nth(1)
        .ok_or_else(|| {
            CoastError::docker("failed to find docker0 IPv4 address inside dind".to_string())
        })?;

    let (ip_str, prefix_str) = cidr.split_once('/').ok_or_else(|| {
        CoastError::docker(format!("failed to parse docker0 CIDR '{cidr}' inside dind"))
    })?;

    let ip = ip_str.parse::<Ipv4Addr>().map_err(|error| {
        CoastError::docker(format!("failed to parse docker0 IP '{ip_str}': {error}"))
    })?;
    let prefix_len = prefix_str.parse::<u8>().map_err(|error| {
        CoastError::docker(format!(
            "failed to parse docker0 prefix length '{prefix_str}': {error}"
        ))
    })?;

    if prefix_len > 32 {
        return Err(CoastError::docker(format!(
            "invalid docker0 prefix length '{prefix_len}' inside dind"
        )));
    }

    Ok((ip, prefix_len))
}

async fn resolve_docker0_cidr(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<(Ipv4Addr, u8)> {
    let runtime = crate::dind::DindRuntime::with_client(docker.clone());
    let result = runtime
        .exec_in_coast(container_id, &["sh", "-lc", "ip -o -4 addr show docker0"])
        .await
        .map_err(|error| {
            CoastError::docker(format!("failed to inspect docker0 inside dind: {error}"))
        })?;

    if !result.success() {
        return Err(CoastError::docker(format!(
            "failed to inspect docker0 inside dind: {}",
            result.stderr.trim()
        )));
    }

    resolve_docker0_cidr_output(&result.stdout)
}

fn build_routing_plan(
    shared_services: &[SharedServiceConfig],
    target_containers: &HashMap<String, String>,
    docker0_ip: Ipv4Addr,
    docker0_prefix_len: u8,
) -> Result<SharedServiceRoutingPlan> {
    let mut services: Vec<_> = shared_services.iter().collect();
    services.sort_by(|left, right| left.name.cmp(&right.name));

    let mut routes = Vec::with_capacity(services.len());
    for (index, service) in services.into_iter().enumerate() {
        if !target_containers.contains_key(&service.name) {
            return Err(CoastError::docker(format!(
                "missing shared-service target container for '{}'",
                service.name
            )));
        }

        routes.push(SharedServiceRoute {
            service_name: service.name.clone(),
            alias_ip: allocate_alias_ip(docker0_ip, docker0_prefix_len, index)?,
            target_container: SOCAT_UPSTREAM_HOST.to_string(),
            ports: dedupe_container_ports(&service.ports),
        });
    }

    Ok(SharedServiceRoutingPlan {
        docker0_prefix_len,
        routes,
    })
}

fn dedupe_container_ports(ports: &[SharedServicePort]) -> Vec<SharedServicePort> {
    let mut deduped: Vec<SharedServicePort> = Vec::new();

    for port in ports {
        if deduped
            .iter()
            .any(|existing| existing.container_port == port.container_port)
        {
            continue;
        }
        deduped.push(*port);
    }

    deduped
}

fn allocate_alias_ip(docker0_ip: Ipv4Addr, prefix_len: u8, index: usize) -> Result<Ipv4Addr> {
    let host_bits = 32_u32.saturating_sub(u32::from(prefix_len));
    let usable_hosts = if host_bits == 0 {
        0
    } else {
        (1_u64 << host_bits).saturating_sub(2)
    };

    if usable_hosts == 0 || index as u64 >= usable_hosts.saturating_sub(1) {
        return Err(CoastError::docker(format!(
            "docker0 subnet {docker0_ip}/{prefix_len} does not have enough room for shared-service aliases"
        )));
    }

    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix_len))
    };
    let ip_u32 = u32::from(docker0_ip);
    let network = ip_u32 & mask;
    let broadcast = network | !mask;

    // Allocate from the top of the subnet downward to stay away from Docker's
    // low-address allocations for bridge gateways and containers.
    let alias = broadcast.checked_sub(1 + index as u32).ok_or_else(|| {
        CoastError::docker("failed to allocate shared-service alias IP".to_string())
    })?;

    Ok(Ipv4Addr::from(alias))
}

fn build_proxy_setup_script(plan: &SharedServiceRoutingPlan) -> String {
    let mut script = String::from(
        "set -eu\n\
         if ! command -v socat >/dev/null 2>&1; then\n\
           if command -v apk >/dev/null 2>&1; then\n\
             apk add --no-cache socat >/dev/null\n\
           elif command -v apt-get >/dev/null 2>&1; then\n\
             apt-get update >/dev/null && DEBIAN_FRONTEND=noninteractive apt-get install -y socat >/dev/null\n\
           else\n\
             echo 'socat is required to proxy shared services inside the Coast container' >&2\n\
             exit 1\n\
           fi\n\
         fi\n\
         SOCAT_BIN=\"$(command -v socat)\"\n\
         mkdir -p /var/run/coast/shared-service-proxies /var/log/coast/shared-service-proxies\n",
    );

    for route in &plan.routes {
        let alias_ip = route.alias_ip.to_string();
        let alias_cidr = format!("{alias_ip}/{}", plan.docker0_prefix_len);
        let alias_cidr = shell_quote(&alias_cidr);
        let alias_check = shell_quote(&format!("{alias_ip}/"));

        let _ = writeln!(
            script,
            "ip addr add {alias_cidr} dev docker0 2>/dev/null || true"
        );

        for port in &route.ports {
            let listen_addr = format!(
                "TCP-LISTEN:{},bind={},fork,reuseaddr",
                port.container_port, alias_ip
            );
            let upstream_addr = format!("TCP:{}:{}", route.target_container, port.forwarding_port);
            let log_path = format!(
                "/var/log/coast/shared-service-proxies/{}-{}.log",
                route.service_name, port.container_port
            );
            let pid_path = format!(
                "/var/run/coast/shared-service-proxies/{}-{}.pid",
                route.service_name, port.container_port
            );

            let _ = writeln!(
                script,
                "if [ -f {} ]; then old_pid=\"$(cat {} 2>/dev/null || true)\"; \
                 if [ -n \"$old_pid\" ] && kill -0 \"$old_pid\" 2>/dev/null; then \
                 kill \"$old_pid\" 2>/dev/null || true; fi; fi",
                shell_quote(&pid_path),
                shell_quote(&pid_path),
            );
            let _ = writeln!(
                script,
                "nohup \"$SOCAT_BIN\" {} {} > {} 2>&1 < /dev/null & echo $! > {}",
                shell_quote(&listen_addr),
                shell_quote(&upstream_addr),
                shell_quote(&log_path),
                shell_quote(&pid_path),
            );
        }

        let _ = writeln!(
            script,
            "ip -o -4 addr show dev docker0 | grep -q {}",
            alias_check
        );
    }

    script
}

fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_docker0_cidr_output_parses_ip_and_prefix() {
        let output = "6: docker0    inet 172.17.0.1/16 brd 172.17.255.255 scope global docker0";
        let (ip, prefix) = resolve_docker0_cidr_output(output).unwrap();

        assert_eq!(ip, Ipv4Addr::new(172, 17, 0, 1));
        assert_eq!(prefix, 16);
    }

    #[test]
    fn test_allocate_alias_ip_uses_high_addresses() {
        let first = allocate_alias_ip(Ipv4Addr::new(172, 17, 0, 1), 16, 0).unwrap();
        let second = allocate_alias_ip(Ipv4Addr::new(172, 17, 0, 1), 16, 1).unwrap();

        assert_eq!(first, Ipv4Addr::new(172, 17, 255, 254));
        assert_eq!(second, Ipv4Addr::new(172, 17, 255, 253));
    }

    #[test]
    fn test_build_routing_plan_is_deterministic_by_service_name() {
        let shared_services = vec![
            SharedServiceConfig {
                name: "redis-db".to_string(),
                image: "redis:7".to_string(),
                ports: vec![SharedServicePort::same(6379)],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: false,
                inject: None,
            },
            SharedServiceConfig {
                name: "db".to_string(),
                image: "postgres:16".to_string(),
                ports: vec![SharedServicePort::same(5432)],
                volumes: vec![],
                env: HashMap::new(),
                auto_create_db: false,
                inject: None,
            },
        ];
        let targets = HashMap::from([
            ("db".to_string(), "shared-db".to_string()),
            ("redis-db".to_string(), "shared-redis".to_string()),
        ]);

        let plan = build_routing_plan(&shared_services, &targets, Ipv4Addr::new(172, 17, 0, 1), 16)
            .unwrap();

        assert_eq!(plan.routes[0].service_name, "db");
        assert_eq!(plan.routes[0].alias_ip, Ipv4Addr::new(172, 17, 255, 254));
        assert_eq!(plan.routes[1].service_name, "redis-db");
        assert_eq!(plan.routes[1].alias_ip, Ipv4Addr::new(172, 17, 255, 253));
    }

    #[test]
    fn test_build_proxy_setup_script_binds_alias_ips() {
        let plan = SharedServiceRoutingPlan {
            docker0_prefix_len: 16,
            routes: vec![SharedServiceRoute {
                service_name: "postgis-db".to_string(),
                alias_ip: Ipv4Addr::new(172, 17, 255, 254),
                target_container: SOCAT_UPSTREAM_HOST.to_string(),
                ports: vec![SharedServicePort::new(5433, 5432)],
            }],
        };

        let script = build_proxy_setup_script(&plan);

        assert!(script.contains("command -v socat"));
        assert!(script.contains("apk add --no-cache socat"));
        assert!(script.contains("ip addr add '172.17.255.254/16' dev docker0"));
        assert!(script.contains("TCP-LISTEN:5432,bind=172.17.255.254,fork,reuseaddr"));
        assert!(
            script.contains("TCP:host.docker.internal:5433"),
            "socat upstream should use host.docker.internal with the host_port"
        );
    }

    #[test]
    fn test_build_routing_plan_uses_host_gateway_not_container_name() {
        let targets = HashMap::from([
            (
                "db".to_string(),
                "my-project-shared-services-db".to_string(),
            ),
            (
                "cache".to_string(),
                "my-project-shared-services-cache".to_string(),
            ),
        ]);

        let plan = build_routing_plan(
            &[
                SharedServiceConfig {
                    name: "db".to_string(),
                    image: "postgres:16".to_string(),
                    ports: vec![SharedServicePort::same(5432)],
                    volumes: vec![],
                    env: HashMap::new(),
                    auto_create_db: false,
                    inject: None,
                },
                SharedServiceConfig {
                    name: "cache".to_string(),
                    image: "redis:7".to_string(),
                    ports: vec![SharedServicePort::same(6379)],
                    volumes: vec![],
                    env: HashMap::new(),
                    auto_create_db: false,
                    inject: None,
                },
            ],
            &targets,
            Ipv4Addr::new(172, 17, 0, 1),
            16,
        )
        .unwrap();

        for route in &plan.routes {
            assert_eq!(
                route.target_container, SOCAT_UPSTREAM_HOST,
                "route for '{}' should target host.docker.internal, not the container name",
                route.service_name
            );
        }

        let script = build_proxy_setup_script(&plan);
        assert!(script.contains("TCP:host.docker.internal:5432"));
        assert!(script.contains("TCP:host.docker.internal:6379"));
        assert!(
            !script.contains("shared-services"),
            "no container names should appear in socat upstream targets"
        );
    }

    #[test]
    fn test_dedupe_container_ports_keeps_first_mapping_for_each_container_port() {
        let deduped = dedupe_container_ports(&[
            SharedServicePort::new(5433, 5432),
            SharedServicePort::new(6433, 5432),
            SharedServicePort::same(6379),
        ]);

        assert_eq!(
            deduped,
            vec![
                SharedServicePort::new(5433, 5432),
                SharedServicePort::same(6379),
            ]
        );
    }

    #[test]
    fn test_build_proxy_setup_script_remote_port_upstream() {
        // Phase 18 symmetric remote-path contract: when `host_port` on
        // the route's ports is a dynamic `remote_port` (not a canonical
        // port), the generated script forwards to that dynamic port on
        // `host.docker.internal`. This mirrors the test above but makes
        // the Phase-18 intent explicit: callers construct routes where
        // `host_port = remote_port`.
        let plan = SharedServiceRoutingPlan {
            docker0_prefix_len: 16,
            routes: vec![SharedServiceRoute {
                service_name: "postgres".to_string(),
                alias_ip: Ipv4Addr::new(172, 17, 255, 254),
                target_container: SOCAT_UPSTREAM_HOST.to_string(),
                ports: vec![SharedServicePort {
                    forwarding_port: 61034, // dynamic remote_port
                    container_port: 5432,   // canonical
                }],
            }],
        };

        let script = build_proxy_setup_script(&plan);

        assert!(script.contains("TCP-LISTEN:5432,bind=172.17.255.254,fork,reuseaddr"));
        assert!(
            script.contains("TCP:host.docker.internal:61034"),
            "socat upstream should forward to host.docker.internal at the dynamic \
             remote_port (passed as host_port on the route's SharedServicePort), \
             not the canonical port"
        );
    }
}
