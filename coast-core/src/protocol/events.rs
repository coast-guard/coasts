use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Error response from the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorResponse {
    pub error: String,
}

/// Events emitted by the daemon when system state changes.
/// Delivered over the WebSocket at `/api/v1/events`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "event")]
pub enum CoastEvent {
    #[serde(rename = "instance.created")]
    InstanceCreated { name: String, project: String },
    #[serde(rename = "instance.removed")]
    InstanceRemoved { name: String, project: String },
    #[serde(rename = "instance.started")]
    InstanceStarted { name: String, project: String },
    #[serde(rename = "instance.stopped")]
    InstanceStopped { name: String, project: String },
    #[serde(rename = "instance.assigned")]
    InstanceAssigned {
        name: String,
        project: String,
        worktree: String,
    },
    #[serde(rename = "instance.unassigned")]
    InstanceUnassigned {
        name: String,
        project: String,
        worktree: String,
    },
    #[serde(rename = "instance.checked_out")]
    InstanceCheckedOut {
        name: Option<String>,
        project: String,
    },
    #[serde(rename = "build.started")]
    BuildStarted { project: String },
    #[serde(rename = "build.completed")]
    BuildCompleted { project: String },
    #[serde(rename = "build.failed")]
    BuildFailed { project: String, error: String },
    #[serde(rename = "build.removing")]
    BuildRemoving {
        project: String,
        #[serde(default)]
        build_ids: Vec<String>,
    },
    #[serde(rename = "build.removed")]
    BuildRemoved {
        project: String,
        #[serde(default)]
        build_ids: Vec<String>,
    },
    #[serde(rename = "project.archived")]
    ProjectArchived { project: String },
    #[serde(rename = "project.unarchived")]
    ProjectUnarchived { project: String },
    #[serde(rename = "service.stopping")]
    ServiceStopping {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.stopped")]
    ServiceStopped {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.starting")]
    ServiceStarting {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.started")]
    ServiceStarted {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.restarting")]
    ServiceRestarting {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.restarted")]
    ServiceRestarted {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.removing")]
    ServiceRemoving {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.removed")]
    ServiceRemoved {
        name: String,
        project: String,
        service: String,
    },
    #[serde(rename = "service.error")]
    ServiceError {
        name: String,
        project: String,
        service: String,
        error: String,
    },
    #[serde(rename = "shared_service.starting")]
    SharedServiceStarting { project: String, service: String },
    #[serde(rename = "shared_service.started")]
    SharedServiceStarted { project: String, service: String },
    #[serde(rename = "shared_service.stopped")]
    SharedServiceStopped { project: String, service: String },
    #[serde(rename = "shared_service.restarted")]
    SharedServiceRestarted { project: String, service: String },
    #[serde(rename = "shared_service.removed")]
    SharedServiceRemoved { project: String, service: String },
    #[serde(rename = "shared_service.error")]
    SharedServiceError {
        project: String,
        service: String,
        error: String,
    },
    #[serde(rename = "instance.status_changed")]
    InstanceStatusChanged {
        name: String,
        project: String,
        status: String,
    },
    #[serde(rename = "project.git_changed")]
    ProjectGitChanged { project: String },
    #[serde(rename = "port.primary_changed")]
    PortPrimaryChanged {
        name: String,
        project: String,
        service: Option<String>,
    },
    #[serde(rename = "config.language_changed")]
    ConfigLanguageChanged { language: String },
    #[serde(rename = "config.analytics_changed")]
    ConfigAnalyticsChanged { enabled: bool },
    #[serde(rename = "agent_shell.spawned")]
    AgentShellSpawned {
        name: String,
        project: String,
        shell_id: i64,
    },
}
