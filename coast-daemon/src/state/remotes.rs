//! Remote VM configuration and tunnel state management.
//!
//! Provides CRUD operations for:
//! - Remote VM configurations (`remotes` table)
//! - SSH tunnel state (`tunnels` table)
//! - Project execution mode (`project_modes` table)
//! - Mutagen sync sessions (`sync_sessions` table)
//! - Local port forwards (`local_port_forwards` table)

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};

use super::StateDb;

// ---------------------------------------------------------------------------
// Remote VM Configuration
// ---------------------------------------------------------------------------

/// A remote VM configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remote {
    /// Unique name for this remote (e.g., "staging", "dev-vm")
    pub name: String,
    /// Hostname or IP address
    pub host: String,
    /// SSH username
    pub user: String,
    /// SSH port (default: 22)
    pub port: u16,
    /// Root directory for coast workspaces on the remote
    pub workspace_root: String,
    /// Path to SSH private key (optional, uses default if not specified)
    pub ssh_key_path: Option<String>,
    /// When this remote was added
    pub created_at: DateTime<Utc>,
}

impl StateDb {
    /// Insert a new remote VM configuration.
    #[instrument(skip(self), fields(name = %remote.name, host = %remote.host))]
    pub fn insert_remote(&self, remote: &Remote) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO remotes (name, host, user, port, workspace_root, ssh_key_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    remote.name,
                    remote.host,
                    remote.user,
                    remote.port,
                    remote.workspace_root,
                    remote.ssh_key_path,
                    remote.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                if super::is_unique_violation(&e) {
                    CoastError::State {
                        message: format!("remote '{}' already exists", remote.name),
                        source: Some(Box::new(e)),
                    }
                } else {
                    CoastError::State {
                        message: format!("failed to insert remote '{}': {e}", remote.name),
                        source: Some(Box::new(e)),
                    }
                }
            })?;

        debug!("inserted remote");
        Ok(())
    }

    /// Get a remote by name.
    #[instrument(skip(self))]
    pub fn get_remote(&self, name: &str) -> Result<Option<Remote>> {
        self.conn
            .query_row(
                "SELECT name, host, user, port, workspace_root, ssh_key_path, created_at
                 FROM remotes WHERE name = ?1",
                params![name],
                |row| {
                    let created_at_str: String = row.get(6)?;
                    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());

                    Ok(Remote {
                        name: row.get(0)?,
                        host: row.get(1)?,
                        user: row.get(2)?,
                        port: row.get(3)?,
                        workspace_root: row.get(4)?,
                        ssh_key_path: row.get(5)?,
                        created_at,
                    })
                },
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query remote '{name}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// List all configured remotes.
    #[instrument(skip(self))]
    pub fn list_remotes(&self) -> Result<Vec<Remote>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, host, user, port, workspace_root, ssh_key_path, created_at
                 FROM remotes ORDER BY name ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare remotes query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(6)?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(Remote {
                    name: row.get(0)?,
                    host: row.get(1)?,
                    user: row.get(2)?,
                    port: row.get(3)?,
                    workspace_root: row.get(4)?,
                    ssh_key_path: row.get(5)?,
                    created_at,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query remotes: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut remotes = Vec::new();
        for row in rows {
            remotes.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read remote row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(remotes)
    }

    /// Update an existing remote configuration.
    #[instrument(skip(self), fields(name = %remote.name))]
    pub fn update_remote(&self, remote: &Remote) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE remotes SET host = ?2, user = ?3, port = ?4, workspace_root = ?5, ssh_key_path = ?6
                 WHERE name = ?1",
                params![
                    remote.name,
                    remote.host,
                    remote.user,
                    remote.port,
                    remote.workspace_root,
                    remote.ssh_key_path,
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update remote '{}': {e}", remote.name),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!("remote '{}' not found", remote.name),
                source: None,
            });
        }

        debug!("updated remote");
        Ok(())
    }

    /// Insert or update a remote configuration.
    #[instrument(skip(self), fields(name = %remote.name))]
    pub fn upsert_remote(&self, remote: &Remote) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO remotes (name, host, user, port, workspace_root, ssh_key_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(name) DO UPDATE SET
                    host = excluded.host,
                    user = excluded.user,
                    port = excluded.port,
                    workspace_root = excluded.workspace_root,
                    ssh_key_path = excluded.ssh_key_path",
                params![
                    remote.name,
                    remote.host,
                    remote.user,
                    remote.port,
                    remote.workspace_root,
                    remote.ssh_key_path,
                    remote.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to upsert remote '{}': {e}", remote.name),
                source: Some(Box::new(e)),
            })?;

        debug!("upserted remote");
        Ok(())
    }

    /// Delete a remote configuration.
    #[instrument(skip(self))]
    pub fn delete_remote(&self, name: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute("DELETE FROM remotes WHERE name = ?1", params![name])
            .map_err(|e| CoastError::State {
                message: format!("failed to delete remote '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        debug!(deleted = rows > 0, "delete remote");
        Ok(rows > 0)
    }
}

// ---------------------------------------------------------------------------
// SSH Tunnel State
// ---------------------------------------------------------------------------

/// SSH tunnel status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelStatus {
    /// Tunnel is being established
    Connecting,
    /// Tunnel is active and healthy
    Connected,
    /// Tunnel has disconnected
    Disconnected,
    /// Tunnel encountered an error
    Error,
}

impl TunnelStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "connecting" => Self::Connecting,
            "connected" => Self::Connected,
            "disconnected" => Self::Disconnected,
            "error" => Self::Error,
            _ => Self::Disconnected,
        }
    }
}

