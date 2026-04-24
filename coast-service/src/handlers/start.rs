use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{StartRequest, StartResponse};

use crate::state::ServiceState;

pub async fn handle(req: StartRequest, state: &ServiceState) -> Result<StartResponse> {
    info!(name = %req.name, project = %req.project, "remote start request");

    let (container_id_opt, persisted_forwards) = {
        let db = state.db.lock().await;
        let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
            CoastError::state(format!(
                "no remote instance '{}' for project '{}'",
                req.name, req.project
            ))
        })?;
        let forwards = db
            .list_remote_shared_forwards_for_instance(&req.project, &req.name)
            .unwrap_or_default();
        (instance.container_id, forwards)
    };

    if let Some(ref container_id) = container_id_opt {
        if let Some(ref docker) = state.docker {
            let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
            use coast_docker::runtime::Runtime;
            rt.start_coast_container(container_id).await?;

            // Phase 18: the DinD's inside-netns alias IPs + socat
            // processes do not survive a container stop. Replay from
            // persisted `remote_shared_forwards` to restore routing
            // before the inner compose services come back up.
            if !persisted_forwards.is_empty() {
                if let Err(e) =
                    restore_shared_service_proxies(docker, container_id, &persisted_forwards).await
                {
                    warn!(
                        instance = %req.name,
                        project = %req.project,
                        error = %e,
                        "failed to restore shared-service proxies on start; inner services may not reach shared services"
                    );
                }
            }
        }
    }

    let db = state.db.lock().await;
    db.update_instance_status(&req.project, &req.name, "running")?;

    Ok(StartResponse {
        name: req.name,
        ports: vec![],
    })
}

/// Rebuild the inside-DinD alias-IP + socat setup from persisted
/// `remote_shared_forwards` rows. Called from `coast start` after a
/// prior stop; the rows were written by `run` and survive stop/start
/// cycles so the same alias IPs and `remote_port`s are re-used.
async fn restore_shared_service_proxies(
    docker: &bollard::Docker,
    container_id: &str,
    forwards: &[crate::state::remote_shared_forwards::RemoteSharedForwardRecord],
) -> Result<()> {
    use coast_docker::shared_service_routing::{
        SharedServiceRoute, SharedServiceRoutingPlan, SOCAT_UPSTREAM_HOST,
    };
    use std::collections::BTreeMap;
    use std::net::Ipv4Addr;

    // Parse the docker0 prefix length once from the DinD so the ip
    // addr add lines in the shell script carry the correct CIDR.
    let prefix_len = inspect_docker0_prefix_len(docker, container_id).await?;

    // Group persisted forwards by (service_name, alias_ip). Each group
    // becomes one SharedServiceRoute with one or more SharedServicePort
    // entries.
    let mut grouped: BTreeMap<(String, String), SharedServiceRoute> = BTreeMap::new();
    for rec in forwards {
        let alias_ip = rec.alias_ip.parse::<Ipv4Addr>().map_err(|e| {
            CoastError::state(format!(
                "invalid persisted alias_ip '{}': {e}",
                rec.alias_ip
            ))
        })?;
        let key = (rec.service_name.clone(), rec.alias_ip.clone());
        grouped
            .entry(key)
            .or_insert_with(|| SharedServiceRoute {
                service_name: rec.service_name.clone(),
                alias_ip,
                target_container: SOCAT_UPSTREAM_HOST.to_string(),
                ports: Vec::new(),
            })
            .ports
            .push(coast_core::types::SharedServicePort {
                forwarding_port: rec.remote_port,
                container_port: rec.port,
            });
    }

    let plan = SharedServiceRoutingPlan {
        docker0_prefix_len: prefix_len,
        routes: grouped.into_values().collect(),
    };

    coast_docker::shared_service_routing::ensure_shared_service_proxies(docker, container_id, &plan)
        .await
}

/// Inspect the DinD's docker0 interface to recover the subnet prefix
/// length. The alias IP itself is already persisted per-forward; only
/// the prefix is needed to render the `ip addr add` line correctly.
async fn inspect_docker0_prefix_len(docker: &bollard::Docker, container_id: &str) -> Result<u8> {
    use coast_docker::runtime::Runtime;
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let result = runtime
        .exec_in_coast(container_id, &["sh", "-lc", "ip -o -4 addr show docker0"])
        .await
        .map_err(|e| CoastError::docker(format!("failed to inspect docker0: {e}")))?;
    if !result.success() {
        return Err(CoastError::docker(format!(
            "failed to inspect docker0: {}",
            result.stderr.trim()
        )));
    }
    let line = result
        .stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| CoastError::docker("docker0 has no addresses".to_string()))?;
    let cidr = line
        .split_whitespace()
        .skip_while(|tok| *tok != "inet")
        .nth(1)
        .ok_or_else(|| CoastError::docker("failed to parse docker0 inet line".to_string()))?;
    let prefix_str = cidr
        .split_once('/')
        .map(|(_, p)| p)
        .ok_or_else(|| CoastError::docker(format!("docker0 CIDR missing '/': {cidr}")))?;
    prefix_str.parse::<u8>().map_err(|e| {
        CoastError::docker(format!(
            "failed to parse docker0 prefix '{prefix_str}': {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};
    use std::sync::Arc;

    fn test_state() -> Arc<ServiceState> {
        Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ))
    }

    async fn insert_stopped_instance(state: &ServiceState, name: &str, project: &str) {
        let db = state.db.lock().await;
        db.insert_instance(&crate::state::instances::RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "stopped".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        })
        .unwrap();
    }

    #[tokio::test]
    async fn test_start_sets_status() {
        let state = test_state();
        insert_stopped_instance(&state, "web", "proj").await;

        let resp = handle(
            StartRequest {
                name: "web".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.name, "web");

        let db = state.db.lock().await;
        let inst = db.get_instance("proj", "web").unwrap().unwrap();
        assert_eq!(inst.status, "running");
    }

    #[tokio::test]
    async fn test_start_nonexistent_errors() {
        let state = test_state();
        let err = handle(
            StartRequest {
                name: "nope".to_string(),
                project: "proj".to_string(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote instance"));
    }
}
