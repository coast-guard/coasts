/// Handler modules for the coast daemon.
///
/// Each handler receives a parsed request and the shared `AppState`,
/// interacts with the state DB and Docker API as needed, and returns
/// a protocol `Response`.
use coast_core::protocol::*;
use std::sync::Arc;
use tracing::error;

use coast_core::protocol::CoastEvent;

use crate::server::AppState;

/// Compose args needed to address the inner compose stack.
/// Mirrors the logic in `run.rs` that starts the stack.
pub struct ComposeContext {
    pub project_name: String,
    pub compose_rel_dir: Option<String>,
}

impl ComposeContext {
    /// Build a `sh -c` command that discovers the compose file at runtime
    /// inside the container and runs the given docker compose subcommand.
    ///
    /// Checks paths in priority order (matching what `run.rs` uses):
    /// 1. `/coast-artifact/compose.yml` (build artifact)
    /// 2. `<project_dir>/docker-compose.yml`
    /// 3. `<project_dir>/docker-compose.yaml`
    /// 4. `/workspace/docker-compose.yml` (root fallback)
    pub fn compose_shell(&self, subcmd: &str) -> Vec<String> {
        let project_dir = match &self.compose_rel_dir {
            Some(dir) => format!("/workspace/{}", dir),
            None => "/workspace".to_string(),
        };

        let script = format!(
            concat!(
                "if [ -f /coast-override/docker-compose.coast.yml ]; then ",
                "  docker compose -p {proj} -f /coast-override/docker-compose.coast.yml --project-directory {dir} {subcmd}; ",
                "elif [ -f /coast-artifact/compose.yml ]; then ",
                "  docker compose -p {proj} -f /coast-artifact/compose.yml --project-directory {dir} {subcmd}; ",
                "elif [ -f {dir}/docker-compose.yml ]; then ",
                "  docker compose -p {proj} -f {dir}/docker-compose.yml --project-directory {dir} {subcmd}; ",
                "elif [ -f {dir}/docker-compose.yaml ]; then ",
                "  docker compose -p {proj} -f {dir}/docker-compose.yaml --project-directory {dir} {subcmd}; ",
                "elif [ -f /workspace/docker-compose.yml ]; then ",
                "  docker compose -p {proj} -f /workspace/docker-compose.yml {subcmd}; ",
                "else ",
                "  echo 'no compose file found' >&2; exit 1; ",
                "fi",
            ),
            proj = self.project_name,
            dir = project_dir,
            subcmd = subcmd,
        );

        vec!["sh".into(), "-c".into(), script]
    }
}

/// Derive compose context for a Coast project by reading the stored Coastfile.
///
/// When `build_id` is provided, reads the coastfile from that specific build
/// directory instead of `latest`. This is critical for typed builds (e.g. light)
/// where `latest` points to the default build, not the instance's actual build.
pub fn compose_context(project: &str) -> ComposeContext {
    compose_context_for_build(project, None)
}

/// Like [`compose_context`] but resolves the coastfile from a specific build.
pub fn compose_context_for_build(project: &str, build_id: Option<&str>) -> ComposeContext {
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let coastfile_path = match build_id {
        Some(bid) => {
            let p = project_dir.join(bid).join("coastfile.toml");
            if p.exists() {
                p
            } else {
                project_dir.join("latest").join("coastfile.toml")
            }
        }
        None => project_dir.join("latest").join("coastfile.toml"),
    };

    let compose_rel_dir = if coastfile_path.exists() {
        coast_core::coastfile::Coastfile::from_file(&coastfile_path)
            .ok()
            .and_then(|cf| {
                cf.compose.as_ref().and_then(|p| {
                    let parent = p.parent()?;
                    let dir_name = parent.file_name()?.to_str()?;
                    Some(dir_name.to_string())
                })
            })
    } else {
        None
    };

    let project_name = compose_rel_dir
        .clone()
        .unwrap_or_else(|| format!("coast-{}", project));

    ComposeContext {
        project_name,
        compose_rel_dir,
    }
}

#[cfg(test)]
mod compose_context_tests {
    use super::*;

