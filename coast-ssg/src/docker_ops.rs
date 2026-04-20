//! Narrow Docker abstraction used by the SSG runtime.
//!
//! Phase: ssg-phase-9. Introduced to unblock unit testing of the
//! `runtime::lifecycle` and `build::images` modules. The existing
//! code path uses `&bollard::Docker` + concrete
//! [`coast_docker::dind::DindRuntime`] directly, which is
//! integration-test-only (you need a real Docker daemon to run any
//! of it). This trait narrows the exact operations the SSG cares
//! about so tests can mock them.
//!
//! ## Scope
//!
//! The trait is **additive**: it coexists with the existing concrete
//! `Docker` + `DindRuntime` usage. Functions that take `&Docker`
//! today keep working unchanged. New code (and extracted pure
//! helpers) can accept `&dyn SsgDockerOps` instead, which makes them
//! trivially mockable.
//!
//! Full migration of `runtime::lifecycle.rs` + `build::images.rs`
//! to take `&dyn SsgDockerOps` is tracked as incremental follow-up
//! work; see DESIGN.md §17 SETTLED #37. The contract here is the
//! stable seam those migrations will bind to.
//!
//! ## Why not a generic parameter?
//!
//! `async fn` in traits exists in stable Rust (1.75+) but async trait
//! methods don't get `Send` bounds by default. Every SSG async
//! function must be `Send` (runs inside the daemon's tokio runtime).
//! We use `#[async_trait::async_trait]` to get `Pin<Box<dyn Future<
//! Output = ...> + Send>>` signatures automatically. The small
//! allocation cost is fine for once-per-lifecycle-verb calls.

use async_trait::async_trait;
use bollard::Docker;
use coast_docker::dind::DindRuntime;
use coast_docker::runtime::{ContainerConfig, ExecResult, Runtime};

use coast_core::error::Result;

/// Result of an `exec` call inside a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsgExecOutput {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

impl SsgExecOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    fn from_coast_exec(r: ExecResult) -> Self {
        Self {
            exit_code: r.exit_code,
            stdout: r.stdout,
            stderr: r.stderr,
        }
    }
}

/// Narrow async trait wrapping the Docker operations the SSG runtime
/// uses. Kept minimal so mocking is cheap.
///
/// Methods mirror [`coast_docker::runtime::Runtime`]. Image-pull /
/// image-cache functions live outside the trait because they take
/// a filesystem cache directory, not a container id, and compose
/// better as free functions.
#[async_trait]
pub trait SsgDockerOps: Send + Sync {
    /// Create a container from the given config. Returns its id.
    async fn create_container(&self, config: &ContainerConfig) -> Result<String>;

    /// Start a previously-created container by id.
    async fn start_container(&self, container_id: &str) -> Result<()>;

    /// Stop a running container by id.
    async fn stop_container(&self, container_id: &str) -> Result<()>;

    /// Remove a container by id.
    async fn remove_container(&self, container_id: &str) -> Result<()>;

    /// Exec a command inside a container, capturing stdout/stderr/exit.
    async fn exec_in_container(&self, container_id: &str, argv: &[String])
        -> Result<SsgExecOutput>;
}

/// Production implementation backed by `bollard::Docker` +
/// [`coast_docker::dind::DindRuntime`]. Thin delegation: the real
/// business logic stays in the coast_docker crate; this adapter just
/// translates method signatures.
pub struct BollardSsgDockerOps {
    docker: Docker,
    runtime: DindRuntime,
}

impl BollardSsgDockerOps {
    pub fn new(docker: Docker) -> Self {
        let runtime = DindRuntime::with_client(docker.clone());
        Self { docker, runtime }
    }

    /// Access the underlying `Docker` handle. Escape hatch for code
    /// that hasn't been migrated to the trait yet.
    pub fn docker(&self) -> &Docker {
        &self.docker
    }

    /// Access the underlying `DindRuntime`. Same escape hatch.
    pub fn runtime(&self) -> &DindRuntime {
        &self.runtime
    }
}

#[async_trait]
impl SsgDockerOps for BollardSsgDockerOps {
    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        self.runtime.create_coast_container(config).await
    }

    async fn start_container(&self, container_id: &str) -> Result<()> {
        self.runtime.start_coast_container(container_id).await
    }

    async fn stop_container(&self, container_id: &str) -> Result<()> {
        self.runtime.stop_coast_container(container_id).await
    }

    async fn remove_container(&self, container_id: &str) -> Result<()> {
        self.runtime.remove_coast_container(container_id).await
    }

    async fn exec_in_container(
        &self,
        container_id: &str,
        argv: &[String],
    ) -> Result<SsgExecOutput> {
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let res = self.runtime.exec_in_coast(container_id, &refs).await?;
        Ok(SsgExecOutput::from_coast_exec(res))
    }
}

