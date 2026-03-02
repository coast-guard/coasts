use rusqlite::params;
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};

use super::{is_unique_violation, StateDb};

/// Represents a shared service record from the database.
#[derive(Debug, Clone)]
pub struct SharedServiceRecord {
    /// Project this shared service belongs to.
    pub project: String,
    /// Name of the shared service (e.g., "postgres").
    pub service_name: String,
    /// Docker container ID on the host daemon (if running).
    pub container_id: Option<String>,
    /// Current status ("running", "stopped", etc.).
    pub status: String,
}

/// Convert a SQLite row into a `SharedServiceRecord`.
///
/// Column order: project, service_name, container_id, status
fn row_to_shared_service(row: &rusqlite::Row<'_>) -> rusqlite::Result<SharedServiceRecord> {
    Ok(SharedServiceRecord {
        project: row.get(0)?,
        service_name: row.get(1)?,
        container_id: row.get(2)?,
        status: row.get(3)?,
    })
}

/// Collect rusqlite mapped rows into a Vec, converting errors.
fn collect_rows(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<SharedServiceRecord>,
    >,
) -> Result<Vec<SharedServiceRecord>> {
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(|e| CoastError::State {
            message: format!("failed to read shared service row: {e}"),
            source: Some(Box::new(e)),
        })?);
    }
    Ok(records)
}

impl StateDb {
    /// Insert a new shared service record.
    ///
    /// Returns an error if a shared service with the same
    /// (project, service_name) already exists.
    #[instrument(skip(self))]
    pub fn insert_shared_service(
        &self,
        project: &str,
        service_name: &str,
        container_id: Option<&str>,
        status: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO shared_services (project, service_name, container_id, status)
                 VALUES (?1, ?2, ?3, ?4)",
                params![project, service_name, container_id, status],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    CoastError::State {
                        message: format!(
                            "shared service '{service_name}' already exists in project '{project}'. \
                             Use `coast shared-services rm {service_name}` to remove it first."
                        ),
                        source: Some(Box::new(e)),
                    }
                } else {
                    CoastError::State {
                        message: format!(
                            "failed to insert shared service '{service_name}': {e}"
                        ),
                        source: Some(Box::new(e)),
                    }
                }
            })?;

        debug!("inserted shared service");
        Ok(())
    }

    /// Get a shared service by project and service name.
    ///
    /// Returns `None` if no matching shared service exists.
    #[instrument(skip(self))]
    pub fn get_shared_service(
        &self,
        project: &str,
        service_name: &str,
    ) -> Result<Option<SharedServiceRecord>> {
        self.conn
            .query_row(
                "SELECT project, service_name, container_id, status
                 FROM shared_services
                 WHERE project = ?1 AND service_name = ?2",
                params![project, service_name],
                row_to_shared_service,
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to query shared service '{service_name}' in project '{project}': {e}"
                ),
                source: Some(Box::new(e)),
            })
    }

    /// List all shared services, optionally filtered by project.
    #[instrument(skip(self))]
    pub fn list_shared_services(&self, project: Option<&str>) -> Result<Vec<SharedServiceRecord>> {
        match project {
            Some(proj) => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT project, service_name, container_id, status
                         FROM shared_services
                         WHERE project = ?1
                         ORDER BY service_name",
                    )
                    .map_err(|e| CoastError::State {
                        message: format!("failed to prepare shared services query: {e}"),
                        source: Some(Box::new(e)),
                    })?;

                let rows = stmt
                    .query_map(params![proj], row_to_shared_service)
                    .map_err(|e| CoastError::State {
                        message: format!(
                            "failed to list shared services for project '{proj}': {e}"
                        ),
                        source: Some(Box::new(e)),
                    })?;

                collect_rows(rows)
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT project, service_name, container_id, status
                         FROM shared_services
                         ORDER BY project, service_name",
                    )
                    .map_err(|e| CoastError::State {
                        message: format!("failed to prepare shared services query: {e}"),
                        source: Some(Box::new(e)),
                    })?;

                let rows =
                    stmt.query_map([], row_to_shared_service)
                        .map_err(|e| CoastError::State {
                            message: format!("failed to list shared services: {e}"),
                            source: Some(Box::new(e)),
                        })?;

                collect_rows(rows)
            }
        }
    }

    /// Update the status of an existing shared service.
    ///
    /// Returns an error if the shared service does not exist.
    #[instrument(skip(self))]
    pub fn update_shared_service_status(
        &self,
        project: &str,
        service_name: &str,
        status: &str,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE shared_services SET status = ?1 WHERE project = ?2 AND service_name = ?3",
                params![status, project, service_name],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to update status for shared service '{service_name}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!(
                    "shared service '{service_name}' not found in project '{project}'. \
                     Run `coast shared-services ps` to see available shared services."
                ),
                source: None,
            });
        }

        debug!("updated shared service status to {status}");
        Ok(())
    }

    /// Update the container ID of an existing shared service.
    ///
    /// Returns an error if the shared service does not exist.
    #[instrument(skip(self))]
    pub fn update_shared_service_container_id(
        &self,
        project: &str,
        service_name: &str,
        container_id: Option<&str>,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE shared_services SET container_id = ?1 WHERE project = ?2 AND service_name = ?3",
                params![container_id, project, service_name],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to update container_id for shared service '{service_name}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!(
                    "shared service '{service_name}' not found in project '{project}'. \
                     Run `coast shared-services ps` to see available shared services."
                ),
                source: None,
            });
        }

        debug!("updated shared service container_id");
        Ok(())
    }

    /// Delete a shared service record.
    ///
    /// Returns an error if the shared service does not exist.
    /// Note: this only removes the database record -- it does NOT stop
    /// the container. Use `coast shared-services rm` for full cleanup.
    #[instrument(skip(self))]
    pub fn delete_shared_service(&self, project: &str, service_name: &str) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM shared_services WHERE project = ?1 AND service_name = ?2",
                params![project, service_name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete shared service '{service_name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::State {
                message: format!(
                    "shared service '{service_name}' not found in project '{project}'. \
                     Run `coast shared-services ps` to see available shared services."
                ),
                source: None,
            });
        }

        debug!("deleted shared service");
        Ok(())
    }

    /// Delete all shared service records for a project.
    #[instrument(skip(self))]
    pub fn delete_shared_services_for_project(&self, project: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM shared_services WHERE project = ?1",
                params![project],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete shared services for project '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;
        debug!("deleted shared services for project");
        Ok(())
    }
}