    #[test]
    fn test_compose_shell_with_subdir() {
        let ctx = ComposeContext {
            project_name: "infra".into(),
            compose_rel_dir: Some("infra".into()),
        };
        let cmd = ctx.compose_shell("ps --format json");
        assert_eq!(cmd[0], "sh");
        assert_eq!(cmd[1], "-c");
        assert!(cmd[2].contains("-p infra"));
        assert!(cmd[2].contains("/coast-artifact/compose.yml"));
        assert!(cmd[2].contains("/workspace/infra/docker-compose.yml"));
        assert!(cmd[2].contains("ps --format json"));
    }

    #[test]
    fn test_compose_shell_no_subdir() {
        let ctx = ComposeContext {
            project_name: "coast-myapp".into(),
            compose_rel_dir: None,
        };
        let cmd = ctx.compose_shell("logs --tail 200");
        assert!(cmd[2].contains("-p coast-myapp"));
        assert!(cmd[2].contains("/workspace/docker-compose.yml"));
        assert!(cmd[2].contains("logs --tail 200"));
    }
}

pub mod agent_shell;
pub mod archive;
pub mod assign;
pub mod build;
pub mod builds;
pub mod checkout;
pub mod docs;
pub mod exec;
pub mod logs;
pub mod lookup;
pub mod ls;
pub mod mcp;
pub mod ports;
pub mod ps;
pub mod rebuild;
pub mod rerun_extractors;
pub mod rm;
pub mod rm_build;
pub mod run;
pub mod secret;
pub mod set_analytics;
pub mod set_language;
pub mod shared;
pub mod start;
pub mod stop;
pub mod unassign;

/// Convert a handler result into a Response, wrapping errors in ErrorResponse.
/// Uses the English error message (for logs and when no language context is available).
fn error_response(e: &coast_core::error::CoastError) -> Response {
    error!("handler error: {e}");
    Response::Error(ErrorResponse {
        error: e.to_string(),
    })
}

/// Translate a `CoastError` into a user-facing string for the given locale.
pub fn translate_error(e: &coast_core::error::CoastError, lang: &str) -> String {
    use coast_core::error::CoastError;
    use rust_i18n::t;

    match e {
        CoastError::CoastfileParse { message, .. } => {
            t!("error.coastfile_parse", locale = lang, message = message).to_string()
        }
        CoastError::Docker { message, .. } => {
            t!("error.docker", locale = lang, message = message).to_string()
        }
        CoastError::Git { message, .. } => {
            t!("error.git", locale = lang, message = message).to_string()
        }
        CoastError::Secret { message, .. } => {
            t!("error.secret", locale = lang, message = message).to_string()
        }
        CoastError::State { message, .. } => {
            t!("error.state", locale = lang, message = message).to_string()
        }
        CoastError::Port { message, .. } => {
            t!("error.port", locale = lang, message = message).to_string()
        }
        CoastError::Io { message, path, .. } => t!(
            "error.io",
            locale = lang,
            message = message,
            path = path.display().to_string()
        )
        .to_string(),
        CoastError::Artifact { message, .. } => {
            t!("error.artifact", locale = lang, message = message).to_string()
        }
        CoastError::Volume { message, .. } => {
            t!("error.volume", locale = lang, message = message).to_string()
        }
        CoastError::InstanceNotFound { name, project } => t!(
            "error.instance_not_found",
            locale = lang,
            name = name,
            project = project
        )
        .to_string(),
        CoastError::InstanceAlreadyExists { name, project } => t!(
            "error.instance_already_exists",
            locale = lang,
            name = name,
            project = project
        )
        .to_string(),
        CoastError::DanglingContainerDetected {
            name,
            project,
            container_name,
        } => t!(
            "error.dangling_container",
            locale = lang,
            name = name,
            project = project,
            container_name = container_name
        )
        .to_string(),
        CoastError::RuntimeUnavailable { runtime, reason } => t!(
            "error.runtime_unavailable",
            locale = lang,
            runtime = runtime,
            reason = reason
        )
        .to_string(),
        CoastError::Protocol { message, .. } => {
            t!("error.protocol", locale = lang, message = message).to_string()
        }
    }
}