/// SSH tunnel state for a remote connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tunnel {
    /// Name of the remote this tunnel connects to
    pub remote_name: String,
    /// Local port the tunnel is bound to
    pub local_port: u16,
    /// PID of the SSH process (if running)
    pub ssh_pid: Option<u32>,
    /// Current tunnel status
    pub status: TunnelStatus,
    /// When the tunnel was established
    pub connected_at: Option<DateTime<Utc>>,
}

impl StateDb {
    /// Insert or update tunnel state.
    #[instrument(skip(self), fields(remote = %tunnel.remote_name))]
    pub fn upsert_tunnel(&self, tunnel: &Tunnel) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO tunnels (remote_name, local_port, ssh_pid, status, connected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(remote_name) DO UPDATE SET
                    local_port = excluded.local_port,
                    ssh_pid = excluded.ssh_pid,
                    status = excluded.status,
                    connected_at = excluded.connected_at",
                params![
                    tunnel.remote_name,
                    tunnel.local_port,
                    tunnel.ssh_pid,
                    tunnel.status.as_str(),
                    tunnel.connected_at.map(|dt| dt.to_rfc3339()),
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to upsert tunnel for '{}': {e}", tunnel.remote_name),
                source: Some(Box::new(e)),
            })?;

        debug!("upserted tunnel state");
        Ok(())
    }

    /// Get tunnel state for a remote.
    #[instrument(skip(self))]
    pub fn get_tunnel(&self, remote_name: &str) -> Result<Option<Tunnel>> {
        self.conn
            .query_row(
                "SELECT remote_name, local_port, ssh_pid, status, connected_at
                 FROM tunnels WHERE remote_name = ?1",
                params![remote_name],
                |row| {
                    let status_str: String = row.get(3)?;
                    let connected_at_str: Option<String> = row.get(4)?;
                    let connected_at = connected_at_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    });

                    Ok(Tunnel {
                        remote_name: row.get(0)?,
                        local_port: row.get(1)?,
                        ssh_pid: row.get(2)?,
                        status: TunnelStatus::from_str(&status_str),
                        connected_at,
                    })
                },
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query tunnel for '{remote_name}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Delete tunnel state for a remote.
    #[instrument(skip(self))]
    pub fn delete_tunnel(&self, remote_name: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM tunnels WHERE remote_name = ?1",
                params![remote_name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete tunnel for '{remote_name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(rows > 0)
    }

    /// List all tunnels.
    #[instrument(skip(self))]
    pub fn list_tunnels(&self) -> Result<Vec<Tunnel>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT remote_name, local_port, ssh_pid, status, connected_at
                 FROM tunnels ORDER BY remote_name ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare tunnels query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(3)?;
                let connected_at_str: Option<String> = row.get(4)?;
                let connected_at = connected_at_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });

                Ok(Tunnel {
                    remote_name: row.get(0)?,
                    local_port: row.get(1)?,
                    ssh_pid: row.get(2)?,
                    status: TunnelStatus::from_str(&status_str),
                    connected_at,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query tunnels: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut tunnels = Vec::new();
        for row in rows {
            tunnels.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read tunnel row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(tunnels)
    }
}

// ---------------------------------------------------------------------------
// Project Execution Mode
// ---------------------------------------------------------------------------

/// Execution mode for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectMode {
    /// Project runs locally with direct Docker access
    Local,
    /// Project runs on a remote VM
    Remote,
}