// --- Pure helpers ---------------------------------------------------
//
// These encode the decision logic that used to live inline in
// lifecycle.rs, now extracted so it can be unit-tested against the
// trait (or against vectors of data — most decisions don't need the
// trait at all).

/// Given the SSG manifest's declared images and the set of image
/// refs already loaded inside the SSG's inner Docker daemon, return
/// the subset that still needs to be loaded from the host cache.
///
/// Pure function — no Docker calls, no filesystem access.
pub fn compute_missing_inner_images<'a, I, J>(declared: I, already_loaded: J) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
    J: IntoIterator<Item = &'a str>,
{
    let already: std::collections::HashSet<&str> = already_loaded.into_iter().collect();
    let mut missing: Vec<String> = declared
        .into_iter()
        .filter(|img| !already.contains(img))
        .map(ToString::to_string)
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

/// Decide the remove strategy for the SSG's outer container based on
/// its current state-DB status. Running/starting/restarting
/// containers need to be stopped first; stopped containers can be
/// removed directly.
pub fn should_stop_before_remove(status: &str) -> bool {
    matches!(status, "running" | "restarting" | "starting")
}

/// Given a stop request's timeout setting, clamp it to the
/// `[5, 120]` second range the daemon enforces. Values below 5
/// become 5; values above 120 become 120. Purely defensive —
/// DESIGN.md §9.4 doesn't spec an upper bound, but we avoid wedging
/// the daemon on a pathologically high value.
pub fn clamp_stop_timeout_seconds(requested: u32) -> u32 {
    requested.clamp(5, 120)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // --- compute_missing_inner_images ---

    #[test]
    fn compute_missing_returns_empty_when_all_present() {
        let declared = vec!["postgres:16", "redis:7-alpine"];
        let loaded = vec!["postgres:16", "redis:7-alpine"];
        assert_eq!(
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn compute_missing_returns_all_when_none_loaded() {
        let declared = vec!["postgres:16", "redis:7-alpine"];
        let loaded: Vec<&str> = vec![];
        let missing =
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied());
        assert_eq!(missing, vec!["postgres:16", "redis:7-alpine"]);
    }

    #[test]
    fn compute_missing_preserves_alphabetical_order() {
        // Input order is arbitrary; output must be sorted so the
        // caller can rely on deterministic progress event ordering.
        let declared = vec!["zeta:1", "alpha:2", "mongo:7"];
        let loaded: Vec<&str> = vec![];
        let missing =
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied());
        assert_eq!(missing, vec!["alpha:2", "mongo:7", "zeta:1"]);
    }

    #[test]
    fn compute_missing_dedupes_declared_duplicates() {
        // Same image declared on two sidecars shouldn't produce two
        // load operations.
        let declared = vec!["postgres:16", "postgres:16", "redis:7"];
        let loaded: Vec<&str> = vec![];
        let missing =
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied());
        assert_eq!(missing, vec!["postgres:16", "redis:7"]);
    }

    #[test]
    fn compute_missing_subtracts_partial_overlap() {
        let declared = vec!["postgres:16", "redis:7", "mongo:7"];
        let loaded = vec!["postgres:16"];
        let missing =
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied());
        assert_eq!(missing, vec!["mongo:7", "redis:7"]);
    }

    // --- should_stop_before_remove ---

    #[test]
    fn should_stop_for_active_statuses() {
        assert!(should_stop_before_remove("running"));
        assert!(should_stop_before_remove("restarting"));
        assert!(should_stop_before_remove("starting"));
    }

    #[test]
    fn should_not_stop_for_inert_statuses() {
        assert!(!should_stop_before_remove("stopped"));
        assert!(!should_stop_before_remove("created"));
        assert!(!should_stop_before_remove("exited"));
        assert!(!should_stop_before_remove(""));
    }

    // --- clamp_stop_timeout_seconds ---

    #[test]
    fn clamp_below_min_promotes_to_5() {
        assert_eq!(clamp_stop_timeout_seconds(0), 5);
        assert_eq!(clamp_stop_timeout_seconds(3), 5);
    }

    #[test]
    fn clamp_above_max_reduces_to_120() {
        assert_eq!(clamp_stop_timeout_seconds(1_000), 120);
    }

    #[test]
    fn clamp_within_range_passes_through() {
        assert_eq!(clamp_stop_timeout_seconds(10), 10);
        assert_eq!(clamp_stop_timeout_seconds(60), 60);
    }

    // --- SsgExecOutput ---

    #[test]
    fn ssg_exec_output_success_reflects_exit_code() {
        let ok = SsgExecOutput {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(ok.success());
        let bad = SsgExecOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(!bad.success());
    }

    #[test]
    fn ssg_exec_output_from_coast_exec_copies_fields() {
        let out = SsgExecOutput::from_coast_exec(ExecResult {
            exit_code: 42,
            stdout: "hello".to_string(),
            stderr: "oops".to_string(),
        });
        assert_eq!(out.exit_code, 42);
        assert_eq!(out.stdout, "hello");
        assert_eq!(out.stderr, "oops");
    }

    // --- MockSsgDockerOps ---
    //
    // Handwritten mock kept in-tree (no `mockall` dep) so tests for
    // higher-level SSG logic can assert call order + response
    // scripting. Every method records its arguments into a
    // `call_log` and returns the next response from a per-method
    // queue. Empty queue -> method returns Ok with a default value.

    type Ops = Vec<MockCall>;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum MockCall {
        CreateContainer(String),
        StartContainer(String),
        StopContainer(String),
        RemoveContainer(String),
        ExecInContainer(String, Vec<String>),
    }

    /// Test double. All methods succeed by default; use
    /// `push_exec_result` / `push_create_id` to script per-call
    /// outputs.
    #[derive(Default)]
    pub struct MockSsgDockerOps {
        log: Mutex<Ops>,
        create_ids: Mutex<std::collections::VecDeque<String>>,
        exec_results: Mutex<std::collections::VecDeque<SsgExecOutput>>,
    }

    impl MockSsgDockerOps {
        pub fn push_create_id(&self, id: impl Into<String>) {
            self.create_ids.lock().unwrap().push_back(id.into());
        }

        pub fn push_exec_result(&self, out: SsgExecOutput) {
            self.exec_results.lock().unwrap().push_back(out);
        }

        pub fn calls(&self) -> Vec<MockCall> {
            self.log.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SsgDockerOps for MockSsgDockerOps {
        async fn create_container(&self, _config: &ContainerConfig) -> Result<String> {
            self.log.lock().unwrap().push(MockCall::CreateContainer(
                "(mock-container-config)".to_string(),
            ));
            Ok(self
                .create_ids
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| "mock-cid".to_string()))
        }

        async fn start_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::StartContainer(cid.to_string()));
            Ok(())
        }

        async fn stop_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::StopContainer(cid.to_string()));
            Ok(())
        }

        async fn remove_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::RemoveContainer(cid.to_string()));
            Ok(())
        }

        async fn exec_in_container(&self, cid: &str, argv: &[String]) -> Result<SsgExecOutput> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::ExecInContainer(cid.to_string(), argv.to_vec()));
            Ok(self
                .exec_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(SsgExecOutput {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                }))
        }
    }

    // Trait-level smoke tests. These prove the mock actually
    // implements the trait and records calls correctly.

    #[tokio::test]
    async fn mock_records_every_call_in_order() {
        let mock = MockSsgDockerOps::default();

        mock.start_container("cid").await.unwrap();
        mock.stop_container("cid").await.unwrap();
        mock.remove_container("cid").await.unwrap();

        assert_eq!(
            mock.calls(),
            vec![
                MockCall::StartContainer("cid".to_string()),
                MockCall::StopContainer("cid".to_string()),
                MockCall::RemoveContainer("cid".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn mock_exec_returns_pushed_result_then_default() {
        let mock = MockSsgDockerOps::default();
        mock.push_exec_result(SsgExecOutput {
            exit_code: 42,
            stdout: "hi".to_string(),
            stderr: "err".to_string(),
        });

        let first = mock
            .exec_in_container("cid", &["ls".to_string()])
            .await
            .unwrap();
        assert_eq!(first.exit_code, 42);
        assert_eq!(first.stdout, "hi");
        assert!(!first.success());

        // Queue exhausted -> default success.
        let second = mock
            .exec_in_container("cid", &["pwd".to_string()])
            .await
            .unwrap();
        assert_eq!(second.exit_code, 0);
        assert!(second.success());
    }

    #[tokio::test]
    async fn mock_create_container_returns_pushed_id_then_default() {
        let mock = MockSsgDockerOps::default();
        mock.push_create_id("custom-cid-123");

        // The mock ignores the config contents, only the call.
        let config = ContainerConfig::new("coast", "ssg", "docker:dind");

        let first = mock.create_container(&config).await.unwrap();
        assert_eq!(first, "custom-cid-123");

        let second = mock.create_container(&config).await.unwrap();
        assert_eq!(second, "mock-cid");
    }
}
