//! `SsgStateExt` impl for `StateDb`.
//!
//! Phase: ssg-phase-2. See `coast-ssg/DESIGN.md §8`.
//!
//! Keeps the SSG state logic colocated with the feature crate
//! ([`coast_ssg::state`]) while using the existing daemon `StateDb`
//! handle. The trait is imported from `coast-ssg`; only the impl lives
//! here, following the pattern the daemon uses for other per-domain
//! record CRUD (see [`super::shared_services`]).

use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};
use coast_ssg::state::{SsgPortCheckoutRecord, SsgRecord, SsgServiceRecord, SsgStateExt};

use super::StateDb;

fn state_err(message: String, source: rusqlite::Error) -> CoastError {
    CoastError::State {
        message,
        source: Some(Box::new(source)),
    }
}

fn row_to_ssg(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgRecord> {
    Ok(SsgRecord {
        container_id: row.get(0)?,
        status: row.get(1)?,
        build_id: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn row_to_service(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgServiceRecord> {
    Ok(SsgServiceRecord {
        service_name: row.get(0)?,
        container_port: row.get::<_, i64>(1)? as u16,
        dynamic_host_port: row.get::<_, i64>(2)? as u16,
        status: row.get(3)?,
    })
}

fn row_to_checkout(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgPortCheckoutRecord> {
    Ok(SsgPortCheckoutRecord {
        canonical_port: row.get::<_, i64>(0)? as u16,
        service_name: row.get(1)?,
        socat_pid: row.get(2)?,
        created_at: row.get(3)?,
    })
}

impl SsgStateExt for StateDb {
    #[instrument(skip(self))]
    fn upsert_ssg(
        &self,
        status: &str,
        container_id: Option<&str>,
        build_id: Option<&str>,
    ) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg (id, container_id, status, build_id, created_at)
                 VALUES (1, ?1, ?2, ?3, ?4)",
                params![container_id, status, build_id, created_at],
            )
            .map_err(|e| state_err(format!("failed to upsert ssg row: {e}"), e))?;
        debug!(status, "upserted ssg singleton");
        Ok(())
    }

    #[instrument(skip(self))]
    fn get_ssg(&self) -> Result<Option<SsgRecord>> {
        self.conn
            .query_row(
                "SELECT container_id, status, build_id, created_at FROM ssg WHERE id = 1",
                [],
                row_to_ssg,
            )
            .optional()
            .map_err(|e| state_err(format!("failed to query ssg row: {e}"), e))
    }

    #[instrument(skip(self))]
    fn clear_ssg(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM ssg WHERE id = 1", [])
            .map_err(|e| state_err(format!("failed to clear ssg row: {e}"), e))?;
        debug!("cleared ssg singleton");
        Ok(())
    }

    #[instrument(skip(self))]
    fn upsert_ssg_service(&self, rec: &SsgServiceRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_services
                   (service_name, container_port, dynamic_host_port, status)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    rec.service_name,
                    rec.container_port as i64,
                    rec.dynamic_host_port as i64,
                    rec.status,
                ],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to upsert ssg service '{}': {e}", rec.service_name),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn list_ssg_services(&self) -> Result<Vec<SsgServiceRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT service_name, container_port, dynamic_host_port, status
                 FROM ssg_services
                 ORDER BY service_name",
            )
            .map_err(|e| state_err(format!("failed to prepare ssg_services query: {e}"), e))?;

        let rows = stmt
            .query_map([], row_to_service)
            .map_err(|e| state_err(format!("failed to list ssg_services: {e}"), e))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| state_err(format!("failed to read ssg_services row: {e}"), e))?,
            );
        }
        Ok(out)
    }

    #[instrument(skip(self))]
    fn update_ssg_service_status(&self, name: &str, status: &str) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE ssg_services SET status = ?1 WHERE service_name = ?2",
                params![status, name],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to update ssg service '{name}' status: {e}"),
                    e,
                )
            })?;
        if changed == 0 {
            return Err(CoastError::State {
                message: format!(
                    "ssg service '{name}' not found in ssg_services. \
                     Run `coast ssg ps` to see registered services."
                ),
                source: None,
            });
        }
        Ok(())
    }

    #[instrument(skip(self))]
    fn clear_ssg_services(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM ssg_services", [])
            .map_err(|e| state_err(format!("failed to clear ssg_services: {e}"), e))?;
        debug!("cleared ssg_services");
        Ok(())
    }

    #[instrument(skip(self))]
    fn upsert_ssg_port_checkout(&self, rec: &SsgPortCheckoutRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_port_checkouts
                   (canonical_port, service_name, socat_pid, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    rec.canonical_port as i64,
                    rec.service_name,
                    rec.socat_pid,
                    rec.created_at,
                ],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to upsert ssg port checkout for canonical {}: {e}",
                        rec.canonical_port
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn list_ssg_port_checkouts(&self) -> Result<Vec<SsgPortCheckoutRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT canonical_port, service_name, socat_pid, created_at
                 FROM ssg_port_checkouts
                 ORDER BY canonical_port",
            )
            .map_err(|e| {
                state_err(
                    format!("failed to prepare ssg_port_checkouts query: {e}"),
                    e,
                )
            })?;

        let rows = stmt
            .query_map([], row_to_checkout)
            .map_err(|e| state_err(format!("failed to list ssg_port_checkouts: {e}"), e))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| {
                state_err(format!("failed to read ssg_port_checkouts row: {e}"), e)
            })?);
        }
        Ok(out)
    }

    #[instrument(skip(self))]
    fn delete_ssg_port_checkout(&self, canonical_port: u16) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_port_checkouts WHERE canonical_port = ?1",
                params![canonical_port as i64],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to delete ssg port checkout for {canonical_port}: {e}"),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn update_ssg_port_checkout_socat_pid(
        &self,
        canonical_port: u16,
        socat_pid: Option<i32>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE ssg_port_checkouts SET socat_pid = ?1 WHERE canonical_port = ?2",
                params![socat_pid, canonical_port as i64],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to update socat_pid for ssg port checkout {canonical_port}: {e}"
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn clear_ssg_port_checkouts(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM ssg_port_checkouts", [])
            .map_err(|e| state_err(format!("failed to clear ssg_port_checkouts: {e}"), e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> StateDb {
        StateDb::open_in_memory().unwrap()
    }

    // --- ssg singleton ---

    #[test]
    fn upsert_get_clear_ssg_round_trip() {
        let db = db();
        assert!(db.get_ssg().unwrap().is_none());

        db.upsert_ssg("created", None, None).unwrap();
        let rec = db.get_ssg().unwrap().unwrap();
        assert_eq!(rec.status, "created");
        assert!(rec.container_id.is_none());
        assert!(rec.build_id.is_none());
        assert!(!rec.created_at.is_empty());

        db.upsert_ssg("running", Some("cid-abc"), Some("b1_20260101"))
            .unwrap();
        let rec = db.get_ssg().unwrap().unwrap();
        assert_eq!(rec.status, "running");
        assert_eq!(rec.container_id.as_deref(), Some("cid-abc"));
        assert_eq!(rec.build_id.as_deref(), Some("b1_20260101"));

        db.clear_ssg().unwrap();
        assert!(db.get_ssg().unwrap().is_none());

        // clear is idempotent.
        db.clear_ssg().unwrap();
    }

    #[test]
    fn ssg_check_id_1_rejects_direct_second_insert() {
        // The app path uses INSERT OR REPLACE so it never hits this,
        // but the CHECK (id = 1) constraint is what guarantees the
        // "one SSG per host" invariant. Verify the constraint itself.
        let db = db();
        db.upsert_ssg("running", None, None).unwrap();

        let direct = db.conn.execute(
            "INSERT INTO ssg (id, container_id, status, build_id, created_at)
             VALUES (2, NULL, 'running', NULL, '2026-01-01T00:00:00Z')",
            [],
        );
        let err = direct.expect_err("CHECK (id = 1) must reject id = 2");
        let msg = err.to_string();
        assert!(
            msg.contains("CHECK constraint failed") || msg.contains("constraint failed"),
            "expected CHECK constraint failure, got: {msg}"
        );
    }

    // --- ssg_services ---

    fn svc(name: &str, container: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            service_name: name.to_string(),
            container_port: container,
            dynamic_host_port: dynamic,
            status: "running".to_string(),
        }
    }

    #[test]
    fn ssg_services_upsert_list_update_clear() {
        let db = db();
        assert!(db.list_ssg_services().unwrap().is_empty());

        db.upsert_ssg_service(&svc("postgres", 5432, 54201))
            .unwrap();
        db.upsert_ssg_service(&svc("redis", 6379, 54202)).unwrap();

        let listed = db.list_ssg_services().unwrap();
        assert_eq!(listed.len(), 2);
        // Alphabetical ordering by service_name.
        assert_eq!(listed[0].service_name, "postgres");
        assert_eq!(listed[0].container_port, 5432);
        assert_eq!(listed[0].dynamic_host_port, 54201);
        assert_eq!(listed[1].service_name, "redis");

        // Update status.
        db.update_ssg_service_status("postgres", "stopped").unwrap();
        let rec = db
            .list_ssg_services()
            .unwrap()
            .into_iter()
            .find(|s| s.service_name == "postgres")
            .unwrap();
        assert_eq!(rec.status, "stopped");

        // Upsert replaces.
        db.upsert_ssg_service(&SsgServiceRecord {
            service_name: "postgres".to_string(),
            container_port: 5432,
            dynamic_host_port: 54299,
            status: "running".to_string(),
        })
        .unwrap();
        let rec = db
            .list_ssg_services()
            .unwrap()
            .into_iter()
            .find(|s| s.service_name == "postgres")
            .unwrap();
        assert_eq!(rec.dynamic_host_port, 54299);

        db.clear_ssg_services().unwrap();
        assert!(db.list_ssg_services().unwrap().is_empty());
    }

    #[test]
    fn update_ssg_service_status_unknown_name_errors() {
        let db = db();
        let err = db
            .update_ssg_service_status("ghost", "running")
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // --- ssg_port_checkouts ---

    fn checkout(canonical: u16, service: &str, pid: Option<i32>) -> SsgPortCheckoutRecord {
        SsgPortCheckoutRecord {
            canonical_port: canonical,
            service_name: service.to_string(),
            socat_pid: pid,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn ssg_port_checkouts_upsert_list_delete() {
        let db = db();
        assert!(db.list_ssg_port_checkouts().unwrap().is_empty());

        db.upsert_ssg_port_checkout(&checkout(5432, "postgres", Some(12345)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(6379, "redis", None))
            .unwrap();

        let listed = db.list_ssg_port_checkouts().unwrap();
        assert_eq!(listed.len(), 2);
        // Ascending canonical_port.
        assert_eq!(listed[0].canonical_port, 5432);
        assert_eq!(listed[0].service_name, "postgres");
        assert_eq!(listed[0].socat_pid, Some(12345));
        assert_eq!(listed[1].canonical_port, 6379);
        assert!(listed[1].socat_pid.is_none());

        // Upsert replaces.
        db.upsert_ssg_port_checkout(&checkout(5432, "postgres", Some(99999)))
            .unwrap();
        let rec = db
            .list_ssg_port_checkouts()
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert_eq!(rec.socat_pid, Some(99999));

        db.delete_ssg_port_checkout(5432).unwrap();
        let listed = db.list_ssg_port_checkouts().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].canonical_port, 6379);

        // Delete is idempotent.
        db.delete_ssg_port_checkout(5432).unwrap();
    }

    #[test]
    fn ssg_port_checkouts_update_socat_pid_preserves_other_columns() {
        let db = db();
        db.upsert_ssg_port_checkout(&checkout(5432, "postgres", Some(111)))
            .unwrap();

        // Null the PID (as stop would do).
        db.update_ssg_port_checkout_socat_pid(5432, None).unwrap();
        let rec = db
            .list_ssg_port_checkouts()
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert!(rec.socat_pid.is_none());
        // Row still exists with its service_name intact.
        assert_eq!(rec.service_name, "postgres");

        // Set a fresh PID (as re-spawn would do).
        db.update_ssg_port_checkout_socat_pid(5432, Some(222))
            .unwrap();
        let rec = db
            .list_ssg_port_checkouts()
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert_eq!(rec.socat_pid, Some(222));
    }

    #[test]
    fn ssg_port_checkouts_clear_removes_all_rows() {
        let db = db();
        db.upsert_ssg_port_checkout(&checkout(5432, "postgres", Some(1)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(6379, "redis", Some(2)))
            .unwrap();

        db.clear_ssg_port_checkouts().unwrap();
        assert!(db.list_ssg_port_checkouts().unwrap().is_empty());

        // Idempotent.
        db.clear_ssg_port_checkouts().unwrap();
    }
}
