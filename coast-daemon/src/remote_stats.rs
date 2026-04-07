use std::collections::VecDeque;
use std::sync::Arc;

use coast_core::protocol::ContainerStats;
use coast_core::types::{RemoteEntry, RemoteStats};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::server::AppState;

const POLL_INTERVAL_SECS: u64 = 30;
const STREAM_INTERVAL_SECS: u64 = 2;
const HISTORY_CAP: usize = 300;

pub fn spawn_remote_stats_poller(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            let remotes = {
                let db = state.db.lock().await;
                db.list_remotes().unwrap_or_default()
            };

            for entry in &remotes {
                match fetch_stats(entry).await {
                    Ok(stats) => {
                        let mut cache = state.remote_stats_cache.lock().await;
                        cache.insert(entry.name.clone(), stats);
                    }
                    Err(e) => {
                        warn!(remote = %entry.name, error = %e, "failed to fetch remote stats");
                        let mut cache = state.remote_stats_cache.lock().await;
                        cache.remove(&entry.name);
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
        }
    });
}

pub async fn fetch_stats(entry: &RemoteEntry) -> Result<RemoteStats, String> {
    let script = r#"
mem=$(free -b 2>/dev/null | awk '/Mem:/{print $2,$3}')
cpus=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 1)
cpu_pct=$(top -bn1 2>/dev/null | awk '/^%?Cpu/{gsub(/[^0-9.]/, "", $2); print $2; exit}')
disk=$(df -B1 / 2>/dev/null | awk 'NR==2{print $2,$3}')
svc_ver=$(curl -sf http://localhost:31420/info 2>/dev/null | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
echo "$mem"
echo "$cpus"
echo "${cpu_pct:-0}"
echo "$disk"
echo "${svc_ver:-unknown}"
"#;

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-p",
        &entry.port.to_string(),
    ]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd.arg(format!("{}@{}", entry.user, entry.host));
    cmd.arg(script.trim());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("ssh command failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "ssh exit {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();

    if lines.len() < 4 {
        warn!(remote = %entry.name, output = %stdout.trim(), "unexpected stats output");
        return Err(format!("expected 4 lines, got {}", lines.len()));
    }

    let mem_parts: Vec<&str> = lines[0].split_whitespace().collect();
    let total_memory_bytes = mem_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let used_memory_bytes = mem_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let cpu_count: u32 = lines[1].trim().parse().unwrap_or(1);
    let cpu_usage_percent: f32 = lines[2].trim().parse().unwrap_or(0.0);

    let disk_parts: Vec<&str> = lines[3].split_whitespace().collect();
    let total_disk_bytes = disk_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let used_disk_bytes = disk_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let service_version = lines
        .get(4)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "unknown");

    Ok(RemoteStats {
        total_memory_bytes,
        used_memory_bytes,
        cpu_count,
        cpu_usage_percent,
        total_disk_bytes,
        used_disk_bytes,
        service_version,
    })
}

/// Raw /proc readings from a single SSH sample.
struct ProcSample {
    cpu_user: u64,
    cpu_nice: u64,
    cpu_system: u64,
    cpu_idle: u64,
    cpu_iowait: u64,
    cpu_irq: u64,
    cpu_softirq: u64,
    mem_total: u64,
    mem_available: u64,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    net_rx_bytes: u64,
    net_tx_bytes: u64,
    running_procs: u64,
}

fn build_ssh_cmd(entry: &RemoteEntry) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-p",
        &entry.port.to_string(),
    ]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd.arg(format!("{}@{}", entry.user, entry.host));
    cmd
}

async fn fetch_proc_sample(entry: &RemoteEntry) -> Result<ProcSample, String> {
    let script = concat!(
        "head -1 /proc/stat;",
        "awk '/MemTotal:|MemAvailable:/{print $1,$2}' /proc/meminfo;",
        "awk '$3 !~ /^(loop|ram|dm-)/{r+=$6; w+=$10} END{print r*512,w*512}' /proc/diskstats;",
        "awk 'NR>2 && $1 !~ /lo:/{rx+=$2; tx+=$10} END{print rx,tx}' /proc/net/dev;",
        "awk '{print $4}' /proc/loadavg",
    );

    let mut cmd = build_ssh_cmd(entry);
    cmd.arg(script);

    let output = cmd.output().await.map_err(|e| format!("ssh failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "ssh exit {}: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();

    if lines.len() < 5 {
        return Err(format!(
            "expected >=5 lines from /proc, got {}",
            lines.len()
        ));
    }

    let cpu_parts: Vec<u64> = lines[0]
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();

    let mut mem_total: u64 = 0;
    let mut mem_available: u64 = 0;
    for line in &lines[1..] {
        if line.starts_with("MemTotal:") {
            mem_total = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
                * 1024;
        } else if line.starts_with("MemAvailable:") {
            mem_available = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
                * 1024;
        }
    }

    let disk_line = lines.iter().rev().find(|l| {
        let parts: Vec<&str> = l.split_whitespace().collect();
        parts.len() == 2
            && parts[0].parse::<u64>().is_ok()
            && parts[1].parse::<u64>().is_ok()
            && !l.contains("MemTotal")
            && !l.contains("MemAvailable")
    });

    let (disk_read_bytes, disk_write_bytes) = if let Some(dl) = disk_line {
        let parts: Vec<u64> = dl
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
        )
    } else {
        (0, 0)
    };

    let net_disk_lines: Vec<&str> = lines
        .iter()
        .filter(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.len() == 2
                && parts[0].parse::<u64>().is_ok()
                && !l.contains("MemTotal")
                && !l.contains("MemAvailable")
        })
        .copied()
        .collect();

    let (net_rx, net_tx) = if net_disk_lines.len() >= 2 {
        let net_line = net_disk_lines.last().unwrap();
        let parts: Vec<u64> = net_line
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
        )
    } else {
        (0, 0)
    };

    let running_procs: u64 = lines
        .last()
        .and_then(|l| l.split('/').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    Ok(ProcSample {
        cpu_user: cpu_parts.first().copied().unwrap_or(0),
        cpu_nice: cpu_parts.get(1).copied().unwrap_or(0),
        cpu_system: cpu_parts.get(2).copied().unwrap_or(0),
        cpu_idle: cpu_parts.get(3).copied().unwrap_or(0),
        cpu_iowait: cpu_parts.get(4).copied().unwrap_or(0),
        cpu_irq: cpu_parts.get(5).copied().unwrap_or(0),
        cpu_softirq: cpu_parts.get(6).copied().unwrap_or(0),
        mem_total,
        mem_available,
        disk_read_bytes,
        disk_write_bytes,
        net_rx_bytes: net_rx,
        net_tx_bytes: net_tx,
        running_procs,
    })
}

