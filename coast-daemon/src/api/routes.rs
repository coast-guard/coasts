use std::sync::Arc;

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use coast_core::protocol::*;
use coast_docker::runtime::Runtime;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use rust_i18n::t;

use crate::handlers;
use crate::handlers::compose_context;
use crate::server::{AppState, UpdateOperationGuard, UpdateOperationKind};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/stop", post(stop))
        .route("/start", post(start))
        .route("/rm", post(rm))
        .route("/checkout", post(checkout))
        .route("/ports", post(ports))
        .route("/exec", post(exec))
        .route("/logs", post(logs))
        .route("/logs/clear", post(clear_logs))
        .route("/ps", post(ps))
        .route("/secret", post(secret))
        .route("/shared", post(shared))
        .route("/rebuild", post(rebuild))
        .route("/restart-services", post(restart_services))
        .route("/upload", post(upload_to_container))
        .route("/upload/host", post(upload_to_host))
        .route("/service/stop", post(service_stop))
        .route("/service/start", post(service_start))
        .route("/service/restart", post(service_restart))
        .route("/service/rm", post(service_rm))
        .route("/bare-service/stop", post(bare_service_stop))
        .route("/bare-service/start", post(bare_service_start))
        .route("/bare-service/restart", post(bare_service_restart))
        .route("/port-health", post(port_health))
        .route("/rm-build", post(rm_build))
        .route("/archive", post(archive_project))
        .route("/unarchive", post(unarchive_project))
        .route("/remote/add", post(remote_add))
        .route("/remote/rm", post(remote_rm))
}

pub(crate) fn to_api_response(resp: Response) -> axum::response::Response {
    match resp {
        Response::Error(e) => {
            let status = if e.error.contains("not found") || e.error.contains("NotFound") {
                StatusCode::NOT_FOUND
            } else if e.error.contains("already")
                || e.error.contains("stopped")
                || e.error.contains("still being")
            {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({ "error": e.error }))).into_response()
        }
        other => (StatusCode::OK, Json(other)).into_response(),
    }
}

/// Like [`to_api_response`] but translates error messages using the provided locale.
#[allow(dead_code)]
pub(crate) fn to_api_response_i18n(resp: Response, lang: &str) -> axum::response::Response {
    match resp {
        Response::Error(ref e) => {
            let status = if e.error.contains("not found") || e.error.contains("NotFound") {
                StatusCode::NOT_FOUND
            } else if e.error.contains("already")
                || e.error.contains("stopped")
                || e.error.contains("still being")
            {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            // The error is already in the response; for i18n, the handler
            // should have already translated it. We just pass it through.
            let _ = lang;
            (status, Json(json!({ "error": e.error }))).into_response()
        }
        other => (StatusCode::OK, Json(other)).into_response(),
    }
}

fn update_operation_error_response(
    error: &coast_core::error::CoastError,
) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": error.to_string() })),
    )
        .into_response()
}

fn begin_api_update_operation(
    state: &AppState,
    kind: UpdateOperationKind,
    project: Option<&str>,
    instance: Option<&str>,
) -> Result<UpdateOperationGuard, Box<axum::response::Response>> {
    state
        .begin_update_operation(kind, project, instance)
        .map_err(|error| Box::new(update_operation_error_response(&error)))
}

async fn stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StopRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::Stop,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    to_api_response(handlers::handle_stop(req, &state).await)
}

async fn start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::Start,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    to_api_response(handlers::handle_start(req, &state).await)
}

async fn rm(State(state): State<Arc<AppState>>, Json(req): Json<RmRequest>) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::Rm,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem.acquire().await;
    to_api_response(handlers::handle_rm(req, &state).await)
}

async fn rm_build(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RmBuildRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::RmBuild,
        Some(&req.project),
        None,
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem.acquire().await;
    to_api_response(handlers::handle_rm_build(req, &state).await)
}

async fn archive_project(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ArchiveProjectRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ArchiveProject,
        Some(&req.project),
        None,
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    to_api_response(handlers::handle_archive_project(req, &state).await)
}

async fn unarchive_project(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnarchiveProjectRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::UnarchiveProject,
        Some(&req.project),
        None,
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    to_api_response(handlers::handle_unarchive_project(req, &state).await)
}

async fn checkout(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CheckoutRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::Checkout,
        Some(&req.project),
        req.name.as_deref(),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    to_api_response(handlers::handle_checkout(req, &state).await)
}

