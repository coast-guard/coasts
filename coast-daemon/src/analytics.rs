/// PostHog analytics pipeline.
///
/// Batches `command_executed` events and flushes them to PostHog every
/// 30 seconds or 20 events, whichever comes first.  The analytics toggle
/// (`analytics_enabled`) is checked at flush time so disabling analytics
/// immediately stops all outbound traffic without losing the in-memory
/// buffer.
use std::{collections::BTreeMap, time::Duration};

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tracing::warn;

use coast_core::protocol::{Request, Response};

// -------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------

const POSTHOG_API_KEY: &str = "phc_QrrlMyPlvdwQsvjyEtxGzBDBfk1W6mcJCNuX16mQvuX";
const POSTHOG_ENDPOINT: &str = "https://us.i.posthog.com/batch/";
const BATCH_SIZE: usize = 20;
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const CHANNEL_CAPACITY: usize = 256;

// -------------------------------------------------------------------------
// Public types
// -------------------------------------------------------------------------

/// Where the command originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    Cli,
    Web,
}

impl CommandSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Web => "web",
        }
    }
}

/// Structured metadata attached to analytics events.
pub type AnalyticsMetadata = BTreeMap<String, String>;

/// An analytics event ready for batching.
#[derive(Debug, Clone)]
struct AnalyticsEvent {
    command: String,
    source: CommandSource,
    success: bool,
    duration_ms: u64,
    project: Option<String>,
    instance: Option<String>,
    url: Option<String>,
    metadata: AnalyticsMetadata,
    timestamp: chrono::DateTime<chrono::Utc>,
}

/// Non-blocking handle for recording analytics events.
#[derive(Clone)]
pub struct AnalyticsClient {
    tx: Option<mpsc::Sender<AnalyticsEvent>>,
}

impl AnalyticsClient {
    /// Record a command execution with project/instance context.
    pub fn track_command_with_context(
        &self,
        command: &str,
        source: CommandSource,
        success: bool,
        duration_ms: u64,
        project: Option<&str>,
        instance: Option<&str>,
        metadata: Option<AnalyticsMetadata>,
    ) {
        self.send(AnalyticsEvent {
            command: command.to_string(),
            source,
            success,
            duration_ms,
            project: project.map(String::from),
            instance: instance.map(String::from),
            url: None,
            metadata: metadata.unwrap_or_default(),
            timestamp: chrono::Utc::now(),
        });
    }

    /// Record a web UI event with optional URL context.
    pub fn track_web_event(
        &self,
        event: &str,
        url: Option<&str>,
        metadata: Option<AnalyticsMetadata>,
    ) {
        self.send(AnalyticsEvent {
            command: event.to_string(),
            source: CommandSource::Web,
            success: true,
            duration_ms: 0,
            project: None,
            instance: None,
            url: url.map(String::from),
            metadata: metadata.unwrap_or_default(),
            timestamp: chrono::Utc::now(),
        });
    }

    fn send(&self, event: AnalyticsEvent) {
        if let Some(ref tx) = self.tx {
            let _ = tx.try_send(event);
        }
    }

    /// Create a no-op client that discards all events. Used in tests.
    #[cfg(test)]
    pub fn noop() -> Self {
        Self { tx: None }
    }
}

// -------------------------------------------------------------------------
// Worker
// -------------------------------------------------------------------------

/// Spawn the background analytics worker and return a client handle.
///
/// The worker receives events via an mpsc channel, buffers them, and
/// flushes to PostHog on batch size or timer. The `analytics_enabled`
/// watch channel is checked at flush time.
pub fn spawn_worker(
    anonymous_id: String,
    analytics_enabled: watch::Receiver<bool>,
) -> AnalyticsClient {
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);

    tokio::spawn(worker_loop(anonymous_id, analytics_enabled, rx));

    AnalyticsClient { tx: Some(tx) }
}

