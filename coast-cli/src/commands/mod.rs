pub mod agent_shell;
/// Command modules for the Coast CLI.
///
/// Each module implements one CLI subcommand, handling argument parsing,
/// request construction, daemon communication, and output formatting.
pub mod archive;
pub mod assign;
pub mod build;
pub mod builds;
pub mod checkout;
pub mod config;
pub mod daemon;
pub mod dns;
pub mod docker;
pub mod docs;
pub mod doctor;
pub mod exec;
pub mod installation_prompt;
pub mod logs;
pub mod lookup;
pub mod ls;
pub mod mcp;
pub mod nuke;
pub mod ports;
pub mod ps;
pub mod rebuild;
pub mod rerun_extractors;
pub mod rm;
pub mod rm_build;
pub mod run;
pub mod search_docs;
pub mod secret;
pub mod shared;
pub mod skills_prompt;
pub mod start;
pub mod stop;
pub mod ui;
pub mod unassign;
pub mod update;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use coast_core::protocol::{self, BuildProgressEvent, Request, Response};
use coast_core::types::PortMapping;
use colored::Colorize;
use rust_i18n::t;

/// Default path for the daemon socket.
fn socket_path() -> std::path::PathBuf {
    coast_core::artifact::coast_home()
        .expect("Could not determine coast home directory")
        .join("coastd.sock")
}

/// Send a request to the coastd daemon and receive a response.
///
/// Connects to the Unix domain socket at `~/.coast/coastd.sock`, writes the
/// JSON-encoded request, reads the JSON-encoded response line, and decodes it.
///
/// Returns a clear error if the daemon is not running.
pub async fn send_request(request: Request) -> Result<Response> {
    send_request_to(request, &socket_path()).await
}

/// Send a request to a specific daemon socket path.
///
/// This is the implementation behind [`send_request`], exposed separately
/// for testing with custom socket paths.
pub async fn send_request_to(request: Request, sock: &std::path::Path) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    // Encode and send request
    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    // Read response line
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader
        .read_line(&mut line)
        .await
        .context("Failed to read response from coastd")?;

    if line.is_empty() {
        bail!("{}", t!("error.daemon_closed"));
    }

    // Decode response
    let response = protocol::decode_response(line.trim_end().as_bytes())
        .context("Failed to decode response from coastd")?;

    Ok(response)
}

/// Send a logs request and stream log chunks to a callback.
///
/// Reads line-delimited responses from the daemon. `LogsProgress` responses are
/// forwarded to `on_chunk`; the final `Logs` or `Error` response is returned.
pub async fn send_logs_request(
    request: Request,
    mut on_chunk: impl FnMut(&str),
) -> Result<Response> {
    send_logs_request_to(request, &socket_path(), &mut on_chunk).await
}

