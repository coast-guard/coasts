//! Mutagen sync manager for remote file synchronization.
//!
//! Manages Mutagen sync sessions that keep local worktrees in sync with remote VMs.
//! Uses unidirectional sync (local → remote) for development workflows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::state::remotes::{Remote, SyncSession, SyncStatus};
use coast_core::error::{CoastError, Result};

/// Default timeout for waiting for sync to complete.
const DEFAULT_SYNC_TIMEOUT_SECS: u64 = 120;

/// Default remote workspace root.
const REMOTE_WORKSPACE_ROOT: &str = "~/coast-workspaces";

/// Manages Mutagen sync sessions.
pub struct MutagenManager {
    /// Active sync sessions in memory, keyed by session name.
    sessions: Arc<RwLock<HashMap<String, MutagenSessionInfo>>>,
}

/// In-memory info about an active Mutagen session.
#[derive(Debug, Clone)]
pub struct MutagenSessionInfo {
    /// Mutagen session identifier (from `mutagen sync create`).
    pub session_id: String,
    /// Session name (coast-<project>-<branch>-<remote>).
    pub session_name: String,
    /// Project name.
    pub project: String,
    /// Branch/worktree name.
    pub branch: String,
    /// Remote name.
    pub remote_name: String,
    /// Local path being synced.
    pub local_path: PathBuf,
    /// Remote path on VM.
    pub remote_path: String,
    /// Current status.
    pub status: SyncStatus,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

/// Result of creating a sync session.
#[derive(Debug, Clone)]
pub struct CreateSessionResult {
    /// The session info.
    pub session: MutagenSessionInfo,
    /// State to persist to database.
    pub db_session: SyncSession,
}

/// Status information from Mutagen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutagenStatus {
    /// Session identifier.
    pub identifier: Option<String>,
    /// Session name/label.
    pub name: Option<String>,
    /// Connection state.
    pub status: Option<String>,
    /// Alpha (local) connected.
    pub alpha_connected: Option<bool>,
    /// Beta (remote) connected.
    pub beta_connected: Option<bool>,
    /// Number of staged entries.
    pub staged_entries: Option<u64>,
    /// Any conflicts.
    pub conflicts: Option<Vec<String>>,
    /// Last error if any.
    pub last_error: Option<String>,
}

