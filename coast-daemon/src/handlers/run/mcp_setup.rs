use tracing::{info, warn};

use coast_core::error::Result;
use coast_core::protocol::BuildProgressEvent;
use coast_docker::runtime::Runtime;

use super::emit;

/// Install internal MCP servers and generate client configs inside the coast container.
#[allow(clippy::cognitive_complexity)]
pub(super) async fn install_mcp_servers(
    container_id: &str,
    mcp_servers: &[coast_core::types::McpServerConfig],
    mcp_clients: &[coast_core::types::McpClientConnectorConfig],
    docker: &bollard::Docker,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<()> {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());

    for server in mcp_servers {
        if server.is_host_proxied() {
            continue;
        }

        emit(
            progress,
            BuildProgressEvent::item("Installing MCP", &server.name, "started"),
        );

        let mkdir_cmd = format!("mkdir -p /mcp/{}", server.name);
        let _ = runtime
            .exec_in_coast(container_id, &["sh", "-c", &mkdir_cmd])
            .await;

        if let Some(ref source) = server.source {
            let cp_cmd = format!(
                "cp -a /workspace/{}/.  /mcp/{}/",
                source.trim_start_matches("./"),
                server.name
            );
            let cp_result = runtime
                .exec_in_coast(container_id, &["sh", "-c", &cp_cmd])
                .await;
            match cp_result {
                Ok(r) if !r.success() => {
                    warn!(
                        server = %server.name,
                        stderr = %r.stderr,
                        "MCP source copy failed"
                    );
                }
                Err(e) => {
                    warn!(server = %server.name, error = %e, "MCP source copy failed");
                }
                _ => {}
            }
        }

        for install_cmd in &server.install {
            emit(
                progress,
                BuildProgressEvent::item("Installing MCP", install_cmd, "started"),
            );
            let full_cmd = format!("cd /mcp/{} && {}", server.name, install_cmd);
            let install_result = runtime
                .exec_in_coast(container_id, &["sh", "-c", &full_cmd])
                .await;
            match install_result {
                Ok(r) if !r.success() => {
                    warn!(
                        server = %server.name,
                        cmd = %install_cmd,
                        stderr = %r.stderr,
                        exit_code = r.exit_code,
                        "MCP install command failed"
                    );
                }
                Err(e) => {
                    warn!(
                        server = %server.name,
                        cmd = %install_cmd,
                        error = %e,
                        "MCP install command failed"
                    );
                }
                _ => {}
            }
        }

        info!(server = %server.name, "MCP server installed at /mcp/{}", server.name);
    }

    if !mcp_clients.is_empty() && !mcp_servers.is_empty() {
        emit(
            progress,
            BuildProgressEvent::item("Installing MCP", "Writing client configs", "started"),
        );

        for client in mcp_clients {
            if let Some(ref format) = client.format {
                let config_json =
                    super::super::mcp::generate_mcp_client_config(mcp_servers, format);
                let config_path = client
                    .resolved_config_path()
                    .unwrap_or(format.default_config_path());

                let parent_dir = std::path::Path::new(config_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let write_cmd = format!(
                    "mkdir -p '{parent}' && cat > '{path}' << 'COAST_MCP_EOF'\n{json}\nCOAST_MCP_EOF",
                    parent = parent_dir,
                    path = config_path,
                    json = config_json,
                );
                let write_result = runtime
                    .exec_in_coast(container_id, &["sh", "-c", &write_cmd])
                    .await;
                match write_result {
                    Ok(r) if r.success() => {
                        info!(
                            client = %client.name,
                            path = %config_path,
                            "MCP client config written"
                        );
                    }
                    Ok(r) => {
                        warn!(
                            client = %client.name,
                            stderr = %r.stderr,
                            "Failed to write MCP client config"
                        );
                    }
                    Err(e) => {
                        warn!(
                            client = %client.name,
                            error = %e,
                            "Failed to write MCP client config"
                        );
                    }
                }
            } else if let Some(ref run_cmd) = client.run {
                let manifest = super::super::mcp::generate_mcp_client_config(
                    mcp_servers,
                    &coast_core::types::McpClientFormat::ClaudeCode,
                );
                let pipe_cmd = format!(
                    "cat << 'COAST_MCP_EOF' | {cmd}\n{json}\nCOAST_MCP_EOF",
                    cmd = run_cmd,
                    json = manifest,
                );
                let _ = runtime
                    .exec_in_coast(container_id, &["sh", "-c", &pipe_cmd])
                    .await;
            }
        }
    }

    Ok(())
}
