use bollard::container::StatsOptions;
use futures_util::StreamExt;
use tracing::info;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{ContainerStats, ContainerStatsRequest, ContainerStatsResponse};

use crate::state::ServiceState;

pub async fn handle(
    req: ContainerStatsRequest,
    state: &ServiceState,
) -> Result<ContainerStatsResponse> {
    info!(name = %req.name, project = %req.project, "container stats request");

    let db = state.db.lock().await;
    let instance = db.get_instance(&req.project, &req.name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote instance '{}' for project '{}'",
            req.name, req.project
        ))
    })?;
    drop(db);

    let container_id = instance.container_id.ok_or_else(|| {
        CoastError::state(format!("remote instance '{}' has no container", req.name))
    })?;

    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::state("docker is not available on this coast-service host"))?;

    let options = StatsOptions {
        stream: false,
        one_shot: true,
    };
    let mut stream = docker.stats(&container_id, Some(options));

    let raw = stream
        .next()
        .await
        .ok_or_else(|| CoastError::state("no stats returned from Docker"))?
        .map_err(|e| CoastError::Docker {
            message: format!("failed to get container stats: {e}"),
            source: Some(Box::new(e)),
        })?;

    let cpu_percent = if let Some(ref sys) = raw.cpu_stats.system_cpu_usage {
        let cpu_delta = raw
            .cpu_stats
            .cpu_usage
            .total_usage
            .saturating_sub(raw.precpu_stats.cpu_usage.total_usage);
        let system_delta = sys.saturating_sub(raw.precpu_stats.system_cpu_usage.unwrap_or(0));
        let online_cpus = raw.cpu_stats.online_cpus.unwrap_or(1);
        if system_delta > 0 {
            (cpu_delta as f64 / system_delta as f64) * online_cpus as f64 * 100.0
        } else {
            0.0
        }
    } else {
        0.0
    };

    let memory_used_bytes = raw.memory_stats.usage.unwrap_or(0)
        - raw
            .memory_stats
            .stats
            .as_ref()
            .map(|s| match s {
                bollard::container::MemoryStatsStats::V1(v1) => v1.cache,
                bollard::container::MemoryStatsStats::V2(v2) => v2.inactive_file,
            })
            .unwrap_or(0);
    let memory_limit_bytes = raw.memory_stats.limit.unwrap_or(0);
    let memory_percent = if memory_limit_bytes > 0 {
        (memory_used_bytes as f64 / memory_limit_bytes as f64) * 100.0
    } else {
        0.0
    };

    let (disk_read_bytes, disk_write_bytes) = raw
        .blkio_stats
        .io_service_bytes_recursive
        .as_ref()
        .map(|entries| {
            let mut read = 0u64;
            let mut write = 0u64;
            for entry in entries {
                match entry.op.as_str() {
                    "read" | "Read" => read += entry.value,
                    "write" | "Write" => write += entry.value,
                    _ => {}
                }
            }
            (read, write)
        })
        .unwrap_or((0, 0));

    let (network_rx_bytes, network_tx_bytes) = raw
        .networks
        .as_ref()
        .map(|nets| {
            let mut rx = 0u64;
            let mut tx = 0u64;
            for net in nets.values() {
                rx += net.rx_bytes;
                tx += net.tx_bytes;
            }
            (rx, tx)
        })
        .unwrap_or((0, 0));

    let pids = raw.pids_stats.current.unwrap_or(0);
    let timestamp = raw.read.clone();

    Ok(ContainerStatsResponse {
        stats: ContainerStats {
            timestamp,
            cpu_percent,
            memory_used_bytes,
            memory_limit_bytes,
            memory_percent,
            disk_read_bytes,
            disk_write_bytes,
            network_rx_bytes,
            network_tx_bytes,
            pids,
        },
    })
}