/// Parsed output from `mutagen sync list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MutagenListOutput {
    sessions: Option<Vec<MutagenListSession>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MutagenListSession {
    identifier: Option<String>,
    name: Option<String>,
    alpha: Option<MutagenEndpoint>,
    beta: Option<MutagenEndpoint>,
    status: Option<MutagenSessionStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MutagenEndpoint {
    path: Option<String>,
    connected: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MutagenSessionStatus {
    description: Option<String>,
    #[serde(rename = "stagingProgress")]
    staging_progress: Option<u64>,
}

impl MutagenManager {
    /// Create a new MutagenManager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if Mutagen is installed and available.
    pub async fn check_mutagen_installed() -> Result<String> {
        let output = Command::new("mutagen")
            .arg("version")
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!(
                    "Mutagen is not installed or not in PATH. Install from https://mutagen.io: {e}"
                ),
            })?;

        if !output.status.success() {
            return Err(CoastError::Remote {
                message: "Mutagen version check failed".to_string(),
            });
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(version)
    }

    /// Generate a session name for a project/branch/remote combination.
    pub fn generate_session_name(project: &str, branch: &str, remote_name: &str) -> String {
        // Sanitize components for valid Mutagen label
        let sanitize = |s: &str| {
            s.chars()
                .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
                .collect::<String>()
                .to_lowercase()
        };

        format!(
            "coast-{}-{}-{}",
            sanitize(project),
            sanitize(branch),
            sanitize(remote_name)
        )
    }

    /// Generate the remote path for a project/branch.
    pub fn generate_remote_path(project: &str, branch: &str) -> String {
        format!("{}/{}/{}", REMOTE_WORKSPACE_ROOT, project, branch)
    }

    /// Build ignore patterns from .coastignore and common patterns.
    fn build_ignore_patterns(local_path: &Path) -> Vec<String> {
        let mut patterns = vec![
            // Always ignore
            ".git".to_string(),
            ".git/**".to_string(),
            "node_modules".to_string(),
            "node_modules/**".to_string(),
            "target".to_string(),
            "target/**".to_string(),
            "__pycache__".to_string(),
            "__pycache__/**".to_string(),
            "*.pyc".to_string(),
            ".coast".to_string(),
            ".coast/**".to_string(),
            ".DS_Store".to_string(),
            "*.swp".to_string(),
            "*.swo".to_string(),
            "*~".to_string(),
        ];

        // Read .coastignore if exists
        let coastignore_path = local_path.join(".coastignore");
        if let Ok(content) = std::fs::read_to_string(&coastignore_path) {
            debug!(path = %coastignore_path.display(), "reading .coastignore");
            for line in content.lines() {
                let line = line.trim();
                if !line.is_empty() && !line.starts_with('#') {
                    patterns.push(line.to_string());
                }
            }
        }

        patterns
    }

    /// Create a new sync session.
    ///
    /// Returns the session info and a SyncSession for database persistence.
    pub async fn create_session(
        &self,
        project: &str,
        branch: &str,
        remote: &Remote,
        local_path: &Path,
    ) -> Result<CreateSessionResult> {
        // Check Mutagen is available
        let _version = Self::check_mutagen_installed().await?;

        let session_name = Self::generate_session_name(project, branch, &remote.name);
        let remote_path = Self::generate_remote_path(project, branch);

        // Check if session already exists
        if let Some(existing) = self.get_session(&session_name).await {
            info!(session = %session_name, "sync session already exists, reusing");
            return Ok(CreateSessionResult {
                session: existing.clone(),
                db_session: SyncSession {
                    project: project.to_string(),
                    remote_name: remote.name.clone(),
                    local_path: existing.local_path.to_string_lossy().to_string(),
                    remote_path: existing.remote_path.clone(),
                    mutagen_session_id: Some(existing.session_id.clone()),
                    status: existing.status,
                    last_sync_at: None,
                    created_at: existing.created_at,
                },
            });
        }

        // Build ignore patterns
        let ignores = Self::build_ignore_patterns(local_path);

        // Build Mutagen command
        let mut cmd = Command::new("mutagen");
        cmd.args(["sync", "create"]);
        cmd.args(["--name", &session_name]);
        cmd.args(["--sync-mode", "one-way-safe"]); // Local → Remote only

        // Add ignore patterns
        for pattern in &ignores {
            cmd.args(["--ignore", pattern]);
        }

        // Configure SSH command if key is specified
        // Mutagen uses SSH under the hood and respects the SSH_COMMAND environment variable
        if let Some(ref key_path) = remote.ssh_key_path {
            cmd.env(
                "MUTAGEN_SSH_COMMAND",
                format!("ssh -i {}", key_path),
            );
        }

        // Source (local) and destination (remote)
        let local_str = local_path.to_string_lossy();
        let remote_str = format!(
            "{}@{}:{}",
            remote.user, remote.host, remote_path
        );

        cmd.arg(local_str.as_ref());
        cmd.arg(&remote_str);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        info!(
            session = %session_name,
            local = %local_str,
            remote = %remote_str,
            "creating Mutagen sync session"
        );

        let output = cmd.output().await.map_err(|e| CoastError::Remote {
            message: format!("failed to create Mutagen sync session: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!("Mutagen sync create failed: {stderr}"),
            });
        }

        // Get the session ID from list
        let session_id = self
            .get_session_id_by_name(&session_name)
            .await?
            .unwrap_or_else(|| session_name.clone());

        let now = Utc::now();
        let session_info = MutagenSessionInfo {
            session_id: session_id.clone(),
            session_name: session_name.clone(),
            project: project.to_string(),
            branch: branch.to_string(),
            remote_name: remote.name.clone(),
            local_path: local_path.to_path_buf(),
            remote_path: remote_path.clone(),
            status: SyncStatus::Initial,
            created_at: now,
        };

        // Store in memory
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_name.clone(), session_info.clone());
        }

        info!(session = %session_name, id = %session_id, "Mutagen sync session created");

        Ok(CreateSessionResult {
            session: session_info,
            db_session: SyncSession {
                project: project.to_string(),
                remote_name: remote.name.clone(),
                local_path: local_path.to_string_lossy().to_string(),
                remote_path,
                mutagen_session_id: Some(session_id),
                status: SyncStatus::Initial,
                last_sync_at: None,
                created_at: now,
            },
        })
    }

    /// Get session ID by name from Mutagen.
    async fn get_session_id_by_name(&self, session_name: &str) -> Result<Option<String>> {
        let output = Command::new("mutagen")
            .args(["sync", "list", "--label-selector", &format!("name={}", session_name), "-o", "json"])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to list Mutagen sessions: {e}"),
            })?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(list) = serde_json::from_str::<MutagenListOutput>(&stdout) {
            if let Some(sessions) = list.sessions {
                if let Some(session) = sessions.first() {
                    return Ok(session.identifier.clone());
                }
            }
        }

        Ok(None)
    }

    /// Get an in-memory session by name.
    pub async fn get_session(&self, session_name: &str) -> Option<MutagenSessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.get(session_name).cloned()
    }

    /// Get session by project name.
    pub async fn get_session_by_project(&self, project: &str) -> Option<MutagenSessionInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .find(|s| s.project == project)
            .cloned()
    }

    /// Pause a sync session.
    pub async fn pause_session(&self, session_name: &str) -> Result<()> {
        info!(session = %session_name, "pausing Mutagen sync session");

        let output = Command::new("mutagen")
            .args(["sync", "pause", session_name])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to pause Mutagen session: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!("Mutagen sync pause failed: {stderr}"),
            });
        }

        // Update in-memory status
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_name) {
                session.status = SyncStatus::Paused;
            }
        }

        info!(session = %session_name, "Mutagen sync session paused");
        Ok(())
    }

    /// Resume a paused sync session.
    pub async fn resume_session(&self, session_name: &str) -> Result<()> {
        info!(session = %session_name, "resuming Mutagen sync session");

        let output = Command::new("mutagen")
            .args(["sync", "resume", session_name])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to resume Mutagen session: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!("Mutagen sync resume failed: {stderr}"),
            });
        }

        // Update in-memory status
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_name) {
                session.status = SyncStatus::Syncing;
            }
        }

        info!(session = %session_name, "Mutagen sync session resumed");
        Ok(())
    }

    /// Terminate and remove a sync session.
    pub async fn terminate_session(&self, session_name: &str) -> Result<()> {
        info!(session = %session_name, "terminating Mutagen sync session");

        let output = Command::new("mutagen")
            .args(["sync", "terminate", session_name])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to terminate Mutagen session: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't error if session doesn't exist
            if !stderr.contains("no matching sessions") {
                return Err(CoastError::Remote {
                    message: format!("Mutagen sync terminate failed: {stderr}"),
                });
            }
        }

        // Remove from memory
        {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_name);
        }

        info!(session = %session_name, "Mutagen sync session terminated");
        Ok(())
    }

    /// Flush pending changes and wait for sync to complete.
    pub async fn flush_session(&self, session_name: &str) -> Result<()> {
        info!(session = %session_name, "flushing Mutagen sync session");

        let output = Command::new("mutagen")
            .args(["sync", "flush", session_name])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to flush Mutagen session: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!("Mutagen sync flush failed: {stderr}"),
            });
        }

        info!(session = %session_name, "Mutagen sync session flushed");
        Ok(())
    }

    /// Wait for initial sync to complete with timeout.
    pub async fn wait_for_sync(
        &self,
        session_name: &str,
        timeout: Option<Duration>,
    ) -> Result<()> {
        let timeout = timeout.unwrap_or(Duration::from_secs(DEFAULT_SYNC_TIMEOUT_SECS));
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(500);

        info!(session = %session_name, timeout_secs = timeout.as_secs(), "waiting for sync to complete");

        loop {
            if start.elapsed() > timeout {
                return Err(CoastError::Remote {
                    message: format!(
                        "Timeout waiting for sync session '{}' to complete",
                        session_name
                    ),
                });
            }

            let status = self.get_session_status(session_name).await?;

            match status.status.as_deref() {
                Some("watching") | Some("Watching for changes") => {
                    info!(session = %session_name, "sync complete, watching for changes");
                    return Ok(());
                }
                Some(s) if s.contains("error") || s.contains("Error") => {
                    return Err(CoastError::Remote {
                        message: format!("Sync session '{}' encountered an error: {}", session_name, s),
                    });
                }
                Some(s) => {
                    debug!(session = %session_name, status = s, "sync in progress");
                }
                None => {
                    debug!(session = %session_name, "sync status unknown");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Get status of a sync session from Mutagen.
    pub async fn get_session_status(&self, session_name: &str) -> Result<MutagenStatus> {
        let output = Command::new("mutagen")
            .args(["sync", "list", "--label-selector", &format!("name={}", session_name), "-o", "json"])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to get Mutagen session status: {e}"),
            })?;

        if !output.status.success() {
            return Err(CoastError::Remote {
                message: "Failed to list Mutagen sessions".to_string(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        if let Ok(list) = serde_json::from_str::<MutagenListOutput>(&stdout) {
            if let Some(sessions) = list.sessions {
                if let Some(session) = sessions.first() {
                    return Ok(MutagenStatus {
                        identifier: session.identifier.clone(),
                        name: session.name.clone(),
                        status: session.status.as_ref().and_then(|s| s.description.clone()),
                        alpha_connected: session.alpha.as_ref().and_then(|a| a.connected),
                        beta_connected: session.beta.as_ref().and_then(|b| b.connected),
                        staged_entries: session.status.as_ref().and_then(|s| s.staging_progress),
                        conflicts: None,
                        last_error: None,
                    });
                }
            }
        }

        // Session not found
        Ok(MutagenStatus {
            identifier: None,
            name: Some(session_name.to_string()),
            status: Some("not found".to_string()),
            alpha_connected: None,
            beta_connected: None,
            staged_entries: None,
            conflicts: None,
            last_error: Some("Session not found".to_string()),
        })
    }

    /// List all sync sessions from Mutagen that match our naming convention.
    pub async fn list_all_sessions(&self) -> Result<Vec<MutagenStatus>> {
        let output = Command::new("mutagen")
            .args(["sync", "list", "--label-selector", "name=coast-*", "-o", "json"])
            .output()
            .await
            .map_err(|e| CoastError::Remote {
                message: format!("failed to list Mutagen sessions: {e}"),
            })?;

        // If no sessions, mutagen may return error
        if !output.status.success() {
            return Ok(vec![]);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        if let Ok(list) = serde_json::from_str::<MutagenListOutput>(&stdout) {
            if let Some(sessions) = list.sessions {
                return Ok(sessions
                    .into_iter()
                    .map(|s| MutagenStatus {
                        identifier: s.identifier,
                        name: s.name,
                        status: s.status.as_ref().and_then(|st| st.description.clone()),
                        alpha_connected: s.alpha.as_ref().and_then(|a| a.connected),
                        beta_connected: s.beta.as_ref().and_then(|b| b.connected),
                        staged_entries: s.status.as_ref().and_then(|st| st.staging_progress),
                        conflicts: None,
                        last_error: None,
                    })
                    .collect());
            }
        }

        Ok(vec![])
    }

    /// Load existing sessions from database into memory.
    pub async fn load_sessions_from_db(&self, sessions: Vec<SyncSession>) {
        let mut mem_sessions = self.sessions.write().await;

        for session in sessions {
            if let Some(ref session_id) = session.mutagen_session_id {
                // Parse project and branch from session name or use stored values
                let session_name = Self::generate_session_name(
                    &session.project,
                    "main", // Default branch, will be updated when we track branches
                    &session.remote_name,
                );

                let info = MutagenSessionInfo {
                    session_id: session_id.clone(),
                    session_name: session_name.clone(),
                    project: session.project,
                    branch: "main".to_string(),
                    remote_name: session.remote_name,
                    local_path: PathBuf::from(&session.local_path),
                    remote_path: session.remote_path,
                    status: session.status,
                    created_at: session.created_at,
                };

                mem_sessions.insert(session_name, info);
            }
        }

        info!(count = mem_sessions.len(), "loaded sync sessions from database");
    }
}

impl Default for MutagenManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_session_name() {
        let name = MutagenManager::generate_session_name("my-app", "feature/auth", "dev-vm");
        assert_eq!(name, "coast-my-app-feature-auth-dev-vm");
    }

    #[test]
    fn test_generate_session_name_sanitizes() {
        let name = MutagenManager::generate_session_name("My App!", "feature_branch", "VM 1");
        assert_eq!(name, "coast-my-app--feature-branch-vm-1");
    }

    #[test]
    fn test_generate_remote_path() {
        let path = MutagenManager::generate_remote_path("myapp", "main");
        assert_eq!(path, "~/coast-workspaces/myapp/main");
    }

    #[test]
    fn test_build_ignore_patterns() {
        let patterns = MutagenManager::build_ignore_patterns(Path::new("/nonexistent"));
        assert!(patterns.contains(&".git".to_string()));
        assert!(patterns.contains(&"node_modules".to_string()));
        assert!(patterns.contains(&"target".to_string()));
    }

    #[tokio::test]
    async fn test_mutagen_manager_new() {
        let manager = MutagenManager::new();
        assert!(manager.get_session("nonexistent").await.is_none());
    }
}