async fn ports(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PortsRequest>,
) -> impl IntoResponse {
    to_api_response(handlers::handle_ports(req, &state).await)
}

async fn exec(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecRequest>,
) -> impl IntoResponse {
    to_api_response(handlers::handle_exec(req, &state).await)
}

async fn logs(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LogsRequest>,
) -> impl IntoResponse {
    to_api_response(handlers::handle_logs(req, &state).await)
}

async fn ps(State(state): State<Arc<AppState>>, Json(req): Json<PsRequest>) -> impl IntoResponse {
    to_api_response(handlers::handle_ps(req, &state).await)
}

async fn secret(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SecretRequest>,
) -> impl IntoResponse {
    if let SecretRequest::Set {
        instance, project, ..
    } = &req
    {
        let _operation_guard = match begin_api_update_operation(
            &state,
            UpdateOperationKind::SecretSet,
            Some(project),
            Some(instance),
        ) {
            Ok(guard) => guard,
            Err(response) => return *response,
        };
        return to_api_response(handlers::handle_secret(req, &state).await);
    }
    to_api_response(handlers::handle_secret(req, &state).await)
}

async fn shared(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SharedRequest>,
) -> impl IntoResponse {
    let kind = match &req {
        SharedRequest::Ps { .. } => None,
        SharedRequest::Stop { .. } => Some(UpdateOperationKind::SharedStop),
        SharedRequest::Start { .. } => Some(UpdateOperationKind::SharedStart),
        SharedRequest::Restart { .. } => Some(UpdateOperationKind::SharedRestart),
        SharedRequest::Rm { .. } => Some(UpdateOperationKind::SharedRm),
    };
    if let Some(kind) = kind {
        let project = match &req {
            SharedRequest::Ps { project }
            | SharedRequest::Stop { project, .. }
            | SharedRequest::Start { project, .. }
            | SharedRequest::Restart { project, .. }
            | SharedRequest::Rm { project, .. } => project,
        };
        let _operation_guard = match begin_api_update_operation(&state, kind, Some(project), None) {
            Ok(guard) => guard,
            Err(response) => return *response,
        };
        return to_api_response(handlers::handle_shared(req, &state).await);
    }
    to_api_response(handlers::handle_shared(req, &state).await)
}

async fn rebuild(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RebuildRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::Rebuild,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem.acquire().await;
    to_api_response(handlers::handle_rebuild(req, &state).await)
}

async fn restart_services(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RestartServicesRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::RestartServices,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let sem = state.project_semaphore(&req.project).await;
    let _permit = sem.acquire().await;
    to_api_response(handlers::handle_restart_services(req, &state).await)
}

#[derive(serde::Deserialize)]
struct ClearLogsRequest {
    name: String,
    project: String,
}

async fn clear_logs(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClearLogsRequest>,
) -> impl IntoResponse {
    use coast_core::types::InstanceStatus;
    use coast_docker::runtime::Runtime;

    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ClearLogs,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let lang = state.language();
    let db = state.db.lock().await;
    let instance = match db.get_instance(&req.project, &req.name) {
        Ok(Some(i)) => i,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = &req.name, project = &req.project).to_string() })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if instance.status == InstanceStatus::Stopped {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": t!("error.instance_stopped", locale = &lang, name = &req.name).to_string() })),
        )
            .into_response();
    }

    let Some(container_id) = instance.container_id.as_deref() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": t!("error.no_container_id", locale = &lang).to_string() })),
        )
            .into_response();
    };

    let Some(docker) = state.docker.as_ref() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": t!("error.docker_not_available", locale = &lang).to_string() })),
        )
            .into_response();
    };

    drop(db);

    let ctx = handlers::compose_context(&req.project);
    let _cmd_parts = ctx.compose_shell("down --remove-orphans && docker compose up -d");

    // Truncate inner container log files by finding and zeroing them
    let truncate_cmd = vec![
        "sh", "-c",
        "for f in $(find /var/lib/docker/containers -name '*-json.log' 2>/dev/null); do truncate -s 0 \"$f\"; done; echo ok",
    ];

    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    match runtime.exec_in_coast(container_id, &truncate_cmd).await {
        Ok(_) => (StatusCode::OK, Json(ClearLogsResponse { cleared: true })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Parse multipart fields for an upload request.
async fn parse_upload_fields(
    multipart: &mut Multipart,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
) {
    let mut project = None;
    let mut name = None;
    let mut file_name = None;
    let mut file_data = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "project" => project = field.text().await.ok(),
            "name" => name = field.text().await.ok(),
            "file" => {
                file_name = field.file_name().map(std::string::ToString::to_string);
                file_data = field.bytes().await.ok().map(|b| b.to_vec());
            }
            _ => {}
        }
    }

    (project, name, file_name, file_data)
}

