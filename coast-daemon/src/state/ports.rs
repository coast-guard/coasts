use rusqlite::params;
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};
use coast_core::types::PortMapping;

use super::{is_unique_violation, StateDb};

/// Represents a port allocation record from the database, including
/// the socat PID for process management.
#[derive(Debug, Clone)]
pub struct PortAllocationRecord {
    /// Project this allocation belongs to.
    pub project: String,
    /// Instance name within the project.
    pub instance_name: String,
    /// Logical port name (e.g., "web", "postgres").
    pub logical_name: String,
    /// The canonical port declared in the Coastfile.
    pub canonical_port: u16,
    /// Dynamically allocated always-on port.
    pub dynamic_port: u16,
    /// PID of the socat process forwarding this port (if active).
    pub socat_pid: Option<i32>,
    /// Whether this is the primary service for the instance.
    pub is_primary: bool,
}

/// Convert a `PortAllocationRecord` to a `PortMapping` (drops socat_pid).
impl From<&PortAllocationRecord> for PortMapping {
    fn from(record: &PortAllocationRecord) -> Self {
        PortMapping {
            logical_name: record.logical_name.clone(),
            canonical_port: record.canonical_port,
            dynamic_port: record.dynamic_port,
            is_primary: record.is_primary,
        }
    }
}

impl From<PortAllocationRecord> for PortMapping {
    fn from(record: PortAllocationRecord) -> Self {
        PortMapping {
            logical_name: record.logical_name,
            canonical_port: record.canonical_port,
            dynamic_port: record.dynamic_port,
            is_primary: record.is_primary,
        }
    }
}

