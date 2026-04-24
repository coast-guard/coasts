//! `SsgStateExt` impl for `StateDb`.
//!
//! Phase: ssg-phase-20 (per-project correction). See
//! `coast-ssg/DESIGN.md §23`.
//!
//! Keeps the SSG state logic colocated with the feature crate
//! ([`coast_ssg::state`]) while using the existing daemon `StateDb`
//! handle. The trait is imported from `coast-ssg`; only the impl lives
//! here, following the pattern the daemon uses for other per-domain
//! record CRUD (see [`super::shared_services`]).

use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};
use coast_ssg::state::{
    SsgConsumerPinRecord, SsgPortCheckoutRecord, SsgRecord, SsgServiceRecord, SsgStateExt,
    SsgVirtualPortRecord,
};

use super::StateDb;

fn state_err(message: String, source: rusqlite::Error) -> CoastError {
    CoastError::State {
        message,
        source: Some(Box::new(source)),
    }
}

fn row_to_ssg(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgRecord> {
    Ok(SsgRecord {
        project: row.get(0)?,
        container_id: row.get(1)?,
        status: row.get(2)?,
        build_id: row.get(3)?,
        latest_build_id: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn row_to_service(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgServiceRecord> {
    Ok(SsgServiceRecord {
        project: row.get(0)?,
        service_name: row.get(1)?,
        container_port: row.get::<_, i64>(2)? as u16,
        dynamic_host_port: row.get::<_, i64>(3)? as u16,
        status: row.get(4)?,
    })
}

fn row_to_checkout(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgPortCheckoutRecord> {
    Ok(SsgPortCheckoutRecord {
        project: row.get(0)?,
        canonical_port: row.get::<_, i64>(1)? as u16,
        service_name: row.get(2)?,
        socat_pid: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn row_to_virtual_port(row: &rusqlite::Row<'_>) -> rusqlite::Result<SsgVirtualPortRecord> {
    Ok(SsgVirtualPortRecord {
        project: row.get(0)?,
        service_name: row.get(1)?,
        container_port: row.get::<_, i64>(2)? as u16,
        port: row.get::<_, i64>(3)? as u16,
        created_at: row.get(4)?,
    })
}

impl SsgStateExt for StateDb {
    #[instrument(skip(self))]
    fn upsert_ssg(
        &self,
        project: &str,
        status: &str,
        container_id: Option<&str>,
        build_id: Option<&str>,
    ) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        // Phase 23: INSERT OR REPLACE would wipe `latest_build_id`
        // because it's not in the column list here. Use an explicit
        // UPSERT that preserves the column when updating.
        self.conn
            .execute(
                "INSERT INTO ssg (project, container_id, status, build_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project) DO UPDATE SET
                     container_id = excluded.container_id,
                     status = excluded.status,
                     build_id = excluded.build_id,
                     created_at = excluded.created_at",
                params![project, container_id, status, build_id, created_at],
            )
            .map_err(|e| state_err(format!("failed to upsert ssg row for '{project}': {e}"), e))?;
        debug!(project, status, "upserted ssg row");
        Ok(())
    }

    #[instrument(skip(self))]
    fn get_ssg(&self, project: &str) -> Result<Option<SsgRecord>> {
        self.conn
            .query_row(
                "SELECT project, container_id, status, build_id, latest_build_id, created_at
                 FROM ssg WHERE project = ?1",
                params![project],
                row_to_ssg,
            )
            .optional()
            .map_err(|e| state_err(format!("failed to query ssg row for '{project}': {e}"), e))
    }

    #[instrument(skip(self))]
    fn clear_ssg(&self, project: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM ssg WHERE project = ?1", params![project])
            .map_err(|e| state_err(format!("failed to clear ssg row for '{project}': {e}"), e))?;
        debug!(project, "cleared ssg row");
        Ok(())
    }

    #[instrument(skip(self))]
    fn set_latest_build_id(&self, project: &str, build_id: &str) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        // Phase 23: creates a row with `status = "built"` when absent;
        // when the row exists, only `latest_build_id` is touched —
        // `container_id`, `build_id`, `status`, `created_at` stay
        // put so a running SSG keeps running after a rebuild. See
        // `coast-ssg/DESIGN.md §23`.
        self.conn
            .execute(
                "INSERT INTO ssg (project, status, build_id, latest_build_id, created_at)
                 VALUES (?1, 'built', NULL, ?2, ?3)
                 ON CONFLICT(project) DO UPDATE SET
                     latest_build_id = excluded.latest_build_id",
                params![project, build_id, created_at],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to set latest_build_id for project '{project}' to '{build_id}': {e}"
                    ),
                    e,
                )
            })?;
        debug!(project, build_id, "set ssg.latest_build_id");
        Ok(())
    }

    #[instrument(skip(self))]
    fn list_ssgs(&self) -> Result<Vec<SsgRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, container_id, status, build_id, latest_build_id, created_at
                 FROM ssg
                 ORDER BY project ASC",
            )
            .map_err(|e| state_err(format!("failed to prepare list_ssgs query: {e}"), e))?;
        let rows = stmt
            .query_map([], row_to_ssg)
            .map_err(|e| state_err(format!("failed to list ssgs: {e}"), e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| state_err(format!("failed to read ssg row: {e}"), e))?);
        }
        Ok(out)
    }

    #[instrument(skip(self))]
    fn upsert_ssg_service(&self, rec: &SsgServiceRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_services
                   (project, service_name, container_port, dynamic_host_port, status)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    rec.project,
                    rec.service_name,
                    rec.container_port as i64,
                    rec.dynamic_host_port as i64,
                    rec.status,
                ],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to upsert ssg service '{}/{}': {e}",
                        rec.project, rec.service_name
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn list_ssg_services(&self, project: &str) -> Result<Vec<SsgServiceRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, service_name, container_port, dynamic_host_port, status
                 FROM ssg_services
                 WHERE project = ?1
                 ORDER BY service_name",
            )
            .map_err(|e| state_err(format!("failed to prepare ssg_services query: {e}"), e))?;

        let rows = stmt
            .query_map(params![project], row_to_service)
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
    fn update_ssg_service_status(&self, project: &str, name: &str, status: &str) -> Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE ssg_services SET status = ?1 WHERE project = ?2 AND service_name = ?3",
                params![status, project, name],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to update ssg service '{project}/{name}' status: {e}"),
                    e,
                )
            })?;
        if changed == 0 {
            return Err(CoastError::State {
                message: format!(
                    "ssg service '{project}/{name}' not found in ssg_services. \
                     Run `coast ssg ps` to see registered services."
                ),
                source: None,
            });
        }
        Ok(())
    }

    #[instrument(skip(self))]
    fn clear_ssg_services(&self, project: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_services WHERE project = ?1",
                params![project],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to clear ssg_services for '{project}': {e}"),
                    e,
                )
            })?;
        debug!(project, "cleared ssg_services");
        Ok(())
    }

    #[instrument(skip(self))]
    fn upsert_ssg_port_checkout(&self, rec: &SsgPortCheckoutRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_port_checkouts
                   (project, canonical_port, service_name, socat_pid, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    rec.project,
                    rec.canonical_port as i64,
                    rec.service_name,
                    rec.socat_pid,
                    rec.created_at,
                ],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to upsert ssg port checkout for '{}:{}': {e}",
                        rec.project, rec.canonical_port
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn list_ssg_port_checkouts(&self, project: &str) -> Result<Vec<SsgPortCheckoutRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, canonical_port, service_name, socat_pid, created_at
                 FROM ssg_port_checkouts
                 WHERE project = ?1
                 ORDER BY canonical_port",
            )
            .map_err(|e| {
                state_err(
                    format!("failed to prepare ssg_port_checkouts query: {e}"),
                    e,
                )
            })?;

        let rows = stmt
            .query_map(params![project], row_to_checkout)
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
    fn delete_ssg_port_checkout(&self, project: &str, canonical_port: u16) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_port_checkouts
                 WHERE project = ?1 AND canonical_port = ?2",
                params![project, canonical_port as i64],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to delete ssg port checkout for '{project}:{canonical_port}': {e}"
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn update_ssg_port_checkout_socat_pid(
        &self,
        project: &str,
        canonical_port: u16,
        socat_pid: Option<i32>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE ssg_port_checkouts
                 SET socat_pid = ?1
                 WHERE project = ?2 AND canonical_port = ?3",
                params![socat_pid, project, canonical_port as i64],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to update socat_pid for ssg port checkout '{project}:{canonical_port}': {e}"
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(skip(self))]
    fn clear_ssg_port_checkouts(&self, project: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_port_checkouts WHERE project = ?1",
                params![project],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to clear ssg_port_checkouts for '{project}': {e}"),
                    e,
                )
            })?;
        Ok(())
    }

    // --- ssg_consumer_pins (Phase 16) ---

    #[instrument(level = "debug", skip(self), fields(project = %rec.project, build_id = %rec.build_id))]
    fn upsert_ssg_consumer_pin(&self, rec: &SsgConsumerPinRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_consumer_pins
                   (project, build_id, created_at)
                 VALUES (?1, ?2, ?3)",
                params![rec.project, rec.build_id, rec.created_at],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to upsert ssg_consumer_pin for '{}': {e}",
                        rec.project
                    ),
                    e,
                )
            })?;
        debug!("upserted ssg_consumer_pin");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(project = %project))]
    fn get_ssg_consumer_pin(&self, project: &str) -> Result<Option<SsgConsumerPinRecord>> {
        self.conn
            .query_row(
                "SELECT project, build_id, created_at FROM ssg_consumer_pins WHERE project = ?1",
                params![project],
                |row| {
                    Ok(SsgConsumerPinRecord {
                        project: row.get(0)?,
                        build_id: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|e| {
                state_err(
                    format!("failed to read ssg_consumer_pin for '{project}': {e}"),
                    e,
                )
            })
    }

    #[instrument(level = "debug", skip(self), fields(project = %project))]
    fn delete_ssg_consumer_pin(&self, project: &str) -> Result<bool> {
        let affected = self
            .conn
            .execute(
                "DELETE FROM ssg_consumer_pins WHERE project = ?1",
                params![project],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to delete ssg_consumer_pin for '{project}': {e}"),
                    e,
                )
            })?;
        Ok(affected > 0)
    }

    fn list_ssg_consumer_pins(&self) -> Result<Vec<SsgConsumerPinRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, build_id, created_at
                 FROM ssg_consumer_pins
                 ORDER BY project ASC",
            )
            .map_err(|e| state_err(format!("failed to prepare list_ssg_consumer_pins: {e}"), e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SsgConsumerPinRecord {
                    project: row.get(0)?,
                    build_id: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| state_err(format!("failed to list ssg_consumer_pins: {e}"), e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| state_err(format!("row parse in list_ssg_consumer_pins: {e}"), e))?,
            );
        }
        Ok(out)
    }

    // --- ssg_virtual_ports (Phase 26 / §24.5; per-port keying — Phase 28) ---

    #[instrument(level = "debug", skip(self), fields(project = %project, service = %service_name, container_port = container_port))]
    fn get_ssg_virtual_port(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
    ) -> Result<Option<u16>> {
        self.conn
            .query_row(
                "SELECT port FROM ssg_virtual_ports
                 WHERE project = ?1 AND service_name = ?2 AND container_port = ?3",
                params![project, service_name, container_port as i64],
                |row| row.get::<_, i64>(0).map(|p| p as u16),
            )
            .optional()
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to read ssg_virtual_port for \
                         '{project}/{service_name}:{container_port}': {e}"
                    ),
                    e,
                )
            })
    }

    #[instrument(level = "debug", skip(self), fields(project = %project, service = %service_name, container_port = container_port, port = port))]
    fn upsert_ssg_virtual_port(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
        port: u16,
    ) -> Result<()> {
        let created_at = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO ssg_virtual_ports
                   (project, service_name, container_port, port, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project,
                    service_name,
                    container_port as i64,
                    port as i64,
                    created_at,
                ],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to upsert ssg_virtual_port for \
                         '{project}/{service_name}:{container_port}': {e}"
                    ),
                    e,
                )
            })?;
        debug!("upserted ssg_virtual_port");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(project = %project))]
    fn list_ssg_virtual_ports(&self, project: &str) -> Result<Vec<SsgVirtualPortRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, service_name, container_port, port, created_at
                 FROM ssg_virtual_ports
                 WHERE project = ?1
                 ORDER BY service_name ASC, container_port ASC",
            )
            .map_err(|e| state_err(format!("failed to prepare list_ssg_virtual_ports: {e}"), e))?;
        let rows = stmt
            .query_map(params![project], row_to_virtual_port)
            .map_err(|e| state_err(format!("failed to list ssg_virtual_ports: {e}"), e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| state_err(format!("row parse in list_ssg_virtual_ports: {e}"), e))?,
            );
        }
        Ok(out)
    }

    #[instrument(level = "debug", skip(self), fields(project = %project))]
    fn clear_ssg_virtual_ports(&self, project: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_virtual_ports WHERE project = ?1",
                params![project],
            )
            .map_err(|e| {
                state_err(
                    format!("failed to clear ssg_virtual_ports for '{project}': {e}"),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(project = %project, service = %service_name, container_port = container_port))]
    fn clear_ssg_virtual_port_one(
        &self,
        project: &str,
        service_name: &str,
        container_port: u16,
    ) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM ssg_virtual_ports
                 WHERE project = ?1 AND service_name = ?2 AND container_port = ?3",
                params![project, service_name, container_port as i64],
            )
            .map_err(|e| {
                state_err(
                    format!(
                        "failed to clear ssg_virtual_port for \
                         '{project}/{service_name}:{container_port}': {e}"
                    ),
                    e,
                )
            })?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn list_all_ssg_virtual_port_numbers(&self) -> Result<Vec<u16>> {
        let mut stmt = self
            .conn
            .prepare("SELECT port FROM ssg_virtual_ports")
            .map_err(|e| {
                state_err(
                    format!("failed to prepare list_all_ssg_virtual_port_numbers: {e}"),
                    e,
                )
            })?;
        let rows = stmt
            .query_map([], |row| row.get::<_, i64>(0).map(|p| p as u16))
            .map_err(|e| {
                state_err(
                    format!("failed to list all ssg_virtual_port numbers: {e}"),
                    e,
                )
            })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| {
                state_err(
                    format!("row parse in list_all_ssg_virtual_port_numbers: {e}"),
                    e,
                )
            })?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> StateDb {
        StateDb::open_in_memory().unwrap()
    }

    const P: &str = "proj-a";
    const Q: &str = "proj-b";

    // --- ssg (per-project) ---

    #[test]
    fn upsert_get_clear_ssg_round_trip() {
        let db = db();
        assert!(db.get_ssg(P).unwrap().is_none());

        db.upsert_ssg(P, "created", None, None).unwrap();
        let rec = db.get_ssg(P).unwrap().unwrap();
        assert_eq!(rec.project, P);
        assert_eq!(rec.status, "created");
        assert!(rec.container_id.is_none());
        assert!(rec.build_id.is_none());
        assert!(!rec.created_at.is_empty());

        db.upsert_ssg(P, "running", Some("cid-abc"), Some("b1_20260101"))
            .unwrap();
        let rec = db.get_ssg(P).unwrap().unwrap();
        assert_eq!(rec.status, "running");
        assert_eq!(rec.container_id.as_deref(), Some("cid-abc"));
        assert_eq!(rec.build_id.as_deref(), Some("b1_20260101"));

        db.clear_ssg(P).unwrap();
        assert!(db.get_ssg(P).unwrap().is_none());

        // clear is idempotent.
        db.clear_ssg(P).unwrap();
    }

    #[test]
    fn two_projects_coexist_under_per_project_schema() {
        // Regression for the singleton→per-project correction (§23).
        // Under the old schema, a second insert failed the CHECK.
        // Under the per-project schema, both must coexist.
        let db = db();
        db.upsert_ssg(P, "running", Some("cid-a"), Some("b1"))
            .unwrap();
        db.upsert_ssg(Q, "running", Some("cid-b"), Some("b2"))
            .unwrap();

        let a = db.get_ssg(P).unwrap().unwrap();
        let b = db.get_ssg(Q).unwrap().unwrap();
        assert_eq!(a.project, P);
        assert_eq!(b.project, Q);
        assert_eq!(a.container_id.as_deref(), Some("cid-a"));
        assert_eq!(b.container_id.as_deref(), Some("cid-b"));

        // list_ssgs returns both, sorted.
        let all = db.list_ssgs().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].project, P);
        assert_eq!(all[1].project, Q);

        // Clearing one does not affect the other.
        db.clear_ssg(P).unwrap();
        assert!(db.get_ssg(P).unwrap().is_none());
        assert!(db.get_ssg(Q).unwrap().is_some());
    }

    // --- Phase 23: set_latest_build_id ---

    #[test]
    fn set_latest_build_id_creates_row_with_built_status() {
        let db = db();
        assert!(db.get_ssg(P).unwrap().is_none());

        db.set_latest_build_id(P, "b_new_20260424").unwrap();

        let rec = db.get_ssg(P).unwrap().expect("row should exist");
        assert_eq!(rec.project, P);
        assert_eq!(rec.status, "built");
        assert!(rec.container_id.is_none(), "no container until ssg run",);
        assert!(rec.build_id.is_none(), "no running-build until ssg run",);
        assert_eq!(rec.latest_build_id.as_deref(), Some("b_new_20260424"));
        assert!(!rec.created_at.is_empty());
    }

    #[test]
    fn set_latest_build_id_preserves_running_state() {
        // Regression for DESIGN §23: when `ssg build` fires while the
        // SSG is running, only `latest_build_id` updates — the running
        // container's state is left untouched.
        let db = db();
        db.upsert_ssg(P, "running", Some("cid-abc"), Some("b_old"))
            .unwrap();
        db.set_latest_build_id(P, "b_new").unwrap();

        let rec = db.get_ssg(P).unwrap().unwrap();
        assert_eq!(rec.status, "running");
        assert_eq!(rec.container_id.as_deref(), Some("cid-abc"));
        assert_eq!(rec.build_id.as_deref(), Some("b_old"));
        assert_eq!(rec.latest_build_id.as_deref(), Some("b_new"));
    }

    #[test]
    fn set_latest_build_id_is_idempotent_and_allows_overwrites() {
        let db = db();
        db.set_latest_build_id(P, "b_one").unwrap();
        db.set_latest_build_id(P, "b_two").unwrap();
        let rec = db.get_ssg(P).unwrap().unwrap();
        assert_eq!(rec.latest_build_id.as_deref(), Some("b_two"));
    }

    #[test]
    fn set_latest_build_id_scoped_by_project() {
        let db = db();
        db.set_latest_build_id(P, "b_p").unwrap();
        db.set_latest_build_id(Q, "b_q").unwrap();

        assert_eq!(
            db.get_ssg(P).unwrap().unwrap().latest_build_id.as_deref(),
            Some("b_p"),
        );
        assert_eq!(
            db.get_ssg(Q).unwrap().unwrap().latest_build_id.as_deref(),
            Some("b_q"),
        );
    }

    #[test]
    fn upsert_ssg_preserves_latest_build_id() {
        // Ensure Phase 20's upsert_ssg (now using explicit UPSERT with
        // named columns) doesn't accidentally clear latest_build_id
        // when a running ssg row is updated.
        let db = db();
        db.set_latest_build_id(P, "b_latest").unwrap();
        db.upsert_ssg(P, "running", Some("cid-1"), Some("b_latest"))
            .unwrap();

        let rec = db.get_ssg(P).unwrap().unwrap();
        assert_eq!(rec.status, "running");
        assert_eq!(rec.container_id.as_deref(), Some("cid-1"));
        assert_eq!(rec.latest_build_id.as_deref(), Some("b_latest"));
    }

    // --- ssg_services ---

    fn svc(project: &str, name: &str, container: u16, dynamic: u16) -> SsgServiceRecord {
        SsgServiceRecord {
            project: project.to_string(),
            service_name: name.to_string(),
            container_port: container,
            dynamic_host_port: dynamic,
            status: "running".to_string(),
        }
    }

    #[test]
    fn ssg_services_upsert_list_update_clear() {
        let db = db();
        assert!(db.list_ssg_services(P).unwrap().is_empty());

        db.upsert_ssg_service(&svc(P, "postgres", 5432, 54201))
            .unwrap();
        db.upsert_ssg_service(&svc(P, "redis", 6379, 54202))
            .unwrap();

        let listed = db.list_ssg_services(P).unwrap();
        assert_eq!(listed.len(), 2);
        // Alphabetical ordering by service_name.
        assert_eq!(listed[0].service_name, "postgres");
        assert_eq!(listed[0].container_port, 5432);
        assert_eq!(listed[0].dynamic_host_port, 54201);
        assert_eq!(listed[1].service_name, "redis");

        // Update status.
        db.update_ssg_service_status(P, "postgres", "stopped")
            .unwrap();
        let rec = db
            .list_ssg_services(P)
            .unwrap()
            .into_iter()
            .find(|s| s.service_name == "postgres")
            .unwrap();
        assert_eq!(rec.status, "stopped");

        // Upsert replaces.
        db.upsert_ssg_service(&SsgServiceRecord {
            project: P.to_string(),
            service_name: "postgres".to_string(),
            container_port: 5432,
            dynamic_host_port: 54299,
            status: "running".to_string(),
        })
        .unwrap();
        let rec = db
            .list_ssg_services(P)
            .unwrap()
            .into_iter()
            .find(|s| s.service_name == "postgres")
            .unwrap();
        assert_eq!(rec.dynamic_host_port, 54299);

        db.clear_ssg_services(P).unwrap();
        assert!(db.list_ssg_services(P).unwrap().is_empty());
    }

    #[test]
    fn ssg_services_isolated_across_projects() {
        let db = db();
        db.upsert_ssg_service(&svc(P, "postgres", 5432, 54201))
            .unwrap();
        db.upsert_ssg_service(&svc(Q, "postgres", 5432, 55201))
            .unwrap();

        // Same service_name, different projects — no collision.
        let a = db.list_ssg_services(P).unwrap();
        let b = db.list_ssg_services(Q).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].dynamic_host_port, 54201);
        assert_eq!(b[0].dynamic_host_port, 55201);

        // Clearing one project leaves the other.
        db.clear_ssg_services(P).unwrap();
        assert!(db.list_ssg_services(P).unwrap().is_empty());
        assert_eq!(db.list_ssg_services(Q).unwrap().len(), 1);
    }

    #[test]
    fn update_ssg_service_status_unknown_name_errors() {
        let db = db();
        let err = db
            .update_ssg_service_status(P, "ghost", "running")
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // --- ssg_port_checkouts ---

    fn checkout(
        project: &str,
        canonical: u16,
        service: &str,
        pid: Option<i32>,
    ) -> SsgPortCheckoutRecord {
        SsgPortCheckoutRecord {
            project: project.to_string(),
            canonical_port: canonical,
            service_name: service.to_string(),
            socat_pid: pid,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn ssg_port_checkouts_upsert_list_delete() {
        let db = db();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());

        db.upsert_ssg_port_checkout(&checkout(P, 5432, "postgres", Some(12345)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(P, 6379, "redis", None))
            .unwrap();

        let listed = db.list_ssg_port_checkouts(P).unwrap();
        assert_eq!(listed.len(), 2);
        // Ascending canonical_port.
        assert_eq!(listed[0].canonical_port, 5432);
        assert_eq!(listed[0].service_name, "postgres");
        assert_eq!(listed[0].socat_pid, Some(12345));
        assert_eq!(listed[1].canonical_port, 6379);
        assert!(listed[1].socat_pid.is_none());

        // Upsert replaces.
        db.upsert_ssg_port_checkout(&checkout(P, 5432, "postgres", Some(99999)))
            .unwrap();
        let rec = db
            .list_ssg_port_checkouts(P)
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert_eq!(rec.socat_pid, Some(99999));

        db.delete_ssg_port_checkout(P, 5432).unwrap();
        let listed = db.list_ssg_port_checkouts(P).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].canonical_port, 6379);

        // Delete is idempotent.
        db.delete_ssg_port_checkout(P, 5432).unwrap();
    }

    #[test]
    fn ssg_port_checkouts_isolated_across_projects() {
        let db = db();
        db.upsert_ssg_port_checkout(&checkout(P, 5432, "postgres", Some(1)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(Q, 5432, "postgres", Some(2)))
            .unwrap();

        // Same canonical port, different projects — no collision.
        let a = db.list_ssg_port_checkouts(P).unwrap();
        let b = db.list_ssg_port_checkouts(Q).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(b.len(), 1);
        assert_eq!(a[0].socat_pid, Some(1));
        assert_eq!(b[0].socat_pid, Some(2));

        // Delete in one does not affect the other.
        db.delete_ssg_port_checkout(P, 5432).unwrap();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());
        assert_eq!(db.list_ssg_port_checkouts(Q).unwrap().len(), 1);
    }

    #[test]
    fn ssg_port_checkouts_update_socat_pid_preserves_other_columns() {
        let db = db();
        db.upsert_ssg_port_checkout(&checkout(P, 5432, "postgres", Some(111)))
            .unwrap();

        // Null the PID (as stop would do).
        db.update_ssg_port_checkout_socat_pid(P, 5432, None)
            .unwrap();
        let rec = db
            .list_ssg_port_checkouts(P)
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert!(rec.socat_pid.is_none());
        // Row still exists with its service_name intact.
        assert_eq!(rec.service_name, "postgres");

        // Set a fresh PID (as re-spawn would do).
        db.update_ssg_port_checkout_socat_pid(P, 5432, Some(222))
            .unwrap();
        let rec = db
            .list_ssg_port_checkouts(P)
            .unwrap()
            .into_iter()
            .find(|c| c.canonical_port == 5432)
            .unwrap();
        assert_eq!(rec.socat_pid, Some(222));
    }

    #[test]
    fn ssg_port_checkouts_clear_removes_all_rows_for_project_only() {
        let db = db();
        db.upsert_ssg_port_checkout(&checkout(P, 5432, "postgres", Some(1)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(P, 6379, "redis", Some(2)))
            .unwrap();
        db.upsert_ssg_port_checkout(&checkout(Q, 5432, "postgres", Some(3)))
            .unwrap();

        db.clear_ssg_port_checkouts(P).unwrap();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());
        // Q untouched.
        assert_eq!(db.list_ssg_port_checkouts(Q).unwrap().len(), 1);

        // Idempotent.
        db.clear_ssg_port_checkouts(P).unwrap();
    }

    // --- Coverage fill-ins ---

    #[test]
    fn ssg_get_returns_null_container_and_build_id_unchanged() {
        let db = db();
        db.upsert_ssg(P, "created", None, None).unwrap();
        let rec = db.get_ssg(P).unwrap().expect("row");
        assert_eq!(rec.status, "created");
        assert!(
            rec.container_id.is_none(),
            "container_id must round-trip as None"
        );
        assert!(rec.build_id.is_none(), "build_id must round-trip as None");
    }

    #[test]
    fn delete_ssg_port_checkout_is_idempotent_when_row_missing() {
        let db = db();
        db.delete_ssg_port_checkout(P, 5432).unwrap();
        db.delete_ssg_port_checkout(P, 5432).unwrap();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());
    }

    #[test]
    fn update_ssg_port_checkout_socat_pid_on_missing_row_is_noop() {
        let db = db();
        db.update_ssg_port_checkout_socat_pid(P, 5432, Some(42))
            .unwrap();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());
    }

    #[test]
    fn clear_ssg_port_checkouts_on_empty_table_is_noop() {
        let db = db();
        db.clear_ssg_port_checkouts(P).unwrap();
        db.clear_ssg_port_checkouts(P).unwrap();
        assert!(db.list_ssg_port_checkouts(P).unwrap().is_empty());
    }

    #[test]
    fn list_ssg_services_orders_alphabetically_with_many_rows() {
        let db = db();
        db.upsert_ssg_service(&svc(P, "redis", 6379, 54202))
            .unwrap();
        db.upsert_ssg_service(&svc(P, "mongo", 27017, 54203))
            .unwrap();
        db.upsert_ssg_service(&svc(P, "postgres", 5432, 54201))
            .unwrap();
        db.upsert_ssg_service(&svc(P, "clickhouse", 9000, 54204))
            .unwrap();

        let listed = db.list_ssg_services(P).unwrap();
        let names: Vec<_> = listed.iter().map(|s| s.service_name.as_str()).collect();
        assert_eq!(names, vec!["clickhouse", "mongo", "postgres", "redis"]);
    }

    #[test]
    fn upsert_ssg_service_replaces_port_on_same_name() {
        let db = db();
        db.upsert_ssg_service(&svc(P, "postgres", 5432, 54201))
            .unwrap();
        db.upsert_ssg_service(&svc(P, "postgres", 5432, 60000))
            .unwrap();

        let listed = db.list_ssg_services(P).unwrap();
        assert_eq!(listed.len(), 1, "must stay a single row, not duplicate");
        assert_eq!(listed[0].dynamic_host_port, 60000);
    }

    #[test]
    fn clear_ssg_services_on_empty_table_is_idempotent() {
        let db = db();
        db.clear_ssg_services(P).unwrap();
        db.upsert_ssg_service(&svc(P, "postgres", 5432, 54201))
            .unwrap();
        db.clear_ssg_services(P).unwrap();
        db.clear_ssg_services(P).unwrap();
        assert!(db.list_ssg_services(P).unwrap().is_empty());
    }

    #[test]
    fn upsert_ssg_port_checkout_roundtrips_created_at_timestamp() {
        let db = db();
        let fixed_ts = "2026-04-20T12:34:56+00:00";
        db.upsert_ssg_port_checkout(&SsgPortCheckoutRecord {
            project: P.to_string(),
            canonical_port: 5432,
            service_name: "postgres".to_string(),
            socat_pid: Some(42),
            created_at: fixed_ts.to_string(),
        })
        .unwrap();
        let rec = db
            .list_ssg_port_checkouts(P)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(rec.created_at, fixed_ts);
    }

    // --- ssg_consumer_pins (Phase 16) ---

    fn pin(project: &str, build_id: &str) -> SsgConsumerPinRecord {
        SsgConsumerPinRecord {
            project: project.to_string(),
            build_id: build_id.to_string(),
            created_at: "2026-04-22T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn get_ssg_consumer_pin_returns_none_before_insert() {
        let db = db();
        assert!(db.get_ssg_consumer_pin("proj").unwrap().is_none());
    }

    #[test]
    fn upsert_and_get_ssg_consumer_pin_round_trips() {
        let db = db();
        db.upsert_ssg_consumer_pin(&pin("proj", "b1_20260422"))
            .unwrap();
        let r = db.get_ssg_consumer_pin("proj").unwrap().unwrap();
        assert_eq!(r.project, "proj");
        assert_eq!(r.build_id, "b1_20260422");
    }

    #[test]
    fn upsert_ssg_consumer_pin_replaces_by_project() {
        let db = db();
        db.upsert_ssg_consumer_pin(&pin("proj", "b1")).unwrap();
        db.upsert_ssg_consumer_pin(&pin("proj", "b2")).unwrap();
        let r = db.get_ssg_consumer_pin("proj").unwrap().unwrap();
        assert_eq!(r.build_id, "b2", "same project should replace the pin");
        assert_eq!(db.list_ssg_consumer_pins().unwrap().len(), 1);
    }

    #[test]
    fn delete_ssg_consumer_pin_reports_whether_a_row_existed() {
        let db = db();
        assert!(!db.delete_ssg_consumer_pin("proj").unwrap());
        db.upsert_ssg_consumer_pin(&pin("proj", "b1")).unwrap();
        assert!(db.delete_ssg_consumer_pin("proj").unwrap());
        assert!(!db.delete_ssg_consumer_pin("proj").unwrap());
    }

    #[test]
    fn delete_ssg_consumer_pin_only_affects_named_project() {
        let db = db();
        db.upsert_ssg_consumer_pin(&pin("proj-a", "b1")).unwrap();
        db.upsert_ssg_consumer_pin(&pin("proj-b", "b2")).unwrap();
        db.delete_ssg_consumer_pin("proj-a").unwrap();
        assert!(db.get_ssg_consumer_pin("proj-a").unwrap().is_none());
        assert_eq!(
            db.get_ssg_consumer_pin("proj-b").unwrap().unwrap().build_id,
            "b2"
        );
    }

    #[test]
    fn list_ssg_consumer_pins_orders_alphabetically_by_project() {
        let db = db();
        db.upsert_ssg_consumer_pin(&pin("zeta", "bz")).unwrap();
        db.upsert_ssg_consumer_pin(&pin("alpha", "ba")).unwrap();
        db.upsert_ssg_consumer_pin(&pin("mike", "bm")).unwrap();
        let names: Vec<_> = db
            .list_ssg_consumer_pins()
            .unwrap()
            .into_iter()
            .map(|p| p.project)
            .collect();
        assert_eq!(names, vec!["alpha", "mike", "zeta"]);
    }

    #[test]
    fn list_ssg_consumer_pins_empty_when_no_rows() {
        let db = db();
        assert!(db.list_ssg_consumer_pins().unwrap().is_empty());
    }

    #[test]
    fn ssg_consumer_pin_created_at_is_preserved_verbatim() {
        let db = db();
        let rec = SsgConsumerPinRecord {
            project: "proj".to_string(),
            build_id: "b1".to_string(),
            created_at: "2026-01-02T03:04:05+00:00".to_string(),
        };
        db.upsert_ssg_consumer_pin(&rec).unwrap();
        let out = db.get_ssg_consumer_pin("proj").unwrap().unwrap();
        assert_eq!(out.created_at, "2026-01-02T03:04:05+00:00");
    }

    #[test]
    fn ssg_consumer_pin_build_id_round_trips_nonalphanumeric_chars() {
        let db = db();
        db.upsert_ssg_consumer_pin(&pin("proj", "df5bddb5b7a39b11_20260422051132"))
            .unwrap();
        let out = db.get_ssg_consumer_pin("proj").unwrap().unwrap();
        assert_eq!(out.build_id, "df5bddb5b7a39b11_20260422051132");
    }

    // --- ssg_virtual_ports (Phase 26 / §24.5; per-port keying — Phase 28) ---

    #[test]
    fn virtual_port_upsert_then_get_returns_port() {
        let db = db();
        assert!(db
            .get_ssg_virtual_port(P, "postgres", 5432)
            .unwrap()
            .is_none());

        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();

        let got = db.get_ssg_virtual_port(P, "postgres", 5432).unwrap();
        assert_eq!(got, Some(42001));
    }

    #[test]
    fn virtual_port_upsert_replaces_by_project_service_and_container_port() {
        let db = db();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42050)
            .unwrap();

        assert_eq!(
            db.get_ssg_virtual_port(P, "postgres", 5432).unwrap(),
            Some(42050)
        );
        // Only one row for this key.
        assert_eq!(db.list_ssg_virtual_ports(P).unwrap().len(), 1);
    }

    #[test]
    fn virtual_port_get_returns_none_when_unset() {
        let db = db();
        assert!(db
            .get_ssg_virtual_port(P, "never-allocated", 1234)
            .unwrap()
            .is_none());
    }

    #[test]
    fn virtual_port_list_returns_all_rows_for_project_sorted() {
        let db = db();
        db.upsert_ssg_virtual_port(P, "redis", 6379, 42002).unwrap();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port(P, "memcached", 11211, 42003)
            .unwrap();

        let rows = db.list_ssg_virtual_ports(P).unwrap();
        let names: Vec<_> = rows.iter().map(|r| r.service_name.as_str()).collect();
        assert_eq!(names, vec!["memcached", "postgres", "redis"]);
    }

    #[test]
    fn virtual_port_list_is_project_scoped() {
        let db = db();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port(Q, "postgres", 5432, 42100)
            .unwrap();

        let p_rows = db.list_ssg_virtual_ports(P).unwrap();
        assert_eq!(p_rows.len(), 1);
        assert_eq!(p_rows[0].project, P);
        assert_eq!(p_rows[0].port, 42001);

        let q_rows = db.list_ssg_virtual_ports(Q).unwrap();
        assert_eq!(q_rows.len(), 1);
        assert_eq!(q_rows[0].project, Q);
        assert_eq!(q_rows[0].port, 42100);
    }

    #[test]
    fn virtual_port_clear_drops_only_target_project() {
        let db = db();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port(P, "redis", 6379, 42002).unwrap();
        db.upsert_ssg_virtual_port(Q, "postgres", 5432, 42100)
            .unwrap();

        db.clear_ssg_virtual_ports(P).unwrap();

        assert!(db.list_ssg_virtual_ports(P).unwrap().is_empty());
        assert_eq!(db.list_ssg_virtual_ports(Q).unwrap().len(), 1);
    }

    #[test]
    fn virtual_port_clear_is_idempotent_on_empty_table() {
        let db = db();
        db.clear_ssg_virtual_ports(P).unwrap();
        db.clear_ssg_virtual_ports(P).unwrap();
        assert!(db.list_ssg_virtual_ports(P).unwrap().is_empty());
    }

    #[test]
    fn virtual_port_distinct_services_within_project_store_separately() {
        let db = db();
        db.upsert_ssg_virtual_port(P, "postgres", 5432, 42001)
            .unwrap();
        db.upsert_ssg_virtual_port(P, "redis", 6379, 42002).unwrap();

        assert_eq!(
            db.get_ssg_virtual_port(P, "postgres", 5432).unwrap(),
            Some(42001)
        );
        assert_eq!(
            db.get_ssg_virtual_port(P, "redis", 6379).unwrap(),
            Some(42002)
        );
        assert_eq!(db.list_ssg_virtual_ports(P).unwrap().len(), 2);
    }

    #[test]
    fn virtual_port_distinct_container_ports_within_one_service_store_separately() {
        // Phase 28: a single service can declare multiple container
        // ports (e.g. minio's 9000 + 9001). Each gets its own
        // virtual-port row keyed by container_port.
        let db = db();
        db.upsert_ssg_virtual_port(P, "minio", 9000, 42010).unwrap();
        db.upsert_ssg_virtual_port(P, "minio", 9001, 42011).unwrap();

        assert_eq!(
            db.get_ssg_virtual_port(P, "minio", 9000).unwrap(),
            Some(42010)
        );
        assert_eq!(
            db.get_ssg_virtual_port(P, "minio", 9001).unwrap(),
            Some(42011)
        );
        let rows = db.list_ssg_virtual_ports(P).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted (service_name, container_port) ascending.
        assert_eq!(rows[0].container_port, 9000);
        assert_eq!(rows[1].container_port, 9001);
    }

    #[test]
    fn virtual_port_clear_one_drops_only_target_row() {
        // Phase 28 collision-rebind path: clearing one row leaves
        // sibling ports for the same service intact.
        let db = db();
        db.upsert_ssg_virtual_port(P, "minio", 9000, 42010).unwrap();
        db.upsert_ssg_virtual_port(P, "minio", 9001, 42011).unwrap();

        db.clear_ssg_virtual_port_one(P, "minio", 9000).unwrap();

        assert!(db.get_ssg_virtual_port(P, "minio", 9000).unwrap().is_none());
        assert_eq!(
            db.get_ssg_virtual_port(P, "minio", 9001).unwrap(),
            Some(42011)
        );
        // Idempotent.
        db.clear_ssg_virtual_port_one(P, "minio", 9000).unwrap();
    }
}