/// Convert a handler result into a translated Response, wrapping errors
/// in ErrorResponse with a translated message for the given locale.
#[allow(dead_code)]
fn error_response_translated(e: &coast_core::error::CoastError, lang: &str) -> Response {
    error!("handler error: {e}");
    Response::Error(ErrorResponse {
        error: translate_error(e, lang),
    })
}

/// Handle a Build request with a progress sender for streaming.
pub async fn handle_build_with_progress(
    req: BuildRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<BuildResponse> {
    let project_hint = req
        .coastfile_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    state.emit_event(CoastEvent::BuildStarted {
        project: project_hint.clone(),
    });
    match build::handle(req, state, progress).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::BuildCompleted {
                project: resp.project.clone(),
            });
            Ok(resp)
        }
        Err(e) => {
            state.emit_event(CoastEvent::BuildFailed {
                project: project_hint,
                error: e.to_string(),
            });
            Err(e)
        }
    }
}

/// Handle a rerun-extractors request with a progress sender for streaming.
pub async fn handle_rerun_extractors_with_progress(
    req: RerunExtractorsRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<RerunExtractorsResponse> {
    rerun_extractors::handle(req, state, progress).await
}

/// Handle a Run request with a progress sender for streaming.
pub async fn handle_run_with_progress(
    req: RunRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<RunResponse> {
    let name = req.name.clone();
    let project = req.project.clone();
    match run::handle(req, state, progress).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceCreated {
                name: resp.name.clone(),
                project,
            });
            Ok(resp)
        }
        Err(e) => {
            error!("run failed for {name}: {e}");
            cleanup_failed_provision(&name, &project, state).await;
            Err(e)
        }
    }
}

/// Remove a failed provisioning instance so it doesn't hang in the UI.
///
/// Best-effort: removes the Docker container, port allocations, and DB record,
/// then emits `InstanceRemoved` so connected clients drop the row.
async fn cleanup_failed_provision(name: &str, project: &str, state: &AppState) {
    let container_name = format!("{project}-coasts-{name}");

    if let Some(ref docker) = state.docker {
        let rm_opts = bollard::container::RemoveContainerOptions {
            force: true,
            v: true,
            ..Default::default()
        };
        let _ = docker
            .remove_container(&container_name, Some(rm_opts))
            .await;

        let dind_vol = format!("coast-dind--{project}--{name}");
        let _ = docker.remove_volume(&dind_vol, None).await;
    }

    {
        let db = state.db.lock().await;
        let _ = db.delete_port_allocations(project, name);
        let _ = db.delete_instance(project, name);
    }

    state.emit_event(CoastEvent::InstanceRemoved {
        name: name.to_string(),
        project: project.to_string(),
    });
    tracing::info!(name, project, "cleaned up failed provisioning instance");
}

/// Handle a Stop request (non-streaming, e.g. from HTTP API).
pub async fn handle_stop(req: StopRequest, state: &AppState) -> Response {
    let name = req.name.clone();
    let project = req.project.clone();
    match stop::handle(req, state, None).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceStopped {
                name: name.clone(),
                project,
            });
            Response::Stop(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle a Start request (non-streaming, e.g. from HTTP API).
pub async fn handle_start(req: StartRequest, state: &AppState) -> Response {
    let name = req.name.clone();
    let project = req.project.clone();
    match start::handle(req, state, None).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceStarted {
                name: name.clone(),
                project,
            });
            Response::Start(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle a Start request with a progress sender for streaming.
pub async fn handle_start_with_progress(
    req: StartRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<StartResponse> {
    let name = req.name.clone();
    let project = req.project.clone();
    match start::handle(req, state, Some(progress)).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceStarted { name, project });
            Ok(resp)
        }
        Err(e) => {
            error!("start failed for {name}: {e}");
            Err(e)
        }
    }
}

/// Handle a Stop request with a progress sender for streaming.
pub async fn handle_stop_with_progress(
    req: StopRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<StopResponse> {
    let name = req.name.clone();
    let project = req.project.clone();
    match stop::handle(req, state, Some(progress)).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceStopped { name, project });
            Ok(resp)
        }
        Err(e) => {
            error!("stop failed for {name}: {e}");
            Err(e)
        }
    }
}

