pub mod instances;

use std::path::PathBuf;

use rusqlite::Connection;
use tokio::sync::Mutex;

use coast_core::error::{CoastError, Result};

pub(crate) fn service_home() -> PathBuf {
    std::env::var("COAST_SERVICE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(".coast-service")
        })
}

pub struct ServiceState {
    pub db: Mutex<ServiceDb>,
    pub docker: Option<bollard::Docker>,
}

impl ServiceState {
    pub fn new() -> Result<Self> {
        let home = service_home();
        std::fs::create_dir_all(&home).map_err(|e| CoastError::Io {
            message: format!("failed to create coast-service home: {e}"),
            path: home.clone(),
            source: Some(e),
        })?;

        let db_path = home.join("state.db");
        let db = ServiceDb::open(&db_path)?;

        let docker = bollard::Docker::connect_with_local_defaults()
            .map(Some)
            .unwrap_or_else(|e| {
                tracing::warn!("docker not available: {e}");
                None
            });

        Ok(Self {
            db: Mutex::new(db),
            docker,
        })
    }

    /// Create a state for testing with the given DB and no Docker client.
    pub fn new_for_testing(db: ServiceDb) -> Self {
        Self {
            db: Mutex::new(db),
            docker: None,
        }
    }
}

pub struct ServiceDb {
    pub conn: Connection,
}

impl ServiceDb {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| CoastError::State {
            message: format!("failed to open service database: {e}"),
            source: Some(Box::new(e)),
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|e| CoastError::State {
            message: format!("failed to set pragmas: {e}"),
            source: Some(Box::new(e)),
        })?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory SQLite database with the same schema.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| CoastError::State {
            message: format!("failed to open in-memory database: {e}"),
            source: Some(Box::new(e)),
        })?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| CoastError::State {
                message: format!("failed to set pragmas: {e}"),
                source: Some(Box::new(e)),
            })?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS instances (
                    name TEXT NOT NULL,
                    project TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'stopped',
                    container_id TEXT,
                    build_id TEXT,
                    coastfile_type TEXT,
                    worktree TEXT,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (project, name)
                );

                CREATE TABLE IF NOT EXISTS secrets (
                    name TEXT PRIMARY KEY,
                    encrypted_value BLOB NOT NULL,
                    created_at TEXT NOT NULL
                );",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to run migrations: {e}"),
                source: Some(Box::new(e)),
            })?;

        self.add_column_if_missing("instances", "worktree", "TEXT")?;

        Ok(())
    }

    fn add_column_if_missing(&self, table: &str, column: &str, col_type: &str) -> Result<()> {
        let has_column: bool = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| {
                        rows.filter_map(std::result::Result::ok)
                            .any(|name| name == column)
                    })
            })
            .unwrap_or(false);

        if !has_column {
            self.conn
                .execute_batch(&format!(
                    "ALTER TABLE {table} ADD COLUMN {column} {col_type}"
                ))
                .map_err(|e| CoastError::State {
                    message: format!("failed to add column {column} to {table}: {e}"),
                    source: Some(Box::new(e)),
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory_creates_tables() {
        let db = ServiceDb::open_in_memory().unwrap();
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"instances".to_string()));
        assert!(tables.contains(&"secrets".to_string()));
    }

    #[test]
    fn test_open_in_memory_instances_has_worktree_column() {
        let db = ServiceDb::open_in_memory().unwrap();
        let columns: Vec<String> = db
            .conn
            .prepare("PRAGMA table_info(instances)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(columns.contains(&"worktree".to_string()));
        assert!(columns.contains(&"name".to_string()));
        assert!(columns.contains(&"project".to_string()));
        assert!(columns.contains(&"status".to_string()));
        assert!(columns.contains(&"container_id".to_string()));
        assert!(columns.contains(&"build_id".to_string()));
        assert!(columns.contains(&"coastfile_type".to_string()));
        assert!(columns.contains(&"created_at".to_string()));
    }

    #[test]
    fn test_open_file_creates_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-state.db");
        let db = ServiceDb::open(&db_path).unwrap();
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"instances".to_string()));
        assert!(tables.contains(&"secrets".to_string()));
    }

    #[test]
    fn test_open_file_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("idempotent.db");

        let db1 = ServiceDb::open(&db_path).unwrap();
        db1.conn
            .execute(
                "INSERT INTO instances (name, project, status, created_at) VALUES ('a', 'p', 'running', '2026-01-01')",
                [],
            )
            .unwrap();
        drop(db1);

        let db2 = ServiceDb::open(&db_path).unwrap();
        let count: i64 = db2
            .conn
            .query_row("SELECT COUNT(*) FROM instances", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_add_column_if_missing_adds_new_column() {
        let db = ServiceDb::open_in_memory().unwrap();
        db.add_column_if_missing("instances", "extra_field", "TEXT")
            .unwrap();

        let columns: Vec<String> = db
            .conn
            .prepare("PRAGMA table_info(instances)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(columns.contains(&"extra_field".to_string()));
    }

    #[test]
    fn test_add_column_if_missing_is_idempotent() {
        let db = ServiceDb::open_in_memory().unwrap();
        db.add_column_if_missing("instances", "new_col", "INTEGER")
            .unwrap();
        db.add_column_if_missing("instances", "new_col", "INTEGER")
            .unwrap();
    }

    #[test]
    fn test_add_column_existing_column_is_noop() {
        let db = ServiceDb::open_in_memory().unwrap();
        db.add_column_if_missing("instances", "name", "TEXT")
            .unwrap();
    }

    #[test]
    fn test_service_home_returns_path() {
        let home = service_home();
        assert!(!home.as_os_str().is_empty());
    }

    #[test]
    fn test_new_for_testing_has_no_docker() {
        let db = ServiceDb::open_in_memory().unwrap();
        let state = ServiceState::new_for_testing(db);
        assert!(state.docker.is_none());
    }

    #[test]
    fn test_open_file_sets_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("wal-test.db");
        let db = ServiceDb::open(&db_path).unwrap();
        let mode: String = db
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn test_in_memory_has_foreign_keys_on() {
        let db = ServiceDb::open_in_memory().unwrap();
        let fk: i64 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn test_new_for_testing_has_working_db() {
        let db = ServiceDb::open_in_memory().unwrap();
        let state = ServiceState::new_for_testing(db);
        let db = state.db.blocking_lock();
        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"instances".to_string()));
    }

    #[test]
    fn test_open_file_has_foreign_keys_on() {
        let dir = tempfile::tempdir().unwrap();
        let db = ServiceDb::open(&dir.path().join("fk.db")).unwrap();
        let fk: i64 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn test_migrate_creates_secrets_table_with_columns() {
        let db = ServiceDb::open_in_memory().unwrap();
        let columns: Vec<String> = db
            .conn
            .prepare("PRAGMA table_info(secrets)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(columns.contains(&"name".to_string()));
        assert!(columns.contains(&"encrypted_value".to_string()));
        assert!(columns.contains(&"created_at".to_string()));
    }

    #[test]
    fn test_open_file_and_insert_persists() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("persist.db");

        {
            let db = ServiceDb::open(&db_path).unwrap();
            db.conn
                .execute(
                    "INSERT INTO instances (name, project, status, created_at) VALUES ('x', 'p', 'running', '2026-01-01')",
                    [],
                )
                .unwrap();
        }

        let db = ServiceDb::open(&db_path).unwrap();
        let name: String = db
            .conn
            .query_row(
                "SELECT name FROM instances WHERE project = 'p'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "x");
    }
}