/// Validate that all required upload fields are present, returning an error response if not.
fn validate_upload_fields(
    project: Option<String>,
    name: Option<String>,
    file_name: Option<String>,
    file_data: Option<Vec<u8>>,
    lang: &str,
) -> Result<(String, String, String, Vec<u8>), Box<axum::response::Response>> {
    let bad_request = |field: &str| -> Box<axum::response::Response> {
        Box::new(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": t!("error.missing_field", locale = lang, field = field).to_string() })),
            )
                .into_response(),
        )
    };
    let project = project.ok_or_else(|| bad_request("project"))?;
    let name = name.ok_or_else(|| bad_request("name"))?;
    let fname = file_name.ok_or_else(|| bad_request("file"))?;
    let data = file_data.ok_or_else(|| {
        Box::new(
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": t!("error.empty_file", locale = lang).to_string() })),
            )
                .into_response(),
        )
    })?;
    Ok((project, name, fname, data))
}

/// Resolve the target container for an upload, validating the instance is running.
async fn resolve_upload_target(
    state: &AppState,
    project: &str,
    name: &str,
    lang: &str,
) -> Result<String, axum::response::Response> {
    use coast_core::types::InstanceStatus;

    let db = state.db.lock().await;
    let instance = match db.get_instance(project, name) {
        Ok(Some(i)) => i,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": t!("error.instance_not_found", locale = lang, name = name, project = project).to_string() })),
            )
                .into_response())
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response())
        }
    };

    if instance.status == InstanceStatus::Stopped {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": t!("error.instance_stopped", locale = lang, name = name).to_string() })),
        )
            .into_response());
    }

    let container_id = instance.container_id.ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(json!({ "error": t!("error.no_container_id", locale = lang).to_string() })),
        )
            .into_response()
    })?;

    if state.docker.is_none() {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": t!("error.docker_not_available", locale = lang).to_string() })),
        )
            .into_response());
    }

    Ok(container_id)
}

/// Copy a file into a container via docker cp.
async fn docker_cp_to_container(
    container_id: &str,
    data: &[u8],
    fname: &str,
) -> axum::response::Response {
    let upload_dir = "/coast-uploads";
    let container_path = format!("{upload_dir}/{fname}");

    let mkdir_result = tokio::process::Command::new("docker")
        .args(["exec", container_id, "mkdir", "-p", upload_dir])
        .output()
        .await;
    if let Ok(out) = &mkdir_result {
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("mkdir failed: {stderr}") })),
            )
                .into_response();
        }
    }

    let tmp = match tempfile::NamedTempFile::new() {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };
    if let Err(e) = std::fs::write(tmp.path(), data) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    let cp_result = tokio::process::Command::new("docker")
        .args([
            "cp",
            &tmp.path().to_string_lossy(),
            &format!("{container_id}:{container_path}"),
        ])
        .output()
        .await;

    match cp_result {
        Ok(output) if output.status.success() => (
            StatusCode::OK,
            Json(UploadResponse {
                path: container_path,
            }),
        )
            .into_response(),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("docker cp failed: {stderr}") })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn upload_to_container(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let (project, name, file_name, file_data) = parse_upload_fields(&mut multipart).await;
    let lang = state.language();

    let (project, name, fname, data) =
        match validate_upload_fields(project, name, file_name, file_data, &lang) {
            Ok(fields) => fields,
            Err(resp) => return *resp,
        };

    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::UploadToContainer,
        Some(&project),
        Some(&name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };

    let container_id = match resolve_upload_target(&state, &project, &name, &lang).await {
        Ok(cid) => cid,
        Err(resp) => return resp,
    };

    docker_cp_to_container(&container_id, &data, &fname).await
}