// Need OptionalExtension for .optional() on query_row
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use coast_core::error::CoastError;

    #[test]
    fn test_insert_and_get_shared_service() {
        let db = test_db();
        db.insert_shared_service("proj", "postgres", Some("container-pg-123"), "running")
            .unwrap();

        let svc = db.get_shared_service("proj", "postgres").unwrap().unwrap();
        assert_eq!(svc.project, "proj");
        assert_eq!(svc.service_name, "postgres");
        assert_eq!(svc.container_id, Some("container-pg-123".to_string()));
        assert_eq!(svc.status, "running");
    }

    #[test]
    fn test_insert_shared_service_with_null_container_id() {
        let db = test_db();
        db.insert_shared_service("proj", "redis", None, "stopped")
            .unwrap();

        let svc = db.get_shared_service("proj", "redis").unwrap().unwrap();
        assert!(svc.container_id.is_none());
        assert_eq!(svc.status, "stopped");
    }

    #[test]
    fn test_insert_duplicate_shared_service_returns_error() {
        let db = test_db();
        db.insert_shared_service("proj", "postgres", None, "running")
            .unwrap();

        let result = db.insert_shared_service("proj", "postgres", None, "running");
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::State { message, .. } => {
                assert!(message.contains("already exists"));
                assert!(message.contains("coast shared-services rm"));
            }
            other => panic!("expected State error, got: {other:?}"),
        }
    }

    #[test]
    fn test_get_nonexistent_shared_service_returns_none() {
        let db = test_db();
        let result = db.get_shared_service("proj", "ghost").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_shared_services_empty() {
        let db = test_db();
        let services = db.list_shared_services(None).unwrap();
        assert!(services.is_empty());
    }

    #[test]
    fn test_list_shared_services_all() {
        let db = test_db();
        db.insert_shared_service("proj-a", "postgres", None, "running")
            .unwrap();
        db.insert_shared_service("proj-a", "redis", None, "running")
            .unwrap();
        db.insert_shared_service("proj-b", "postgres", None, "stopped")
            .unwrap();

        let all = db.list_shared_services(None).unwrap();
        assert_eq!(all.len(), 3);

        // Ordered by project, then service_name
        assert_eq!(all[0].project, "proj-a");
        assert_eq!(all[0].service_name, "postgres");
        assert_eq!(all[1].project, "proj-a");
        assert_eq!(all[1].service_name, "redis");
        assert_eq!(all[2].project, "proj-b");
        assert_eq!(all[2].service_name, "postgres");
    }

    #[test]
    fn test_list_shared_services_by_project() {
        let db = test_db();
        db.insert_shared_service("proj-a", "postgres", None, "running")
            .unwrap();
        db.insert_shared_service("proj-a", "redis", None, "running")
            .unwrap();
        db.insert_shared_service("proj-b", "postgres", None, "stopped")
            .unwrap();

        let proj_a = db.list_shared_services(Some("proj-a")).unwrap();
        assert_eq!(proj_a.len(), 2);

        let proj_b = db.list_shared_services(Some("proj-b")).unwrap();
        assert_eq!(proj_b.len(), 1);

        let proj_c = db.list_shared_services(Some("proj-c")).unwrap();
        assert!(proj_c.is_empty());
    }

    #[test]
    fn test_update_shared_service_status() {
        let db = test_db();
        db.insert_shared_service("proj", "postgres", None, "running")
            .unwrap();

        db.update_shared_service_status("proj", "postgres", "stopped")
            .unwrap();
        let svc = db.get_shared_service("proj", "postgres").unwrap().unwrap();
        assert_eq!(svc.status, "stopped");
    }

    #[test]
    fn test_update_shared_service_status_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_shared_service_status("proj", "ghost", "running");
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::State { message, .. } => {
                assert!(message.contains("not found"));
                assert!(message.contains("coast shared-services ps"));
            }
            other => panic!("expected State error, got: {other:?}"),
        }
    }

    #[test]
    fn test_update_shared_service_container_id() {
        let db = test_db();
        db.insert_shared_service("proj", "postgres", None, "running")
            .unwrap();

        db.update_shared_service_container_id("proj", "postgres", Some("new-container-456"))
            .unwrap();
        let svc = db.get_shared_service("proj", "postgres").unwrap().unwrap();
        assert_eq!(svc.container_id, Some("new-container-456".to_string()));

        // Clear container ID
        db.update_shared_service_container_id("proj", "postgres", None)
            .unwrap();
        let svc = db.get_shared_service("proj", "postgres").unwrap().unwrap();
        assert!(svc.container_id.is_none());
    }

    #[test]
    fn test_update_shared_service_container_id_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_shared_service_container_id("proj", "ghost", Some("abc"));
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::State { message, .. } => {
                assert!(message.contains("not found"));
            }
            other => panic!("expected State error, got: {other:?}"),
        }
    }

    #[test]
    fn test_delete_shared_service() {
        let db = test_db();
        db.insert_shared_service("proj", "postgres", None, "running")
            .unwrap();

        db.delete_shared_service("proj", "postgres").unwrap();
        let result = db.get_shared_service("proj", "postgres").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_nonexistent_shared_service_returns_error() {
        let db = test_db();
        let result = db.delete_shared_service("proj", "ghost");
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::State { message, .. } => {
                assert!(message.contains("not found"));
            }
            other => panic!("expected State error, got: {other:?}"),
        }
    }

    #[test]
    fn test_same_service_name_different_projects() {
        let db = test_db();
        db.insert_shared_service("proj-a", "postgres", Some("c1"), "running")
            .unwrap();
        db.insert_shared_service("proj-b", "postgres", Some("c2"), "running")
            .unwrap();

        let a = db
            .get_shared_service("proj-a", "postgres")
            .unwrap()
            .unwrap();
        let b = db
            .get_shared_service("proj-b", "postgres")
            .unwrap()
            .unwrap();

        assert_eq!(a.container_id, Some("c1".to_string()));
        assert_eq!(b.container_id, Some("c2".to_string()));
    }
}