fn cpu_percent(prev: &ProcSample, curr: &ProcSample) -> f64 {
    let prev_total = prev.cpu_user
        + prev.cpu_nice
        + prev.cpu_system
        + prev.cpu_idle
        + prev.cpu_iowait
        + prev.cpu_irq
        + prev.cpu_softirq;
    let curr_total = curr.cpu_user
        + curr.cpu_nice
        + curr.cpu_system
        + curr.cpu_idle
        + curr.cpu_iowait
        + curr.cpu_irq
        + curr.cpu_softirq;
    let total_delta = curr_total.saturating_sub(prev_total);
    let idle_delta = curr.cpu_idle.saturating_sub(prev.cpu_idle);
    if total_delta == 0 {
        return 0.0;
    }
    ((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0
}

pub async fn start_remote_stats_collector(state: Arc<AppState>, remote_name: String) {
    let mut collectors = state.remote_streaming_collectors.lock().await;
    if collectors.contains_key(&remote_name) {
        return;
    }

    let (tx, _) = broadcast::channel::<serde_json::Value>(64);
    state
        .remote_streaming_broadcasts
        .lock()
        .await
        .insert(remote_name.clone(), tx.clone());

    let state2 = state.clone();
    let name2 = remote_name.clone();
    let handle = tokio::spawn(async move {
        run_streaming_collector(state2, name2, tx).await;
    });

    collectors.insert(remote_name, handle);
}

fn sample_to_stats(sample: &ProcSample, prev: Option<&ProcSample>) -> ContainerStats {
    let cpu_pct = prev.map(|p| cpu_percent(p, sample)).unwrap_or(0.0);
    let mem_used = sample.mem_total.saturating_sub(sample.mem_available);
    let mem_pct = if sample.mem_total > 0 {
        (mem_used as f64 / sample.mem_total as f64) * 100.0
    } else {
        0.0
    };

    ContainerStats {
        timestamp: chrono::Utc::now().to_rfc3339(),
        cpu_percent: cpu_pct,
        memory_used_bytes: mem_used,
        memory_limit_bytes: sample.mem_total,
        memory_percent: mem_pct,
        disk_read_bytes: sample.disk_read_bytes,
        disk_write_bytes: sample.disk_write_bytes,
        network_rx_bytes: sample.net_rx_bytes,
        network_tx_bytes: sample.net_tx_bytes,
        pids: sample.running_procs,
    }
}

async fn run_streaming_collector(
    state: Arc<AppState>,
    remote_name: String,
    tx: broadcast::Sender<serde_json::Value>,
) {
    let entry = {
        let db = state.db.lock().await;
        match db.get_remote(&remote_name) {
            Ok(Some(e)) => e,
            _ => {
                warn!(remote = %remote_name, "remote not found for stats collector");
                return;
            }
        }
    };

    info!(remote = %remote_name, "remote streaming stats collector started");

    let mut prev_sample: Option<ProcSample> = None;

    loop {
        match fetch_proc_sample(&entry).await {
            Ok(sample) => {
                let cs = sample_to_stats(&sample, prev_sample.as_ref());

                if let Ok(json_val) = serde_json::to_value(&cs) {
                    {
                        let mut history = state.remote_streaming_history.lock().await;
                        let ring = history
                            .entry(remote_name.clone())
                            .or_insert_with(VecDeque::new);
                        if ring.len() >= HISTORY_CAP {
                            ring.pop_front();
                        }
                        ring.push_back(json_val.clone());
                    }
                    let _ = tx.send(json_val);
                }

                prev_sample = Some(sample);
            }
            Err(e) => {
                warn!(remote = %remote_name, error = %e, "failed to fetch /proc stats");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(STREAM_INTERVAL_SECS)).await;
    }
}
