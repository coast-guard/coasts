/// High-level coast container lifecycle management.
///
/// Delegates to the `Runtime` trait for runtime-specific operations while
/// providing common higher-level functionality like waiting for the inner
/// Docker daemon to be ready.
use std::net::IpAddr;
use std::time::Duration;

use tracing::{debug, info, warn};

use coast_core::error::{CoastError, Result};

use crate::runtime::{ContainerConfig, ExecResult, Runtime};

/// Default timeout for waiting for the inner daemon to be ready (120 seconds).
pub const DEFAULT_INNER_DAEMON_TIMEOUT_SECS: u64 = 120;

/// Default interval between readiness polls (1 second).
pub const READINESS_POLL_INTERVAL_SECS: u64 = 1;

/// The command used to check if the inner Docker daemon is ready.
pub const DOCKER_INFO_CMD: &[&str] = &["docker", "info"];

/// The command used to check if the inner Podman daemon is ready.
pub const PODMAN_INFO_CMD: &[&str] = &["podman", "info"];

/// High-level container manager that wraps a `Runtime` implementation.
///
/// Provides additional lifecycle functionality on top of raw runtime
/// operations, such as waiting for the inner daemon to become ready
/// after container startup.
pub struct ContainerManager<R: Runtime> {
    /// The underlying runtime implementation.
    runtime: R,
    /// Timeout for inner daemon readiness in seconds.
    inner_daemon_timeout: Duration,
    /// Interval between readiness polls.
    poll_interval: Duration,
}

impl<R: Runtime> ContainerManager<R> {
    /// Create a new container manager wrapping the given runtime.
    pub fn new(runtime: R) -> Self {
        Self {
            runtime,
            inner_daemon_timeout: Duration::from_secs(DEFAULT_INNER_DAEMON_TIMEOUT_SECS),
            poll_interval: Duration::from_secs(READINESS_POLL_INTERVAL_SECS),
        }
    }

    /// Create a new container manager with a custom timeout.
    pub fn with_timeout(runtime: R, timeout_secs: u64) -> Self {
        Self {
            runtime,
            inner_daemon_timeout: Duration::from_secs(timeout_secs),
            poll_interval: Duration::from_secs(READINESS_POLL_INTERVAL_SECS),
        }
    }

    /// Create a new container manager with custom timeout and poll interval.
    ///
    /// Useful for testing with shorter intervals.
    pub fn with_timing(runtime: R, timeout: Duration, poll_interval: Duration) -> Self {
        Self {
            runtime,
            inner_daemon_timeout: timeout,
            poll_interval,
        }
    }

    /// Get a reference to the underlying runtime.
    pub fn runtime(&self) -> &R {
        &self.runtime
    }

    /// Get the runtime name.
    pub fn runtime_name(&self) -> &str {
        self.runtime.name()
    }

    /// Create a new coast container.
    ///
    /// Returns the container ID on success.
    pub async fn create(&self, config: &ContainerConfig) -> Result<String> {
        info!(
            project = %config.project,
            instance = %config.instance_name,
            runtime = %self.runtime.name(),
            "Creating coast container"
        );
        self.runtime.create_coast_container(config).await
    }

    /// Start a coast container.
    pub async fn start(&self, container_id: &str) -> Result<()> {
        info!(container_id = %container_id, "Starting coast container");
        self.runtime.start_coast_container(container_id).await
    }

    /// Stop a coast container.
    pub async fn stop(&self, container_id: &str) -> Result<()> {
        info!(container_id = %container_id, "Stopping coast container");
        self.runtime.stop_coast_container(container_id).await
    }

    /// Remove a coast container.
    pub async fn remove(&self, container_id: &str) -> Result<()> {
        info!(container_id = %container_id, "Removing coast container");
        self.runtime.remove_coast_container(container_id).await
    }