impl ProjectMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "remote" => Self::Remote,
            _ => Self::Local,
        }
    }
}

/// Project mode configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectModeConfig {
    /// Project name
    pub project: String,
    /// Execution mode
    pub mode: ProjectMode,
    /// Remote name (only set if mode is Remote)
    pub remote_name: Option<String>,
    /// When this mode was set
    pub created_at: DateTime<Utc>,
}

impl StateDb {
    /// Set the execution mode for a project.
    #[instrument(skip(self), fields(project = %config.project, mode = ?config.mode))]
    pub fn set_project_mode(&self, config: &ProjectModeConfig) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO project_modes (project, mode, remote_name, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(project) DO UPDATE SET
                    mode = excluded.mode,
                    remote_name = excluded.remote_name",
                params![
                    config.project,
                    config.mode.as_str(),
                    config.remote_name,
                    config.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set project mode for '{}': {e}", config.project),
                source: Some(Box::new(e)),
            })?;

        debug!("set project mode");
        Ok(())
    }

    /// Get the execution mode for a project.
    #[instrument(skip(self))]
    pub fn get_project_mode(&self, project: &str) -> Result<Option<ProjectModeConfig>> {
        self.conn
            .query_row(
                "SELECT project, mode, remote_name, created_at
                 FROM project_modes WHERE project = ?1",
                params![project],
                |row| {
                    let mode_str: String = row.get(1)?;
                    let created_at_str: String = row.get(3)?;
                    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());

                    Ok(ProjectModeConfig {
                        project: row.get(0)?,
                        mode: ProjectMode::from_str(&mode_str),
                        remote_name: row.get(2)?,
                        created_at,
                    })
                },
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query project mode for '{project}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Delete project mode configuration.
    #[instrument(skip(self))]
    pub fn delete_project_mode(&self, project: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM project_modes WHERE project = ?1",
                params![project],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete project mode for '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(rows > 0)
    }

    /// List all projects with their execution modes.
    #[instrument(skip(self))]
    pub fn list_project_modes(&self) -> Result<Vec<ProjectModeConfig>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, mode, remote_name, created_at
                 FROM project_modes ORDER BY project ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare project_modes query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let mode_str: String = row.get(1)?;
                let created_at_str: String = row.get(3)?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(ProjectModeConfig {
                    project: row.get(0)?,
                    mode: ProjectMode::from_str(&mode_str),
                    remote_name: row.get(2)?,
                    created_at,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query project_modes: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut modes = Vec::new();
        for row in rows {
            modes.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read project_mode row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(modes)
    }

    /// Count the number of projects using a specific remote.
    #[instrument(skip(self))]
    pub fn count_projects_for_remote(&self, remote_name: &str) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM project_modes WHERE remote_name = ?1",
                params![remote_name],
                |row| row.get(0),
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to count projects for remote '{remote_name}': {e}"),
                source: Some(Box::new(e)),
            })
    }
}

// ---------------------------------------------------------------------------
// Sync Sessions
// ---------------------------------------------------------------------------