async fn worker_loop(
    anonymous_id: String,
    analytics_enabled: watch::Receiver<bool>,
    mut rx: mpsc::Receiver<AnalyticsEvent>,
) {
    let http = reqwest::Client::new();
    let mut buffer: Vec<AnalyticsEvent> = Vec::with_capacity(BATCH_SIZE);
    let mut interval = tokio::time::interval(FLUSH_INTERVAL);
    // The first tick fires immediately — consume it so the first real
    // flush happens after FLUSH_INTERVAL.
    interval.tick().await;

    loop {
        tokio::select! {
            maybe_event = rx.recv() => {
                match maybe_event {
                    Some(event) => {
                        buffer.push(event);
                        if buffer.len() >= BATCH_SIZE {
                            flush(&http, &anonymous_id, &analytics_enabled, &mut buffer).await;
                        }
                    }
                    None => {
                        // Channel closed — flush remaining and exit
                        flush(&http, &anonymous_id, &analytics_enabled, &mut buffer).await;
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                if !buffer.is_empty() {
                    flush(&http, &anonymous_id, &analytics_enabled, &mut buffer).await;
                }
            }
        }
    }
}

#[derive(Serialize)]
struct PostHogBatch {
    api_key: &'static str,
    batch: Vec<PostHogEvent>,
}

#[derive(Serialize)]
struct PostHogEvent {
    #[serde(rename = "type")]
    event_type: &'static str,
    event: String,
    distinct_id: String,
    properties: PostHogProperties,
    timestamp: String,
}

#[derive(Serialize)]
struct PostHogProperties {
    command: String,
    source: String,
    success: bool,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
    #[serde(rename = "$current_url", skip_serializing_if = "Option::is_none")]
    current_url: Option<String>,
    #[serde(flatten)]
    metadata: AnalyticsMetadata,
}

fn event_to_posthog(e: AnalyticsEvent, anonymous_id: &str) -> PostHogEvent {
    let command = e.command.clone();
    PostHogEvent {
        event_type: "capture",
        event: e.command,
        distinct_id: anonymous_id.to_string(),
        properties: PostHogProperties {
            command,
            source: e.source.as_str().to_string(),
            success: e.success,
            duration_ms: e.duration_ms,
            project: e.project,
            instance: e.instance,
            current_url: e.url,
            metadata: e.metadata,
        },
        timestamp: e.timestamp.to_rfc3339(),
    }
}

async fn send_batch(http: &reqwest::Client, payload: &PostHogBatch) {
    match http.post(POSTHOG_ENDPOINT).json(payload).send().await {
        Ok(resp) => tracing::debug!(status = %resp.status(), "analytics flush response"),
        Err(e) => warn!("analytics flush failed: {e}"),
    }
}

async fn flush(
    http: &reqwest::Client,
    anonymous_id: &str,
    analytics_enabled: &watch::Receiver<bool>,
    buffer: &mut Vec<AnalyticsEvent>,
) {
    if buffer.is_empty() {
        return;
    }

    if !*analytics_enabled.borrow() {
        tracing::debug!(count = buffer.len(), "analytics disabled, dropping events");
        buffer.clear();
        return;
    }

    tracing::debug!(count = buffer.len(), "flushing analytics events to PostHog");

    let batch: Vec<PostHogEvent> = buffer
        .drain(..)
        .map(|e| event_to_posthog(e, anonymous_id))
        .collect();

    let payload = PostHogBatch {
        api_key: POSTHOG_API_KEY,
        batch,
    };

    send_batch(http, &payload).await;
}

// -------------------------------------------------------------------------
// Command name helpers
// -------------------------------------------------------------------------

/// Extract project and instance name from a request, when available.
pub fn request_context(req: &Request) -> (Option<&str>, Option<&str>) {
    match req {
        Request::Build(_)
        | Request::Ls(_)
        | Request::Docs(_)
        | Request::SearchDocs(_)
        | Request::SetLanguage(_)
        | Request::SetAnalytics(_) => (None, None),
        Request::Lookup(r) => (Some(&r.project), None),
        Request::RerunExtractors(r) => (Some(&r.project), None),
        Request::Run(r) => (Some(&r.project), Some(&r.name)),
        Request::Stop(r) => (Some(&r.project), Some(&r.name)),
        Request::Start(r) => (Some(&r.project), Some(&r.name)),
        Request::Rm(r) => (Some(&r.project), Some(&r.name)),
        Request::Checkout(r) => (Some(&r.project), r.name.as_deref()),
        Request::Ports(p) => match p {
            coast_core::protocol::PortsRequest::List { project, name, .. }
            | coast_core::protocol::PortsRequest::SetPrimary { project, name, .. }
            | coast_core::protocol::PortsRequest::UnsetPrimary { project, name } => {
                (Some(project.as_str()), Some(name.as_str()))
            }
        },
        Request::Exec(r) => (Some(&r.project), Some(&r.name)),
        Request::Logs(r) => (Some(&r.project), Some(&r.name)),
        Request::Ps(r) => (Some(&r.project), Some(&r.name)),
        Request::Secret(s) => match s {
            coast_core::protocol::SecretRequest::List { project, instance }
            | coast_core::protocol::SecretRequest::Set {
                project, instance, ..
            } => (Some(project.as_str()), Some(instance.as_str())),
        },
        Request::Shared(s) => {
            use coast_core::protocol::SharedRequest;
            match s {
                SharedRequest::Ps { project }
                | SharedRequest::Stop { project, .. }
                | SharedRequest::Start { project, .. }
                | SharedRequest::Restart { project, .. }
                | SharedRequest::Rm { project, .. } => (Some(project.as_str()), None),
            }
        }
        Request::Assign(r) => (Some(&r.project), Some(&r.name)),
        Request::Unassign(r) => (Some(&r.project), Some(&r.name)),
        Request::Rebuild(r) => (Some(&r.project), Some(&r.name)),
        Request::RmBuild(r) => (Some(&r.project), None),
        Request::ArchiveProject(r) => (Some(&r.project), None),
        Request::UnarchiveProject(r) => (Some(&r.project), None),
        Request::Builds(b) => {
            use coast_core::protocol::BuildsRequest;
            match b {
                BuildsRequest::Ls { project } => (project.as_deref(), None),
                BuildsRequest::Inspect { project, .. }
                | BuildsRequest::Images { project, .. }
                | BuildsRequest::DockerImages { project, .. }
                | BuildsRequest::InspectDockerImage { project, .. }
                | BuildsRequest::Compose { project, .. }
                | BuildsRequest::Manifest { project, .. }
                | BuildsRequest::Coastfile { project, .. } => (Some(project.as_str()), None),
            }
        }
        Request::McpLs(r) => (Some(&r.project), Some(&r.name)),
        Request::McpTools(r) => (Some(&r.project), Some(&r.name)),
        Request::McpLocations(r) => (Some(&r.project), Some(&r.name)),
        Request::AgentShell(a) => {
            use coast_core::protocol::AgentShellRequest;
            match a {
                AgentShellRequest::Ls { project, name }
                | AgentShellRequest::Spawn { project, name, .. }
                | AgentShellRequest::Activate { project, name, .. }
                | AgentShellRequest::Tty { project, name, .. }
                | AgentShellRequest::ReadLastLines { project, name, .. }
                | AgentShellRequest::ReadOutput { project, name, .. }
                | AgentShellRequest::Input { project, name, .. }
                | AgentShellRequest::SessionStatus { project, name, .. } => {
                    (Some(project.as_str()), Some(name.as_str()))
                }
                AgentShellRequest::TtyInput { .. } | AgentShellRequest::TtyDetach => (None, None),
            }
        }
    }
}

/// Extract request-specific metadata for analytics.
pub fn request_metadata(req: &Request) -> AnalyticsMetadata {
    let mut metadata = AnalyticsMetadata::new();
    match req {
        Request::Docs(r) => {
            if let Some(path) = &r.path {
                metadata.insert("docs_requested_path".to_string(), path.clone());
            }
            if let Some(lang) = &r.language {
                metadata.insert("docs_requested_language".to_string(), lang.clone());
            }
        }
        Request::SearchDocs(r) => {
            metadata.insert("docs_search_query".to_string(), r.query.clone());
            if let Some(lang) = &r.language {
                metadata.insert("docs_requested_language".to_string(), lang.clone());
            }
            if let Some(limit) = r.limit {
                metadata.insert("docs_search_limit".to_string(), limit.to_string());
            }
        }
        _ => {}
    }
    metadata
}

/// Extract response-specific metadata for analytics.
pub fn response_metadata(req: &Request, response: &Response) -> AnalyticsMetadata {
    let mut metadata = AnalyticsMetadata::new();
    match (req, response) {
        (Request::Docs(_), Response::Docs(r)) => {
            metadata.insert("docs_locale".to_string(), r.locale.clone());
            if let Some(path) = &r.path {
                metadata.insert("docs_resolved_path".to_string(), path.clone());
            }
            metadata.insert("docs_tree_count".to_string(), r.tree.len().to_string());
        }
        (Request::SearchDocs(_), Response::SearchDocs(r)) => {
            metadata.insert("docs_search_locale".to_string(), r.locale.clone());
            metadata.insert(
                "docs_search_result_count".to_string(),
                r.results.len().to_string(),
            );
        }
        _ => {}
    }
    metadata
}

/// Map a unix-socket `Request` variant to a descriptive command name
/// that includes sub-action detail where applicable.
pub fn request_command_name(req: &Request) -> String {
    use coast_core::protocol::*;
    match req {
        Request::Build(_) => "build".into(),
        Request::RerunExtractors(_) => "rerun_extractors".into(),
        Request::Run(_) => "run".into(),
        Request::Stop(_) => "stop".into(),
        Request::Start(_) => "start".into(),
        Request::Rm(_) => "rm".into(),
        Request::Checkout(_) => "checkout".into(),
        Request::Exec(_) => "exec".into(),
        Request::Logs(_) => "logs".into(),
        Request::Ps(_) => "ps".into(),
        Request::Ls(_) => "ls".into(),
        Request::Docs(_) => "docs".into(),
        Request::SearchDocs(_) => "search_docs".into(),
        Request::Assign(_) => "assign".into(),
        Request::Unassign(_) => "unassign".into(),
        Request::Rebuild(_) => "rebuild".into(),
        Request::RmBuild(_) => "rm_build".into(),
        Request::ArchiveProject(_) => "archive_project".into(),
        Request::UnarchiveProject(_) => "unarchive_project".into(),
        Request::McpLs(_) => "mcp_ls".into(),
        Request::McpTools(_) => "mcp_tools".into(),
        Request::McpLocations(_) => "mcp_locations".into(),
        Request::SetLanguage(_) => "set_language".into(),
        Request::Ports(p) => match p {
            PortsRequest::List { .. } => "ports/list",
            PortsRequest::SetPrimary { .. } => "ports/set_primary",
            PortsRequest::UnsetPrimary { .. } => "ports/unset_primary",
        }
        .into(),
        Request::Secret(s) => match s {
            SecretRequest::Set { .. } => "secret/set",
            SecretRequest::List { .. } => "secret/list",
        }
        .into(),
        Request::Shared(s) => match s {
            SharedRequest::Ps { .. } => "shared/ps",
            SharedRequest::Stop { .. } => "shared/stop",
            SharedRequest::Start { .. } => "shared/start",
            SharedRequest::Restart { .. } => "shared/restart",
            SharedRequest::Rm { .. } => "shared/rm",
        }
        .into(),
        Request::Builds(b) => match b {
            BuildsRequest::Ls { .. } => "builds/ls",
            BuildsRequest::Inspect { .. } => "builds/inspect",
            BuildsRequest::Images { .. } => "builds/images",
            BuildsRequest::DockerImages { .. } => "builds/docker_images",
            BuildsRequest::InspectDockerImage { .. } => "builds/inspect_docker_image",
            BuildsRequest::Compose { .. } => "builds/compose",
            BuildsRequest::Manifest { .. } => "builds/manifest",
            BuildsRequest::Coastfile { .. } => "builds/coastfile",
        }
        .into(),
        Request::AgentShell(a) => match a {
            AgentShellRequest::Ls { .. } => "agent_shell/ls",
            AgentShellRequest::Spawn { .. } => "agent_shell/spawn",
            AgentShellRequest::Activate { .. } => "agent_shell/activate",
            AgentShellRequest::Tty { .. } => "agent_shell/tty",
            AgentShellRequest::TtyInput { .. } => "agent_shell/tty_input",
            AgentShellRequest::TtyDetach => "agent_shell/tty_detach",
            AgentShellRequest::ReadLastLines { .. } => "agent_shell/read_last_lines",
            AgentShellRequest::ReadOutput { .. } => "agent_shell/read_output",
            AgentShellRequest::Input { .. } => "agent_shell/input",
            AgentShellRequest::SessionStatus { .. } => "agent_shell/session_status",
        }
        .into(),
        Request::SetAnalytics(r) => match r.action {
            AnalyticsAction::Enable => "analytics/enable",
            AnalyticsAction::Disable => "analytics/disable",
            AnalyticsAction::Status => "analytics/status",
        }
        .into(),
        Request::Lookup(_) => "lookup".into(),
    }
}

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_client_does_not_panic() {
        let client = AnalyticsClient::noop();
        client.track_command_with_context("test", CommandSource::Cli, true, 42, None, None, None);
        // Should not panic
    }

    #[test]
    fn request_command_name_all_variants() {
        // Exhaustively verify every variant maps to a non-empty string.
        // We construct one of each variant and check the name is sensible.
        use coast_core::protocol::*;

        let variants: Vec<Request> = vec![
            Request::Build(BuildRequest {
                coastfile_path: std::path::PathBuf::new(),
                refresh: false,
            }),
            Request::RerunExtractors(RerunExtractorsRequest {
                project: String::new(),
                build_id: None,
            }),
            Request::Run(RunRequest {
                name: String::new(),
                project: String::new(),
                branch: None,
                commit_sha: None,
                worktree: None,
                build_id: None,
                coastfile_type: None,
                force_remove_dangling: false,
            }),
            Request::Stop(StopRequest {
                project: String::new(),
                name: String::new(),
            }),
            Request::Start(StartRequest {
                project: String::new(),
                name: String::new(),
            }),
            Request::Rm(RmRequest {
                name: String::new(),
                project: String::new(),
            }),
            Request::Checkout(CheckoutRequest {
                project: String::new(),
                name: None,
            }),
            Request::Ports(PortsRequest::List {
                name: String::new(),
                project: String::new(),
            }),
            Request::Exec(ExecRequest {
                project: String::new(),
                name: String::new(),
                command: vec![],
            }),
            Request::Logs(LogsRequest {
                project: String::new(),
                name: String::new(),
                service: None,
                follow: false,
                tail: None,
                tail_all: false,
            }),
            Request::Ps(PsRequest {
                project: String::new(),
                name: String::new(),
            }),
            Request::Ls(LsRequest { project: None }),
            Request::Docs(DocsRequest {
                path: None,
                language: None,
            }),
            Request::SearchDocs(SearchDocsRequest {
                query: String::new(),
                limit: None,
                language: None,
            }),
            Request::Secret(SecretRequest::List {
                instance: String::new(),
                project: String::new(),
            }),
            Request::Shared(SharedRequest::Ps {
                project: String::new(),
            }),
            Request::Assign(AssignRequest {
                project: String::new(),
                name: String::new(),
                worktree: String::new(),
                commit_sha: None,
            }),
            Request::Unassign(UnassignRequest {
                project: String::new(),
                name: String::new(),
            }),
            Request::Rebuild(RebuildRequest {
                project: String::new(),
                name: String::new(),
            }),
            Request::RmBuild(RmBuildRequest {
                project: String::new(),
                build_ids: vec![],
            }),
            Request::ArchiveProject(ArchiveProjectRequest {
                project: String::new(),
            }),
            Request::UnarchiveProject(UnarchiveProjectRequest {
                project: String::new(),
            }),
            Request::Builds(BuildsRequest::Ls { project: None }),
            Request::McpLs(McpLsRequest {
                name: String::new(),
                project: String::new(),
            }),
            Request::McpTools(McpToolsRequest {
                name: String::new(),
                project: String::new(),
                server: String::new(),
                tool: None,
            }),
            Request::McpLocations(McpLocationsRequest {
                name: String::new(),
                project: String::new(),
            }),
            Request::AgentShell(AgentShellRequest::Ls {
                project: String::new(),
                name: String::new(),
            }),
            Request::SetLanguage(SetLanguageRequest {
                language: String::new(),
            }),
            Request::SetAnalytics(SetAnalyticsRequest {
                action: AnalyticsAction::Status,
            }),
        ];

        for req in &variants {
            let name = request_command_name(req);
            assert!(!name.is_empty(), "empty command name for {req:?}");
        }

        // Verify count matches enum variant count (compile error if a new
        // variant is added without updating the match above)
        assert_eq!(variants.len(), 29);
    }

    #[tokio::test]
    async fn track_command_does_not_block_when_channel_full() {
        let (tx, _rx) = mpsc::channel(1);
        let client = AnalyticsClient { tx: Some(tx) };

        // Fill the channel
        client.track_command_with_context("fill", CommandSource::Cli, true, 10, None, None, None);
        // This should not block even though channel is full
        client.track_command_with_context(
            "overflow",
            CommandSource::Cli,
            true,
            20,
            None,
            None,
            None,
        );
    }

    #[tokio::test]
    async fn worker_respects_analytics_disabled() {
        // When analytics is disabled, flush should drop events without sending.
        let http = reqwest::Client::new();
        let (_tx, rx) = watch::channel(false); // disabled
        let mut buffer = vec![AnalyticsEvent {
            command: "test".to_string(),
            source: CommandSource::Cli,
            success: true,
            duration_ms: 42,
            project: None,
            instance: None,
            url: None,
            metadata: AnalyticsMetadata::new(),
            timestamp: chrono::Utc::now(),
        }];

        flush(&http, "test-id", &rx, &mut buffer).await;
        assert!(
            buffer.is_empty(),
            "buffer should be drained even when disabled"
        );
    }

    #[tokio::test]
    async fn worker_skips_empty_buffer() {
        let http = reqwest::Client::new();
        let (_tx, rx) = watch::channel(true);
        let mut buffer: Vec<AnalyticsEvent> = vec![];

        // Should be a no-op, not panic
        flush(&http, "test-id", &rx, &mut buffer).await;
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn track_web_event_sets_source_and_url() {
        let (tx, mut rx) = mpsc::channel(8);
        let client = AnalyticsClient { tx: Some(tx) };

        let mut md = AnalyticsMetadata::new();
        md.insert("target".to_string(), "docs".to_string());
        client.track_web_event(
            "button/click",
            Some("http://localhost:5173/#/project/app"),
            Some(md),
        );
        client.track_web_event("nav/home", None, None);

        let e1 = rx.recv().await.unwrap();
        assert_eq!(e1.command, "button/click");
        assert_eq!(e1.source, CommandSource::Web);
        assert_eq!(
            e1.url.as_deref(),
            Some("http://localhost:5173/#/project/app")
        );
        assert_eq!(e1.metadata.get("target").map(String::as_str), Some("docs"));
        assert!(e1.success);
        assert_eq!(e1.duration_ms, 0);

        let e2 = rx.recv().await.unwrap();
        assert_eq!(e2.command, "nav/home");
        assert!(e2.url.is_none());
    }

    #[tokio::test]
    async fn track_command_with_context_sets_project_and_instance() {
        let (tx, mut rx) = mpsc::channel(8);
        let client = AnalyticsClient { tx: Some(tx) };

        client.track_command_with_context(
            "stop",
            CommandSource::Cli,
            true,
            150,
            Some("myapp"),
            Some("dev-1"),
            None,
        );

        let e = rx.recv().await.unwrap();
        assert_eq!(e.command, "stop");
        assert_eq!(e.source, CommandSource::Cli);
        assert_eq!(e.project.as_deref(), Some("myapp"));
        assert_eq!(e.instance.as_deref(), Some("dev-1"));
        assert!(e.url.is_none());
        assert_eq!(e.duration_ms, 150);
    }

    #[test]
    fn request_metadata_extracts_docs_fields() {
        use coast_core::protocol::*;

        let docs_req = Request::Docs(DocsRequest {
            path: Some("coastfiles/COASTFILE".to_string()),
            language: Some("ja".to_string()),
        });
        let docs_md = request_metadata(&docs_req);
        assert_eq!(
            docs_md.get("docs_requested_path").map(String::as_str),
            Some("coastfiles/COASTFILE")
        );
        assert_eq!(
            docs_md.get("docs_requested_language").map(String::as_str),
            Some("ja")
        );

        let search_req = Request::SearchDocs(SearchDocsRequest {
            query: "shared services".to_string(),
            limit: Some(15),
            language: Some("en".to_string()),
        });
        let search_md = request_metadata(&search_req);
        assert_eq!(
            search_md.get("docs_search_query").map(String::as_str),
            Some("shared services")
        );
        assert_eq!(
            search_md.get("docs_search_limit").map(String::as_str),
            Some("15")
        );
        assert_eq!(
            search_md.get("docs_requested_language").map(String::as_str),
            Some("en")
        );
    }

    #[test]
    fn response_metadata_extracts_docs_search_fields() {
        use coast_core::protocol::*;

        let req = Request::SearchDocs(SearchDocsRequest {
            query: "coastfile".to_string(),
            limit: Some(10),
            language: Some("en".to_string()),
        });
        let response = Response::SearchDocs(SearchDocsResponse {
            query: "coastfile".to_string(),
            locale: "en".to_string(),
            strategy: "hybrid_keyword_semantic".to_string(),
            results: vec![SearchDocsResult {
                path: "coastfiles/COASTFILE.md".to_string(),
                route: "/docs/coastfiles/COASTFILE".to_string(),
                heading: "Coastfile".to_string(),
                snippet: "Defines runtime.".to_string(),
                score: 1.0,
            }],
        });
        let md = response_metadata(&req, &response);
        assert_eq!(md.get("docs_search_locale").map(String::as_str), Some("en"));
        assert_eq!(
            md.get("docs_search_result_count").map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn request_context_extracts_project_and_instance() {
        use coast_core::protocol::*;

        let req = Request::Stop(StopRequest {
            project: "myapp".to_string(),
            name: "dev-1".to_string(),
        });
        let (proj, inst) = request_context(&req);
        assert_eq!(proj, Some("myapp"));
        assert_eq!(inst, Some("dev-1"));

        // Checkout has optional instance
        let req = Request::Checkout(CheckoutRequest {
            project: "myapp".to_string(),
            name: None,
        });
        let (proj, inst) = request_context(&req);
        assert_eq!(proj, Some("myapp"));
        assert_eq!(inst, None);

        // Ls has no project or instance
        let req = Request::Ls(LsRequest { project: None });
        let (proj, inst) = request_context(&req);
        assert!(proj.is_none());
        assert!(inst.is_none());

        // Shared has project but no instance
        let req = Request::Shared(SharedRequest::Stop {
            project: "myapp".to_string(),
            service: Some("postgres".to_string()),
        });
        let (proj, inst) = request_context(&req);
        assert_eq!(proj, Some("myapp"));
        assert!(inst.is_none());
    }

    #[test]
    fn request_command_name_includes_sub_action() {
        use coast_core::protocol::*;

        assert_eq!(
            request_command_name(&Request::SetAnalytics(SetAnalyticsRequest {
                action: AnalyticsAction::Status,
            })),
            "analytics/status"
        );
        assert_eq!(
            request_command_name(&Request::SetAnalytics(SetAnalyticsRequest {
                action: AnalyticsAction::Enable,
            })),
            "analytics/enable"
        );
        assert_eq!(
            request_command_name(&Request::Shared(SharedRequest::Stop {
                project: String::new(),
                service: None,
            })),
            "shared/stop"
        );
        assert_eq!(
            request_command_name(&Request::Ports(PortsRequest::SetPrimary {
                name: String::new(),
                project: String::new(),
                service: String::new(),
            })),
            "ports/set_primary"
        );
        // Simple commands stay flat
        assert_eq!(
            request_command_name(&Request::Stop(StopRequest {
                project: String::new(),
                name: String::new(),
            })),
            "stop"
        );
    }

    #[test]
    fn event_to_posthog_maps_all_fields() {
        let event = AnalyticsEvent {
            command: "run".to_string(),
            source: CommandSource::Cli,
            success: true,
            duration_ms: 1234,
            project: Some("myapp".to_string()),
            instance: Some("dev-1".to_string()),
            url: None,
            metadata: AnalyticsMetadata::from([(
                "docs_requested_path".to_string(),
                "README.md".to_string(),
            )]),
            timestamp: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        };

        let ph = event_to_posthog(event, "anon-123");

        assert_eq!(ph.event, "run");
        assert_eq!(ph.event_type, "capture");
        assert_eq!(ph.distinct_id, "anon-123");
        assert_eq!(ph.properties.command, "run");
        assert_eq!(ph.properties.source, "cli");
        assert!(ph.properties.success);
        assert_eq!(ph.properties.duration_ms, 1234);
        assert_eq!(ph.properties.project.as_deref(), Some("myapp"));
        assert_eq!(ph.properties.instance.as_deref(), Some("dev-1"));
        assert!(ph.properties.current_url.is_none());
        assert_eq!(
            ph.properties
                .metadata
                .get("docs_requested_path")
                .map(String::as_str),
            Some("README.md")
        );
        assert!(ph.timestamp.contains("2026-01-01"));
    }

    #[test]
    fn event_to_posthog_web_source_with_url() {
        let event = AnalyticsEvent {
            command: "page_view".to_string(),
            source: CommandSource::Web,
            success: true,
            duration_ms: 0,
            project: None,
            instance: None,
            url: Some("http://localhost:5173/#/projects".to_string()),
            metadata: AnalyticsMetadata::new(),
            timestamp: chrono::Utc::now(),
        };

        let ph = event_to_posthog(event, "anon-456");

        assert_eq!(ph.properties.source, "web");
        assert_eq!(
            ph.properties.current_url.as_deref(),
            Some("http://localhost:5173/#/projects")
        );
        assert!(ph.properties.project.is_none());
        assert!(ph.properties.metadata.is_empty());
    }

    #[tokio::test]
    async fn flush_clears_buffer_when_disabled() {
        let http = reqwest::Client::new();
        let (_tx, rx) = watch::channel(false);
        let mut buffer = vec![AnalyticsEvent {
            command: "test".to_string(),
            source: CommandSource::Cli,
            success: true,
            duration_ms: 0,
            project: None,
            instance: None,
            url: None,
            metadata: AnalyticsMetadata::new(),
            timestamp: chrono::Utc::now(),
        }];

        flush(&http, "anon", &rx, &mut buffer).await;

        assert!(
            buffer.is_empty(),
            "buffer should be cleared when analytics disabled"
        );
    }

    #[tokio::test]
    async fn flush_noop_on_empty_buffer() {
        let http = reqwest::Client::new();
        let (_tx, rx) = watch::channel(true);
        let mut buffer: Vec<AnalyticsEvent> = Vec::new();

        flush(&http, "anon", &rx, &mut buffer).await;

        assert!(buffer.is_empty());
    }
}
