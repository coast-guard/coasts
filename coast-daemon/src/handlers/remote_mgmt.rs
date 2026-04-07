/// Handler for `coast remote` commands — manage registered remote machines.
use std::sync::Arc;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{CoastEvent, RemoteRequest, RemoteResponse, Response};
use coast_core::types::RemoteEntry;

use crate::server::AppState;

/// Dispatch a remote management request.
pub async fn handle(req: RemoteRequest, state: &Arc<AppState>) -> Response {
    match dispatch(req, state).await {
        Ok(resp) => Response::Remote(resp),
        Err(e) => Response::Error(coast_core::protocol::ErrorResponse {
            error: e.to_string(),
        }),
    }
}

async fn dispatch(req: RemoteRequest, state: &Arc<AppState>) -> Result<RemoteResponse> {
    match req {
        RemoteRequest::Add {
            name,
            host,
            user,
            port,
            ssh_key,
            sync_strategy,
        } => handle_add(state, name, host, user, port, ssh_key, sync_strategy).await,
        RemoteRequest::Ls => handle_ls(state).await,
        RemoteRequest::Rm { name } => handle_rm(state, name).await,
        RemoteRequest::Test { name } => handle_test(state, name).await,
        RemoteRequest::Setup { name, docker } => handle_setup(state, name, docker).await,
        RemoteRequest::Prune { name, dry_run } => handle_prune(state, name, dry_run).await,
    }
}

async fn handle_add(
    state: &Arc<AppState>,
    name: String,
    host: String,
    user: String,
    port: u16,
    ssh_key: Option<String>,
    sync_strategy: String,
) -> Result<RemoteResponse> {
    if name.is_empty() {
        return Err(CoastError::state("remote name cannot be empty"));
    }
    if host.is_empty() {
        return Err(CoastError::state("remote host cannot be empty"));
    }
    if port == 0 {
        return Err(CoastError::state("remote port cannot be 0"));
    }

    let entry = RemoteEntry {
        name: name.clone(),
        host: host.clone(),
        user: user.clone(),
        port,
        ssh_key,
        sync_strategy,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let db = state.db.lock().await;
    db.insert_remote(&entry)?;
    drop(db);

    info!(name = %name, host = %host, user = %user, port, "registered remote");

    state.emit_event(CoastEvent::RemoteAdded { name: name.clone() });

    Ok(RemoteResponse {
        message: format!("Remote '{name}' added ({user}@{host}:{port})"),
        remotes: vec![entry],
    })
}

async fn handle_ls(state: &Arc<AppState>) -> Result<RemoteResponse> {
    let db = state.db.lock().await;
    let remotes = db.list_remotes()?;

    Ok(RemoteResponse {
        message: format!("{} remote(s)", remotes.len()),
        remotes,
    })
}

async fn handle_rm(state: &Arc<AppState>, name: String) -> Result<RemoteResponse> {
    let db = state.db.lock().await;
    let deleted = db.delete_remote(&name)?;
    drop(db);

    if !deleted {
        return Err(CoastError::state(format!(
            "no remote named '{name}'. Run `coast remote ls` to see registered remotes."
        )));
    }

    info!(name = %name, "removed remote");

    state.emit_event(CoastEvent::RemoteRemoved { name: name.clone() });

    Ok(RemoteResponse {
        message: format!("Remote '{name}' removed"),
        remotes: vec![],
    })
}

async fn handle_test(state: &Arc<AppState>, name: String) -> Result<RemoteResponse> {
    let db = state.db.lock().await;
    let entry = db.get_remote(&name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote named '{name}'. Run `coast remote ls` to see registered remotes."
        ))
    })?;
    drop(db);

    info!(name = %name, host = %entry.host, "testing remote connectivity");

    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-p",
        &entry.port.to_string(),
    ]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd.arg(format!("{}@{}", entry.user, entry.host));
    cmd.arg("echo coast-service-ping");

    let output = cmd
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to run ssh: {e}")))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let service_reachable = stdout.trim().contains("coast-service-ping");

        let msg = if service_reachable {
            format!(
                "Remote '{name}' is reachable ({}@{}:{})",
                entry.user, entry.host, entry.port
            )
        } else {
            format!(
                "SSH to '{name}' succeeded but unexpected output. \
                 Check that the remote is accessible."
            )
        };

        Ok(RemoteResponse {
            message: msg,
            remotes: vec![entry],
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(name = %name, stderr = %stderr.trim(), "SSH connectivity test failed");
        Err(CoastError::state(format!(
            "SSH to '{name}' failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )))
    }
}

fn ssh_cmd(entry: &RemoteEntry) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("ssh");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
    ]);
    cmd.args(["-p", &entry.port.to_string()]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd.arg(format!("{}@{}", entry.user, entry.host));
    cmd
}

fn scp_cmd(entry: &RemoteEntry) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("scp");
    cmd.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
    ]);
    cmd.args(["-P", &entry.port.to_string()]);
    if let Some(ref key) = entry.ssh_key {
        cmd.args(["-i", key]);
    }
    cmd
}

