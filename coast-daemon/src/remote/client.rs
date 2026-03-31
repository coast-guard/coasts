//! Remote daemon client for forwarding requests over SSH tunnels.
//!
//! This client connects to a remote coastd daemon via an established SSH tunnel
//! and forwards requests, proxying responses back to the caller.

use std::net::SocketAddr;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::debug;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{self, Request, Response};

/// Client for communicating with a remote coastd daemon over an SSH tunnel.
///
/// The tunnel forwards a local TCP port to the remote daemon's Unix socket,
/// so we connect via TCP to localhost:tunnel_port.
#[allow(dead_code)]
pub struct RemoteDaemonClient {
    /// Local port where the SSH tunnel is listening.
    local_port: u16,
    /// Name of the remote for logging purposes.
    remote_name: String,
}

#[allow(dead_code)]
impl RemoteDaemonClient {
    /// Create a new client for the given tunnel port.
    pub fn new(local_port: u16, remote_name: String) -> Self {
        Self {
            local_port,
            remote_name,
        }
    }

    /// Get the socket address to connect to.
    fn socket_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.local_port))
    }

    /// Send a request and receive a single response.
    ///
    /// This is for non-streaming requests where we expect exactly one response.
    #[allow(dead_code)]
    pub async fn send_request(&self, request: &Request) -> Result<Response> {
        let addr = self.socket_addr();
        debug!(remote = %self.remote_name, addr = %addr, "connecting to remote daemon");

        let stream = TcpStream::connect(addr).await.map_err(|e| CoastError::Io {
            message: format!(
                "failed to connect to remote daemon '{}' at {}: {}",
                self.remote_name, addr, e
            ),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;

        let (reader, mut writer) = stream.into_split();

        // Encode and send request
        let encoded = protocol::encode_request(request)?;
        writer.write_all(&encoded).await.map_err(|e| CoastError::Io {
            message: format!("failed to send request to remote daemon: {}", e),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;
        writer.shutdown().await.map_err(|e| CoastError::Io {
            message: format!("failed to flush request to remote daemon: {}", e),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;

        // Read response line
        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();
        buf_reader.read_line(&mut line).await.map_err(|e| CoastError::Io {
            message: format!("failed to read response from remote daemon: {}", e),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;

        if line.is_empty() {
            return Err(CoastError::state("remote daemon closed connection unexpectedly"));
        }

        // Decode response
        let response = protocol::decode_response(line.trim_end().as_bytes())?;
        debug!(remote = %self.remote_name, "received response from remote daemon");

        Ok(response)
    }

    /// Send a request and stream multiple responses.
    ///
    /// This is for streaming requests (build/run) where the daemon sends
    /// multiple progress responses before the final response.
    ///
    /// The callback `on_progress` is called for each intermediate response
    /// (BuildProgress, RunProgress). Returns the final response.
    pub async fn send_streaming_request<F>(
        &self,
        request: &Request,
        mut on_progress: F,
    ) -> Result<Response>
    where
        F: FnMut(Response) -> Result<()>,
    {
        let addr = self.socket_addr();
        debug!(remote = %self.remote_name, addr = %addr, "connecting to remote daemon for streaming request");

        let stream = TcpStream::connect(addr).await.map_err(|e| CoastError::Io {
            message: format!(
                "failed to connect to remote daemon '{}' at {}: {}",
                self.remote_name, addr, e
            ),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;

        let (reader, mut writer) = stream.into_split();

        // Encode and send request
        let encoded = protocol::encode_request(request)?;
        writer.write_all(&encoded).await.map_err(|e| CoastError::Io {
            message: format!("failed to send request to remote daemon: {}", e),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;
        writer.shutdown().await.map_err(|e| CoastError::Io {
            message: format!("failed to flush request to remote daemon: {}", e),
            path: std::path::PathBuf::new(),
            source: Some(e),
        })?;

        // Read responses line by line
        let mut buf_reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = buf_reader.read_line(&mut line).await.map_err(|e| CoastError::Io {
                message: format!("failed to read response from remote daemon: {}", e),
                path: std::path::PathBuf::new(),
                source: Some(e),
            })?;

            if bytes_read == 0 {
                return Err(CoastError::state("remote daemon closed connection unexpectedly"));
            }

            let response = protocol::decode_response(line.trim_end().as_bytes())?;

            // Check if this is a final response or progress
            match &response {
                Response::BuildProgress(_) | Response::RunProgress(_) | Response::LogsProgress(_) => {
                    // Progress response - forward to callback and continue
                    on_progress(response)?;
                }
                _ => {
                    // Final response - return it
                    debug!(remote = %self.remote_name, "received final response from remote daemon");
                    return Ok(response);
                }
            }
        }
    }
}

/// Information needed to route a request to a remote daemon.
#[derive(Debug, Clone)]
pub struct RemoteRoute {
    /// Name of the remote.
    pub remote_name: String,
    /// Local port where the SSH tunnel is listening.
    pub tunnel_port: u16,
    /// Active sync session for path translation (optional).
    pub sync_session: Option<crate::state::remotes::SyncSession>,
}

impl RemoteRoute {
    /// Create a client for this route.
    #[allow(dead_code)]
    pub fn client(&self) -> RemoteDaemonClient {
        RemoteDaemonClient::new(self.tunnel_port, self.remote_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_addr() {
        let client = RemoteDaemonClient::new(31416, "test-remote".to_string());
        let addr = client.socket_addr();
        assert_eq!(addr.port(), 31416);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_remote_route_client() {
        let route = RemoteRoute {
            remote_name: "dev-vm".to_string(),
            tunnel_port: 31417,
            sync_session: None,
        };
        let client = route.client();
        assert_eq!(client.local_port, 31417);
        assert_eq!(client.remote_name, "dev-vm");
    }
}