/// Streaming logs request implementation with custom socket path.
async fn send_logs_request_to(
    request: Request,
    sock: &std::path::Path,
    on_chunk: &mut impl FnMut(&str),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::LogsProgress(ref event) => {
                on_chunk(&event.output);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send a build request and stream progress events to a callback.
///
/// Connects to the daemon, sends the request, then reads JSON lines in a loop.
/// `BuildProgress` lines are passed to `on_progress`; the final
/// `Build` or `Error` response is returned.
pub async fn send_build_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_build_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming build request implementation with custom socket path.
async fn send_build_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::BuildProgress(ref event) => {
                on_progress(event);
            }
            Response::RerunExtractorsProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send a run request and stream progress events to a callback.
///
/// Connects to the daemon, sends the request, then reads JSON lines in a loop.
/// `RunProgress` lines are passed to `on_progress`; the final
/// `Run` or `Error` response is returned.
pub async fn send_run_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_run_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming run request implementation with custom socket path.
async fn send_run_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::RunProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send an assign request and stream progress events to a callback.
///
/// Connects to the daemon, sends the request, then reads JSON lines in a loop.
/// `AssignProgress` lines are passed to `on_progress`; the final
/// `Assign` or `Error` response is returned.
pub async fn send_assign_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_assign_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming assign request implementation with custom socket path.
async fn send_assign_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::AssignProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send a streaming unassign request to coastd.
pub async fn send_unassign_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_unassign_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming unassign request implementation with custom socket path.
async fn send_unassign_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::UnassignProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send a start request and stream progress events to a callback.
///
/// Connects to the daemon, sends the request, then reads JSON lines in a loop.
/// `StartProgress` lines are passed to `on_progress`; the final
/// `Start` or `Error` response is returned.
pub async fn send_start_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_start_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming start request implementation with custom socket path.
async fn send_start_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::StartProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send a stop request and stream progress events to a callback.
///
/// Connects to the daemon, sends the request, then reads JSON lines in a loop.
/// `StopProgress` lines are passed to `on_progress`; the final
/// `Stop` or `Error` response is returned.
pub async fn send_stop_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_stop_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming stop request implementation with custom socket path.
async fn send_stop_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::StopProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Send an rm-build request and stream progress events to a callback.
pub async fn send_rm_build_request(
    request: Request,
    mut on_progress: impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    send_rm_build_request_to(request, &socket_path(), &mut on_progress).await
}

/// Streaming rm-build request implementation with custom socket path.
async fn send_rm_build_request_to(
    request: Request,
    sock: &std::path::Path,
    on_progress: &mut impl FnMut(&BuildProgressEvent),
) -> Result<Response> {
    let stream = UnixStream::connect(sock).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused
            || e.kind() == std::io::ErrorKind::NotFound
        {
            anyhow::anyhow!("{}", t!("error.daemon_not_running"))
        } else {
            anyhow::anyhow!(
                "{}",
                t!(
                    "error.daemon_connect_failed",
                    path = sock.display(),
                    message = e
                )
            )
        }
    })?;

    let (reader, mut writer) = stream.into_split();

    let encoded = protocol::encode_request(&request).context("Failed to encode request")?;
    writer
        .write_all(&encoded)
        .await
        .context("Failed to send request to coastd")?;
    writer
        .shutdown()
        .await
        .context("Failed to flush request to coastd")?;

    let mut buf_reader = BufReader::new(reader);

    loop {
        let mut line = String::new();
        let bytes = buf_reader
            .read_line(&mut line)
            .await
            .context("Failed to read response from coastd")?;

        if bytes == 0 {
            bail!("{}", t!("error.daemon_closed"));
        }

        let response = protocol::decode_response(line.trim_end().as_bytes())
            .context("Failed to decode response from coastd")?;

        match response {
            Response::RmBuildProgress(ref event) => {
                on_progress(event);
            }
            _ => {
                return Ok(response);
            }
        }
    }
}

/// Format a table of port mappings for display.
///
/// When `subdomain_host` is provided (e.g., `"dev-1.localhost"`), the
/// DYNAMIC column shows `host:port` instead of just the port number.
pub fn format_port_table(ports: &[PortMapping], subdomain_host: Option<&str>) -> String {
    if ports.is_empty() {
        return format!("  {}", t!("cli.info.no_port_mappings"));
    }

    let service_width: usize = 22;
    let canonical_width: usize = 15;
    let dynamic_width: usize = if subdomain_host.is_some() { 30 } else { 15 };

    let mut lines = Vec::new();
    let service_header = "SERVICE".bold().to_string();
    let canonical_header = "CANONICAL".bold().to_string();
    let dynamic_header = "DYNAMIC".bold().to_string();
    lines.push(format!(
        "  {} {} {}",
        pad_colored_visible(&service_header, "SERVICE".len(), service_width),
        pad_colored_visible(&canonical_header, "CANONICAL".len(), canonical_width),
        pad_plain_visible(&dynamic_header, dynamic_width),
    ));

    for port in ports {
        let service_plain = if port.is_primary {
            format!("★ {}", port.logical_name)
        } else {
            format!("  {}", port.logical_name)
        };
        let service_display = if port.is_primary {
            format!("{} {}", "★".yellow(), port.logical_name)
        } else {
            service_plain.clone()
        };
        let canonical_val = port.canonical_port.to_string();
        let dynamic_val = match subdomain_host {
            Some(host) => format!("{host}:{}", port.dynamic_port),
            None => port.dynamic_port.to_string(),
        };
        lines.push(format!(
            "  {} {} {}",
            pad_colored_visible(
                &service_display,
                service_plain.chars().count(),
                service_width
            ),
            pad_plain_visible(&canonical_val, canonical_width),
            pad_plain_visible(&dynamic_val, dynamic_width),
        ));
    }

    lines.join("\n")
}

/// Pad a plain string to a target width with trailing spaces.
fn pad_plain_visible(s: &str, width: usize) -> String {
    let visible_len = s.chars().count();
    if visible_len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - visible_len))
    }
}