/// Handle an Rm request.
pub async fn handle_rm(req: RmRequest, state: &AppState) -> Response {
    let name = req.name.clone();
    let project = req.project.clone();
    match rm::handle(req, state).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceRemoved {
                name: name.clone(),
                project,
            });
            Response::Rm(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle an RmBuild request (non-streaming).
pub async fn handle_rm_build(req: RmBuildRequest, state: &AppState) -> Response {
    let project = req.project.clone();
    match rm_build::handle(req, state, None).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::BuildRemoved {
                project,
                build_ids: Vec::new(),
            });
            Response::RmBuild(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle an RmBuild request with a progress sender for streaming.
pub async fn handle_rm_build_with_progress(
    req: RmBuildRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<RmBuildResponse> {
    rm_build::handle(req, state, Some(progress)).await
}

/// Handle an ArchiveProject request.
pub async fn handle_archive_project(req: ArchiveProjectRequest, state: &AppState) -> Response {
    let project = req.project.clone();
    match archive::handle_archive(req, state).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::ProjectArchived { project });
            Response::ArchiveProject(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle an UnarchiveProject request.
pub async fn handle_unarchive_project(req: UnarchiveProjectRequest, state: &AppState) -> Response {
    let project = req.project.clone();
    match archive::handle_unarchive(req, state).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::ProjectUnarchived { project });
            Response::UnarchiveProject(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle a Checkout request.
pub async fn handle_checkout(req: CheckoutRequest, state: &AppState) -> Response {
    let project = req.project.clone();
    let name = req.name.clone();
    match checkout::handle(req, state).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceCheckedOut { name, project });
            Response::Checkout(resp)
        }
        Err(e) => error_response(&e),
    }
}

/// Handle a Ports request.
pub async fn handle_ports(req: PortsRequest, state: &AppState) -> Response {
    match ports::handle(req, state).await {
        Ok(resp) => Response::Ports(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an Exec request.
pub async fn handle_exec(req: ExecRequest, state: &AppState) -> Response {
    match exec::handle(req, state).await {
        Ok(resp) => Response::Exec(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Logs request.
pub async fn handle_logs(req: LogsRequest, state: &AppState) -> Response {
    match logs::handle(req, state).await {
        Ok(resp) => Response::Logs(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Logs request with a progress sender for streaming chunks.
pub async fn handle_logs_with_progress(
    req: LogsRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<LogsResponse>,
) -> coast_core::error::Result<LogsResponse> {
    logs::handle_with_progress(req, state, progress).await
}

/// Handle a Ps request.
pub async fn handle_ps(req: PsRequest, state: &AppState) -> Response {
    match ps::handle(req, state).await {
        Ok(resp) => Response::Ps(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an Ls request.
pub async fn handle_ls(req: LsRequest, state: &AppState) -> Response {
    match ls::handle(req, state).await {
        Ok(resp) => Response::Ls(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Lookup request.
pub async fn handle_lookup(req: LookupRequest, state: &AppState) -> Response {
    match lookup::handle(req, state).await {
        Ok(resp) => Response::Lookup(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Docs request.
pub async fn handle_docs(req: DocsRequest, state: &AppState) -> Response {
    match docs::handle_docs(req, state).await {
        Ok(resp) => Response::Docs(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a SearchDocs request.
pub async fn handle_search_docs(req: SearchDocsRequest, state: &AppState) -> Response {
    match docs::handle_search_docs(req, state).await {
        Ok(resp) => Response::SearchDocs(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Secret request.
pub async fn handle_secret(req: SecretRequest, state: &AppState) -> Response {
    match secret::handle(req, state).await {
        Ok(resp) => Response::Secret(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Shared request.
pub async fn handle_shared(req: SharedRequest, state: &AppState) -> Response {
    match shared::handle(req, state).await {
        Ok(resp) => Response::Shared(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an Assign request with a progress sender for streaming.
pub async fn handle_assign_with_progress(
    req: AssignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<AssignResponse> {
    let name = req.name.clone();
    let project = req.project.clone();
    let worktree = req.worktree.clone();
    match assign::handle(req, state, progress).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceAssigned {
                name,
                project,
                worktree,
            });
            Ok(resp)
        }
        Err(e) => {
            error!("assign failed for {name}: {e}");
            Err(e)
        }
    }
}

/// Handle an Unassign request with a progress sender for streaming.
pub async fn handle_unassign_with_progress(
    req: UnassignRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> coast_core::error::Result<UnassignResponse> {
    let name = req.name.clone();
    let project = req.project.clone();
    match unassign::handle(req, state, progress).await {
        Ok(resp) => {
            state.emit_event(CoastEvent::InstanceUnassigned {
                name,
                project,
                worktree: resp.worktree.clone(),
            });
            Ok(resp)
        }
        Err(e) => {
            error!("unassign failed for {name}: {e}");
            Err(e)
        }
    }
}

/// Handle a Rebuild request.
pub async fn handle_rebuild(req: RebuildRequest, state: &AppState) -> Response {
    match rebuild::handle(req, state).await {
        Ok(resp) => Response::Rebuild(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a Builds request.
pub async fn handle_builds(req: BuildsRequest, state: &AppState) -> Response {
    match builds::handle(req, state).await {
        Ok(resp) => Response::Builds(Box::new(resp)),
        Err(e) => error_response(&e),
    }
}

/// Handle an MCP Ls request.
pub async fn handle_mcp_ls(req: McpLsRequest, state: &AppState) -> Response {
    match mcp::handle_ls(req, state).await {
        Ok(resp) => Response::McpLs(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an MCP Tools request.
pub async fn handle_mcp_tools(req: McpToolsRequest, state: &AppState) -> Response {
    match mcp::handle_tools(req, state).await {
        Ok(resp) => Response::McpTools(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an MCP Locations request.
pub async fn handle_mcp_locations(req: McpLocationsRequest, state: &AppState) -> Response {
    match mcp::handle_locations(req, state).await {
        Ok(resp) => Response::McpLocations(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle an AgentShell request.
pub async fn handle_agent_shell(req: AgentShellRequest, state: &Arc<AppState>) -> Response {
    match agent_shell::handle(req, state).await {
        Ok(resp) => Response::AgentShell(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a SetAnalytics request.
pub async fn handle_set_analytics(req: SetAnalyticsRequest, state: &AppState) -> Response {
    match set_analytics::handle(req, state).await {
        Ok(resp) => Response::SetAnalytics(resp),
        Err(e) => error_response(&e),
    }
}

/// Handle a SetLanguage request.
pub async fn handle_set_language(req: SetLanguageRequest, state: &AppState) -> Response {
    match set_language::handle(req, state).await {
        Ok(resp) => Response::SetLanguage(resp),
        Err(e) => error_response(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let err = coast_core::error::CoastError::state("test error");
        let resp = error_response(&err);
        match resp {
            Response::Error(e) => {
                assert!(e.error.contains("test error"));
            }
            _ => panic!("expected Error response"),
        }
    }

    #[test]
    fn test_translate_error_instance_not_found_en_contains_name() {
        let err = coast_core::error::CoastError::InstanceNotFound {
            name: "feature-x".to_string(),
            project: "my-app".to_string(),
        };
        let msg = translate_error(&err, "en");
        assert!(
            msg.contains("feature-x"),
            "English translation should contain the instance name"
        );
        assert!(
            msg.contains("my-app"),
            "English translation should contain the project name"
        );
    }

    #[test]
    fn test_translate_error_instance_not_found_zh_differs_from_en() {
        let err = coast_core::error::CoastError::InstanceNotFound {
            name: "feature-x".to_string(),
            project: "my-app".to_string(),
        };
        let en_msg = translate_error(&err, "en");
        let zh_msg = translate_error(&err, "zh");
        assert_ne!(
            en_msg, zh_msg,
            "Chinese translation should differ from English"
        );
        assert!(
            zh_msg.contains("feature-x"),
            "Chinese translation should still contain the instance name"
        );
    }

    #[test]
    fn test_translate_error_instance_already_exists_contains_name() {
        let err = coast_core::error::CoastError::InstanceAlreadyExists {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
        };
        let msg = translate_error(&err, "en");
        assert!(
            msg.contains("dev-1"),
            "Translation should contain the instance name"
        );
        assert!(
            msg.contains("my-app"),
            "Translation should contain the project name"
        );
    }
}
