use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to manage agent shells for a coast instance.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "action")]
pub enum AgentShellRequest {
    /// List all agent shells for an instance.
    Ls { project: String, name: String },
    /// Activate an existing agent shell by shell ID.
    Activate {
        project: String,
        name: String,
        shell_id: i64,
    },
    /// Spawn a new agent shell for the instance.
    Spawn {
        project: String,
        name: String,
        #[serde(default)]
        activate: bool,
    },
    /// Attach an interactive tty stream to an agent shell.
    ///
    /// When `shell_id` is omitted, attaches to the active agent shell.
    Tty {
        project: String,
        name: String,
        #[serde(default)]
        shell_id: Option<i64>,
    },
    /// TTY input event sent by the CLI on an attached stream.
    TtyInput { data: String },
    /// TTY detach event sent by the CLI to end an attached stream.
    TtyDetach,
    /// Read the last N lines from the shell scrollback.
    ReadLastLines {
        project: String,
        name: String,
        lines: usize,
        #[serde(default)]
        shell_id: Option<i64>,
    },
    /// Read the full currently buffered scrollback output.
    ReadOutput {
        project: String,
        name: String,
        #[serde(default)]
        shell_id: Option<i64>,
    },
    /// Write raw input bytes/text to the shell (no implicit newline).
    Input {
        project: String,
        name: String,
        input: String,
        #[serde(default)]
        shell_id: Option<i64>,
    },
    /// Get runtime status for a shell.
    ///
    /// When `shell_id` is omitted, inspects the active shell.
    SessionStatus {
        project: String,
        name: String,
        #[serde(default)]
        shell_id: Option<i64>,
    },
}

/// Summary of an agent shell record.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellSummary {
    pub shell_id: i64,
    pub is_active: bool,
    pub status: String,
    pub is_live: bool,
}

/// Response for `agent-shell ls`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellLsResponse {
    pub name: String,
    pub shells: Vec<AgentShellSummary>,
}

/// Response for `agent-shell activate`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellActivateResponse {
    pub shell_id: i64,
    pub changed: bool,
    pub message: String,
}

/// Response for `agent-shell spawn`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellSpawnResponse {
    pub shell_id: i64,
    pub session_id: String,
    pub is_active: bool,
}

/// Response carrying shell output text.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellReadResponse {
    pub shell_id: i64,
    pub output: String,
}

/// Response for `agent-shell input`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellInputResponse {
    pub shell_id: i64,
    pub bytes_written: usize,
}

/// Response for `agent-shell session-status`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellSessionStatusResponse {
    pub shell_id: Option<i64>,
    pub status: String,
    pub is_active: bool,
    pub is_live: bool,
    pub message: String,
}

/// Initial event emitted when a tty stream is attached.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellTtyAttachedResponse {
    pub shell_id: i64,
    pub session_id: String,
}

/// TTY output chunk event emitted by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellTtyOutputResponse {
    pub data: String,
}

/// TTY stream-closed event emitted by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentShellTtyClosedResponse {
    #[serde(default)]
    pub reason: Option<String>,
}

/// Agent shell response variants.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind")]
pub enum AgentShellResponse {
    /// List response.
    Ls(AgentShellLsResponse),
    /// Activate response.
    Activate(AgentShellActivateResponse),
    /// Spawn response.
    Spawn(AgentShellSpawnResponse),
    /// Read-last-lines response.
    ReadLastLines(AgentShellReadResponse),
    /// Read-output response.
    ReadOutput(AgentShellReadResponse),
    /// Input response.
    Input(AgentShellInputResponse),
    /// Session-status response.
    SessionStatus(AgentShellSessionStatusResponse),
    /// TTY attach acknowledgement.
    TtyAttached(AgentShellTtyAttachedResponse),
    /// TTY output event.
    TtyOutput(AgentShellTtyOutputResponse),
    /// TTY closed event.
    TtyClosed(AgentShellTtyClosedResponse),
}
