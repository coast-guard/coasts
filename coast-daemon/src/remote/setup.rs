//! Remote coastd setup and installation.
//!
//! Handles installing and configuring coastd on remote VMs via SSH.

use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::state::remotes::Remote;
use coast_core::error::{CoastError, Result};

/// Default installation path for coastd on remote machines.
const REMOTE_COASTD_PATH: &str = "/usr/local/bin/coastd";

/// Default systemd service name.
const SYSTEMD_SERVICE_NAME: &str = "coastd";

/// Progress callback for setup operations.
pub type ProgressCallback = Box<dyn Fn(&str) + Send + Sync>;

/// Setup status for tracking installation progress.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SetupStatus {
    /// Starting setup process
    Starting,
    /// Checking SSH connectivity
    CheckingConnectivity,
    /// Checking if Docker is installed
    CheckingDocker,
    /// Installing Docker
    InstallingDocker,
    /// Downloading coastd binary
    DownloadingCoastd,
    /// Installing coastd binary
    InstallingCoastd,
    /// Configuring systemd service
    ConfiguringService,
    /// Starting coastd service
    StartingService,
    /// Verifying installation
    Verifying,
    /// Setup completed successfully
    Completed,
    /// Setup failed
    Failed(String),
}

/// Handles remote coastd setup and installation.
pub struct RemoteSetup {
    /// Optional progress callback
    progress: Option<ProgressCallback>,
}

impl RemoteSetup {
    /// Create a new RemoteSetup instance.
    pub fn new() -> Self {
        Self { progress: None }
    }

    /// Create a new RemoteSetup instance with a progress callback.
    #[allow(dead_code)]
    pub fn with_progress(progress: ProgressCallback) -> Self {
        Self {
            progress: Some(progress),
        }
    }

    /// Report progress to the callback if set.
    fn report(&self, message: &str) {
        if let Some(ref cb) = self.progress {
            cb(message);
        }
    }

