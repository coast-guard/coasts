use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use tracing::{debug, instrument};

use coast_core::error::{CoastError, Result};
use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

use super::{is_unique_violation, StateDb};

/// Convert a SQLite row into a `CoastInstance`.
///
/// Column order: name, project, status, branch, commit_sha, container_id, runtime, created_at, worktree_name, build_id, coastfile_type, remote_name
pub(super) fn row_to_instance(row: &rusqlite::Row<'_>) -> rusqlite::Result<CoastInstance> {
    let status_str: String = row.get(2)?;
    let runtime_str: String = row.get(6)?;
    let created_at_str: String = row.get(7)?;

    let status = InstanceStatus::from_db_str(&status_str).unwrap_or(InstanceStatus::Stopped);
    let runtime = RuntimeType::from_str_value(&runtime_str).unwrap_or(RuntimeType::Dind);
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let worktree_name: Option<String> = row.get(8).unwrap_or(None);
    let build_id: Option<String> = row.get(9).unwrap_or(None);
    let coastfile_type: Option<String> = row.get(10).unwrap_or(None);
    let remote_name: Option<String> = row.get(11).unwrap_or(None);

    Ok(CoastInstance {
        name: row.get(0)?,
        project: row.get(1)?,
        status,
        branch: row.get(3)?,
        commit_sha: row.get(4)?,
        container_id: row.get(5)?,
        runtime,
        created_at,
        worktree_name,
        build_id,
        coastfile_type,
        remote_name,
    })
}

