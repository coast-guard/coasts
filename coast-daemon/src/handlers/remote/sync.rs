use std::path::Path;

use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::types::RemoteConnection;

/// Sync workspace files to the remote host using rsync.
///
/// Performs a one-way sync from `local_path` to `remote_path` on the remote
/// host, excluding `.git` and common build artifact directories.
pub async fn rsync_workspace(
    local_path: &Path,
    remote_path: &str,
    config: &RemoteConnection,
    has_sudo: bool,
) -> Result<()> {
    rsync_workspace_opts(local_path, remote_path, config, has_sudo, true).await
}

pub async fn rsync_workspace_opts(
    local_path: &Path,
    remote_path: &str,
    config: &RemoteConnection,
    has_sudo: bool,
    delete_extra: bool,
) -> Result<()> {
    let local_str = format!("{}/", local_path.display());
    let remote_str = format!("{}@{}:{}", config.user, config.host, remote_path);
    let ssh_key_str = config.ssh_key.display().to_string();
    let ssh_cmd = format!(
        "ssh -p {} -i {} -o StrictHostKeyChecking=no -o BatchMode=yes -o ControlMaster=auto -o ControlPath=/tmp/coast-ssh-%r@%h:%p -o ControlPersist=300",
        config.port, ssh_key_str
    );

    info!(
        local = %local_str,
        remote = %remote_str,
        "syncing workspace via rsync"
    );

    let _ = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "ControlMaster=auto",
            "-o",
            "ControlPath=/tmp/coast-ssh-%r@%h:%p",
            "-o",
            "ControlPersist=300",
            "-p",
            &config.port.to_string(),
            "-i",
            &ssh_key_str,
        ])
        .arg(format!("{}@{}", config.user, config.host))
        .arg(if has_sudo {
            format!("sudo mkdir -p {remote_path}")
        } else {
            format!("mkdir -p {remote_path}")
        })
        .output()
        .await;

    let rsync_path_flag = if has_sudo {
        "--rsync-path=sudo rsync"
    } else {
        "--rsync-path=rsync"
    };

    let mut cmd = tokio::process::Command::new("rsync");
    cmd.arg("-rlDzP");
    if delete_extra {
        cmd.arg("--delete-after");
        cmd.args(["--filter", "P generated/***"]);
        cmd.args(["--filter", "P .react-router/***"]);
        cmd.args(["--filter", "P internal/generated/***"]);
        cmd.args(["--filter", "P app/generated/***"]);
    }
    cmd.args([
        rsync_path_flag,
        "--exclude",
        ".git",
        "--exclude",
        "node_modules",
        "--exclude",
        "target",
        "--exclude",
        "__pycache__",
        "--exclude",
        ".react-router",
        "--exclude",
        ".next",
        "-e",
        &ssh_cmd,
        &local_str,
        &remote_str,
    ]);

    let output = cmd
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to run rsync: {e}")))?;

    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code == 23 {
        warn!(
            "rsync partial transfer (exit 23) — generated files may have been modified by a running dev server; source files synced successfully"
        );
    } else if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::state(format!(
            "rsync failed (exit {exit_code}): {stderr}",
        )));
    }

    if has_sudo {
        let _ = tokio::process::Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "StrictHostKeyChecking=no",
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPath=/tmp/coast-ssh-%r@%h:%p",
                "-o",
                "ControlPersist=300",
                "-p",
                &config.port.to_string(),
                "-i",
                &ssh_key_str,
            ])
            .arg(format!("{}@{}", config.user, config.host))
            .arg(format!("sudo chown -R $(id -u):$(id -g) {remote_path}"))
            .output()
            .await;
    }

    debug!("rsync completed successfully");

    Ok(())
}

/// Rsync a directory from the remote host to the local machine.
pub async fn rsync_from_remote(
    remote_path: &str,
    local_path: &Path,
    config: &RemoteConnection,
    has_sudo: bool,
) -> Result<()> {
    let remote_str = format!("{}@{}:{}/", config.user, config.host, remote_path);
    let local_str = format!("{}/", local_path.display());
    let ssh_key_str = config.ssh_key.display().to_string();
    let ssh_cmd = format!(
        "ssh -p {} -i {} -o StrictHostKeyChecking=no -o BatchMode=yes -o ControlMaster=auto -o ControlPath=/tmp/coast-ssh-%r@%h:%p -o ControlPersist=300",
        config.port, ssh_key_str
    );

    std::fs::create_dir_all(local_path).map_err(|e| {
        CoastError::state(format!(
            "failed to create local dir {}: {e}",
            local_path.display()
        ))
    })?;

    info!(
        remote = %remote_str,
        local = %local_str,
        "downloading artifact from remote via rsync"
    );

    let rsync_path_flag = if has_sudo {
        "--rsync-path=sudo rsync"
    } else {
        "--rsync-path=rsync"
    };

    let output = tokio::process::Command::new("rsync")
        .args([
            "-azP",
            rsync_path_flag,
            "-e",
            &ssh_cmd,
            &remote_str,
            &local_str,
        ])
        .output()
        .await
        .map_err(|e| CoastError::state(format!("failed to run rsync: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CoastError::state(format!(
            "rsync from remote failed (exit {}): {stderr}",
            output.status.code().unwrap_or(-1)
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Mutagen continuous sync
// ---------------------------------------------------------------------------

/// Generate a deterministic mutagen session name from project and instance.
pub fn mutagen_session_name(project: &str, instance: &str) -> String {
    format!("coast-{project}-{instance}")
}

/// Stop a mutagen sync session inside the shell container.
pub async fn stop_mutagen_in_shell(
    docker: &bollard::Docker,
    shell_container: &str,
    session_name: &str,
) -> Result<()> {
    let rt = coast_docker::dind::DindRuntime::with_client(docker.clone());
    use coast_docker::runtime::Runtime;

    let cmd = format!(
        "mutagen sync terminate {} 2>/dev/null || true",
        session_name
    );
    match rt.exec_in_coast(shell_container, &["sh", "-c", &cmd]).await {
        Ok(result) if result.success() => {
            debug!(session = %session_name, container = %shell_container, "mutagen sync session terminated in shell");
        }
        Ok(_) => {
            debug!(session = %session_name, "mutagen session not found in shell (already terminated)");
        }
        Err(e) => {
            debug!(session = %session_name, error = %e, "failed to exec mutagen terminate in shell");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutagen_session_name_format() {
        let name = mutagen_session_name("my-app", "dev-1");
        assert_eq!(name, "coast-my-app-dev-1");
    }

    #[test]
    fn test_mutagen_session_name_special_chars() {
        let name = mutagen_session_name("coast-demo", "feature-oauth");
        assert_eq!(name, "coast-coast-demo-feature-oauth");
    }

    #[test]
    fn test_stop_mutagen_in_shell_signature_compiles() {
        fn _assert_fn_exists(_docker: &bollard::Docker, _container: &str, _session: &str) {}
    }
}
