use tracing::debug;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::*;

use super::RemoteClient;

/// Forward a BuildRequest to the remote coast-service.
pub async fn forward_build(client: &RemoteClient, req: &BuildRequest) -> Result<BuildResponse> {
    forward_request(&client.service_url, "/build", req).await
}

/// Forward a RunRequest to the remote coast-service.
pub async fn forward_run(client: &RemoteClient, req: &RunRequest) -> Result<RunResponse> {
    forward_request(&client.service_url, "/run", req).await
}

/// Forward an AssignRequest to the remote coast-service.
pub async fn forward_assign(client: &RemoteClient, req: &AssignRequest) -> Result<AssignResponse> {
    forward_request(&client.service_url, "/assign", req).await
}

/// Forward an ExecRequest to the remote coast-service.
pub async fn forward_exec(client: &RemoteClient, req: &ExecRequest) -> Result<ExecResponse> {
    forward_request(&client.service_url, "/exec", req).await
}

/// Forward a StopRequest to the remote coast-service.
pub async fn forward_stop(client: &RemoteClient, req: &StopRequest) -> Result<StopResponse> {
    forward_request(&client.service_url, "/stop", req).await
}

/// Forward a StartRequest to the remote coast-service.
pub async fn forward_start(client: &RemoteClient, req: &StartRequest) -> Result<StartResponse> {
    forward_request(&client.service_url, "/start", req).await
}

/// Forward an RmRequest to the remote coast-service.
pub async fn forward_rm(client: &RemoteClient, req: &RmRequest) -> Result<RmResponse> {
    forward_request(&client.service_url, "/rm", req).await
}

/// Forward a PruneRequest to the remote coast-service.
pub async fn forward_prune(
    client: &RemoteClient,
    req: &coast_core::protocol::api_types::PruneRequest,
) -> Result<coast_core::protocol::api_types::PruneResponse> {
    forward_request(&client.service_url, "/prune", req).await
}

/// Forward a PsRequest to the remote coast-service.
pub async fn forward_ps(client: &RemoteClient, req: &PsRequest) -> Result<PsResponse> {
    forward_request(&client.service_url, "/ps", req).await
}

/// Forward a LogsRequest to the remote coast-service.
pub async fn forward_logs(client: &RemoteClient, req: &LogsRequest) -> Result<LogsResponse> {
    forward_request(&client.service_url, "/logs", req).await
}

/// Forward a SecretRequest to the remote coast-service.
pub async fn forward_secret(client: &RemoteClient, req: &SecretRequest) -> Result<SecretResponse> {
    forward_request(&client.service_url, "/secret", req).await
}

/// Forward a per-service control request (stop/start/restart) to the remote coast-service.
pub async fn forward_service_control(
    client: &RemoteClient,
    req: &RemoteServiceControlRequest,
) -> Result<RemoteServiceControlResponse> {
    forward_request(&client.service_url, "/service/control", req).await
}

/// Forward a RestartServicesRequest to the remote coast-service.
pub async fn forward_restart_services(
    client: &RemoteClient,
    req: &RestartServicesRequest,
) -> Result<RestartServicesResponse> {
    forward_request(&client.service_url, "/restart-services", req).await
}

/// Forward a SecretRequest (List) to the remote coast-service.
pub async fn forward_secret_list(
    client: &RemoteClient,
    req: &SecretRequest,
) -> Result<SecretResponse> {
    forward_request(&client.service_url, "/secret", req).await
}

/// Forward a RevealSecretRequest to the remote coast-service.
pub async fn forward_secrets_reveal(
    client: &RemoteClient,
    req: &RevealSecretRequest,
) -> Result<RevealSecretResponse> {
    forward_request(&client.service_url, "/secrets/reveal", req).await
}

/// Forward an McpLsRequest to the remote coast-service.
pub async fn forward_mcp_ls(client: &RemoteClient, req: &McpLsRequest) -> Result<McpLsResponse> {
    forward_request(&client.service_url, "/mcp/ls", req).await
}

/// Forward an McpToolsRequest to the remote coast-service.
pub async fn forward_mcp_tools(
    client: &RemoteClient,
    req: &McpToolsRequest,
) -> Result<McpToolsResponse> {
    forward_request(&client.service_url, "/mcp/tools", req).await
}

/// Forward a ContainerStatsRequest to the remote coast-service.
pub async fn forward_container_stats(
    client: &RemoteClient,
    req: &ContainerStatsRequest,
) -> Result<ContainerStatsResponse> {
    forward_request(&client.service_url, "/container-stats", req).await
}

/// Generic request forwarder: POST JSON to coast-service and deserialize response.
async fn forward_request<Req, Resp>(base_url: &str, path: &str, req: &Req) -> Result<Resp>
where
    Req: serde::Serialize + std::fmt::Debug,
    Resp: serde::de::DeserializeOwned,
{
    let url = format!("{base_url}{path}");
    debug!(?req, url = %url, "forwarding request to coast-service");

    let client = reqwest::Client::new();
    let response = client.post(&url).json(req).send().await.map_err(|e| {
        CoastError::state(format!(
            "failed to send request to coast-service at {url}: {e}"
        ))
    })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(CoastError::state(format!(
            "coast-service returned error {status}: {body}"
        )));
    }

    let resp: Resp = response
        .json()
        .await
        .map_err(|e| CoastError::state(format!("failed to parse coast-service response: {e}")))?;

    Ok(resp)
}