async fn upload_to_host(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        if field_name == "file" {
            file_name = field.file_name().map(std::string::ToString::to_string);
            file_data = field.bytes().await.ok().map(|b| b.to_vec());
        }
    }

    let Some(fname) = file_name else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "missing 'file' field" })),
        )
            .into_response();
    };
    let Some(data) = file_data else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "empty file" })),
        )
            .into_response();
    };

    let _operation_guard =
        match begin_api_update_operation(&state, UpdateOperationKind::UploadToHost, None, None) {
            Ok(guard) => guard,
            Err(response) => return *response,
        };

    let dest = format!("/tmp/{fname}");
    match std::fs::write(&dest, &data) {
        Ok(()) => (StatusCode::OK, Json(UploadResponse { path: dest })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Service lifecycle controls
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, TS)]
#[ts(export)]
pub struct ServiceControlRequest {
    pub project: String,
    pub name: String,
    pub service: String,
}

async fn resolve_container_id(
    state: &AppState,
    project: &str,
    name: &str,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let lang = state.language();
    let db = state.db.lock().await;
    let instance = db
        .get_instance(project, name)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": t!("error.instance_not_found", locale = &lang, name = name, project = project).to_string() })),
            )
        })?;

    if instance.status == coast_core::types::InstanceStatus::Stopped {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({ "error": t!("error.instance_stopped", locale = &lang, name = name).to_string() }),
            ),
        ));
    }

    if instance.status == coast_core::types::InstanceStatus::Provisioning
        || instance.status == coast_core::types::InstanceStatus::Assigning
    {
        let err = if instance.status == coast_core::types::InstanceStatus::Provisioning {
            t!(
                "error.instance_still_provisioning",
                locale = &lang,
                name = name
            )
            .to_string()
        } else {
            t!(
                "error.instance_still_assigning",
                locale = &lang,
                name = name
            )
            .to_string()
        };
        return Err((StatusCode::CONFLICT, Json(json!({ "error": err }))));
    }

    instance.container_id.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": t!("error.no_container_id", locale = &lang).to_string() })),
        )
    })
}

async fn try_remote_service_control(
    state: &AppState,
    project: &str,
    name: &str,
    service: &str,
    action: &str,
) -> Option<Result<(), String>> {
    let db = state.db.lock().await;
    let is_remote = db
        .get_instance(project, name)
        .ok()
        .flatten()
        .is_some_and(|i| i.remote_host.is_some());
    drop(db);

    if !is_remote {
        return None;
    }

    let remote_config =
        match crate::handlers::remote::resolve_remote_for_instance(project, name, state).await {
            Ok(c) => c,
            Err(e) => return Some(Err(e.to_string())),
        };
    let client = match crate::handlers::remote::RemoteClient::connect(&remote_config).await {
        Ok(c) => c,
        Err(e) => return Some(Err(e.to_string())),
    };
    let req = coast_core::protocol::RemoteServiceControlRequest {
        project: project.to_string(),
        name: name.to_string(),
        service: service.to_string(),
        action: action.to_string(),
    };
    match crate::handlers::remote::forward::forward_service_control(&client, &req).await {
        Ok(_) => Some(Ok(())),
        Err(e) => Some(Err(e.to_string())),
    }
}

async fn run_compose_in_coast(
    state: &AppState,
    container_id: &str,
    project: &str,
    subcmd: &str,
) -> Result<String, String> {
    let lang = state.language();
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| t!("error.docker_not_available", locale = &lang).to_string())?;
    let ctx = compose_context(project);
    let cmd_parts = ctx.compose_shell(subcmd);

    let exec_options = CreateExecOptions {
        cmd: Some(cmd_parts),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        ..Default::default()
    };

    let exec = docker
        .create_exec(container_id, exec_options)
        .await
        .map_err(|e| format!("Failed to create exec: {e}"))?;

    let start_options = StartExecOptions {
        detach: false,
        ..Default::default()
    };

    let output = docker
        .start_exec(&exec.id, Some(start_options))
        .await
        .map_err(|e| format!("Failed to start exec: {e}"))?;

    let mut stdout = String::new();
    let mut stderr = String::new();

    if let StartExecResults::Attached { mut output, .. } = output {
        while let Some(chunk) = output.next().await {
            if let Ok(msg) = chunk {
                match msg {
                    bollard::container::LogOutput::StdOut { message } => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    bollard::container::LogOutput::StdErr { message } => {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    _ => {}
                }
            }
        }
    }

    let inspect = docker.inspect_exec(&exec.id).await.ok();
    let exit_code = inspect.and_then(|i| i.exit_code).unwrap_or(0);

    if exit_code != 0 {
        let msg = if stderr.is_empty() { stdout } else { stderr };
        Err(format!("Exit code {exit_code}: {}", msg.trim()))
    } else {
        Ok(stdout)
    }
}