fn find_coast_service_binary() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe()
        .map_err(|e| CoastError::state(format!("cannot determine current executable path: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CoastError::state("cannot determine directory of current executable"))?;
    let candidate = dir.join("coast-service");
    if candidate.exists() {
        return Ok(candidate);
    }

    if let Ok(output) = std::process::Command::new("which")
        .arg("coast-service")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(std::path::PathBuf::from(path));
            }
        }
    }

    Err(CoastError::state(
        "coast-service binary not found. Expected next to coastd or on PATH.",
    ))
}

async fn deploy_binary_to_remote(entry: &RemoteEntry, binary_path: &std::path::Path) -> Result<()> {
    let output = ssh_cmd(entry)
        .arg("mkdir -p ~/.coast-service")
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to run ssh: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::state(format!(
            "failed to create remote directory: {}",
            stderr.trim()
        )));
    }

    let dest = format!(
        "{}@{}:~/.coast-service/coast-service",
        entry.user, entry.host
    );
    let output = scp_cmd(entry)
        .arg(binary_path.to_str().unwrap_or("coast-service"))
        .arg(&dest)
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to run scp: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::state(format!(
            "failed to copy binary to remote: {}",
            stderr.trim()
        )));
    }

    Ok(())
}

async fn start_and_health_check(entry: &RemoteEntry, name: &str) -> Result<bool> {
    let start_script = "chmod +x ~/.coast-service/coast-service && \
        (pgrep -f coast-service && pkill -f coast-service && sleep 1 || true) && \
        nohup ~/.coast-service/coast-service > ~/.coast-service/coast-service.log 2>&1 &";
    let output = ssh_cmd(entry)
        .arg(start_script)
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to start coast-service: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(name = %name, stderr = %stderr.trim(), "start command had non-zero exit");
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let health_output = ssh_cmd(entry)
        .arg("curl -sf http://localhost:31420/health || ~/.coast-service/coast-service --version 2>/dev/null || pgrep -f coast-service")
        .output()
        .await
        .map_err(|e| CoastError::state(format!("health check failed: {e}")))?;

    if !health_output.status.success() {
        let stderr = String::from_utf8_lossy(&health_output.stderr);
        warn!(name = %name, stderr = %stderr.trim(), "health check failed after setup");
    }

    Ok(health_output.status.success())
}

async fn handle_setup(state: &Arc<AppState>, name: String, docker: bool) -> Result<RemoteResponse> {
    if docker {
        return Err(CoastError::state(
            "Docker-based setup is not yet implemented. Omit --docker to deploy the binary directly.",
        ));
    }

    let db = state.db.lock().await;
    let entry = db.get_remote(&name)?.ok_or_else(|| {
        CoastError::state(format!(
            "no remote named '{name}'. Run `coast remote ls` to see registered remotes."
        ))
    })?;
    drop(db);

    let binary_path = find_coast_service_binary()?;
    info!(
        name = %name,
        binary = %binary_path.display(),
        host = %entry.host,
        "setting up coast-service on remote"
    );

    deploy_binary_to_remote(&entry, &binary_path).await?;
    let health_ok = start_and_health_check(&entry, &name).await?;

    let msg = if health_ok {
        format!(
            "coast-service deployed and running on '{name}' ({}@{}:{})",
            entry.user, entry.host, entry.port
        )
    } else {
        format!(
            "coast-service deployed to '{name}' but health check failed. \
             Check ~/.coast-service/coast-service.log on the remote."
        )
    };

    info!(name = %name, healthy = health_ok, "setup complete");

    Ok(RemoteResponse {
        message: msg,
        remotes: vec![entry],
    })
}

