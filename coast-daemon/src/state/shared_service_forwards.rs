//! Per-forward reverse-tunnel state for remote coasts.
//!
//! Phase 18: every reverse tunnel on a shared remote VM is bound at a
//! dynamic `remote_port` so concurrent consumer coasts on one VM cannot
//! collide on a canonical port. This table persists the allocation so
//! daemon-restart recovery re-opens the same tunnels.
//!
//! A row is written before the `ssh -R` child is spawned, and deleted by
//! `handle_stop` / `handle_rm` when the tunnel is torn down. Daemon
//! restart reads the rows to drive `reverse_forward_ports` with the same
//! `(remote_port, local_port)` pairs the previous session used.
//!
//! Scope: only remote shared-service reverse tunnels. Local shared
//! services do not use this table (they're tracked via
//! `shared_services` plus in-DinD socat PIDs written to
//! `/var/run/coast/shared-service-proxies/`).

use rusqlite::params;
use tracing::instrument;

use coast_core::error::{CoastError, Result};

use super::StateDb;

fn forward_row_err(e: rusqlite::Error) -> CoastError {
    CoastError::State {
        message: format!("failed to decode shared_service_forwards row: {e}"),
        source: Some(Box::new(e)),
    }
}

/// One (canonical, local, remote) tuple persisted for an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedServiceForwardRecord {
    pub project: String,
    pub instance: String,
    pub service_name: String,
    /// Canonical port the consumer app dials.
    pub port: u16,
    /// Local end of the reverse tunnel. For inline shared services this
    /// equals `port`; for SSG-backed services it is the SSG's dynamic
    /// host port.
    pub local_port: u16,
    /// Dynamic port on the remote VM that sshd binds via `ssh -R`.
    pub remote_port: u16,
}

