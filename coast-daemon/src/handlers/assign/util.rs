use coast_core::protocol::{BuildProgressEvent, CoastEvent};
use coast_core::types::{AssignConfig, InstanceStatus};

use crate::server::AppState;

pub(super) const TOTAL_STEPS: u32 = 7;

pub(super) fn health_poll_interval(elapsed: tokio::time::Duration) -> tokio::time::Duration {
    if elapsed.as_secs() < 5 {
        tokio::time::Duration::from_millis(500)
    } else if elapsed.as_secs() < 30 {
        tokio::time::Duration::from_secs(1)
    } else {
        tokio::time::Duration::from_secs(2)
    }
}

pub(super) struct CoastfileData {
    pub assign: AssignConfig,
    pub worktree_dirs: Vec<String>,
    pub default_worktree_dir: String,
    pub has_compose: bool,
}

pub(super) fn load_coastfile_data(project: &str) -> CoastfileData {
    let home = dirs::home_dir().unwrap_or_default();
    let coastfile_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            return CoastfileData {
                assign: cf.assign,
                worktree_dirs: cf.worktree_dirs,
                default_worktree_dir: cf.default_worktree_dir,
                has_compose: cf.compose.is_some(),
            };
        }
    }
    CoastfileData {
        assign: AssignConfig::default(),
        worktree_dirs: vec![".worktrees".to_string()],
        default_worktree_dir: ".worktrees".to_string(),
        has_compose: true,
    }
}

pub fn has_compose(project: &str) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let coastfile_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    if coastfile_path.exists() {
        if let Ok(cf) = coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
            return cf.compose.is_some();
        }
    }
    true
}

pub fn read_project_root(project: &str) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = project_dir.join("latest").join("manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&content).ok()?;
    manifest
        .get("project_root")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from)
}

pub(super) async fn emit(
    tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    event: BuildProgressEvent,
) {
    let _ = tx.send(event).await;
}

pub(super) async fn revert_assign_status(
    state: &AppState,
    project: &str,
    name: &str,
    prev_status: &InstanceStatus,
) {
    if let Ok(db) = state.db.try_lock() {
        let _ = db.update_instance_status(project, name, prev_status);
    }
    state.emit_event(CoastEvent::InstanceStatusChanged {
        name: name.to_string(),
        project: project.to_string(),
        status: prev_status.as_db_str().into(),
    });
}

pub(super) fn check_has_bare_install(project: &str, build_id: Option<&str>) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    let cf_path = build_id
        .map(|bid| {
            home.join(".coast")
                .join("images")
                .join(project)
                .join(bid)
                .join("coastfile.toml")
        })
        .filter(|p| p.exists())
        .unwrap_or_else(|| {
            home.join(".coast")
                .join("images")
                .join(project)
                .join("latest")
                .join("coastfile.toml")
        });
    coast_core::coastfile::Coastfile::from_file(&cf_path)
        .map(|cf| cf.services.iter().any(|s| !s.install.is_empty()))
        .unwrap_or(false)
}
