use rusqlite::{params, OptionalExtension};

use coast_core::error::{CoastError, Result};

use super::StateDb;

/// Represents an agent shell record from the database.
#[derive(Debug, Clone)]
pub struct AgentShellRecord {
    /// Auto-incremented shell ID.
    pub id: i64,
    /// Per-instance shell ID shown to users (starts at 1).
    pub shell_id: i64,
    /// Project this shell belongs to.
    pub project: String,
    /// Instance name within the project.
    pub instance_name: String,
    /// Command executed inside the DinD container.
    pub command: String,
    /// Whether this is the active (routable) agent shell.
    pub is_active: bool,
    /// Links to the PTY exec session ID in AppState.
    pub session_id: Option<String>,
    /// Current status: "running", "exited".
    pub status: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
}

/// Convert a SQLite row into an `AgentShellRecord`.
///
/// Column order: id, project, instance_name, shell_id, command, is_active, session_id, status, created_at
fn row_to_agent_shell(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentShellRecord> {
    let is_active_int: i32 = row.get(5)?;
    Ok(AgentShellRecord {
        id: row.get(0)?,
        project: row.get(1)?,
        instance_name: row.get(2)?,
        shell_id: row.get(3)?,
        command: row.get(4)?,
        is_active: is_active_int != 0,
        session_id: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
    })
}

impl StateDb {
    /// Insert a new agent shell and return its auto-generated ID.
    pub fn create_agent_shell(
        &self,
        project: &str,
        instance_name: &str,
        command: &str,
    ) -> Result<i64> {
        let next_shell_id: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(shell_id), 0) + 1
                 FROM agent_shells
                 WHERE project = ?1 AND instance_name = ?2",
                params![project, instance_name],
                |row| row.get(0),
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to compute next agent shell id: {e}"),
                source: Some(Box::new(e)),
            })?;
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO agent_shells (project, instance_name, shell_id, command, is_active, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, 0, 'running', ?5)",
                params![project, instance_name, next_shell_id, command, now],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to create agent shell: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List all agent shells for a specific instance.
    pub fn list_agent_shells(
        &self,
        project: &str,
        instance_name: &str,
    ) -> Result<Vec<AgentShellRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, instance_name, shell_id, command, is_active, session_id, status, created_at
                 FROM agent_shells WHERE project = ?1 AND instance_name = ?2
                 ORDER BY id ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare agent shell list: {e}"),
                source: Some(Box::new(e)),
            })?;
        let rows = stmt
            .query_map(params![project, instance_name], row_to_agent_shell)
            .map_err(|e| CoastError::State {
                message: format!("failed to list agent shells: {e}"),
                source: Some(Box::new(e)),
            })?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read agent shell row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }
        Ok(records)
    }

    /// Get a single agent shell by its ID.
    pub fn get_agent_shell_by_id(&self, shell_id: i64) -> Result<Option<AgentShellRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, instance_name, shell_id, command, is_active, session_id, status, created_at
                 FROM agent_shells WHERE id = ?1",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare agent shell by id query: {e}"),
                source: Some(Box::new(e)),
            })?;
        stmt.query_row(params![shell_id], row_to_agent_shell)
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to get agent shell by id: {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Get a single agent shell by its per-instance shell_id.
    pub fn get_agent_shell_by_shell_id(
        &self,
        project: &str,
        instance_name: &str,
        shell_id: i64,
    ) -> Result<Option<AgentShellRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, instance_name, shell_id, command, is_active, session_id, status, created_at
                 FROM agent_shells
                 WHERE project = ?1 AND instance_name = ?2 AND shell_id = ?3",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare agent shell by shell_id query: {e}"),
                source: Some(Box::new(e)),
            })?;
        stmt.query_row(
            params![project, instance_name, shell_id],
            row_to_agent_shell,
        )
        .optional()
        .map_err(|e| CoastError::State {
            message: format!("failed to get agent shell by shell_id: {e}"),
            source: Some(Box::new(e)),
        })
    }

    /// Get the currently active agent shell for an instance (if any).
    pub fn get_active_agent_shell(
        &self,
        project: &str,
        instance_name: &str,
    ) -> Result<Option<AgentShellRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project, instance_name, shell_id, command, is_active, session_id, status, created_at
                 FROM agent_shells WHERE project = ?1 AND instance_name = ?2 AND is_active = 1",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare active agent shell query: {e}"),
                source: Some(Box::new(e)),
            })?;
        stmt.query_row(params![project, instance_name], row_to_agent_shell)
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to get active agent shell: {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Set one agent shell as active, clearing all others for the same instance.
    pub fn set_active_agent_shell(
        &self,
        project: &str,
        instance_name: &str,
        shell_id: i64,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_shells SET is_active = 0 WHERE project = ?1 AND instance_name = ?2",
                params![project, instance_name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to clear active agent shells: {e}"),
                source: Some(Box::new(e)),
            })?;
        self.conn
            .execute(
                "UPDATE agent_shells SET is_active = 1 WHERE id = ?1",
                params![shell_id],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set active agent shell: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Update the status of an agent shell.
    pub fn update_agent_shell_status(&self, shell_id: i64, status: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_shells SET status = ?1 WHERE id = ?2",
                params![status, shell_id],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update agent shell status: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Link an agent shell to an exec session ID (PTY handle key).
    pub fn update_agent_shell_session_id(&self, shell_id: i64, session_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE agent_shells SET session_id = ?1 WHERE id = ?2",
                params![session_id, shell_id],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update agent shell session_id: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Delete a single agent shell by ID.
    pub fn delete_agent_shell(&self, shell_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM agent_shells WHERE id = ?1", params![shell_id])
            .map_err(|e| CoastError::State {
                message: format!("failed to delete agent shell: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Delete all agent shells for a given instance.
    pub fn delete_agent_shells_for_instance(
        &self,
        project: &str,
        instance_name: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM agent_shells WHERE project = ?1 AND instance_name = ?2",
                params![project, instance_name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete agent shells: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

    #[test]
    fn test_create_agent_shell() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id = db
            .create_agent_shell("proj", "dev-1", "claude --skip")
            .unwrap();
        assert!(id > 0);
        let shell = db.get_agent_shell_by_id(id).unwrap().unwrap();
        assert_eq!(shell.shell_id, 1);
    }

    #[test]
    fn test_list_agent_shells() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        db.create_agent_shell("proj", "dev-1", "claude --skip")
            .unwrap();
        db.create_agent_shell("proj", "dev-1", "claude --skip")
            .unwrap();
        let shells = db.list_agent_shells("proj", "dev-1").unwrap();
        assert_eq!(shells.len(), 2);
        assert_eq!(shells[0].shell_id, 1);
        assert_eq!(shells[1].shell_id, 2);
    }

    #[test]
    fn test_get_agent_shell_by_id() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let shell = db.get_agent_shell_by_id(id).unwrap().unwrap();
        assert_eq!(shell.id, id);
        assert_eq!(shell.shell_id, 1);
        assert_eq!(shell.project, "proj");
        assert_eq!(shell.instance_name, "dev-1");
    }

    #[test]
    fn test_get_agent_shell_by_shell_id() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let shell = db
            .get_agent_shell_by_shell_id("proj", "dev-1", 2)
            .unwrap()
            .unwrap();
        assert_eq!(shell.shell_id, 2);
    }

    #[test]
    fn test_get_active_agent_shell() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id1 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let _id2 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.set_active_agent_shell("proj", "dev-1", id1).unwrap();
        let active = db.get_active_agent_shell("proj", "dev-1").unwrap().unwrap();
        assert_eq!(active.id, id1);
        assert!(active.is_active);
    }

    #[test]
    fn test_set_active_clears_previous() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id1 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let id2 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.set_active_agent_shell("proj", "dev-1", id1).unwrap();
        db.set_active_agent_shell("proj", "dev-1", id2).unwrap();
        let shells = db.list_agent_shells("proj", "dev-1").unwrap();
        let active_count = shells.iter().filter(|s| s.is_active).count();
        assert_eq!(active_count, 1);
        assert!(shells.iter().find(|s| s.id == id2).unwrap().is_active);
        assert!(!shells.iter().find(|s| s.id == id1).unwrap().is_active);
    }

    #[test]
    fn test_update_agent_shell_status() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.update_agent_shell_status(id, "exited").unwrap();
        let shells = db.list_agent_shells("proj", "dev-1").unwrap();
        assert_eq!(shells[0].status, "exited");
    }

    #[test]
    fn test_update_agent_shell_session_id() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.update_agent_shell_session_id(id, "sess-abc-123")
            .unwrap();
        let shells = db.list_agent_shells("proj", "dev-1").unwrap();
        assert_eq!(shells[0].session_id.as_deref(), Some("sess-abc-123"));
    }

    #[test]
    fn test_delete_agent_shells_for_instance() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        db.insert_instance(&sample_instance("dev-2", "proj"))
            .unwrap();
        db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.create_agent_shell("proj", "dev-2", "claude").unwrap();
        db.delete_agent_shells_for_instance("proj", "dev-1")
            .unwrap();
        assert!(db.list_agent_shells("proj", "dev-1").unwrap().is_empty());
        assert_eq!(db.list_agent_shells("proj", "dev-2").unwrap().len(), 1);
    }

    #[test]
    fn test_agent_shell_local_id_per_instance_and_reset_after_cleanup() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        db.insert_instance(&sample_instance("dev-2", "proj"))
            .unwrap();

        let dev1_a = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let dev1_b = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let dev2_a = db.create_agent_shell("proj", "dev-2", "claude").unwrap();

        assert_eq!(
            db.get_agent_shell_by_id(dev1_a).unwrap().unwrap().shell_id,
            1
        );
        assert_eq!(
            db.get_agent_shell_by_id(dev1_b).unwrap().unwrap().shell_id,
            2
        );
        assert_eq!(
            db.get_agent_shell_by_id(dev2_a).unwrap().unwrap().shell_id,
            1
        );

        db.delete_agent_shells_for_instance("proj", "dev-1")
            .unwrap();
        let dev1_after_reset = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        assert_eq!(
            db.get_agent_shell_by_id(dev1_after_reset)
                .unwrap()
                .unwrap()
                .shell_id,
            1
        );
    }

    #[test]
    fn test_delete_single_agent_shell() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        let id1 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        let _id2 = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.delete_agent_shell(id1).unwrap();
        assert!(db.get_agent_shell_by_id(id1).unwrap().is_none());
        assert_eq!(db.list_agent_shells("proj", "dev-1").unwrap().len(), 1);
    }

    #[test]
    fn test_agent_shells_cascade_on_instance_delete() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();
        db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.delete_instance("proj", "dev-1").unwrap();
        assert!(db.list_agent_shells("proj", "dev-1").unwrap().is_empty());
    }

    #[test]
    fn test_agent_shell_session_crossref() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();

        let agent_id = db.create_agent_shell("proj", "dev-1", "claude").unwrap();
        db.set_active_agent_shell("proj", "dev-1", agent_id)
            .unwrap();
        db.update_agent_shell_session_id(agent_id, "sess-agent-1")
            .unwrap();

        let shells = db.list_agent_shells("proj", "dev-1").unwrap();
        assert_eq!(shells.len(), 1);

        let session_ids = vec!["sess-agent-1".to_string(), "sess-bash-2".to_string()];
        let mut results = Vec::new();
        for sid in &session_ids {
            let agent_match = shells.iter().find(|a| a.session_id.as_deref() == Some(sid));
            results.push((
                sid.clone(),
                agent_match.map(|a| a.id),
                agent_match.map(|a| a.is_active),
            ));
        }

        assert_eq!(results[0].1, Some(agent_id));
        assert_eq!(results[0].2, Some(true));
        assert_eq!(results[1].1, None);
        assert_eq!(results[1].2, None);
    }
}
