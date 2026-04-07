use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};
use coast_core::types::RemoteEntry;

use super::{is_unique_violation, StateDb};

impl StateDb {
    /// Register a new remote machine.
    #[instrument(skip(self), fields(name = %entry.name, host = %entry.host))]
    pub fn insert_remote(&self, entry: &RemoteEntry) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO remotes (name, host, user, port, ssh_key, sync_strategy, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry.name,
                    entry.host,
                    entry.user,
                    entry.port,
                    entry.ssh_key,
                    entry.sync_strategy,
                    entry.created_at,
                ],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    CoastError::state(format!(
                        "remote '{}' already exists. Use `coast remote rm {}` first.",
                        entry.name, entry.name
                    ))
                } else {
                    CoastError::State {
                        message: format!("failed to insert remote '{}': {e}", entry.name),
                        source: Some(Box::new(e)),
                    }
                }
            })?;

        debug!("registered remote");
        Ok(())
    }

    /// Get a remote by name.
    #[instrument(skip(self))]
    pub fn get_remote(&self, name: &str) -> Result<Option<RemoteEntry>> {
        self.conn
            .query_row(
                "SELECT name, host, user, port, ssh_key, sync_strategy, created_at
                 FROM remotes WHERE name = ?1",
                params![name],
                row_to_remote,
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query remote '{name}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// List all registered remotes.
    #[instrument(skip(self))]
    pub fn list_remotes(&self) -> Result<Vec<RemoteEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, host, user, port, ssh_key, sync_strategy, created_at
                 FROM remotes ORDER BY name",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare list remotes query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], row_to_remote)
            .map_err(|e| CoastError::State {
                message: format!("failed to list remotes: {e}"),
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

    /// Get the cached architecture for a remote.
    pub fn get_remote_arch(&self, name: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT arch FROM remotes WHERE name = ?1",
                params![name],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(|e| CoastError::State {
                message: format!("failed to query remote arch for '{name}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Cache the architecture for a remote.
    pub fn set_remote_arch(&self, name: &str, arch: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE remotes SET arch = ?1 WHERE name = ?2",
                params![arch, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set remote arch for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Delete a remote by name. Returns true if a row was deleted.
    #[instrument(skip(self))]
    pub fn delete_remote(&self, name: &str) -> Result<bool> {
        let rows = self
            .conn
            .execute("DELETE FROM remotes WHERE name = ?1", params![name])
            .map_err(|e| CoastError::State {
                message: format!("failed to delete remote '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        Ok(rows > 0)
    }
}

fn row_to_remote(row: &rusqlite::Row<'_>) -> rusqlite::Result<RemoteEntry> {
    Ok(RemoteEntry {
        name: row.get(0)?,
        host: row.get(1)?,
        user: row.get(2)?,
        port: row.get::<_, i64>(3)? as u16,
        ssh_key: row.get(4)?,
        sync_strategy: row.get(5)?,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_helpers::test_db;

    fn sample_remote(name: &str) -> RemoteEntry {
        RemoteEntry {
            name: name.to_string(),
            host: "192.168.1.100".to_string(),
            user: "ubuntu".to_string(),
            port: 22,
            ssh_key: Some("~/.ssh/id_rsa".to_string()),
            sync_strategy: "rsync".to_string(),
            created_at: "2026-03-31T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_insert_and_get_remote() {
        let db = test_db();
        let entry = sample_remote("my-vm");
        db.insert_remote(&entry).unwrap();

        let fetched = db.get_remote("my-vm").unwrap().unwrap();
        assert_eq!(fetched.name, "my-vm");
        assert_eq!(fetched.host, "192.168.1.100");
        assert_eq!(fetched.user, "ubuntu");
        assert_eq!(fetched.port, 22);
        assert_eq!(fetched.ssh_key.as_deref(), Some("~/.ssh/id_rsa"));
        assert_eq!(fetched.sync_strategy, "rsync");
    }

    #[test]
    fn test_get_nonexistent_remote() {
        let db = test_db();
        assert!(db.get_remote("nope").unwrap().is_none());
    }

    #[test]
    fn test_list_remotes_empty() {
        let db = test_db();
        assert!(db.list_remotes().unwrap().is_empty());
    }

    #[test]
    fn test_list_remotes_sorted() {
        let db = test_db();
        db.insert_remote(&sample_remote("z-vm")).unwrap();
        db.insert_remote(&sample_remote("a-vm")).unwrap();
        db.insert_remote(&sample_remote("m-vm")).unwrap();

        let list = db.list_remotes().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "a-vm");
        assert_eq!(list[1].name, "m-vm");
        assert_eq!(list[2].name, "z-vm");
    }

    #[test]
    fn test_insert_duplicate_remote_errors() {
        let db = test_db();
        db.insert_remote(&sample_remote("dupe")).unwrap();
        let err = db.insert_remote(&sample_remote("dupe")).unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "expected duplicate error, got: {err}"
        );
    }

    #[test]
    fn test_delete_remote() {
        let db = test_db();
        db.insert_remote(&sample_remote("to-delete")).unwrap();
        assert!(db.delete_remote("to-delete").unwrap());
        assert!(db.get_remote("to-delete").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_remote() {
        let db = test_db();
        assert!(!db.delete_remote("nope").unwrap());
    }

    #[test]
    fn test_remote_without_ssh_key() {
        let db = test_db();
        let mut entry = sample_remote("no-key");
        entry.ssh_key = None;
        db.insert_remote(&entry).unwrap();

        let fetched = db.get_remote("no-key").unwrap().unwrap();
        assert!(fetched.ssh_key.is_none());
    }

    #[test]
    fn test_remote_custom_port() {
        let db = test_db();
        let mut entry = sample_remote("custom-port");
        entry.port = 2222;
        db.insert_remote(&entry).unwrap();

        let fetched = db.get_remote("custom-port").unwrap().unwrap();
        assert_eq!(fetched.port, 2222);
    }
}