impl StateDb {
    /// Insert a new coast instance into the database.
    ///
    /// Returns an error if an instance with the same (project, name) already exists.
    #[instrument(skip(self), fields(project = %instance.project, name = %instance.name))]
    pub fn insert_instance(&self, instance: &CoastInstance) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO instances (name, project, status, branch, commit_sha, container_id, runtime, created_at, worktree_name, build_id, coastfile_type, remote_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    instance.name,
                    instance.project,
                    instance.status.as_db_str(),
                    instance.branch,
                    instance.commit_sha,
                    instance.container_id,
                    instance.runtime.as_str(),
                    instance.created_at.to_rfc3339(),
                    instance.worktree_name,
                    instance.build_id,
                    instance.coastfile_type,
                    instance.remote_name,
                ],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    CoastError::InstanceAlreadyExists {
                        name: instance.name.clone(),
                        project: instance.project.clone(),
                    }
                } else {
                    CoastError::State {
                        message: format!("failed to insert instance '{}': {e}", instance.name),
                        source: Some(Box::new(e)),
                    }
                }
            })?;

        debug!("inserted instance");
        Ok(())
    }

    /// Get a single coast instance by project and name.
    ///
    /// Returns `None` if no matching instance exists.
    #[instrument(skip(self))]
    pub fn get_instance(&self, project: &str, name: &str) -> Result<Option<CoastInstance>> {
        self.conn
            .query_row(
                "SELECT name, project, status, branch, commit_sha, container_id, runtime, created_at, worktree_name, build_id, coastfile_type, remote_name
                 FROM instances
                 WHERE project = ?1 AND name = ?2",
                params![project, name],
                row_to_instance,
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query instance '{name}' in project '{project}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// List all coast instances across all projects.
    #[instrument(skip(self))]
    pub fn list_instances(&self) -> Result<Vec<CoastInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, project, status, branch, commit_sha, container_id, runtime, created_at, worktree_name, build_id, coastfile_type, remote_name
                 FROM instances
                 ORDER BY project, name",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare list instances query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], row_to_instance)
            .map_err(|e| CoastError::State {
                message: format!("failed to list instances: {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut instances = Vec::new();
        for row in rows {
            instances.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read instance row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(instances)
    }

    /// List all coast instances for a specific project.
    #[instrument(skip(self))]
    pub fn list_instances_for_project(&self, project: &str) -> Result<Vec<CoastInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, project, status, branch, commit_sha, container_id, runtime, created_at, worktree_name, build_id, coastfile_type, remote_name
                 FROM instances
                 WHERE project = ?1
                 ORDER BY name",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare list instances for project query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![project], row_to_instance)
            .map_err(|e| CoastError::State {
                message: format!("failed to list instances for project '{project}': {e}"),
                source: Some(Box::new(e)),
            })?;

        let mut instances = Vec::new();
        for row in rows {
            instances.push(row.map_err(|e| CoastError::State {
                message: format!("failed to read instance row: {e}"),
                source: Some(Box::new(e)),
            })?);
        }

        Ok(instances)
    }

    /// Update the status of an existing instance.
    ///
    /// Returns an error if the instance does not exist.
    #[instrument(skip(self))]
    pub fn update_instance_status(
        &self,
        project: &str,
        name: &str,
        status: &InstanceStatus,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE instances SET status = ?1 WHERE project = ?2 AND name = ?3",
                params![status.as_db_str(), project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update status for instance '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::InstanceNotFound {
                name: name.to_string(),
                project: project.to_string(),
            });
        }

        debug!("updated instance status to {}", status.as_db_str());
        Ok(())
    }

    /// Set the worktree name for an instance (None = project root).
    #[instrument(skip(self))]
    pub fn set_worktree(
        &self,
        project: &str,
        name: &str,
        worktree_name: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET worktree_name = ?1 WHERE project = ?2 AND name = ?3",
                params![worktree_name, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set worktree for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Update the build_id for an existing instance (used for backfilling pre-migration instances).
    #[instrument(skip(self))]
    pub fn set_build_id(&self, project: &str, name: &str, build_id: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET build_id = ?1 WHERE project = ?2 AND name = ?3",
                params![build_id, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set build_id for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Update the coastfile_type for an existing instance.
    #[instrument(skip(self))]
    pub fn set_coastfile_type(
        &self,
        project: &str,
        name: &str,
        coastfile_type: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET coastfile_type = ?1 WHERE project = ?2 AND name = ?3",
                params![coastfile_type, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set coastfile_type for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Update the branch for an existing instance.
    #[instrument(skip(self))]
    pub fn set_branch(&self, project: &str, name: &str, branch: Option<&str>) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET branch = ?1 WHERE project = ?2 AND name = ?3",
                params![branch, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set branch for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Set the remote name for an instance (None = local).
    ///
    /// This tracks which remote VM an instance is running on.
    #[instrument(skip(self))]
    pub fn set_remote_name(
        &self,
        project: &str,
        name: &str,
        remote_name: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET remote_name = ?1 WHERE project = ?2 AND name = ?3",
                params![remote_name, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to set remote_name for '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    /// Update the Docker container ID for an existing instance.
    ///
    /// Returns an error if the instance does not exist.
    #[instrument(skip(self))]
    pub fn update_instance_container_id(
        &self,
        project: &str,
        name: &str,
        container_id: Option<&str>,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE instances SET container_id = ?1 WHERE project = ?2 AND name = ?3",
                params![container_id, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update container_id for instance '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::InstanceNotFound {
                name: name.to_string(),
                project: project.to_string(),
            });
        }

        debug!("updated instance container_id");
        Ok(())
    }

    /// Update the branch and status of an existing instance atomically.
    ///
    /// Used by `coast assign` to reassign a branch to a slot. Updates both
    /// the branch field and the status (typically to Running) in a single statement.
    ///
    /// Returns an error if the instance does not exist.
    #[instrument(skip(self))]
    pub fn update_instance_branch(
        &self,
        project: &str,
        name: &str,
        branch: Option<&str>,
        commit_sha: Option<&str>,
        status: &InstanceStatus,
    ) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "UPDATE instances SET branch = ?1, commit_sha = ?2, status = ?3 WHERE project = ?4 AND name = ?5",
                params![branch, commit_sha, status.as_db_str(), project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update branch for instance '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::InstanceNotFound {
                name: name.to_string(),
                project: project.to_string(),
            });
        }

        debug!(
            branch = ?branch,
            status = %status.as_db_str(),
            "updated instance branch and status"
        );
        Ok(())
    }

    /// Delete an instance and its associated port allocations (via CASCADE).
    ///
    /// Returns an error if the instance does not exist.
    #[instrument(skip(self))]
    pub fn delete_instance(&self, project: &str, name: &str) -> Result<()> {
        let rows = self
            .conn
            .execute(
                "DELETE FROM instances WHERE project = ?1 AND name = ?2",
                params![project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete instance '{name}': {e}"),
                source: Some(Box::new(e)),
            })?;

        if rows == 0 {
            return Err(CoastError::InstanceNotFound {
                name: name.to_string(),
                project: project.to_string(),
            });
        }

        debug!("deleted instance");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use chrono::Utc;
    use coast_core::error::CoastError;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    #[test]
    fn test_insert_and_get_instance() {
        let db = test_db();
        let instance = sample_instance("feature-oauth", "my-app");

        db.insert_instance(&instance).unwrap();
        let retrieved = db.get_instance("my-app", "feature-oauth").unwrap().unwrap();

        assert_eq!(retrieved.name, "feature-oauth");
        assert_eq!(retrieved.project, "my-app");
        assert_eq!(retrieved.status, InstanceStatus::Running);
        assert_eq!(retrieved.branch, Some("main".to_string()));
        assert_eq!(
            retrieved.container_id,
            Some("container-feature-oauth".to_string())
        );
        assert_eq!(retrieved.runtime, RuntimeType::Dind);
    }

    #[test]
    fn test_insert_instance_with_null_optionals() {
        let db = test_db();
        let instance = CoastInstance {
            name: "minimal".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Stopped,
            branch: None,
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Sysbox,
            created_at: Utc::now(),
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        };

        db.insert_instance(&instance).unwrap();
        let retrieved = db.get_instance("proj", "minimal").unwrap().unwrap();

        assert_eq!(retrieved.name, "minimal");
        assert!(retrieved.branch.is_none());
        assert!(retrieved.container_id.is_none());
        assert_eq!(retrieved.runtime, RuntimeType::Sysbox);
    }

    #[test]
    fn test_insert_duplicate_instance_returns_error() {
        let db = test_db();
        let instance = sample_instance("feature-x", "my-app");

        db.insert_instance(&instance).unwrap();
        let result = db.insert_instance(&instance);

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            CoastError::InstanceAlreadyExists { name, project } => {
                assert_eq!(name, "feature-x");
                assert_eq!(project, "my-app");
            }
            other => panic!("expected InstanceAlreadyExists, got: {other:?}"),
        }
    }

    #[test]
    fn test_get_nonexistent_instance_returns_none() {
        let db = test_db();
        let result = db.get_instance("no-project", "no-instance").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_instances_empty() {
        let db = test_db();
        let instances = db.list_instances().unwrap();
        assert!(instances.is_empty());
    }

    #[test]
    fn test_list_instances_multiple() {
        let db = test_db();
        db.insert_instance(&sample_instance("alpha", "proj-a"))
            .unwrap();
        db.insert_instance(&sample_instance("beta", "proj-a"))
            .unwrap();
        db.insert_instance(&sample_instance("gamma", "proj-b"))
            .unwrap();

        let all = db.list_instances().unwrap();
        assert_eq!(all.len(), 3);

        // Should be ordered by project, then name
        assert_eq!(all[0].project, "proj-a");
        assert_eq!(all[0].name, "alpha");
        assert_eq!(all[1].project, "proj-a");
        assert_eq!(all[1].name, "beta");
        assert_eq!(all[2].project, "proj-b");
        assert_eq!(all[2].name, "gamma");
    }

    #[test]
    fn test_list_instances_for_project() {
        let db = test_db();
        db.insert_instance(&sample_instance("alpha", "proj-a"))
            .unwrap();
        db.insert_instance(&sample_instance("beta", "proj-a"))
            .unwrap();
        db.insert_instance(&sample_instance("gamma", "proj-b"))
            .unwrap();

        let proj_a = db.list_instances_for_project("proj-a").unwrap();
        assert_eq!(proj_a.len(), 2);
        assert_eq!(proj_a[0].name, "alpha");
        assert_eq!(proj_a[1].name, "beta");

        let proj_b = db.list_instances_for_project("proj-b").unwrap();
        assert_eq!(proj_b.len(), 1);
        assert_eq!(proj_b[0].name, "gamma");

        let proj_c = db.list_instances_for_project("proj-c").unwrap();
        assert!(proj_c.is_empty());
    }

    #[test]
    fn test_update_instance_status() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        // Running -> CheckedOut
        db.update_instance_status("proj", "inst", &InstanceStatus::CheckedOut)
            .unwrap();
        let inst = db.get_instance("proj", "inst").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::CheckedOut);

        // CheckedOut -> Stopped
        db.update_instance_status("proj", "inst", &InstanceStatus::Stopped)
            .unwrap();
        let inst = db.get_instance("proj", "inst").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Stopped);

        // Stopped -> Running
        db.update_instance_status("proj", "inst", &InstanceStatus::Running)
            .unwrap();
        let inst = db.get_instance("proj", "inst").unwrap().unwrap();
        assert_eq!(inst.status, InstanceStatus::Running);
    }

    #[test]
    fn test_update_status_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_instance_status("proj", "ghost", &InstanceStatus::Running);
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::InstanceNotFound { name, project } => {
                assert_eq!(name, "ghost");
                assert_eq!(project, "proj");
            }
            other => panic!("expected InstanceNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_update_instance_branch() {
        let db = test_db();
        db.insert_instance(&sample_instance("dev-1", "proj"))
            .unwrap();

        // Assign a branch and set to Running
        db.update_instance_branch(
            "proj",
            "dev-1",
            Some("feature/oauth"),
            Some("abc123"),
            &InstanceStatus::Running,
        )
        .unwrap();
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.branch, Some("feature/oauth".to_string()));
        assert_eq!(inst.commit_sha, Some("abc123".to_string()));
        assert_eq!(inst.status, InstanceStatus::Running);

        // Reassign to a different branch
        db.update_instance_branch(
            "proj",
            "dev-1",
            Some("feature/billing"),
            Some("def456"),
            &InstanceStatus::Running,
        )
        .unwrap();
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert_eq!(inst.branch, Some("feature/billing".to_string()));
        assert_eq!(inst.commit_sha, Some("def456".to_string()));
        assert_eq!(inst.status, InstanceStatus::Running);

        // Clear branch and set to Idle
        db.update_instance_branch("proj", "dev-1", None, None, &InstanceStatus::Idle)
            .unwrap();
        let inst = db.get_instance("proj", "dev-1").unwrap().unwrap();
        assert!(inst.branch.is_none());
        assert!(inst.commit_sha.is_none());
        assert_eq!(inst.status, InstanceStatus::Idle);
    }

    #[test]
    fn test_update_branch_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_instance_branch(
            "proj",
            "ghost",
            Some("main"),
            None,
            &InstanceStatus::Running,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::InstanceNotFound { name, project } => {
                assert_eq!(name, "ghost");
                assert_eq!(project, "proj");
            }
            other => panic!("expected InstanceNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_update_instance_container_id() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        db.update_instance_container_id("proj", "inst", Some("new-container-123"))
            .unwrap();
        let inst = db.get_instance("proj", "inst").unwrap().unwrap();
        assert_eq!(inst.container_id, Some("new-container-123".to_string()));

        // Clear the container ID
        db.update_instance_container_id("proj", "inst", None)
            .unwrap();
        let inst = db.get_instance("proj", "inst").unwrap().unwrap();
        assert!(inst.container_id.is_none());
    }

    #[test]
    fn test_update_container_id_nonexistent_returns_error() {
        let db = test_db();
        let result = db.update_instance_container_id("proj", "ghost", Some("abc"));
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::InstanceNotFound { name, project } => {
                assert_eq!(name, "ghost");
                assert_eq!(project, "proj");
            }
            other => panic!("expected InstanceNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_delete_instance() {
        let db = test_db();
        db.insert_instance(&sample_instance("inst", "proj"))
            .unwrap();

        db.delete_instance("proj", "inst").unwrap();
        let result = db.get_instance("proj", "inst").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_nonexistent_instance_returns_error() {
        let db = test_db();
        let result = db.delete_instance("proj", "ghost");
        assert!(result.is_err());
        match result.unwrap_err() {
            CoastError::InstanceNotFound { name, project } => {
                assert_eq!(name, "ghost");
                assert_eq!(project, "proj");
            }
            other => panic!("expected InstanceNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_instance_created_at_roundtrip() {
        let db = test_db();
        let now = Utc::now();
        let instance = CoastInstance {
            name: "ts-test".to_string(),
            project: "proj".to_string(),
            status: InstanceStatus::Running,
            branch: None,
            commit_sha: None,
            container_id: None,
            runtime: RuntimeType::Dind,
            created_at: now,
            worktree_name: None,
            build_id: None,
            coastfile_type: None,
            remote_name: None,
        };

        db.insert_instance(&instance).unwrap();
        let retrieved = db.get_instance("proj", "ts-test").unwrap().unwrap();

        // RFC3339 round-trip preserves second-level precision.
        // Sub-nanosecond precision may be lost, so compare at second granularity.
        assert_eq!(
            retrieved.created_at.timestamp(),
            now.timestamp(),
            "created_at timestamp should round-trip through the database"
        );
    }

    #[test]
    fn test_all_runtime_types_roundtrip() {
        let db = test_db();

        for (i, runtime) in [RuntimeType::Dind, RuntimeType::Sysbox, RuntimeType::Podman]
            .into_iter()
            .enumerate()
        {
            let name = format!("inst-{i}");
            let instance = CoastInstance {
                name: name.clone(),
                project: "proj".to_string(),
                status: InstanceStatus::Running,
                branch: None,
                commit_sha: None,
                container_id: None,
                runtime: runtime.clone(),
                created_at: Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_name: None,
            };

            db.insert_instance(&instance).unwrap();
            let retrieved = db.get_instance("proj", &name).unwrap().unwrap();
            assert_eq!(retrieved.runtime, runtime);
        }
    }

    #[test]
    fn test_all_instance_statuses_roundtrip() {
        let db = test_db();

        for (i, status) in [
            InstanceStatus::Running,
            InstanceStatus::Stopped,
            InstanceStatus::CheckedOut,
        ]
        .into_iter()
        .enumerate()
        {
            let name = format!("inst-{i}");
            let instance = CoastInstance {
                name: name.clone(),
                project: "proj".to_string(),
                status: status.clone(),
                branch: None,
                commit_sha: None,
                container_id: None,
                runtime: RuntimeType::Dind,
                created_at: Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
                remote_name: None,
            };

            db.insert_instance(&instance).unwrap();
            let retrieved = db.get_instance("proj", &name).unwrap().unwrap();
            assert_eq!(retrieved.status, status);
        }
    }

    #[test]
    fn test_same_name_different_projects() {
        let db = test_db();
        db.insert_instance(&sample_instance("main", "proj-a"))
            .unwrap();
        db.insert_instance(&sample_instance("main", "proj-b"))
            .unwrap();

        let a = db.get_instance("proj-a", "main").unwrap().unwrap();
        let b = db.get_instance("proj-b", "main").unwrap().unwrap();

        assert_eq!(a.project, "proj-a");
        assert_eq!(b.project, "proj-b");
    }
}