/// Mutagen sync session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    /// Initial sync in progress
    Initial,
    /// Actively syncing
    Syncing,
    /// Sync paused
    Paused,
    /// Sync encountered an error
    Error,
}

impl SyncStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Syncing => "syncing",
            Self::Paused => "paused",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "initial" => Self::Initial,
            "syncing" => Self::Syncing,
            "paused" => Self::Paused,
            "error" => Self::Error,
            _ => Self::Error,
        }
    }
}

/// Mutagen sync session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSession {
    /// Project name
    pub project: String,
    /// Remote this session syncs to
    pub remote_name: String,
    /// Mutagen session identifier
    pub mutagen_session_id: Option<String>,
    /// Local path being synced
    pub local_path: String,
    /// Remote path being synced to
    pub remote_path: String,
    /// Current sync status
    pub status: SyncStatus,
    /// Last successful sync time
    pub last_sync_at: Option<DateTime<Utc>>,
    /// When this session was created
    pub created_at: DateTime<Utc>,
}

impl StateDb {
    /// Insert or update a sync session.
    #[instrument(skip(self), fields(project = %session.project, remote = %session.remote_name))]
    pub fn upsert_sync_session(&self, session: &SyncSession) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO sync_sessions (project, remote_name, mutagen_session_id, local_path, remote_path, status, last_sync_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(project) DO UPDATE SET
                    remote_name = excluded.remote_name,
                    mutagen_session_id = excluded.mutagen_session_id,
                    local_path = excluded.local_path,
                    remote_path = excluded.remote_path,
                    status = excluded.status,
                    last_sync_at = excluded.last_sync_at",
                params![
                    session.project,
                    session.remote_name,
                    session.mutagen_session_id,
                    session.local_path,
                    session.remote_path,
                    session.status.as_str(),
                    session.last_sync_at.map(|dt| dt.to_rfc3339()),
                    session.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to upsert sync session for '{}': {e}", session.project),
                source: Some(Box::new(e)),
            })?;