async fn handle_prune(
    state: &Arc<AppState>,
    name: String,
    dry_run: bool,
) -> Result<RemoteResponse> {
    let db = state.db.lock().await;
    let entry = db
        .get_remote(&name)?
        .ok_or_else(|| CoastError::state(format!("Remote '{name}' not found.")))?;
    drop(db);

    let connection = coast_core::types::RemoteConnection::from_entry(
        &entry,
        &coast_core::types::RemoteConfig {
            workspace_sync: coast_core::types::SyncStrategy::default(),
        },
    );

    let client = super::remote::RemoteClient::connect(&connection).await?;
    let prune_req = coast_core::protocol::api_types::PruneRequest { dry_run };
    let resp = super::remote::forward::forward_prune(&client, &prune_req).await?;

    let action = if dry_run { "would prune" } else { "pruned" };
    let mut lines = Vec::new();
    for item in &resp.items {
        let size_mb = item.size_bytes as f64 / (1024.0 * 1024.0);
        lines.push(format!("  {} {} ({:.1} MB)", item.kind, item.name, size_mb));
    }
    let total_mb = resp.items.iter().map(|i| i.size_bytes).sum::<u64>() as f64 / (1024.0 * 1024.0);
    let summary = if resp.items.is_empty() {
        format!("Nothing to prune on '{name}'.")
    } else {
        format!(
            "{} {} item(s) on '{name}' ({:.1} MB):\n{}",
            action,
            resp.items.len(),
            total_mb,
            lines.join("\n")
        )
    };

    Ok(RemoteResponse {
        message: summary,
        remotes: vec![entry],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_add_ls_rm_lifecycle() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let resp = dispatch(
            RemoteRequest::Add {
                name: "test-vm".into(),
                host: "10.0.0.1".into(),
                user: "ubuntu".into(),
                port: 22,
                ssh_key: None,
                sync_strategy: "rsync".into(),
            },
            &state,
        )
        .await
        .unwrap();
        assert_eq!(resp.remotes.len(), 1);
        assert_eq!(resp.remotes[0].name, "test-vm");

        let resp = dispatch(RemoteRequest::Ls, &state).await.unwrap();
        assert_eq!(resp.remotes.len(), 1);

        let resp = dispatch(
            RemoteRequest::Rm {
                name: "test-vm".into(),
            },
            &state,
        )
        .await
        .unwrap();
        assert!(resp.remotes.is_empty());

        let resp = dispatch(RemoteRequest::Ls, &state).await.unwrap();
        assert!(resp.remotes.is_empty());
    }

    #[tokio::test]
    async fn test_handler_add_duplicate_error() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let add = RemoteRequest::Add {
            name: "dupe".into(),
            host: "10.0.0.1".into(),
            user: "u".into(),
            port: 22,
            ssh_key: None,
            sync_strategy: "rsync".into(),
        };
        dispatch(add.clone(), &state).await.unwrap();
        let err = dispatch(add, &state).await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_handler_rm_nonexistent_error() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let err = dispatch(
            RemoteRequest::Rm {
                name: "ghost".into(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote named"));
    }

    #[tokio::test]
    async fn test_handler_add_empty_name_error() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let err = dispatch(
            RemoteRequest::Add {
                name: "".into(),
                host: "h".into(),
                user: "u".into(),
                port: 22,
                ssh_key: None,
                sync_strategy: "rsync".into(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("name cannot be empty"));
    }

    #[tokio::test]
    async fn test_handler_setup_nonexistent_remote_error() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let err = dispatch(
            RemoteRequest::Setup {
                name: "ghost".into(),
                docker: false,
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no remote named"));
    }

    #[tokio::test]
    async fn test_handler_setup_docker_not_implemented() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let add = RemoteRequest::Add {
            name: "docker-vm".into(),
            host: "10.0.0.1".into(),
            user: "ubuntu".into(),
            port: 22,
            ssh_key: None,
            sync_strategy: "rsync".into(),
        };
        dispatch(add, &state).await.unwrap();

        let err = dispatch(
            RemoteRequest::Setup {
                name: "docker-vm".into(),
                docker: true,
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[tokio::test]
    async fn test_handler_add_empty_host_error() {
        let db = crate::state::StateDb::open_in_memory().unwrap();
        let state = Arc::new(AppState::new_for_testing(db));

        let err = dispatch(
            RemoteRequest::Add {
                name: "x".into(),
                host: "".into(),
                user: "u".into(),
                port: 22,
                ssh_key: None,
                sync_strategy: "rsync".into(),
            },
            &state,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("host cannot be empty"));
    }
}