impl StateDb {
    /// Insert a new port allocation for an instance.
    ///
    /// Returns an error if a port allocation with the same
    /// (project, instance_name, logical_name) already exists.
    #[instrument(skip(self))]
    pub fn insert_port_allocation(
        &self,
        project: &str,
        instance_name: &str,
        mapping: &PortMapping,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO port_allocations (project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![
                    project,
                    instance_name,
                    mapping.logical_name,
                    mapping.canonical_port as i64,
                    mapping.dynamic_port as i64,
                ],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    CoastError::Port {
                        message: format!(
                            "port allocation for '{}/{}' logical name '{}' already exists. \
                             Remove the existing allocation first with `coast rm {}`.",
                            project, instance_name, mapping.logical_name, instance_name
                        ),
                        source: Some(Box::new(e)),
                    }
                } else {
                    CoastError::State {
                        message: format!(
                            "failed to insert port allocation for '{}/{}': {e}",
                            project, instance_name
                        ),
                        source: Some(Box::new(e)),
                    }
                }
            })?;

        debug!(
            logical_name = %mapping.logical_name,
            canonical = mapping.canonical_port,
            dynamic = mapping.dynamic_port,
            "inserted port allocation"
        );
        Ok(())
    }

    /// Get all port allocations for a given instance.
    ///
    /// Returns port allocations as `PortAllocationRecord` structs which
    /// include the socat PID. For consumers that only need port numbers,
    /// use the `.into()` conversion to `PortMapping`.
    #[instrument(skip(self))]
    pub fn get_port_allocations(
        &self,
        project: &str,
        instance_name: &str,
    ) -> Result<Vec<PortAllocationRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, instance_name, logical_name, canonical_port, dynamic_port, socat_pid
                 FROM port_allocations
                 WHERE project = ?1 AND instance_name = ?2
                 ORDER BY logical_name",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare port allocations query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![project, instance_name], |row| {
                Ok(PortAllocationRecord {
                    project: row.get(0)?,
                    instance_name: row.get(1)?,
                    logical_name: row.get(2)?,
                    canonical_port: row.get::<_, i64>(3)? as u16,
                    dynamic_port: row.get::<_, i64>(4)? as u16,
                    socat_pid: row.get::<_, Option<i64>>(5)?.map(|p| p as i32),
                    is_primary: false,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to query port allocations for '{project}/{instance_name}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        let mut allocations = Vec::new();
        for row in rows {
            allocations.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read port allocation row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(allocations)
    }

    /// Get port allocation counts for all instances in a project.
    ///
    /// Returns a map of `(project, instance_name) -> count`. Runs a single
    /// aggregate query instead of N+1 per-instance lookups.
    pub fn port_counts_for_project(
        &self,
        project: &str,
    ) -> Result<std::collections::HashMap<String, u32>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT instance_name, COUNT(*) as cnt
                 FROM port_allocations
                 WHERE project = ?1
                 GROUP BY instance_name",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare port counts query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut counts = std::collections::HashMap::new();
        let rows = stmt
            .query_map(params![project], |row| {
                let name: String = row.get(0)?;
                let cnt: u32 = row.get(1)?;
                Ok((name, cnt))
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to query port counts: {e}"),
                source: Some(Box::new(e)),
            })?;
        for row in rows {
            let (name, cnt) = row.map_err(|e| CoastError::State {
                message: format!("failed to read port count row: {e}"),
                source: Some(Box::new(e)),
            })?;
            counts.insert(name, cnt);
        }
        Ok(counts)
    }

    /// Update the socat PID for a specific port allocation.
    ///
    /// Used when spawning or killing socat processes during checkout.
    #[instrument(skip(self))]
    pub fn update_socat_pid(
        &self,
        project: &str,
        instance_name: &str,
        logical_name: &str,
        socat_pid: Option<i32>,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE port_allocations SET socat_pid = ?1
                 WHERE project = ?2 AND instance_name = ?3 AND logical_name = ?4",
                params![
                    socat_pid.map(|p| p as i64),
                    project,
                    instance_name,
                    logical_name
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to update socat PID for '{project}/{instance_name}/{logical_name}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::Port {
                message: format!(
                    "no port allocation found for '{project}/{instance_name}' with logical name '{logical_name}'. \
                     Ensure the instance is running with `coast start {instance_name}`."
                ),
                source: None,
            });
        }

        debug!("updated socat PID");
        Ok(())
    }

    /// Delete all port allocations for an instance.
    ///
    /// This is typically called when removing an instance. Note that
    /// CASCADE on the foreign key will also handle this when deleting
    /// the instance, but this method allows explicit cleanup.
    #[instrument(skip(self))]
    pub fn delete_port_allocations(&self, project: &str, instance_name: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM port_allocations WHERE project = ?1 AND instance_name = ?2",
                params![project, instance_name],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to delete port allocations for '{project}/{instance_name}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;

        debug!("deleted port allocations");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::*;
    use coast_core::error::CoastError;

    #[test]
    fn test_insert_and_get_port_allocations() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        let web = sample_port_mapping("web", 3000, 52340);
        let pg = sample_port_mapping("postgres", 5432, 52341);

        db.insert_port_allocation("proj", "inst", &web).unwrap();
        db.insert_port_allocation("proj", "inst", &pg).unwrap();

        let allocations = db.get_port_allocations("proj", "inst").unwrap();
        assert_eq!(allocations.len(), 2);

        // Ordered by logical_name
        assert_eq!(allocations[0].logical_name, "postgres");
        assert_eq!(allocations[0].canonical_port, 5432);
        assert_eq!(allocations[0].dynamic_port, 52341);
        assert!(allocations[0].socat_pid.is_none());

        assert_eq!(allocations[1].logical_name, "web");
        assert_eq!(allocations[1].canonical_port, 3000);
        assert_eq!(allocations[1].dynamic_port, 52340);
    }

    #[test]
    fn test_insert_duplicate_port_allocation_returns_error() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        let mapping = sample_port_mapping("web", 3000, 52340);
        db.insert_port_allocation("proj", "inst", &mapping).unwrap();

        let result = db.insert_port_allocation("proj", "inst", &mapping);
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::Port { message, .. } => {
                assert!(message.contains("already exists"));
            }
            other => panic!("expected Port error, got: {other:?}"),
        }
    }

    #[test]
    fn test_get_port_allocations_empty() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        let allocations = db.get_port_allocations("proj", "inst").unwrap();
        assert!(allocations.is_empty());
    }

    #[test]
    fn test_get_port_allocations_nonexistent_instance() {
        let db = test_db();
        // No error — just returns empty. The instance doesn't need to exist
        // for the query to run, it just won't find any rows.
        let allocations = db.get_port_allocations("proj", "ghost").unwrap();
        assert!(allocations.is_empty());
    }

    #[test]
    fn test_update_socat_pid() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();
        let mapping = sample_port_mapping("web", 3000, 52340);
        db.insert_port_allocation("proj", "inst", &mapping).unwrap();

        // Set PID
        db.update_socat_pid("proj", "inst", "web", Some(12345))
            .unwrap();
        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        assert_eq!(allocs[0].socat_pid, Some(12345));

        // Clear PID
        db.update_socat_pid("proj", "inst", "web", None).unwrap();
        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        assert!(allocs[0].socat_pid.is_none());
    }

    #[test]
    fn test_update_socat_pid_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_socat_pid("proj", "ghost", "web", Some(12345));
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::Port { message, .. } => {
                assert!(message.contains("no port allocation found"));
            }
            other => panic!("expected Port error, got: {other:?}"),
        }
    }

    #[test]
    fn test_delete_port_allocations() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        db.insert_port_allocation("proj", "inst", &sample_port_mapping("web", 3000, 52340))
            .unwrap();
        db.insert_port_allocation("proj", "inst", &sample_port_mapping("pg", 5432, 52341))
            .unwrap();

        db.delete_port_allocations("proj", "inst").unwrap();
        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        assert!(allocs.is_empty());
    }

    #[test]
    fn test_delete_port_allocations_empty_is_ok() {
        let db = test_db();
        // Deleting allocations for an instance that has none should succeed.
        db.delete_port_allocations("proj", "ghost").unwrap();
    }

    #[test]
    fn test_delete_instance_cascades_port_allocations() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();
        db.insert_port_allocation("proj", "inst", &sample_port_mapping("web", 3000, 52340))
            .unwrap();
        db.insert_port_allocation("proj", "inst", &sample_port_mapping("pg", 5432, 52341))
            .unwrap();

        // Verify ports exist
        assert_eq!(db.get_port_allocations("proj", "inst").unwrap().len(), 2);

        // Delete the instance — port allocations should cascade
        db.delete_instance("proj", "inst").unwrap();
        let allocs = db.get_port_allocations("proj", "inst").unwrap();
        assert!(
            allocs.is_empty(),
            "port allocations should be deleted when instance is deleted (CASCADE)"
        );
    }

    #[test]
    fn test_port_allocation_record_to_port_mapping_conversion() {
        let record = PortAllocationRecord {
            project: "proj".to_string(),
            instance_name: "inst".to_string(),
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            socat_pid: Some(12345),
            is_primary: false,
        };

        let mapping: PortMapping = PortMapping::from(&record);
        assert_eq!(mapping.logical_name, "web");
        assert_eq!(mapping.canonical_port, 3000);
        assert_eq!(mapping.dynamic_port, 52340);
        assert!(!mapping.is_primary);

        // Also test owned conversion
        let mapping2: PortMapping = record.into();
        assert_eq!(mapping2.logical_name, "web");
    }

    #[test]
    fn test_port_allocations_isolated_between_instances() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst-a", "proj"))
            .unwrap();
        db.insert_instance(&sample_instance("inst-b", "proj"))
            .unwrap();

        db.insert_port_allocation("proj", "inst-a", &sample_port_mapping("web", 3000, 52340))
            .unwrap();
        db.insert_port_allocation("proj", "inst-b", &sample_port_mapping("web", 3000, 52341))
            .unwrap();

        let a_allocs = db.get_port_allocations("proj", "inst-a").unwrap();
        let b_allocs = db.get_port_allocations("proj", "inst-b").unwrap();

        assert_eq!(a_allocs.len(), 1);
        assert_eq!(a_allocs[0].dynamic_port, 52340);

        assert_eq!(b_allocs.len(), 1);
        assert_eq!(b_allocs[0].dynamic_port, 52341);
    }
}