impl StateDb {
    /// Upsert a forward record. Uses `(project, instance, service_name, port)`
    /// as the primary key so re-running `setup_shared_service_tunnels`
    /// on the same instance rewrites the row rather than erroring.
    #[instrument(skip(self))]
    pub fn upsert_shared_service_forward(&self, record: &SharedServiceForwardRecord) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO shared_service_forwards
                     (project, instance, service_name, port, local_port, remote_port)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project, instance, service_name, port)
                 DO UPDATE SET local_port = excluded.local_port,
                               remote_port = excluded.remote_port",
                params![
                    record.project,
                    record.instance,
                    record.service_name,
                    i64::from(record.port),
                    i64::from(record.local_port),
                    i64::from(record.remote_port),
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to upsert shared_service_forwards row for '{}/{}::{}:{}': {e}",
                    record.project, record.instance, record.service_name, record.port
                ),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// List all forward records for a given `(project, instance)`.
    #[instrument(skip(self))]
    pub fn list_shared_service_forwards_for_instance(
        &self,
        project: &str,
        instance: &str,
    ) -> Result<Vec<SharedServiceForwardRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, instance, service_name, port, local_port, remote_port
                 FROM shared_service_forwards
                 WHERE project = ?1 AND instance = ?2
                 ORDER BY service_name ASC, port ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare shared_service_forwards query: {e}"),
                source: Some(Box::new(e)),
            })?;
        let rows = stmt
            .query_map(params![project, instance], |row| {
                Ok(SharedServiceForwardRecord {
                    project: row.get(0)?,
                    instance: row.get(1)?,
                    service_name: row.get(2)?,
                    port: u16::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    local_port: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    remote_port: u16::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to iterate shared_service_forwards rows: {e}"),
                source: Some(Box::new(e)),
            })?;
        rows.map(|row| row.map_err(forward_row_err)).collect()
    }

    /// Delete every forward record for a given `(project, instance)`.
    /// Called from `handle_stop` and `handle_rm` after the tunnels are
    /// torn down.
    #[instrument(skip(self))]
    pub fn delete_shared_service_forwards_for_instance(
        &self,
        project: &str,
        instance: &str,
    ) -> Result<usize> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM shared_service_forwards
                 WHERE project = ?1 AND instance = ?2",
                params![project, instance],
            )
            .map_err(|e| CoastError::State {
                message: format!(
                    "failed to delete shared_service_forwards for '{project}/{instance}': {e}"
                ),
                source: Some(Box::new(e)),
            })?;
        Ok(deleted)
    }

    /// List every forward record across all instances. Used by
    /// `restore_tunnels_for_instance` on daemon restart.
    #[instrument(skip(self))]
    pub fn list_all_shared_service_forwards(&self) -> Result<Vec<SharedServiceForwardRecord>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project, instance, service_name, port, local_port, remote_port
                 FROM shared_service_forwards
                 ORDER BY project ASC, instance ASC, service_name ASC, port ASC",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare shared_service_forwards query: {e}"),
                source: Some(Box::new(e)),
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SharedServiceForwardRecord {
                    project: row.get(0)?,
                    instance: row.get(1)?,
                    service_name: row.get(2)?,
                    port: u16::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    local_port: u16::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
                    remote_port: u16::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to iterate shared_service_forwards rows: {e}"),
                source: Some(Box::new(e)),
            })?;
        rows.map(|row| row.map_err(forward_row_err)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    fn fresh_db() -> StateDb {
        let db = StateDb::open_in_memory().unwrap();
        // Seed two instances so the FK on shared_service_forwards is
        // satisfied. Every test in this module writes rows for either
        // "i1" or "i2" under project "p".
        for name in ["i1", "i2"] {
            db.insert_instance(&CoastInstance {
                name: name.to_string(),
                project: "p".to_string(),
                status: InstanceStatus::Running,
                branch: None,
                commit_sha: None,
                container_id: None,
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_host: None,
            })
            .unwrap();
        }
        db
    }

    /// Phase 24 harness: seed two projects that each own a
    /// same-named instance `dev-1`, pointed at the same remote host.
    /// Emulates the contract that `restore_shared_service_tunnels`
    /// now has to honor: every `(project, instance)` on a shared
    /// remote owns its own forward rows.
    fn fresh_multi_project_db() -> StateDb {
        let db = StateDb::open_in_memory().unwrap();
        for project in ["proj-a", "proj-b"] {
            db.insert_instance(&CoastInstance {
                name: "dev-1".to_string(),
                project: project.to_string(),
                status: InstanceStatus::Running,
                branch: None,
                commit_sha: None,
                container_id: None,
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_host: Some("shared-vm".to_string()),
            })
            .unwrap();
        }
        db
    }

    fn sample_record(instance: &str, service: &str, port: u16) -> SharedServiceForwardRecord {
        // Stay well within u16 when computing the derived ports so
        // tests with canonical ports like 6379 don't overflow.
        SharedServiceForwardRecord {
            project: "p".to_string(),
            instance: instance.to_string(),
            service_name: service.to_string(),
            port,
            local_port: 50000 + (port % 1000),
            remote_port: 55000 + (port % 1000),
        }
    }

    #[test]
    fn test_upsert_and_list_single_forward() {
        let db = fresh_db();
        let record = sample_record("i1", "postgres", 5432);
        db.upsert_shared_service_forward(&record).unwrap();

        let rows = db
            .list_shared_service_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(rows, vec![record]);
    }

    #[test]
    fn test_upsert_replaces_existing_on_conflict() {
        let db = fresh_db();
        let mut record = sample_record("i1", "postgres", 5432);
        db.upsert_shared_service_forward(&record).unwrap();

        record.remote_port = 63000;
        record.local_port = 53000;
        db.upsert_shared_service_forward(&record).unwrap();

        let rows = db
            .list_shared_service_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].remote_port, 63000);
        assert_eq!(rows[0].local_port, 53000);
    }

    #[test]
    fn test_list_scopes_to_instance() {
        let db = fresh_db();
        db.upsert_shared_service_forward(&sample_record("i1", "postgres", 5432))
            .unwrap();
        db.upsert_shared_service_forward(&sample_record("i2", "postgres", 5432))
            .unwrap();
        db.upsert_shared_service_forward(&sample_record("i1", "redis", 6379))
            .unwrap();

        let i1 = db
            .list_shared_service_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(i1.len(), 2);
        let services: Vec<_> = i1.iter().map(|r| r.service_name.as_str()).collect();
        assert_eq!(services, vec!["postgres", "redis"]);

        let i2 = db
            .list_shared_service_forwards_for_instance("p", "i2")
            .unwrap();
        assert_eq!(i2.len(), 1);
    }

    #[test]
    fn test_delete_scopes_to_instance() {
        let db = fresh_db();
        db.upsert_shared_service_forward(&sample_record("i1", "postgres", 5432))
            .unwrap();
        db.upsert_shared_service_forward(&sample_record("i2", "postgres", 5432))
            .unwrap();

        let deleted = db
            .delete_shared_service_forwards_for_instance("p", "i1")
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(db
            .list_shared_service_forwards_for_instance("p", "i1")
            .unwrap()
            .is_empty());
        assert_eq!(
            db.list_shared_service_forwards_for_instance("p", "i2")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn test_delete_noop_when_no_rows() {
        let db = fresh_db();
        let deleted = db
            .delete_shared_service_forwards_for_instance("p", "does-not-exist")
            .unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn test_list_all_returns_every_row_ordered() {
        let db = fresh_db();
        db.upsert_shared_service_forward(&sample_record("i2", "redis", 6379))
            .unwrap();
        db.upsert_shared_service_forward(&sample_record("i1", "postgres", 5432))
            .unwrap();
        let all = db.list_all_shared_service_forwards().unwrap();
        assert_eq!(all.len(), 2);
        // ORDER BY project, instance, service_name, port:
        assert_eq!(all[0].instance, "i1");
        assert_eq!(all[1].instance, "i2");
    }

    #[test]
    fn test_list_empty_when_no_rows() {
        let db = fresh_db();
        assert!(db
            .list_shared_service_forwards_for_instance("p", "i1")
            .unwrap()
            .is_empty());
    }

    // --- Phase 24: multi-project, shared-remote lookups ---

    #[test]
    fn multi_project_same_remote_lookups_scope_to_project() {
        // Phase 24: two projects each have a `dev-1` instance on the
        // same remote VM and each declares a `postgres:5432` forward.
        // `restore_tunnels_for_instance` MUST retrieve each project's
        // own pair without cross-contamination — this is what replaces
        // the old `restored_hosts` host-keyed skip.
        let db = fresh_multi_project_db();
        let rec_a = SharedServiceForwardRecord {
            project: "proj-a".to_string(),
            instance: "dev-1".to_string(),
            service_name: "postgres".to_string(),
            port: 5432,
            local_port: 60001,
            remote_port: 55001,
        };
        let rec_b = SharedServiceForwardRecord {
            project: "proj-b".to_string(),
            instance: "dev-1".to_string(),
            service_name: "postgres".to_string(),
            port: 5432,
            local_port: 60002,
            remote_port: 55002,
        };
        db.upsert_shared_service_forward(&rec_a).unwrap();
        db.upsert_shared_service_forward(&rec_b).unwrap();

        // Same `(instance, service, port)` across projects must not
        // collide on the composite PK.
        let a = db
            .list_shared_service_forwards_for_instance("proj-a", "dev-1")
            .unwrap();
        let b = db
            .list_shared_service_forwards_for_instance("proj-b", "dev-1")
            .unwrap();
        assert_eq!(a, vec![rec_a.clone()]);
        assert_eq!(b, vec![rec_b.clone()]);

        // Each project's local_port is its own SSG dynamic port, so
        // the reverse pairs are distinct (no leak across projects
        // even though the remote VM, instance name, and canonical
        // service port are all identical).
        let pairs_a: Vec<(u16, u16)> = a.iter().map(|r| (r.remote_port, r.local_port)).collect();
        let pairs_b: Vec<(u16, u16)> = b.iter().map(|r| (r.remote_port, r.local_port)).collect();
        assert_eq!(pairs_a, vec![(55001, 60001)]);
        assert_eq!(pairs_b, vec![(55002, 60002)]);

        // `list_all` returns every row so the daemon-restart walker
        // can iterate them without a project-aware caller.
        let all = db.list_all_shared_service_forwards().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn multi_project_same_remote_delete_is_project_scoped() {
        // Removing proj-a's `dev-1` forwards leaves proj-b's `dev-1`
        // untouched — Phase 24 invariant for per-project teardown.
        let db = fresh_multi_project_db();
        db.upsert_shared_service_forward(&SharedServiceForwardRecord {
            project: "proj-a".to_string(),
            instance: "dev-1".to_string(),
            service_name: "postgres".to_string(),
            port: 5432,
            local_port: 60001,
            remote_port: 55001,
        })
        .unwrap();
        db.upsert_shared_service_forward(&SharedServiceForwardRecord {
            project: "proj-b".to_string(),
            instance: "dev-1".to_string(),
            service_name: "postgres".to_string(),
            port: 5432,
            local_port: 60002,
            remote_port: 55002,
        })
        .unwrap();

        let deleted = db
            .delete_shared_service_forwards_for_instance("proj-a", "dev-1")
            .unwrap();
        assert_eq!(deleted, 1);
        assert!(db
            .list_shared_service_forwards_for_instance("proj-a", "dev-1")
            .unwrap()
            .is_empty());
        assert_eq!(
            db.list_shared_service_forwards_for_instance("proj-b", "dev-1")
                .unwrap()
                .len(),
            1
        );
    }
}
