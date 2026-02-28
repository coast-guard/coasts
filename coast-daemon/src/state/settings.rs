use rusqlite::params;
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};

use super::StateDb;

impl StateDb {
    // -----------------------------------------------------------------------
    // Settings CRUD
    // -----------------------------------------------------------------------

    /// Get a setting value by key.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query setting '{key}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Upsert a setting value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set setting '{key}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!(key = %key, "setting saved");
        Ok(())
    }

    /// Delete a setting by key.
    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map_err(|e| CoastError::State {
                message: format!("failed to delete setting '{key}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!(key = %key, "setting deleted");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Archived projects CRUD
    // -----------------------------------------------------------------------

    /// Mark a project as archived.
    #[instrument(skip(self))]
    pub fn archive_project(&self, project: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO archived_projects (project, archived_at) VALUES (?1, ?2)",
                params![project, now],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to archive project '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!("project archived");
        Ok(())
    }

    /// Remove the archived flag from a project.
    #[instrument(skip(self))]
    pub fn unarchive_project(&self, project: &str) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM archived_projects WHERE project = ?1",
                params![project],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to unarchive project '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;
        if rows == 0 {
            return Err(CoastError::state(format!(
                "Project '{project}' is not archived."
            )));
        }
        debug!("project unarchived");
        Ok(())
    }

    /// Check if a project is currently archived.
    pub fn is_project_archived(&self, project: &str) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM archived_projects WHERE project = ?1",
                params![project],
                |row| row.get(0),
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to check archive status for '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(count > 0)
    }

    /// Return the set of all archived project names.
    pub fn list_archived_projects(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT project FROM archived_projects")
            .map_err(|e| CoastError::State {
                message: format!("failed to list archived projects: {e}"),
                source: Some(Box::new(e)),
            })?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CoastError::State {
                message: format!("failed to list archived projects: {e}"),
                source: Some(Box::new(e)),
            })?
            .filter_map(std::result::Result::ok)
            .collect();
        Ok(names)
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

    #[test]
    fn test_delete_setting() {
        let db = test_db();
        db.set_setting("my_key", "my_value").unwrap();
        assert_eq!(
            db.get_setting("my_key").unwrap(),
            Some("my_value".to_string())
        );
        db.delete_setting("my_key").unwrap();
        assert_eq!(db.get_setting("my_key").unwrap(), None);
    }

    #[test]
    fn test_get_setting_nonexistent() {
        let db = test_db();
        assert_eq!(db.get_setting("nonexistent").unwrap(), None);
    }

    #[test]
    fn test_set_setting_upsert() {
        let db = test_db();
        db.set_setting("key", "val1").unwrap();
        assert_eq!(db.get_setting("key").unwrap(), Some("val1".to_string()));
        db.set_setting("key", "val2").unwrap();
        assert_eq!(db.get_setting("key").unwrap(), Some("val2".to_string()));
    }

    #[test]
    fn test_archive_and_unarchive_project() {
        let db = test_db();

        assert!(!db.is_project_archived("proj").unwrap());

        db.archive_project("proj").unwrap();
        assert!(db.is_project_archived("proj").unwrap());

        let archived = db.list_archived_projects().unwrap();
        assert!(archived.contains("proj"));

        db.unarchive_project("proj").unwrap();
        assert!(!db.is_project_archived("proj").unwrap());
    }

    #[test]
    fn test_unarchive_nonexistent_returns_error() {
        let db = test_db();
        let result = db.unarchive_project("ghost");
        assert!(result.is_err());
    }

    #[test]
    fn test_archive_is_idempotent() {
        let db = test_db();
        db.archive_project("proj").unwrap();
        db.archive_project("proj").unwrap();
        assert!(db.is_project_archived("proj").unwrap());
    }
}