async fn service_stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ServiceStop,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };

    state.emit_event(CoastEvent::ServiceStopping {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });

    if let Some(result) =
        try_remote_service_control(&state, &req.project, &req.name, &req.service, "stop").await
    {
        return match result {
            Ok(()) => {
                state.emit_event(CoastEvent::ServiceStopped {
                    name: req.name,
                    project: req.project,
                    service: req.service,
                });
                (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
            }
            Err(e) => {
                state.emit_event(CoastEvent::ServiceError {
                    name: req.name,
                    project: req.project,
                    service: req.service.clone(),
                    error: e.clone(),
                });
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response()
            }
        };
    }

    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    match run_compose_in_coast(
        &state,
        &container_id,
        &req.project,
        &format!("stop {}", req.service),
    )
    .await
    {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceStopped {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service.clone(),
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn service_start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ServiceStart,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };

    state.emit_event(CoastEvent::ServiceStarting {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });

    if let Some(result) =
        try_remote_service_control(&state, &req.project, &req.name, &req.service, "start").await
    {
        return match result {
            Ok(()) => {
                state.emit_event(CoastEvent::ServiceStarted {
                    name: req.name,
                    project: req.project,
                    service: req.service,
                });
                (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
            }
            Err(e) => {
                state.emit_event(CoastEvent::ServiceError {
                    name: req.name,
                    project: req.project,
                    service: req.service.clone(),
                    error: e.clone(),
                });
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response()
            }
        };
    }

    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    match run_compose_in_coast(
        &state,
        &container_id,
        &req.project,
        &format!("start {}", req.service),
    )
    .await
    {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceStarted {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service.clone(),
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn service_restart(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ServiceRestart,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };

    state.emit_event(CoastEvent::ServiceRestarting {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });

    if let Some(result) =
        try_remote_service_control(&state, &req.project, &req.name, &req.service, "restart").await
    {
        return match result {
            Ok(()) => {
                state.emit_event(CoastEvent::ServiceRestarted {
                    name: req.name,
                    project: req.project,
                    service: req.service,
                });
                (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
            }
            Err(e) => {
                state.emit_event(CoastEvent::ServiceError {
                    name: req.name,
                    project: req.project,
                    service: req.service.clone(),
                    error: e.clone(),
                });
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response()
            }
        };
    }

    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    match run_compose_in_coast(
        &state,
        &container_id,
        &req.project,
        &format!("restart {}", req.service),
    )
    .await
    {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceRestarted {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service.clone(),
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn service_rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::ServiceRm,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    state.emit_event(CoastEvent::ServiceRemoving {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });

    match run_compose_in_coast(
        &state,
        &container_id,
        &req.project,
        &format!("rm -f -s {}", req.service),
    )
    .await
    {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceRemoved {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service.clone(),
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Bare service lifecycle controls
// ---------------------------------------------------------------------------

fn resolve_bare_service_port(_state: &AppState, project: &str, service: &str) -> Option<u16> {
    let home = dirs::home_dir()?;
    let cf_path = home
        .join(".coast")
        .join("images")
        .join(project)
        .join("latest")
        .join("coastfile.toml");
    let cf = coast_core::coastfile::Coastfile::from_file(&cf_path).ok()?;
    cf.services
        .iter()
        .find(|s| s.name == service)
        .and_then(|s| s.port)
}

async fn run_bare_exec(
    state: &AppState,
    container_id: &str,
    cmd: &str,
) -> std::result::Result<String, String> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| "Docker not available".to_string())?;
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    let result = rt
        .exec_in_coast(container_id, &["sh", "-c", cmd])
        .await
        .map_err(|e| format!("exec failed: {e}"))?;
    if result.success() {
        Ok(result.stdout)
    } else {
        Err(result.stderr)
    }
}

/// Kill a bare service's process tree. The .pid file tracks the wrapper shell,
/// but the actual server (Node/Vite) is a child. We kill the wrapper, then
/// find and kill any remaining children and any process using the service's port.
fn bare_stop_script(svc: &str, port: Option<u16>) -> String {
    let dir = crate::bare_services::SUPERVISOR_DIR;
    let mut script = format!(
        "PID=$(cat {dir}/{svc}.pid 2>/dev/null); \
         if [ -n \"$PID\" ]; then \
           kill \"$PID\" 2>/dev/null; \
           for CHILD in $(ps -o pid= --ppid \"$PID\" 2>/dev/null || true); do \
             kill \"$CHILD\" 2>/dev/null; \
           done; \
         fi; \
         rm -f {dir}/{svc}.pid"
    );
    if let Some(p) = port {
        script.push_str(&format!(
            "; sleep 0.5; \
             PIDS=$(fuser {p}/tcp 2>/dev/null || true); \
             for P in $PIDS; do kill \"$P\" 2>/dev/null; done",
        ));
    }
    script
}

async fn bare_service_stop(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::BareServiceStop,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    state.emit_event(CoastEvent::ServiceStopping {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });
    let port = resolve_bare_service_port(&state, &req.project, &req.service);
    let cmd = bare_stop_script(&req.service, port);
    match run_bare_exec(&state, &container_id, &cmd).await {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceStopped {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service,
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn bare_service_start(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::BareServiceStart,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    state.emit_event(CoastEvent::ServiceStarting {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });
    let cmd = format!(
        "sh {dir}/{svc}.sh &",
        dir = crate::bare_services::SUPERVISOR_DIR,
        svc = req.service,
    );
    match run_bare_exec(&state, &container_id, &cmd).await {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceStarted {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service,
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

async fn bare_service_restart(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceControlRequest>,
) -> impl IntoResponse {
    let _operation_guard = match begin_api_update_operation(
        &state,
        UpdateOperationKind::BareServiceRestart,
        Some(&req.project),
        Some(&req.name),
    ) {
        Ok(guard) => guard,
        Err(response) => return *response,
    };
    let container_id = match resolve_container_id(&state, &req.project, &req.name).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    state.emit_event(CoastEvent::ServiceRestarting {
        name: req.name.clone(),
        project: req.project.clone(),
        service: req.service.clone(),
    });
    let port = resolve_bare_service_port(&state, &req.project, &req.service);
    let dir = crate::bare_services::SUPERVISOR_DIR;
    let cmd = format!(
        "{}; sleep 1; sh {dir}/{svc}.sh &",
        bare_stop_script(&req.service, port),
        dir = dir,
        svc = req.service,
    );
    match run_bare_exec(&state, &container_id, &cmd).await {
        Ok(_) => {
            state.emit_event(CoastEvent::ServiceRestarted {
                name: req.name,
                project: req.project,
                service: req.service,
            });
            (StatusCode::OK, Json(SuccessResponse { success: true })).into_response()
        }
        Err(e) => {
            state.emit_event(CoastEvent::ServiceError {
                name: req.name,
                project: req.project,
                service: req.service,
                error: e.clone(),
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Port health
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PortHealthRequest {
    project: String,
    name: String,
}

async fn port_health(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PortHealthRequest>,
) -> impl IntoResponse {
    let key = format!("{}:{}", req.project, req.name);
    let cache = state.port_health_cache.lock().await;
    let ports = cache.get(&key).cloned().unwrap_or_default();
    Json(json!({ "ports": ports }))
}

// ---------------------------------------------------------------------------
// Remote management
// ---------------------------------------------------------------------------

async fn remote_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoteRequest>,
) -> impl IntoResponse {
    to_api_response(handlers::handle_remote(req, &state).await)
}

async fn remote_rm(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RemoteRequest>,
) -> impl IntoResponse {
    to_api_response(handlers::handle_remote(req, &state).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_upload_fields_all_present() {
        let result = validate_upload_fields(
            Some("my-app".into()),
            Some("feat-a".into()),
            Some("data.csv".into()),
            Some(vec![1, 2, 3]),
            "en",
        );
        let (project, name, fname, data) = result.unwrap();
        assert_eq!(project, "my-app");
        assert_eq!(name, "feat-a");
        assert_eq!(fname, "data.csv");
        assert_eq!(data, vec![1, 2, 3]);
    }

    #[test]
    fn test_validate_upload_fields_missing_project() {
        let result = validate_upload_fields(
            None,
            Some("a".into()),
            Some("f".into()),
            Some(vec![1]),
            "en",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_upload_fields_missing_name() {
        let result = validate_upload_fields(
            Some("p".into()),
            None,
            Some("f".into()),
            Some(vec![1]),
            "en",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_upload_fields_missing_file() {
        let result = validate_upload_fields(
            Some("p".into()),
            Some("n".into()),
            None,
            Some(vec![1]),
            "en",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_upload_fields_missing_data() {
        let result = validate_upload_fields(
            Some("p".into()),
            Some("n".into()),
            Some("f".into()),
            None,
            "en",
        );
        assert!(result.is_err());
    }
}