    /// Execute a command inside a coast container.
    pub async fn exec(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult> {
        self.runtime.exec_in_coast(container_id, cmd).await
    }

    /// Get the IP address of a coast container.
    pub async fn get_ip(&self, container_id: &str) -> Result<IpAddr> {
        self.runtime.get_container_ip(container_id).await
    }

    /// Wait for the inner Docker/Podman daemon inside a coast container to become ready.
    ///
    /// Polls the inner daemon by running `docker info` (or `podman info` for Podman
    /// runtime) inside the coast container until it succeeds or the timeout expires.
    ///
    /// Returns Ok(()) when the inner daemon is ready, or an error if the timeout
    /// is exceeded.
    #[allow(clippy::cognitive_complexity)]
    pub async fn wait_for_inner_daemon(&self, container_id: &str) -> Result<()> {
        let info_cmd = self.inner_daemon_info_cmd();
        let timeout = self.inner_daemon_timeout;
        let start = tokio::time::Instant::now();

        info!(
            container_id = %container_id,
            timeout_secs = timeout.as_secs(),
            "Waiting for inner daemon to be ready"
        );

        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(CoastError::docker(format!(
                    "Inner daemon in container '{container_id}' did not become ready \
                     within {timeout_secs}s. The coast container may have failed to start \
                     its Docker daemon. Try `coast exec <name> docker info` to diagnose, \
                     or increase the timeout.",
                    timeout_secs = timeout.as_secs()
                )));
            }

            match self.runtime.exec_in_coast(container_id, info_cmd).await {
                Ok(result) if result.success() => {
                    info!(
                        container_id = %container_id,
                        elapsed_secs = elapsed.as_secs(),
                        "Inner daemon is ready"
                    );
                    return Ok(());
                }
                Ok(result) => {
                    debug!(
                        container_id = %container_id,
                        exit_code = result.exit_code,
                        stderr = %result.stderr,
                        "Inner daemon not ready yet, retrying..."
                    );
                }
                Err(e) => {
                    debug!(
                        container_id = %container_id,
                        error = %e,
                        "Inner daemon check failed, retrying..."
                    );
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// Create, start, and wait for the inner daemon to be ready.
    ///
    /// This is the typical flow for `coast run`: create the container,
    /// start it, then wait for the inner daemon before proceeding to
    /// run docker compose.
    pub async fn create_and_start(&self, config: &ContainerConfig) -> Result<String> {
        let container_id = self.create(config).await?;
        self.start(&container_id).await?;
        self.wait_for_inner_daemon(&container_id).await?;
        Ok(container_id)
    }

    /// Get the appropriate info command for the inner daemon.
    fn inner_daemon_info_cmd(&self) -> &'static [&'static str] {
        match self.runtime.name() {
            "podman" => PODMAN_INFO_CMD,
            _ => DOCKER_INFO_CMD,
        }
    }

    /// Stop and remove a coast container.
    ///
    /// Attempts to stop first, then remove. If stop fails (e.g., container
    /// already stopped), still attempts removal.
    pub async fn stop_and_remove(&self, container_id: &str) -> Result<()> {
        match self.stop(container_id).await {
            Ok(()) => {
                debug!(container_id = %container_id, "Container stopped");
            }
            Err(e) => {
                warn!(
                    container_id = %container_id,
                    error = %e,
                    "Failed to stop container, attempting removal anyway"
                );
            }
        }

        self.remove(container_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_timeout() {
        assert_eq!(DEFAULT_INNER_DAEMON_TIMEOUT_SECS, 120);
    }

    #[test]
    fn test_default_poll_interval() {
        assert_eq!(READINESS_POLL_INTERVAL_SECS, 1);
    }

    #[test]
    fn test_docker_info_cmd() {
        assert_eq!(DOCKER_INFO_CMD, &["docker", "info"]);
    }

    #[test]
    fn test_podman_info_cmd() {
        assert_eq!(PODMAN_INFO_CMD, &["podman", "info"]);
    }

    use async_trait::async_trait;

    // Mock runtime for testing ContainerManager logic without Docker
    struct MockRuntime {
        name: String,
        privileged: bool,
        create_result: std::sync::Mutex<Option<Result<String>>>,
        start_result: std::sync::Mutex<Option<Result<()>>>,
        stop_result: std::sync::Mutex<Option<Result<()>>>,
        remove_result: std::sync::Mutex<Option<Result<()>>>,
        exec_results: std::sync::Mutex<Vec<Result<ExecResult>>>,
    }

    impl MockRuntime {
        fn new_dind() -> Self {
            Self {
                name: "dind".to_string(),
                privileged: true,
                create_result: std::sync::Mutex::new(None),
                start_result: std::sync::Mutex::new(None),
                stop_result: std::sync::Mutex::new(None),
                remove_result: std::sync::Mutex::new(None),
                exec_results: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn new_podman() -> Self {
            Self {
                name: "podman".to_string(),
                privileged: true,
                create_result: std::sync::Mutex::new(None),
                start_result: std::sync::Mutex::new(None),
                stop_result: std::sync::Mutex::new(None),
                remove_result: std::sync::Mutex::new(None),
                exec_results: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn set_create_result(&self, result: Result<String>) {
            *self.create_result.lock().unwrap() = Some(result);
        }

        fn set_start_result(&self, result: Result<()>) {
            *self.start_result.lock().unwrap() = Some(result);
        }

        fn set_stop_result(&self, result: Result<()>) {
            *self.stop_result.lock().unwrap() = Some(result);
        }

        fn set_remove_result(&self, result: Result<()>) {
            *self.remove_result.lock().unwrap() = Some(result);
        }

        fn push_exec_result(&self, result: Result<ExecResult>) {
            self.exec_results.lock().unwrap().push(result);
        }
    }

    #[async_trait]
    impl Runtime for MockRuntime {
        fn name(&self) -> &str {
            &self.name
        }

        async fn create_coast_container(&self, _config: &ContainerConfig) -> Result<String> {
            self.create_result
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Ok("mock-container-id".to_string()))
        }

        async fn start_coast_container(&self, _container_id: &str) -> Result<()> {
            self.start_result.lock().unwrap().take().unwrap_or(Ok(()))
        }

        async fn stop_coast_container(&self, _container_id: &str) -> Result<()> {
            self.stop_result.lock().unwrap().take().unwrap_or(Ok(()))
        }

        async fn remove_coast_container(&self, _container_id: &str) -> Result<()> {
            self.remove_result.lock().unwrap().take().unwrap_or(Ok(()))
        }

        async fn exec_in_coast(&self, _container_id: &str, _cmd: &[&str]) -> Result<ExecResult> {
            let mut results = self.exec_results.lock().unwrap();
            if results.is_empty() {
                Ok(ExecResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })
            } else {
                results.remove(0)
            }
        }

        async fn get_container_ip(&self, _container_id: &str) -> Result<IpAddr> {
            Ok("172.17.0.2".parse().unwrap())
        }

        fn requires_privileged(&self) -> bool {
            self.privileged
        }
    }

    #[test]
    fn test_container_manager_runtime_name() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);
        assert_eq!(manager.runtime_name(), "dind");
    }

    #[test]
    fn test_container_manager_with_timeout() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::with_timeout(runtime, 60);
        assert_eq!(manager.inner_daemon_timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_container_manager_with_timing() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_secs(30),
            Duration::from_millis(500),
        );
        assert_eq!(manager.inner_daemon_timeout, Duration::from_secs(30));
        assert_eq!(manager.poll_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_inner_daemon_info_cmd_dind() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);
        assert_eq!(manager.inner_daemon_info_cmd(), DOCKER_INFO_CMD);
    }

    #[test]
    fn test_inner_daemon_info_cmd_podman() {
        let runtime = MockRuntime::new_podman();
        let manager = ContainerManager::new(runtime);
        assert_eq!(manager.inner_daemon_info_cmd(), PODMAN_INFO_CMD);
    }

    #[tokio::test]
    async fn test_create_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        runtime.set_create_result(Ok("test-container-123".to_string()));
        let manager = ContainerManager::new(runtime);

        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let result = manager.create(&config).await;
        assert_eq!(result.unwrap(), "test-container-123");
    }

    #[tokio::test]
    async fn test_create_propagates_error() {
        let runtime = MockRuntime::new_dind();
        runtime.set_create_result(Err(CoastError::docker("creation failed")));
        let manager = ContainerManager::new(runtime);

        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let result = manager.create(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);

        let result = manager.start("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_propagates_error() {
        let runtime = MockRuntime::new_dind();
        runtime.set_start_result(Err(CoastError::docker("start failed")));
        let manager = ContainerManager::new(runtime);

        let result = manager.start("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);

        let result = manager.stop("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);

        let result = manager.remove("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_exec_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "hello".to_string(),
            stderr: String::new(),
        }));
        let manager = ContainerManager::new(runtime);

        let result = manager.exec("test-id", &["echo", "hello"]).await.unwrap();
        assert!(result.success());
        assert_eq!(result.stdout, "hello");
    }

    #[tokio::test]
    async fn test_get_ip_delegates_to_runtime() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);

        let ip = manager.get_ip("test-id").await.unwrap();
        assert_eq!(ip, "172.17.0.2".parse::<IpAddr>().unwrap());
    }

    #[tokio::test]
    async fn test_wait_for_inner_daemon_success_first_try() {
        let runtime = MockRuntime::new_dind();
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "docker info output".to_string(),
            stderr: String::new(),
        }));

        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let result = manager.wait_for_inner_daemon("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_inner_daemon_success_after_retries() {
        let runtime = MockRuntime::new_dind();
        // First two attempts fail, third succeeds
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "daemon not ready".to_string(),
        }));
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: "daemon not ready".to_string(),
        }));
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "ready".to_string(),
            stderr: String::new(),
        }));

        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let result = manager.wait_for_inner_daemon("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_inner_daemon_timeout() {
        let runtime = MockRuntime::new_dind();
        // Always return failure - will hit timeout
        for _ in 0..100 {
            runtime.push_exec_result(Ok(ExecResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "not ready".to_string(),
            }));
        }

        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_millis(50),
            Duration::from_millis(10),
        );

        let result = manager.wait_for_inner_daemon("test-id").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("did not become ready"));
    }

    #[tokio::test]
    async fn test_wait_for_inner_daemon_exec_error_retries() {
        let runtime = MockRuntime::new_dind();
        // First attempt has exec error, second succeeds
        runtime.push_exec_result(Err(CoastError::docker("exec failed")));
        runtime.push_exec_result(Ok(ExecResult {
            exit_code: 0,
            stdout: "ready".to_string(),
            stderr: String::new(),
        }));

        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let result = manager.wait_for_inner_daemon("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_and_remove_both_succeed() {
        let runtime = MockRuntime::new_dind();
        let manager = ContainerManager::new(runtime);

        let result = manager.stop_and_remove("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_and_remove_stop_fails_still_removes() {
        let runtime = MockRuntime::new_dind();
        runtime.set_stop_result(Err(CoastError::docker("already stopped")));
        let manager = ContainerManager::new(runtime);

        let result = manager.stop_and_remove("test-id").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_stop_and_remove_remove_fails() {
        let runtime = MockRuntime::new_dind();
        runtime.set_remove_result(Err(CoastError::docker("remove failed")));
        let manager = ContainerManager::new(runtime);

        let result = manager.stop_and_remove("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_and_start_success() {
        let runtime = MockRuntime::new_dind();
        runtime.set_create_result(Ok("new-container".to_string()));
        // wait_for_inner_daemon will call exec, which returns success by default

        let manager = ContainerManager::with_timing(
            runtime,
            Duration::from_secs(5),
            Duration::from_millis(10),
        );

        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let container_id = manager.create_and_start(&config).await.unwrap();
        assert_eq!(container_id, "new-container");
    }

    #[tokio::test]
    async fn test_create_and_start_create_fails() {
        let runtime = MockRuntime::new_dind();
        runtime.set_create_result(Err(CoastError::docker("no space")));

        let manager = ContainerManager::new(runtime);
        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let result = manager.create_and_start(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_and_start_start_fails() {
        let runtime = MockRuntime::new_dind();
        runtime.set_create_result(Ok("container-id".to_string()));
        runtime.set_start_result(Err(CoastError::docker("start failed")));

        let manager = ContainerManager::new(runtime);
        let config = ContainerConfig::new("my-app", "test", "docker:dind");
        let result = manager.create_and_start(&config).await;
        assert!(result.is_err());
    }
}
