use rusqlite::params;

use coast_core::error::{CoastError, Result};

use super::ServiceDb;

/// Minimal instance record for the remote side.
#[derive(Debug, Clone)]
pub struct RemoteInstance {
    pub name: String,
    pub project: String,
    pub status: String,
    pub container_id: Option<String>,
    pub build_id: Option<String>,
    pub coastfile_type: Option<String>,
    pub worktree: Option<String>,
    pub created_at: String,
}

impl ServiceDb {
    pub fn insert_instance(&self, inst: &RemoteInstance) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO instances (name, project, status, container_id, build_id, coastfile_type, worktree, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    inst.name,
                    inst.project,
                    inst.status,
                    inst.container_id,
                    inst.build_id,
                    inst.coastfile_type,
                    inst.worktree,
                    inst.created_at,
                ],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to insert remote instance '{}': {e}", inst.name),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn get_instance(&self, project: &str, name: &str) -> Result<Option<RemoteInstance>> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row(
                "SELECT name, project, status, container_id, build_id, coastfile_type, worktree, created_at
                 FROM instances WHERE project = ?1 AND name = ?2",
                params![project, name],
                |row| {
                    Ok(RemoteInstance {
                        name: row.get(0)?,
                        project: row.get(1)?,
                        status: row.get(2)?,
                        container_id: row.get(3)?,
                        build_id: row.get(4)?,
                        coastfile_type: row.get(5)?,
                        worktree: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|e| CoastError::State {
                message: format!("failed to query remote instance '{name}': {e}"),
                source: Some(Box::new(e)),
            })
    }

    pub fn update_instance_status(&self, project: &str, name: &str, status: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET status = ?1 WHERE project = ?2 AND name = ?3",
                params![status, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update instance status: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn update_instance_container_id(
        &self,
        project: &str,
        name: &str,
        container_id: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET container_id = ?1 WHERE project = ?2 AND name = ?3",
                params![container_id, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update container_id: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn update_instance_worktree(
        &self,
        project: &str,
        name: &str,
        worktree: Option<&str>,
    ) -> Result<()> {
        self.conn
            .execute(
                "UPDATE instances SET worktree = ?1 WHERE project = ?2 AND name = ?3",
                params![worktree, project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to update worktree: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn delete_instance(&self, project: &str, name: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM instances WHERE project = ?1 AND name = ?2",
                params![project, name],
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to delete instance: {e}"),
                source: Some(Box::new(e)),
            })?;
        Ok(())
    }

    pub fn list_all_instances(&self) -> Result<Vec<RemoteInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, project, status, container_id, build_id, coastfile_type, worktree, created_at
                 FROM instances",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare list-all query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok(RemoteInstance {
                    name: row.get(0)?,
                    project: row.get(1)?,
                    status: row.get(2)?,
                    container_id: row.get(3)?,
                    build_id: row.get(4)?,
                    coastfile_type: row.get(5)?,
                    worktree: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| CoastError::State {
                message: format!("failed to list all instances: {e}"),
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

    pub fn list_instances(&self, project: &str) -> Result<Vec<RemoteInstance>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, project, status, container_id, build_id, coastfile_type, worktree, created_at
                 FROM instances WHERE project = ?1",
            )
            .map_err(|e| CoastError::State {
                message: format!("failed to prepare list query: {e}"),
                source: Some(Box::new(e)),
            })?;

        let rows = stmt
            .query_map(params![project], |row| {
                Ok(RemoteInstance {
                    name: row.get(0)?,
                    project: row.get(1)?,
                    status: row.get(2)?,
                    container_id: row.get(3)?,
                    build_id: row.get(4)?,
                    coastfile_type: row.get(5)?,
                    worktree: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServiceDb;

    fn test_db() -> ServiceDb {
        ServiceDb::open_in_memory().unwrap()
    }

    fn make_instance(name: &str, project: &str) -> RemoteInstance {
        RemoteInstance {
            name: name.to_string(),
            project: project.to_string(),
            status: "stopped".to_string(),
            container_id: None,
            build_id: None,
            coastfile_type: None,
            worktree: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_insert_and_get_instance() {
        let db = test_db();
        let inst = make_instance("web", "myapp");
        db.insert_instance(&inst).unwrap();

        let fetched = db.get_instance("myapp", "web").unwrap().unwrap();
        assert_eq!(fetched.name, "web");
        assert_eq!(fetched.project, "myapp");
        assert_eq!(fetched.status, "stopped");
        assert!(fetched.container_id.is_none());
        assert_eq!(fetched.created_at, "2024-01-01T00:00:00Z");
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let db = test_db();
        let result = db.get_instance("nope", "nope").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_insert_duplicate_errors() {
        let db = test_db();
        let inst = make_instance("dup", "proj");
        db.insert_instance(&inst).unwrap();
        let err = db.insert_instance(&inst).unwrap_err();
        assert!(err.to_string().contains("failed to insert"));
    }

    #[test]
    fn test_update_instance_status() {
        let db = test_db();
        db.insert_instance(&make_instance("s", "p")).unwrap();

        db.update_instance_status("p", "s", "running").unwrap();
        let inst = db.get_instance("p", "s").unwrap().unwrap();
        assert_eq!(inst.status, "running");

        db.update_instance_status("p", "s", "stopped").unwrap();
        let inst = db.get_instance("p", "s").unwrap().unwrap();
        assert_eq!(inst.status, "stopped");
    }

    #[test]
    fn test_update_instance_container_id() {
        let db = test_db();
        db.insert_instance(&make_instance("c", "p")).unwrap();

        db.update_instance_container_id("p", "c", Some("abc123"))
            .unwrap();
        let inst = db.get_instance("p", "c").unwrap().unwrap();
        assert_eq!(inst.container_id.as_deref(), Some("abc123"));

        db.update_instance_container_id("p", "c", None).unwrap();
        let inst = db.get_instance("p", "c").unwrap().unwrap();
        assert!(inst.container_id.is_none());
    }

    #[test]
    fn test_delete_instance() {
        let db = test_db();
        db.insert_instance(&make_instance("del", "p")).unwrap();

        db.delete_instance("p", "del").unwrap();
        assert!(db.get_instance("p", "del").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_is_ok() {
        let db = test_db();
        db.delete_instance("nope", "nope").unwrap();
    }

    #[test]
    fn test_list_instances_empty() {
        let db = test_db();
        let list = db.list_instances("proj").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_instances_multiple() {
        let db = test_db();
        db.insert_instance(&make_instance("a", "proj")).unwrap();
        db.insert_instance(&make_instance("b", "proj")).unwrap();
        db.insert_instance(&make_instance("c", "proj")).unwrap();

        let list = db.list_instances("proj").unwrap();
        assert_eq!(list.len(), 3);
        let names: Vec<&str> = list.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn test_list_instances_filters_by_project() {
        let db = test_db();
        db.insert_instance(&make_instance("x", "alpha")).unwrap();
        db.insert_instance(&make_instance("y", "alpha")).unwrap();
        db.insert_instance(&make_instance("z", "beta")).unwrap();

        let alpha = db.list_instances("alpha").unwrap();
        assert_eq!(alpha.len(), 2);

        let beta = db.list_instances("beta").unwrap();
        assert_eq!(beta.len(), 1);
        assert_eq!(beta[0].name, "z");

        let empty = db.list_instances("gamma").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_insert_with_all_fields() {
        let db = test_db();
        let inst = RemoteInstance {
            name: "full".to_string(),
            project: "p".to_string(),
            status: "running".to_string(),
            container_id: Some("cid-123".to_string()),
            build_id: Some("build-456".to_string()),
            coastfile_type: Some("default".to_string()),
            worktree: Some("feature-branch".to_string()),
            created_at: "2024-06-15T12:00:00Z".to_string(),
        };
        db.insert_instance(&inst).unwrap();

        let fetched = db.get_instance("p", "full").unwrap().unwrap();
        assert_eq!(fetched.container_id.as_deref(), Some("cid-123"));
        assert_eq!(fetched.build_id.as_deref(), Some("build-456"));
        assert_eq!(fetched.coastfile_type.as_deref(), Some("default"));
    }

    #[test]
    fn test_list_all_instances() {
        let db = test_db();
        db.insert_instance(&make_instance("a", "proj1")).unwrap();
        db.insert_instance(&make_instance("b", "proj1")).unwrap();
        db.insert_instance(&make_instance("c", "proj2")).unwrap();

        let all = db.list_all_instances().unwrap();
        assert_eq!(all.len(), 3);
        let names: Vec<&str> = all.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn test_list_all_instances_empty() {
        let db = test_db();
        let all = db.list_all_instances().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_same_name_different_projects() {
        let db = test_db();
        db.insert_instance(&make_instance("web", "app1")).unwrap();
        db.insert_instance(&make_instance("web", "app2")).unwrap();

        assert!(db.get_instance("app1", "web").unwrap().is_some());
        assert!(db.get_instance("app2", "web").unwrap().is_some());
    }
}