        debug!("upserted sync session");
        Ok(())
    }

    /// Get sync session for a project.
    #[instrument(skip(self))]
    pub fn get_sync_session(&self, project: &str) -> Result<Option<SyncSession>> {
        self.conn
            .query_row(
                "SELECT project, remote_name, mutagen_session_id, local_path, remote_path, status, last_sync_at, created_at
                 FROM sync_sessions WHERE project = ?1",
                params![project],
                |row| {
                    let status_str: String = row.get(5)?;
                    let last_sync_at_str: Option<String> = row.get(6)?;
                    let created_at_str: String = row.get(7)?;

                    let last_sync_at = last_sync_at_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    });
                    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());

                    Ok(SyncSession {
                        project: row.get(0)?,
                        remote_name: row.get(1)?,
                        mutagen_session_id: row.get(2)?,
                        local_path: row.get(3)?,
                        remote_path: row.get(4)?,
                        status: SyncStatus::from_str(&status_str),
                        last_sync_at,
                        created_at,
                    })
                },
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query sync session for '{project}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Delete sync session for a project.
    #[instrument(skip(self))]
    pub fn delete_sync_session(&self, project: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM sync_sessions WHERE project = ?1",
                params![project],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete sync session for '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(rows > 0)
    }

    /// List all sync sessions.
    #[instrument(skip(self))]
    pub fn list_sync_sessions(&self) -> Result<Vec<SyncSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, remote_name, mutagen_session_id, local_path, remote_path, status, last_sync_at, created_at
                 FROM sync_sessions ORDER BY project ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare sync_sessions query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let status_str: String = row.get(5)?;
                let last_sync_at_str: Option<String> = row.get(6)?;
                let created_at_str: String = row.get(7)?;

                let last_sync_at = last_sync_at_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(SyncSession {
                    project: row.get(0)?,
                    remote_name: row.get(1)?,
                    mutagen_session_id: row.get(2)?,
                    local_path: row.get(3)?,
                    remote_path: row.get(4)?,
                    status: SyncStatus::from_str(&status_str),
                    last_sync_at,
                    created_at,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query sync_sessions: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read sync_session row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(sessions)
    }

    /// Update sync session status.
    #[instrument(skip(self))]
    pub fn update_sync_session_status(&self, project: &str, status: SyncStatus) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE sync_sessions SET status = ?2 WHERE project = ?1",
                params![project, status.as_str()],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update sync session status for '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!("sync session for project '{}' not found", project),
                source: None,
            });
        }

        debug!(project = %project, status = ?status, "updated sync session status");
        Ok(())
    }

    /// Update last sync timestamp.
    #[instrument(skip(self))]
    pub fn update_sync_last_sync_at(&self, project: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let rows = self
            .conn
            .execute(
                "UPDATE sync_sessions SET last_sync_at = ?2 WHERE project = ?1",
                params![project, now],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update sync last_sync_at for '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!("sync session for project '{}' not found", project),
                source: None,
            });
        }

        debug!(project = %project, "updated sync last_sync_at");
        Ok(())
    }

    /// List sync sessions for a specific remote.
    #[instrument(skip(self))]
    pub fn list_sync_sessions_for_remote(&self, remote_name: &str) -> Result<Vec<SyncSession>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, remote_name, mutagen_session_id, local_path, remote_path, status, last_sync_at, created_at
                 FROM sync_sessions WHERE remote_name = ?1 ORDER BY project ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare sync_sessions query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![remote_name], |row| {
                let status_str: String = row.get(5)?;
                let last_sync_at_str: Option<String> = row.get(6)?;
                let created_at_str: String = row.get(7)?;

                let last_sync_at = last_sync_at_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(SyncSession {
                    project: row.get(0)?,
                    remote_name: row.get(1)?,
                    mutagen_session_id: row.get(2)?,
                    local_path: row.get(3)?,
                    remote_path: row.get(4)?,
                    status: SyncStatus::from_str(&status_str),
                    last_sync_at,
                    created_at,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query sync_sessions for remote: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read sync_session row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(sessions)
    }
}

// ---------------------------------------------------------------------------
// Local Port Forwards
// ---------------------------------------------------------------------------

/// Local port forward for accessing remote container ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPortForward {
    /// Project name
    pub project: String,
    /// Instance name
    pub instance_name: String,
    /// Service name (optional)
    pub service_name: Option<String>,
    /// Local port to bind
    pub local_port: u16,
    /// Remote port to forward to
    pub remote_port: u16,
    /// SSH process PID (if active)
    pub ssh_pid: Option<u32>,
}

