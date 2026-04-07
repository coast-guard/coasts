/// Remote coast execution module.
///
/// Handles communication with a remote `coast-service` instance:
/// - SSH tunnel management for the control channel
/// - Workspace file sync via rsync or mutagen
/// - Request forwarding to the remote coast-service
pub mod forward;
pub mod sync;
pub mod tunnel;

use coast_core::error::{CoastError, Result};
use coast_core::types::RemoteConnection;
use tracing::debug;

use crate::server::AppState;

/// Active connection to a remote coast-service instance.
pub struct RemoteClient {
    pub config: RemoteConnection,
    /// Holds the control-channel `SshTunnel` so it is not dropped while this client exists.
    pub tunnel: tunnel::SshTunnel,
    pub service_url: String,
    /// Whether the remote SSH user has passwordless sudo access.
    pub has_sudo: bool,
}

impl RemoteClient {
    /// Connect to a remote coast-service via SSH tunnel.
    /// Uses a cached tunnel when available to avoid connection churn.
    pub async fn connect(config: &RemoteConnection) -> Result<Self> {
        let tunnel = tunnel::SshTunnel::establish_cached(config).await?;
        let service_url = format!("http://127.0.0.1:{}", tunnel.local_port);
        let has_sudo = probe_sudo(config).await;

        Ok(Self {
            config: config.clone(),
            tunnel,
            service_url,
            has_sudo,
        })
    }

    /// Sync workspace files to the remote host (deletes extra files on remote).
    pub async fn sync_workspace(
        &self,
        local_path: &std::path::Path,
        remote_workspace_path: &str,
    ) -> Result<()> {
        debug!(
            local_port = self.tunnel.local_port,
            has_sudo = self.has_sudo,
            "sync_workspace (control tunnel stays open for coast-service)"
        );
        sync::rsync_workspace(
            local_path,
            remote_workspace_path,
            &self.config,
            self.has_sudo,
        )
        .await
    }

    /// Sync workspace files without deleting remote-only files (e.g. generated code).
    pub async fn sync_workspace_no_delete(
        &self,
        local_path: &std::path::Path,
        remote_workspace_path: &str,
    ) -> Result<()> {
        sync::rsync_workspace_opts(
            local_path,
            remote_workspace_path,
            &self.config,
            self.has_sudo,
            false,
        )
        .await
    }

    /// Query the remote coast-service's `service_home` directory.
    ///
    /// Calls `GET /info` on the coast-service API and extracts the
    /// `service_home` field. Falls back to `~/.coast-service` if the
    /// endpoint is unavailable (backward compat).
    pub async fn query_service_home(&self) -> String {
        let url = format!("{}/info", self.service_url);
        match reqwest::get(&url).await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(version) = body.get("version").and_then(|v| v.as_str()) {
                        tracing::info!(version, "remote coast-service version");
                    }
                    if let Some(home) = body.get("service_home").and_then(|v| v.as_str()) {
                        return home.to_string();
                    }
                }
                "~/.coast-service".to_string()
            }
            Err(_) => "~/.coast-service".to_string(),
        }
    }
}

/// Probe whether the remote SSH user has passwordless sudo.
async fn probe_sudo(config: &RemoteConnection) -> bool {
    let ssh_key_str = config.ssh_key.display().to_string();
    let output = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/coast-ssh-%r@%h:%p",
            "-o",
            "ControlPersist=300",
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
        ])
        .arg(format!("{}@{}", config.user, config.host))
        .arg("sudo -n true 2>/dev/null")
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            debug!(user = %config.user, "remote user has passwordless sudo");
            true
        }
        _ => {
            debug!(user = %config.user, "remote user does not have sudo (will use plain rsync)");
            false
        }
    }
}

/// Resolve the remote workspace path for a project and instance.
///
/// `service_home` should be queried from coast-service via
/// `RemoteClient::query_service_home()`.
pub fn remote_workspace_path(service_home: &str, project: &str, instance_name: &str) -> String {
    format!("{service_home}/workspaces/{project}/{instance_name}")
}

/// Resolve a `RemoteConnection` for a project by looking up the instance's
/// `remote_host` from the DB and finding the matching registered remote.
pub async fn resolve_remote_for_instance(
    project: &str,
    instance_name: &str,
    state: &AppState,
) -> Result<RemoteConnection> {
    let db = state.db.lock().await;
    let instance = db
        .get_instance(project, instance_name)?
        .ok_or_else(|| CoastError::state(format!("instance '{instance_name}' not found")))?;

    let remote_host = instance
        .remote_host
        .as_deref()
        .ok_or_else(|| CoastError::state("instance is not a remote instance"))?;

    let remotes = db.list_remotes()?;
    let entry = remotes
        .iter()
        .find(|r| r.name == remote_host || r.host == remote_host)
        .ok_or_else(|| {
            CoastError::state(format!(
                "no registered remote matching '{}'. Run `coast remote add` to register it.",
                remote_host
            ))
        })?;

    let cf_remote = load_remote_config_from_artifact(project);
    let workspace_sync = cf_remote.map(|c| c.workspace_sync).unwrap_or_default();

    Ok(RemoteConnection::from_entry(
        entry,
        &coast_core::types::RemoteConfig { workspace_sync },
    ))
}

/// Load the `RemoteConfig` from a project's build artifact, trying both
/// `latest-remote` and `latest` symlinks.
fn load_remote_config_from_artifact(project: &str) -> Option<coast_core::types::RemoteConfig> {
    let images_dir = coast_core::artifact::coast_home()
        .ok()?
        .join("images")
        .join(project);

    for latest_name in &["latest-remote", "latest"] {
        let cf_path = images_dir.join(latest_name).join("coastfile.toml");
        if cf_path.exists() {
            let content = std::fs::read_to_string(&cf_path).ok()?;
            let cf_dir = cf_path.parent()?;
            if let Ok(cf) = coast_core::coastfile::Coastfile::parse(&content, cf_dir) {
                if cf.remote.is_some() {
                    return cf.remote;
                }
            }
        }
    }

    None
}
