/// SQLite state database management for the coast daemon.
///
/// Manages instance metadata, port allocations, and shared service state
/// in a SQLite database at `~/.coast/state.db`. Provides CRUD operations
/// for all tables with proper error handling and type conversions.
///
/// Domain-specific CRUD operations are split across submodules:
/// - [`instances`]: Coast instance lifecycle
/// - [`ports`]: Port allocation and socat PID tracking
/// - [`shared_services`]: Shared service records
/// - [`settings`]: Key-value settings and project archival
/// - [`agent_shells`]: Agent shell session management
/// - [`remotes`]: Remote VM configurations and tunnel state
mod agent_shells;
mod instances;
mod ports;
pub mod remotes;
mod settings;
mod shared_services;
mod user_config;

use std::path::Path;

use rusqlite::{params, Connection};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};

/// Wraps a SQLite connection and provides typed CRUD operations
/// for coast instance state, port allocations, and shared services.
pub struct StateDb {
    conn: Connection,
}

impl StateDb {
    /// Open (or create) the state database at the given file path.
    ///
    /// Creates all required tables if they do not already exist.
    /// Enables WAL mode for better concurrent read performance.
    #[instrument(skip_all, fields(path = %path.as_ref().display()))]
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref()).map_err(|e| CoastError::State {
            message: format!(
                "failed to open state database at '{}': {e}",
                path.as_ref().display()
            ),
            source: Some(Box::new(e)),
        })?;

        let db = Self { conn };
        db.initialize()?;
        debug!("state database opened successfully");
        Ok(db)
    }

    /// Open an in-memory database for testing purposes.
    ///
    /// Creates all required tables immediately. The database is destroyed
    /// when the `StateDb` is dropped.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| CoastError::State {
            message: format!("failed to open in-memory state database: {e}"),
            source: Some(Box::new(e)),
        })?;

        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Create tables and configure pragmas.
    fn initialize(&self) -> Result<()> {
        // Enable WAL mode for better concurrency.
        self.conn
            .execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| CoastError::State {
                message: format!("failed to set WAL mode: {e}"),
                source: Some(Box::new(e)),
            })?;

        // Enable foreign keys.
        self.conn
            .execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| CoastError::State {
                message: format!("failed to enable foreign keys: {e}"),
                source: Some(Box::new(e)),
            })?;

        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS instances (
                    name TEXT NOT NULL,
                    project TEXT NOT NULL,
                    status TEXT NOT NULL,
                    branch TEXT,
                    commit_sha TEXT,
                    container_id TEXT,
                    runtime TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    worktree_name TEXT,
                    PRIMARY KEY (project, name)
                );

                CREATE TABLE IF NOT EXISTS port_allocations (
                    project TEXT NOT NULL,
                    instance_name TEXT NOT NULL,
                    logical_name TEXT NOT NULL,
                    canonical_port INTEGER NOT NULL,
                    dynamic_port INTEGER NOT NULL,
                    socat_pid INTEGER,
                    PRIMARY KEY (project, instance_name, logical_name),
                    FOREIGN KEY (project, instance_name) REFERENCES instances(project, name) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS shared_services (
                    project TEXT NOT NULL,
                    service_name TEXT NOT NULL,
                    container_id TEXT,
                    status TEXT NOT NULL,
                    PRIMARY KEY (project, service_name)
                );

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS archived_projects (
                    project TEXT PRIMARY KEY,
                    archived_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS user_config (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS agent_shells (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    project TEXT NOT NULL,
                    instance_name TEXT NOT NULL,
                    shell_id INTEGER NOT NULL,
                    command TEXT NOT NULL,
                    is_active INTEGER NOT NULL DEFAULT 0,
                    session_id TEXT,
                    status TEXT NOT NULL DEFAULT 'running',
                    created_at TEXT NOT NULL,
                    UNIQUE(project, instance_name, shell_id),
                    FOREIGN KEY (project, instance_name) REFERENCES instances(project, name) ON DELETE CASCADE
                );

                -- Remote VM configurations
                CREATE TABLE IF NOT EXISTS remotes (
                    name TEXT PRIMARY KEY,
                    host TEXT NOT NULL,
                    user TEXT NOT NULL,
                    port INTEGER NOT NULL DEFAULT 22,
                    workspace_root TEXT NOT NULL,
                    ssh_key_path TEXT,
                    created_at TEXT NOT NULL
                );

                -- SSH tunnel state for remote connections
                CREATE TABLE IF NOT EXISTS tunnels (
                    remote_name TEXT PRIMARY KEY,
                    local_port INTEGER NOT NULL,
                    ssh_pid INTEGER,
                    status TEXT NOT NULL,
                    connected_at TEXT,
                    FOREIGN KEY (remote_name) REFERENCES remotes(name) ON DELETE CASCADE
                );

                -- Project execution mode (local vs remote)
                CREATE TABLE IF NOT EXISTS project_modes (
                    project TEXT PRIMARY KEY,
                    mode TEXT NOT NULL,
                    remote_name TEXT,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (remote_name) REFERENCES remotes(name) ON DELETE SET NULL
                );

                -- Mutagen sync sessions for remote projects
                CREATE TABLE IF NOT EXISTS sync_sessions (
                    project TEXT PRIMARY KEY,
                    remote_name TEXT NOT NULL,
                    mutagen_session_id TEXT,
                    local_path TEXT NOT NULL,
                    remote_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    last_sync_at TEXT,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY (remote_name) REFERENCES remotes(name) ON DELETE CASCADE
                );

                -- Local port forwards for accessing remote container ports
                CREATE TABLE IF NOT EXISTS local_port_forwards (
                    project TEXT NOT NULL,
                    instance_name TEXT NOT NULL,
                    service_name TEXT,
                    local_port INTEGER NOT NULL,
                    remote_port INTEGER NOT NULL,
                    ssh_pid INTEGER,
                    PRIMARY KEY (project, instance_name, local_port)
                );
                ",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to create tables: {e}"),
                source: Some(Box::new(e)),
            })?;

        self.migrate_add_build_id()?;
        self.migrate_add_coastfile_type()?;
        self.migrate_add_agent_shell_local_id()?;
        self.migrate_add_port_is_primary()?;
        self.migrate_preferred_language_to_user_config()?;
        self.migrate_add_instance_remote_name()?;

        Ok(())
    }

    /// Migration: add `build_id` column to the instances table if it doesn't exist.
    fn migrate_add_build_id(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT build_id FROM instances LIMIT 0")
            .is_ok();
        if !has_column {
            self.conn
                .execute_batch("ALTER TABLE instances ADD COLUMN build_id TEXT;")
                .map_err(|e| CoastError::State {
                    message: format!("failed to add build_id column: {e}"),
                    source: Some(Box::new(e)),
                })?;
        }
        Ok(())
    }

    /// Migration: add `coastfile_type` column to the instances table if it doesn't exist.
    fn migrate_add_coastfile_type(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT coastfile_type FROM instances LIMIT 0")
            .is_ok();
        if !has_column {
            self.conn
                .execute_batch("ALTER TABLE instances ADD COLUMN coastfile_type TEXT;")
                .map_err(|e| CoastError::State {
                    message: format!("failed to add coastfile_type column: {e}"),
                    source: Some(Box::new(e)),
                })?;
        }
        Ok(())
    }

    /// Migration: add `shell_id` to `agent_shells` and backfill per-instance IDs.
    fn migrate_add_agent_shell_local_id(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT shell_id FROM agent_shells LIMIT 0")
            .is_ok();
        if !has_column {
            self.conn
                .execute_batch(
                    "ALTER TABLE agent_shells ADD COLUMN shell_id INTEGER NOT NULL DEFAULT 0;",
                )
                .map_err(|e| CoastError::State {
                    message: format!("failed to add shell_id column: {e}"),
                    source: Some(Box::new(e)),
                })?;

            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, project, instance_name
                     FROM agent_shells
                     ORDER BY project ASC, instance_name ASC, id ASC",
                )
                .map_err(|e| CoastError::State {
                    message: format!("failed to prepare agent_shells backfill query: {e}"),
                    source: Some(Box::new(e)),
                })?;

            let rows = stmt
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let project: String = row.get(1)?;
                    let instance_name: String = row.get(2)?;
                    Ok((id, project, instance_name))
                })
                .map_err(|e| CoastError::State {
                    message: format!("failed to iterate agent_shells backfill rows: {e}"),
                    source: Some(Box::new(e)),
                })?;

            let mut last_project = String::new();
            let mut last_instance = String::new();
            let mut next_shell_id = 1_i64;
            for row in rows {
                let (id, project, instance_name) = row.map_err(|e| CoastError::State {
                    message: format!("failed to read agent_shells backfill row: {e}"),
                    source: Some(Box::new(e)),
                })?;

                if project != last_project || instance_name != last_instance {
                    next_shell_id = 1;
                    last_project = project.clone();
                    last_instance = instance_name.clone();
                }

                self.conn
                    .execute(
                        "UPDATE agent_shells SET shell_id = ?1 WHERE id = ?2",
                        params![next_shell_id, id],
                    )
                    .map_err(|e| CoastError::State {
                        message: format!("failed to backfill shell_id for agent shell {id}: {e}"),
                        source: Some(Box::new(e)),
                    })?;
                next_shell_id += 1;
            }
        }

        self.conn
            .execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_shells_project_instance_shell_id
                 ON agent_shells(project, instance_name, shell_id);",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to create agent shell local id index: {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(())
    }

    /// Migration: add `is_primary` column to `port_allocations` if it doesn't exist.
    fn migrate_add_port_is_primary(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT is_primary FROM port_allocations LIMIT 0")
            .is_ok();
        if !has_column {
            self.conn
                .execute_batch(
                    "ALTER TABLE port_allocations ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 0;",
                )
                .map_err(|e| CoastError::State {
                    message: format!("failed to add is_primary column to port_allocations: {e}"),
                    source: Some(Box::new(e)),
                })?;
        }
        Ok(())
    }

    /// Migration: move `preferred_language` from `settings` to `user_config` if present.
    fn migrate_preferred_language_to_user_config(&self) -> Result<()> {
        use rusqlite::OptionalExtension;
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'preferred_language'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to check settings for preferred_language: {e}"),
                source: Some(Box::new(e)),
            })?;

        if let Some(lang) = existing {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO user_config (key, value) VALUES ('language', ?1)",
                    params![lang],
                )
                .map_err(|e| CoastError::State {
                    message: format!("failed to migrate preferred_language to user_config: {e}"),
                    source: Some(Box::new(e)),
                })?;
            self.conn
                .execute("DELETE FROM settings WHERE key = 'preferred_language'", [])
                .map_err(|e| CoastError::State {
                    message: format!("failed to remove preferred_language from settings: {e}"),
                    source: Some(Box::new(e)),
                })?;
            debug!("migrated preferred_language from settings to user_config");
        }
        Ok(())
    }

    /// Migration: add `remote_name` column to the instances table if it doesn't exist.
    ///
    /// This column tracks which remote VM an instance is running on (None = local).
    fn migrate_add_instance_remote_name(&self) -> Result<()> {
        let has_column = self
            .conn
            .prepare("SELECT remote_name FROM instances LIMIT 0")
            .is_ok();
        if !has_column {
            self.conn
                .execute_batch("ALTER TABLE instances ADD COLUMN remote_name TEXT;")
                .map_err(|e| CoastError::State {
                    message: format!("failed to add remote_name column to instances: {e}"),
                    source: Some(Box::new(e)),
                })?;
            debug!("added remote_name column to instances table");
        }
        Ok(())
    }
}