/// Pad a colored string to a target visible width.
fn pad_colored_visible(colored: &str, visible_len: usize, width: usize) -> String {
    if visible_len >= width {
        colored.to_string()
    } else {
        format!("{colored}{}", " ".repeat(width - visible_len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::control;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && chars.peek() == Some(&'[') {
                let _ = chars.next();
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
                continue;
            }
            out.push(ch);
        }

        out
    }

    fn visible_column_index(haystack: &str, needle: &str) -> Option<usize> {
        let byte_idx = haystack.find(needle)?;
        Some(haystack[..byte_idx].chars().count())
    }

    #[test]
    fn test_socket_path() {
        let path = socket_path();
        assert!(path.ends_with(".coast/coastd.sock"));
    }

    #[test]
    fn test_format_port_table_empty() {
        let output = format_port_table(&[], None);
        assert_eq!(output, "  No port mappings configured.");
    }

    #[test]
    fn test_format_port_table_with_ports() {
        let ports = vec![
            PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: false,
            },
            PortMapping {
                logical_name: "postgres".to_string(),
                canonical_port: 5432,
                dynamic_port: 52341,
                is_primary: false,
            },
        ];

        let output = format_port_table(&ports, None);
        assert!(output.contains("web"));
        assert!(output.contains("3000"));
        assert!(output.contains("52340"));
        assert!(output.contains("postgres"));
        assert!(output.contains("5432"));
        assert!(output.contains("52341"));
    }

    #[test]
    fn test_format_port_table_has_header() {
        let ports = vec![PortMapping {
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            is_primary: false,
        }];

        let output = format_port_table(&ports, None);
        // Header should contain SERVICE, CANONICAL, DYNAMIC (with ANSI codes from bold)
        assert!(output.contains("SERVICE"));
        assert!(output.contains("CANONICAL"));
        assert!(output.contains("DYNAMIC"));
    }

    #[tokio::test]
    async fn test_send_request_daemon_not_running() {
        // Use a socket path that definitely does not exist so this test
        // is not affected by a running coastd instance.
        let sock = std::path::PathBuf::from("/tmp/coast-test-nonexistent.sock");
        let _ = std::fs::remove_file(&sock); // ensure it doesn't exist

        let request = Request::Ls(coast_core::protocol::LsRequest { project: None });
        let result = send_request_to(request, &sock).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("coastd is not running"),
            "Expected 'coastd is not running' error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_format_port_table_with_subdomain_host() {
        let ports = vec![PortMapping {
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            is_primary: false,
        }];

        let output = format_port_table(&ports, Some("dev.localhost"));
        assert!(
            output.contains("dev.localhost"),
            "Expected subdomain host in output, got: {}",
            output,
        );
        assert!(output.contains("dev.localhost:52340"));
    }

    #[test]
    fn test_format_port_table_with_subdomain_multiple_ports() {
        let ports = vec![
            PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: true,
            },
            PortMapping {
                logical_name: "api".to_string(),
                canonical_port: 8080,
                dynamic_port: 52341,
                is_primary: false,
            },
            PortMapping {
                logical_name: "postgres".to_string(),
                canonical_port: 5432,
                dynamic_port: 52342,
                is_primary: false,
            },
        ];

        let output = format_port_table(&ports, Some("dev.localhost"));
        assert!(output.contains("dev.localhost:52340"));
        assert!(output.contains("dev.localhost:52341"));
        assert!(output.contains("dev.localhost:52342"));
        assert!(output.contains("web"));
        assert!(output.contains("api"));
        assert!(output.contains("postgres"));
    }

    #[test]
    fn test_format_port_table_primary_row_alignment_with_color() {
        control::set_override(true);

        let ports = vec![
            PortMapping {
                logical_name: "backend".to_string(),
                canonical_port: 8080,
                dynamic_port: 52669,
                is_primary: false,
            },
            PortMapping {
                logical_name: "coasts-dev".to_string(),
                canonical_port: 3000,
                dynamic_port: 65061,
                is_primary: true,
            },
        ];

        let output = format_port_table(&ports, None);
        control::set_override(false);

        let lines: Vec<&str> = output.lines().collect();
        let backend_line = lines
            .iter()
            .find(|line| line.contains("backend"))
            .copied()
            .expect("expected backend row");
        let primary_line = lines
            .iter()
            .find(|line| line.contains("coasts-dev"))
            .copied()
            .expect("expected primary row");

        assert!(
            primary_line.contains("\u{1b}["),
            "expected ANSI color codes in primary row: {primary_line:?}"
        );

        let backend_plain = strip_ansi(backend_line);
        let primary_plain = strip_ansi(primary_line);

        assert_eq!(
            visible_column_index(&backend_plain, "8080"),
            visible_column_index(&primary_plain, "3000"),
            "canonical column should align for primary and non-primary rows"
        );
    }
}