    /// Run an SSH command on the remote and return stdout.
    async fn ssh_exec(&self, remote: &Remote, command: &str) -> Result<String> {
        let mut cmd = Command::new("ssh");

        // Add SSH key if specified
        if let Some(ref key_path) = remote.ssh_key_path {
            cmd.arg("-i").arg(key_path);
        }

        cmd.args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=30",
            "-p",
            &remote.port.to_string(),
            &format!("{}@{}", remote.user, remote.host),
            command,
        ]);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!(remote = %remote.name, command = %command, "executing SSH command");

        let output = cmd.output().await.map_err(|e| CoastError::Remote {
            message: format!("failed to execute SSH command: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!(
                    "SSH command failed (exit {}): {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                ),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Run an SSH command on the remote, streaming output to the progress callback.
    async fn ssh_exec_streaming(&self, remote: &Remote, command: &str) -> Result<i32> {
        let mut cmd = Command::new("ssh");

        // Add SSH key if specified
        if let Some(ref key_path) = remote.ssh_key_path {
            cmd.arg("-i").arg(key_path);
        }

        cmd.args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=30",
            "-p",
            &remote.port.to_string(),
            &format!("{}@{}", remote.user, remote.host),
            command,
        ]);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!(remote = %remote.name, command = %command, "executing SSH command (streaming)");

        let mut child = cmd.spawn().map_err(|e| CoastError::Remote {
            message: format!("failed to spawn SSH command: {e}"),
        })?;

        // Stream stdout
        let stdout = child.stdout.take();
        if let Some(stdout) = stdout {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                self.report(&line);
            }
        }

        // Stream stderr
        let stderr = child.stderr.take();
        if let Some(stderr) = stderr {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                self.report(&format!("stderr: {}", line));
            }
        }

        let status = child.wait().await.map_err(|e| CoastError::Remote {
            message: format!("failed to wait for SSH command: {e}"),
        })?;

        Ok(status.code().unwrap_or(-1))
    }

    /// Copy a file to the remote via SCP.
    #[allow(dead_code)]
    async fn scp_to_remote(
        &self,
        remote: &Remote,
        local_path: &str,
        remote_path: &str,
    ) -> Result<()> {
        let mut cmd = Command::new("scp");

        // Add SSH key if specified
        if let Some(ref key_path) = remote.ssh_key_path {
            cmd.arg("-i").arg(key_path);
        }

        cmd.args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-P",
            &remote.port.to_string(),
            local_path,
            &format!("{}@{}:{}", remote.user, remote.host, remote_path),
        ]);

        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!(
            remote = %remote.name,
            local_path,
            remote_path,
            "copying file via SCP"
        );

        let output = cmd.output().await.map_err(|e| CoastError::Remote {
            message: format!("failed to execute SCP: {e}"),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoastError::Remote {
                message: format!("SCP failed: {}", stderr.trim()),
            });
        }

        Ok(())
    }

    /// Check SSH connectivity to the remote.
    pub async fn check_connectivity(&self, remote: &Remote) -> Result<bool> {
        self.report("Checking SSH connectivity...");

        match self.ssh_exec(remote, "echo ok").await {
            Ok(output) if output.trim() == "ok" => {
                info!(remote = %remote.name, "SSH connectivity confirmed");
                Ok(true)
            }
            Ok(output) => {
                warn!(remote = %remote.name, output = %output, "unexpected SSH response");
                Ok(false)
            }
            Err(e) => {
                warn!(remote = %remote.name, error = %e, "SSH connectivity check failed");
                Err(e)
            }
        }
    }

    /// Check if Docker is installed and running on the remote.
    pub async fn check_docker(&self, remote: &Remote) -> Result<bool> {
        self.report("Checking Docker installation...");

        match self
            .ssh_exec(remote, "docker version --format '{{.Server.Version}}'")
            .await
        {
            Ok(version) => {
                let version = version.trim();
                info!(remote = %remote.name, version = %version, "Docker is installed");
                self.report(&format!("Docker version: {}", version));
                Ok(true)
            }
            Err(_) => {
                info!(remote = %remote.name, "Docker not found or not running");
                Ok(false)
            }
        }
    }

    /// Install Docker on the remote (assumes Ubuntu/Debian).
    pub async fn install_docker(&self, remote: &Remote) -> Result<()> {
        self.report("Installing Docker...");

        // Install Docker using the official convenience script
        let install_script = r#"
            set -e
            if ! command -v docker &> /dev/null; then
                curl -fsSL https://get.docker.com -o /tmp/get-docker.sh
                sudo sh /tmp/get-docker.sh
                sudo usermod -aG docker $USER
                rm /tmp/get-docker.sh
            fi
            sudo systemctl enable docker
            sudo systemctl start docker
        "#;

        let exit_code = self
            .ssh_exec_streaming(
                remote,
                &format!("bash -c '{}'", install_script.replace('\n', " ")),
            )
            .await?;

        if exit_code != 0 {
            return Err(CoastError::Remote {
                message: format!("Docker installation failed with exit code {}", exit_code),
            });
        }

        info!(remote = %remote.name, "Docker installed successfully");
        Ok(())
    }

    /// Check if coastd is installed on the remote.
    pub async fn check_coastd(&self, remote: &Remote) -> Result<Option<String>> {
        self.report("Checking coastd installation...");

        match self
            .ssh_exec(
                remote,
                &format!(
                    "{} --version 2>/dev/null || echo 'not-found'",
                    REMOTE_COASTD_PATH
                ),
            )
            .await
        {
            Ok(output) => {
                let output = output.trim();
                if output == "not-found" || output.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(output.to_string()))
                }
            }
            Err(_) => Ok(None),
        }
    }

    /// Download and install coastd binary on the remote.
    ///
    /// This downloads the latest coastd binary from the release server.
    pub async fn install_coastd(&self, remote: &Remote, binary_url: Option<&str>) -> Result<()> {
        self.report("Installing coastd...");

        // Detect architecture
        let arch = self.ssh_exec(remote, "uname -m").await?.trim().to_string();
        let arch_suffix = match arch.as_str() {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => {
                return Err(CoastError::Remote {
                    message: format!("unsupported architecture: {}", other),
                });
            }
        };

        // Determine download URL
        let url = binary_url
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| {
                // Default to a placeholder - in production this would be a real release URL
                format!(
                    "https://github.com/anomalyco/coast/releases/latest/download/coastd-linux-{}",
                    arch_suffix
                )
            });

        self.report(&format!("Downloading coastd from {}...", url));

        // Download and install
        let install_cmd = format!(
            r#"
            set -e
            TMP=$(mktemp)
            curl -fsSL -o "$TMP" "{url}"
            chmod +x "$TMP"
            sudo mv "$TMP" {path}
            {path} --version
            "#,
            url = url,
            path = REMOTE_COASTD_PATH,
        );

        let output = self
            .ssh_exec(remote, &install_cmd.replace('\n', " "))
            .await?;
        let version = output.lines().last().unwrap_or("unknown").trim();

        info!(remote = %remote.name, version = %version, "coastd installed");
        self.report(&format!("coastd version: {}", version));

        Ok(())
    }

    /// Configure and enable the coastd systemd service.
    pub async fn configure_systemd_service(&self, remote: &Remote) -> Result<()> {
        self.report("Configuring systemd service...");

        // Create systemd service unit
        let service_unit = format!(
            r#"[Unit]
Description=Coast Development Environment Daemon
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
ExecStart={coastd_path}
Restart=always
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
"#,
            coastd_path = REMOTE_COASTD_PATH,
        );

        // Write and enable the service
        let setup_cmd = format!(
            r#"
            set -e
            echo '{}' | sudo tee /etc/systemd/system/{}.service > /dev/null
            sudo systemctl daemon-reload
            sudo systemctl enable {}
            "#,
            service_unit.replace('\'', "'\\''"),
            SYSTEMD_SERVICE_NAME,
            SYSTEMD_SERVICE_NAME,
        );

        self.ssh_exec(remote, &setup_cmd.replace('\n', " ")).await?;

        info!(remote = %remote.name, "systemd service configured");
        Ok(())
    }

    /// Start the coastd service on the remote.
    pub async fn start_service(&self, remote: &Remote) -> Result<()> {
        self.report("Starting coastd service...");

        self.ssh_exec(
            remote,
            &format!("sudo systemctl start {}", SYSTEMD_SERVICE_NAME),
        )
        .await?;

        // Wait a moment for the service to start
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Check service status
        let status = self
            .ssh_exec(
                remote,
                &format!("systemctl is-active {}", SYSTEMD_SERVICE_NAME),
            )
            .await?;

        if status.trim() != "active" {
            return Err(CoastError::Remote {
                message: format!("coastd service failed to start: status = {}", status.trim()),
            });
        }

        info!(remote = %remote.name, "coastd service started");
        Ok(())
    }

    /// Stop the coastd service on the remote.
    #[allow(dead_code)]
    pub async fn stop_service(&self, remote: &Remote) -> Result<()> {
        self.report("Stopping coastd service...");

        self.ssh_exec(
            remote,
            &format!("sudo systemctl stop {} || true", SYSTEMD_SERVICE_NAME),
        )
        .await?;

        info!(remote = %remote.name, "coastd service stopped");
        Ok(())
    }

    /// Restart the coastd service on the remote.
    #[allow(dead_code)]
    pub async fn restart_service(&self, remote: &Remote) -> Result<()> {
        self.report("Restarting coastd service...");

        self.ssh_exec(
            remote,
            &format!("sudo systemctl restart {}", SYSTEMD_SERVICE_NAME),
        )
        .await?;

        info!(remote = %remote.name, "coastd service restarted");
        Ok(())
    }

    /// Get the coastd service status on the remote.
    #[allow(dead_code)]
    pub async fn service_status(&self, remote: &Remote) -> Result<String> {
        match self
            .ssh_exec(
                remote,
                &format!("systemctl is-active {}", SYSTEMD_SERVICE_NAME),
            )
            .await
        {
            Ok(status) => Ok(status.trim().to_string()),
            Err(_) => Ok("unknown".to_string()),
        }
    }

    /// Verify that coastd is running and responding on the remote.
    pub async fn verify_coastd(&self, remote: &Remote) -> Result<bool> {
        self.report("Verifying coastd is responding...");

        // Try to ping the coastd API
        let result = self
            .ssh_exec(
                remote,
                "curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:31415/health || echo 'failed'",
            )
            .await;

        match result {
            Ok(output) => {
                let output = output.trim();
                if output == "200" {
                    info!(remote = %remote.name, "coastd is responding");
                    Ok(true)
                } else {
                    warn!(remote = %remote.name, response = %output, "coastd not responding as expected");
                    Ok(false)
                }
            }
            Err(e) => {
                warn!(remote = %remote.name, error = %e, "failed to verify coastd");
                Ok(false)
            }
        }
    }

    /// Perform full setup of coastd on a remote VM.
    ///
    /// This includes:
    /// 1. Checking SSH connectivity
    /// 2. Installing Docker if needed
    /// 3. Downloading and installing coastd
    /// 4. Configuring systemd service
    /// 5. Starting the service
    /// 6. Verifying the installation
    pub async fn full_setup(&self, remote: &Remote, binary_url: Option<&str>) -> Result<()> {
        info!(remote = %remote.name, "starting full remote setup");
        self.report("Starting remote setup...");

        // Step 1: Check connectivity
        self.check_connectivity(remote).await?;

        // Step 2: Check/install Docker
        if !self.check_docker(remote).await? {
            self.install_docker(remote).await?;
            // Need to verify Docker is working after install
            if !self.check_docker(remote).await? {
                return Err(CoastError::Remote {
                    message: "Docker installation completed but Docker is not responding"
                        .to_string(),
                });
            }
        }

        // Step 3: Install coastd
        self.install_coastd(remote, binary_url).await?;

        // Step 4: Configure systemd
        self.configure_systemd_service(remote).await?;

        // Step 5: Start service
        self.start_service(remote).await?;

        // Step 6: Verify
        if !self.verify_coastd(remote).await? {
            return Err(CoastError::Remote {
                message: "coastd installed but not responding on port 31415".to_string(),
            });
        }

        self.report("Remote setup completed successfully!");
        info!(remote = %remote.name, "remote setup completed");

        Ok(())
    }

    /// Upgrade coastd on a remote VM.
    #[allow(dead_code)]
    pub async fn upgrade(&self, remote: &Remote, binary_url: Option<&str>) -> Result<()> {
        info!(remote = %remote.name, "upgrading coastd");
        self.report("Upgrading coastd...");

        // Stop the service
        self.stop_service(remote).await?;

        // Install new binary
        self.install_coastd(remote, binary_url).await?;

        // Restart service
        self.start_service(remote).await?;

        // Verify
        if !self.verify_coastd(remote).await? {
            return Err(CoastError::Remote {
                message: "coastd upgraded but not responding".to_string(),
            });
        }

        self.report("Upgrade completed successfully!");
        info!(remote = %remote.name, "coastd upgrade completed");

        Ok(())
    }

    /// Uninstall coastd from a remote VM.
    #[allow(dead_code)]
    pub async fn uninstall(&self, remote: &Remote) -> Result<()> {
        info!(remote = %remote.name, "uninstalling coastd");
        self.report("Uninstalling coastd...");

        // Stop and disable the service
        let uninstall_cmd = format!(
            r#"
            set -e
            sudo systemctl stop {} || true
            sudo systemctl disable {} || true
            sudo rm -f /etc/systemd/system/{}.service
            sudo systemctl daemon-reload
            sudo rm -f {}
            "#,
            SYSTEMD_SERVICE_NAME, SYSTEMD_SERVICE_NAME, SYSTEMD_SERVICE_NAME, REMOTE_COASTD_PATH,
        );

        self.ssh_exec(remote, &uninstall_cmd.replace('\n', " "))
            .await?;

        self.report("Uninstall completed successfully!");
        info!(remote = %remote.name, "coastd uninstalled");

        Ok(())
    }
}

impl Default for RemoteSetup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_setup_creation() {
        let setup = RemoteSetup::new();
        assert!(setup.progress.is_none());
    }

    #[test]
    fn test_remote_setup_with_progress() {
        let setup = RemoteSetup::with_progress(Box::new(|_msg| {}));
        assert!(setup.progress.is_some());
    }
}
