//! Per-forward routing state for shared services consumed by remote
//! coasts. Phase 18 symmetric routing.
//!
//! Each row captures one `(name, canonical port)` forward on the remote
//! side of the reverse tunnel: the `remote_port` coast-daemon told
//! coast-service about in the `RunRequest`, plus the `alias_ip` that
//! `plan_shared_service_routing` allocated on the DinD's docker0 when
//! the coast was first provisioned.
//!
//! On `coast start` (after a stop), coast-service re-runs the
//! in-DinD socat setup using the persisted values so the consumer's
//! app still finds `postgres:5432` at the same alias IP, forwarding to
//! the same remote sshd port that coast-daemon is now rebinding.

use rusqlite::params;

use coast_core::error::{CoastError, Result};

use super::ServiceDb;

/// One (canonical, remote_port, alias_ip) triple persisted for a remote
/// instance's shared service forward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSharedForwardRecord {
    pub project: String,
    pub instance: String,
    pub service_name: String,
    /// Canonical container port the consumer app dials.
    pub port: u16,
    /// Dynamic port on this VM that sshd binds for the reverse tunnel.
    pub remote_port: u16,
    /// docker0 alias IP inside the consumer's remote DinD where the
    /// in-DinD socat listens for canonical traffic.
    pub alias_ip: String,
}

impl ServiceDb {
    pub fn upsert_remote_shared_forward(&self, record: &RemoteSharedForwardRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO remote_shared_forwards
                     (project, instance, service_name, port, remote_port, alias_ip)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project, instance, service_name, port)
                 DO UPDATE SET remote_port = excluded.remote_port,
                               alias_ip = excluded.alias_ip",
                params![
                    record.project,
                    record.instance,
                    record.service_name,
                    i64::from(record.port),
                    i64::from(record.remote_port),
                    record.alias_ip,
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to upsert remote_shared_forwards row for '{}/{}::{}:{}': {e}",
                    record.project, record.instance, record.service_name, record.port
                ),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn list_remote_shared_forwards_for_instance(
        &self,
        project: &str,
        instance: &str,
    ) -> Result<Vec<RemoteSharedForwardRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, instance, service_name, port, remote_port, alias_ip
                 FROM remote_shared_forwards
                 WHERE project = ?1 AND instance = ?2
                 ORDER BY service_name ASC, port ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare remote_shared_forwards query: {e}"),
                source: Some(Box::new(e)),
            })?;
        let rows = stmt
            .query_map(params![project, instance], |row| {
                Ok(RemoteSharedForwardRecord {
                    project: row.get(0)?,
                    instance: row.get(1)?,
                    service_name: row.get(2)?,
                    port: u16::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    remote_port: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    alias_ip: row.get(5)?,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to iterate remote_shared_forwards rows: {e}"),
                source: Some(Box::new(e)),
            })?;
        rows.map(|row| {
            row.map_err(|e| CoastError::State {
                message: format!("failed to decode remote_shared_forwards row: {e}"),
                source: Some(Box::new(e)),
            })
        })
        .collect()
    }

    pub fn delete_remote_shared_forwards_for_instance(
        &self,
        project: &str,
        instance: &str,
    ) -> Result<usize> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM remote_shared_forwards
                 WHERE project = ?1 AND instance = ?2",
                params![project, instance],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to delete remote_shared_forwards for '{project}/{instance}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::instances::RemoteInstance;

    fn fresh_db() -> ServiceDb {
        let db = ServiceDb::open_in_memory().unwrap();
        // Seed instances so the FK on remote_shared_forwards is satisfied.
        for name in ["i1", "i2"] {
            db.insert_instance(&RemoteInstance {
                name: name.to_string(),
                project: "p".to_string(),
                status: "running".to_string(),
                container_id: None,
                build_id: None,
                coastfile_type: None,
                worktree: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .unwrap();
        }
        db
    }

    fn sample(instance: &str, service: &str, port: u16) -> RemoteSharedForwardRecord {
        // Stay well within u16 when computing remote_port so canonical
        // ports like 6379 don't overflow.
        RemoteSharedForwardRecord {
            project: "p".to_string(),
            instance: instance.to_string(),
            service_name: service.to_string(),
            port,
            remote_port: 55000 + (port % 1000),
            alias_ip: format!("172.17.255.{}", 254_u16.saturating_sub(port % 10)),
        }
    }

    #[test]
    fn test_upsert_and_list_single_row() {
        let db = fresh_db();
        let rec = sample("i1", "postgres", 5432);
        db.upsert_remote_shared_forward(&rec).unwrap();

        let rows = db
            .list_remote_shared_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(rows, vec![rec]);
    }

    #[test]
    fn test_upsert_replaces_on_conflict() {
        let db = fresh_db();
        let mut rec = sample("i1", "postgres", 5432);
        db.upsert_remote_shared_forward(&rec).unwrap();

        rec.remote_port = 63000;
        rec.alias_ip = "10.0.0.254".to_string();
        db.upsert_remote_shared_forward(&rec).unwrap();

        let rows = db
            .list_remote_shared_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].remote_port, 63000);
        assert_eq!(rows[0].alias_ip, "10.0.0.254");
    }

    #[test]
    fn test_list_scopes_to_instance() {
        let db = fresh_db();
        db.upsert_remote_shared_forward(&sample("i1", "postgres", 5432))
            .unwrap();
        db.upsert_remote_shared_forward(&sample("i2", "postgres", 5432))
            .unwrap();
        db.upsert_remote_shared_forward(&sample("i1", "redis", 6379))
            .unwrap();

        let i1 = db
            .list_remote_shared_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(i1.len(), 2);
        let i2 = db
            .list_remote_shared_forwards_for_instance("p", "i2")
            .unwrap();
        assert_eq!(i2.len(), 1);
    }

    #[test]
    fn test_delete_scopes_to_instance() {
        let db = fresh_db();
        db.upsert_remote_shared_forward(&sample("i1", "postgres", 5432))
            .unwrap();
        db.upsert_remote_shared_forward(&sample("i2", "postgres", 5432))
            .unwrap();

        let deleted = db
            .delete_remote_shared_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(db
            .list_remote_shared_forwards_for_instance("p", "i1")
            .unwrap()
            .is_empty());
        assert_eq!(
            db.list_remote_shared_forwards_for_instance("p", "i2")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn test_list_empty_when_no_rows() {
        let db = fresh_db();
        assert!(db
            .list_remote_shared_forwards_for_instance("p", "i1")
            .unwrap()
            .is_empty());
    }
}