/// Check if a rusqlite error is a UNIQUE constraint violation.
pub(super) fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    )
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use super::*;
    use chrono::Utc;
    use coast_core::types::{CoastInstance, InstanceStatus, PortMapping, RuntimeType};

    /// Create a fresh in-memory StateDb for each test.
    pub fn test_db() -> StateDb {
        StateDb::open_in_memory().unwrap()
    }

    /// Create a sample CoastInstance.
    pub fn sample_instance(name: &str, project: &str) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some(format!("container-{name}")),
            runtime: RuntimeType::Dind,
            created_at: Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        }
    }

    /// Create a sample PortMapping.
    pub fn sample_port_mapping(logical_name: &str, canonical: u16, dynamic: u16) -> PortMapping {
        PortMapping {
            logical_name: logical_name.to_string(),
            canonical_port: canonical,
            dynamic_port: dynamic,
            is_primary: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use coast_core::types::InstanceStatus;

    // =======================================================================
    // Cross-table / integration tests
    // =======================================================================

    #[test]
    fn test_delete_instance_does_not_affect_shared_services() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();
        db.insert_shared_service("proj", "postgres", Some("container-pg-123"), "running")
            .unwrap();

        db.delete_instance("proj", "inst").unwrap();

        // Shared service should still exist
        let svc = db.get_shared_service("proj", "postgres").unwrap();
        assert!(
            svc.is_some(),
            "shared services should NOT be deleted when instance is deleted"
        );
    }

    #[test]
    fn test_multiple_instances_with_ports_cascade_independently() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst-a", "proj"))
            .unwrap();
        db.insert_instance(&sample_instance("inst-b", "proj"))
            .unwrap();

        db.insert_port_allocation("proj", "inst-a", &sample_port_mapping("web", 3000, 52340))
            .unwrap();
        db.insert_port_allocation("proj", "inst-b", &sample_port_mapping("web", 3000, 52341))
            .unwrap();

        // Delete inst-a
        db.delete_instance("proj", "inst-a").unwrap();

        // inst-a ports should be gone
        assert!(db
            .get_port_allocations("proj", "inst-a")
            .unwrap()
            .is_empty());

        // inst-b ports should still exist
        let b_allocs = db.get_port_allocations("proj", "inst-b").unwrap();
        assert_eq!(b_allocs.len(), 1);
        assert_eq!(b_allocs[0].dynamic_port, 52341);
    }

    #[test]
    fn test_full_lifecycle() {
        let db = test_db();

        // 1. Create instance
        let instance = sample_instance("feature-oauth", "my-app");
        db.insert_instance(&instance).unwrap();

        // 2. Allocate ports
        db.insert_port_allocation(
            "my-app",
            "feature-oauth",
            &sample_port_mapping("web", 3000, 52340),
        )
        .unwrap();

        // 3. Start shared service
        db.insert_shared_service("my-app", "postgres", Some("pg-123"), "running")
            .unwrap();

        // 4. Checkout instance
        db.update_instance_status("my-app", "feature-oauth", &InstanceStatus::CheckedOut)
            .unwrap();
        db.update_socat_pid("my-app", "feature-oauth", "web", Some(99999))
            .unwrap();

        // 5. Verify state
        let inst = db.get_instance("my-app", "feature-oauth").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);

        let allocs = db.get_port_allocations("my-app", "feature-oauth").unwrap();
        assert_eq!(allocs[0].socat_pid, Some(99999));

        let svc = db
            .get_shared_service("my-app", "postgres")
            .unwrap()
            .unwrap();
        assert_eq!(svc.status, "running");

        // 6. Stop instance
        db.update_instance_status("my-app", "feature-oauth", &InstanceStatus::Stopped)
            .unwrap();
        db.update_socat_pid("my-app", "feature-oauth", "web", None)
            .unwrap();

        // 7. Remove instance
        db.delete_instance("my-app", "feature-oauth").unwrap();

        // 8. Verify cleanup — ports cascaded, shared service persists
        assert!(db
            .get_instance("my-app", "feature-oauth")
            .unwrap()
            .is_none());
        assert!(db
            .get_port_allocations("my-app", "feature-oauth")
            .unwrap()
            .is_empty());
        assert!(db
            .get_shared_service("my-app", "postgres")
            .unwrap()
            .is_some());

        // 9. Clean up shared service explicitly
        db.delete_shared_service("my-app", "postgres").unwrap();
        assert!(db
            .get_shared_service("my-app", "postgres")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_open_in_memory_creates_tables() {
        let db = test_db();
        assert!(db.list_instances().unwrap().is_empty());
        assert!(db.list_shared_services(None).unwrap().is_empty());
        assert!(db.get_port_allocations("a", "b").unwrap().is_empty());
    }

    #[test]
    fn test_open_file_creates_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-state.db");

        {
            let db = StateDb::open(&db_path).unwrap();
            db.insert_instance(&sample_instance("inst", "proj"))
                .unwrap();
        }

        // Reopen — should not fail and data should persist
        {
            let db = StateDb::open(&db_path).unwrap();
            let inst = db.get_instance("proj", "inst").unwrap();
            assert!(inst.is_some());
        }
    }

    #[test]
    fn test_idempotent_table_creation() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-state.db");

        // Open twice — CREATE TABLE IF NOT EXISTS should not fail
        let _db1 = StateDb::open(&db_path).unwrap();
        let db2 = StateDb::open(&db_path).unwrap();
        assert!(db2.list_instances().unwrap().is_empty());
    }
}