impl StateDb {
    /// Insert or update a local port forward.
    #[instrument(skip(self), fields(project = %forward.project, local_port = forward.local_port))]
    pub fn upsert_local_port_forward(&self, forward: &LocalPortForward) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO local_port_forwards (project, instance_name, service_name, local_port, remote_port, ssh_pid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project, instance_name, local_port) DO UPDATE SET
                    service_name = excluded.service_name,
                    remote_port = excluded.remote_port,
                    ssh_pid = excluded.ssh_pid",
                params![
                    forward.project,
                    forward.instance_name,
                    forward.service_name,
                    forward.local_port,
                    forward.remote_port,
                    forward.ssh_pid,
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to upsert local port forward for '{}:{}': {e}",
                    forward.project, forward.local_port
                ),
                source: Some(Box::new(e)),
            })?;

        debug!("upserted local port forward");
        Ok(())
    }

    /// Get local port forwards for a project/instance.
    #[instrument(skip(self))]
    pub fn get_local_port_forwards(
        &self,
        project: &str,
        instance_name: &str,
    ) -> Result<Vec<LocalPortForward>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, instance_name, service_name, local_port, remote_port, ssh_pid
                 FROM local_port_forwards
                 WHERE project = ?1 AND instance_name = ?2
                 ORDER BY local_port ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare local_port_forwards query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![project, instance_name], |row| {
                Ok(LocalPortForward {
                    project: row.get(0)?,
                    instance_name: row.get(1)?,
                    service_name: row.get(2)?,
                    local_port: row.get(3)?,
                    remote_port: row.get(4)?,
                    ssh_pid: row.get(5)?,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query local_port_forwards: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut forwards = Vec::new();
        for row in rows {
            forwards.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read local_port_forward row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(forwards)
    }

    /// Delete local port forwards for a project/instance.
    #[instrument(skip(self))]
    pub fn delete_local_port_forwards(&self, project: &str, instance_name: &str) -> Result<usize> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM local_port_forwards WHERE project = ?1 AND instance_name = ?2",
                params![project, instance_name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete local_port_forwards: {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_crud() {
        let db = StateDb::open_in_memory().unwrap();

        let remote = Remote {
            name: "test-remote".to_string(),
            host: "192.168.1.100".to_string(),
            user: "ubuntu".to_string(),
            port: 22,
            workspace_root: "/home/ubuntu/coasts".to_string(),
            ssh_key_path: None,
            created_at: Utc::now(),
        };

        // Insert
        db.insert_remote(&remote).unwrap();

        // Get
        let fetched = db.get_remote("test-remote").unwrap().unwrap();
        assert_eq!(fetched.host, "192.168.1.100");
        assert_eq!(fetched.user, "ubuntu");

        // List
        let remotes = db.list_remotes().unwrap();
        assert_eq!(remotes.len(), 1);

        // Delete
        assert!(db.delete_remote("test-remote").unwrap());
        assert!(db.get_remote("test-remote").unwrap().is_none());
    }

    #[test]
    fn test_tunnel_crud() {
        let db = StateDb::open_in_memory().unwrap();

        // Insert remote first (foreign key)
        let remote = Remote {
            name: "test-remote".to_string(),
            host: "192.168.1.100".to_string(),
            user: "ubuntu".to_string(),
            port: 22,
            workspace_root: "/home/ubuntu/coasts".to_string(),
            ssh_key_path: None,
            created_at: Utc::now(),
        };
        db.insert_remote(&remote).unwrap();

        let tunnel = Tunnel {
            remote_name: "test-remote".to_string(),
            local_port: 31416,
            ssh_pid: Some(12345),
            status: TunnelStatus::Connected,
            connected_at: Some(Utc::now()),
        };

        // Upsert
        db.upsert_tunnel(&tunnel).unwrap();

        // Get
        let fetched = db.get_tunnel("test-remote").unwrap().unwrap();
        assert_eq!(fetched.local_port, 31416);
        assert_eq!(fetched.status, TunnelStatus::Connected);

        // Update
        let updated = Tunnel {
            status: TunnelStatus::Disconnected,
            ssh_pid: None,
            ..tunnel
        };
        db.upsert_tunnel(&updated).unwrap();
        let fetched = db.get_tunnel("test-remote").unwrap().unwrap();
        assert_eq!(fetched.status, TunnelStatus::Disconnected);
    }

    #[test]
    fn test_project_mode_crud() {
        let db = StateDb::open_in_memory().unwrap();

        // Insert remote first
        let remote = Remote {
            name: "staging".to_string(),
            host: "192.168.1.100".to_string(),
            user: "ubuntu".to_string(),
            port: 22,
            workspace_root: "/home/ubuntu/coasts".to_string(),
            ssh_key_path: None,
            created_at: Utc::now(),
        };
        db.insert_remote(&remote).unwrap();

        let config = ProjectModeConfig {
            project: "my-project".to_string(),
            mode: ProjectMode::Remote,
            remote_name: Some("staging".to_string()),
            created_at: Utc::now(),
        };

        // Set
        db.set_project_mode(&config).unwrap();

        // Get
        let fetched = db.get_project_mode("my-project").unwrap().unwrap();
        assert_eq!(fetched.mode, ProjectMode::Remote);
        assert_eq!(fetched.remote_name, Some("staging".to_string()));

        // Delete
        assert!(db.delete_project_mode("my-project").unwrap());
        assert!(db.get_project_mode("my-project").unwrap().is_none());
    }
}
