//! Docker abstraction used by the SSG runtime.
//!
//! Phase 9 introduced a narrow 5-method trait. Phase 12 expanded it
//! into the **fat trait** below — every SSG async lifecycle + image
//! code path now goes through `&dyn SsgDockerOps`, so lifecycle
//! orchestration can be unit-tested without Docker. The real impl
//! [`BollardSsgDockerOps`] is a thin delegator to
//! `bollard::Docker` + `coast_docker::dind::DindRuntime` +
//! `coast_docker::container::ContainerManager` +
//! `coast_docker::image_cache::pull_and_cache_image`. The test impl
//! [`MockSsgDockerOps`] records every call and lets tests script
//! per-method return values.
//!
//! See `DESIGN.md §17 SETTLED #37` (completion criterion) and
//! SETTLED #39 (fat-trait design rationale).
//!
//! ## Why `#[async_trait::async_trait]`?
//!
//! `async fn` in traits exists in stable Rust (1.75+) but async trait
//! methods don't get `Send` bounds by default. Every SSG async
//! function must be `Send` (runs inside the daemon's tokio runtime).
//! We use `#[async_trait::async_trait]` to get
//! `Pin<Box<dyn Future<Output = ...> + Send>>` signatures automatically.
//! The small allocation cost is fine for once-per-lifecycle-verb
//! calls.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bollard::Docker;
use coast_docker::container::ContainerManager;
use coast_docker::dind::DindRuntime;
use coast_docker::runtime::{ContainerConfig, ExecResult, Runtime};

use coast_core::error::{CoastError, Result};

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

    /// Conversion to the legacy [`ExecResult`] for callers that still
    /// expect that type (notably
    /// [`crate::daemon_integration::create_instance_db_for_consumer`]).
    pub fn to_coast_exec(&self) -> ExecResult {
        ExecResult {
            exit_code: self.exit_code,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        }
    }
}

/// Fat async trait wrapping every Docker operation the SSG runtime
/// uses. One named method per semantic op (DESIGN §17 SETTLED #39).
/// Lifecycle orchestrators in [`crate::runtime::lifecycle`] compose
/// these primitives; tests substitute [`MockSsgDockerOps`] and assert
/// on call sequences.
#[async_trait]
pub trait SsgDockerOps: Send + Sync {
    // --- outer DinD lifecycle ---

    /// Create a container from the given config. Returns its id.
    async fn create_container(&self, config: &ContainerConfig) -> Result<String>;

    /// Start a previously-created container by id.
    async fn start_container(&self, container_id: &str) -> Result<()>;

    /// Stop a running container by id.
    async fn stop_container(&self, container_id: &str) -> Result<()>;

    /// Remove a container by id.
    async fn remove_container(&self, container_id: &str) -> Result<()>;

    /// Poll the inner Docker daemon inside `container_id` until it
    /// reports ready, or error after `timeout_s` seconds.
    async fn wait_for_inner_daemon(&self, container_id: &str, timeout_s: u64) -> Result<()>;

    /// Exec an arbitrary command on the **outer** DinD container
    /// (`docker exec <cid> <argv>`). Used for ad-hoc `coast ssg exec`
    /// without a named service.
    async fn exec_in_container(&self, container_id: &str, argv: &[String])
        -> Result<SsgExecOutput>;

    // --- image pull + load ---

    /// Pull `image` from a registry and cache it as a tarball under
    /// `cache_dir`. Returns the on-disk tarball path. Skip the pull
    /// if the tarball already exists; this method always performs the
    /// pull and is not expected to be skip-aware (callers do that).
    async fn pull_and_cache_image(&self, image: &str, cache_dir: &Path) -> Result<PathBuf>;

    /// For each tarball, run `docker load -i <inner_path>` inside the
    /// outer DinD. `inner_tarball_paths` are paths visible from
    /// INSIDE `container_id` (typically `/image-cache/<safe>.tar`).
    /// Returns the number of images actually loaded.
    async fn load_images_into_inner(
        &self,
        container_id: &str,
        inner_tarball_paths: &[String],
    ) -> Result<u32>;

    /// List the image refs (`repo:tag`) already present in the inner
    /// daemon. Used to skip re-loading images that a cached DinD
    /// volume already has.
    async fn list_inner_images(&self, container_id: &str) -> Result<HashSet<String>>;

    // --- inner volumes ---

    /// Remove every inner named volume matching a
    /// `docker volume ls --filter label=<label_filter>` query.
    /// Returns the number of volumes actually removed. Errors on
    /// individual volumes are logged and swallowed.
    async fn remove_inner_volumes(&self, container_id: &str, label_filter: &str) -> Result<u32>;

    // --- inner docker compose ---

    /// Run `docker compose -f <compose_path> [-f <override>...] -p <project> up -d --remove-orphans`
    /// inside `container_id`.
    ///
    /// `extra_compose_files` is an optional list of additional
    /// compose files to layer on top of the base `compose_path` —
    /// each element becomes another `-f <path>` argv pair, in
    /// order. Phase 33 uses this to inject the per-run
    /// `compose.override.yml` carrying decrypted secret env-vars
    /// and bind-mounts. Empty slice = old behaviour (single `-f`).
    async fn inner_compose_up(
        &self,
        container_id: &str,
        compose_path: &str,
        extra_compose_files: &[String],
        project: &str,
    ) -> Result<()>;

    /// Run `docker compose -f <compose_path> -p <project> down`
    /// inside `container_id`. Errors are returned; the caller
    /// decides whether to swallow them (stop / rm are best-effort).
    async fn inner_compose_down(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
    ) -> Result<()>;

    /// Run `docker compose -f <compose_path> -p <project> exec -T <service> <argv>`
    /// inside `container_id`.
    async fn inner_compose_exec(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        service: &str,
        argv: &[String],
    ) -> Result<SsgExecOutput>;

    /// Run a per-service `docker compose <verb> <service>` command
    /// inside `container_id`. Used for the toolbar Stop / Start /
    /// Restart / Remove actions on the SSG services tab. `verb`
    /// must be one of `stop`, `start`, `restart`, or `rm` (with
    /// `rm -sf` semantics implemented as `rm -s -f`). On failure
    /// returns a descriptive `CoastError::Docker` so the caller can
    /// surface stderr verbatim.
    async fn inner_compose_service_action(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        verb: &str,
        service: &str,
    ) -> Result<()>;

    /// Run `docker compose -f <compose_path> -p <project> logs --tail N <service>`
    /// inside `container_id`. Returns the combined output.
    async fn inner_compose_logs(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        service: &str,
        tail: Option<u32>,
    ) -> Result<String>;

    // --- host-level ops ---

    /// Run `docker logs --tail N <container_id>` on the **host**
    /// daemon (not inside the container). Used to read the outer
    /// DinD's own logs for `coast ssg logs` without a service arg.
    async fn host_container_logs(&self, container_id: &str, tail: Option<u32>) -> Result<String>;
}

// --- Pure argv builders ---------------------------------------------
//
// These compose the canonical `docker compose ...` argv used by both
// the real Bollard impl and the lifecycle orchestrators. Extracting
// them makes the argv format unit-testable without any Docker client.

/// `docker compose -f <path> [-f <extra>...] -p <project> up -d --remove-orphans`.
///
/// `extras` is appended as additional `-f <path>` argv pairs in
/// the supplied order, so callers can layer override files on top
/// of the base compose file. Empty `extras` reproduces the legacy
/// single-`-f` argv.
pub fn build_inner_compose_up_argv(
    compose_path: &str,
    extras: &[String],
    project: &str,
) -> Vec<String> {
    let mut argv = Vec::with_capacity(8 + 2 * extras.len());
    argv.push("docker".to_string());
    argv.push("compose".to_string());
    argv.push("-f".to_string());
    argv.push(compose_path.to_string());
    for extra in extras {
        argv.push("-f".to_string());
        argv.push(extra.clone());
    }
    argv.push("-p".to_string());
    argv.push(project.to_string());
    argv.push("up".to_string());
    argv.push("-d".to_string());
    argv.push("--remove-orphans".to_string());
    argv
}

/// `docker compose -f <path> -p <project> down`.
pub fn build_inner_compose_down_argv(compose_path: &str, project: &str) -> Vec<String> {
    vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string(),
        "-p".to_string(),
        project.to_string(),
        "down".to_string(),
    ]
}

/// `docker compose -f <path> -p <project> exec -T <service> <argv...>`.
pub fn build_inner_compose_exec_argv(
    compose_path: &str,
    project: &str,
    service: &str,
    argv: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string(),
        "-p".to_string(),
        project.to_string(),
        "exec".to_string(),
        "-T".to_string(),
        service.to_string(),
    ];
    out.extend(argv.iter().cloned());
    out
}

/// `docker compose -f <path> -p <project> logs --tail N <service>`.
pub fn build_inner_compose_logs_argv(
    compose_path: &str,
    project: &str,
    service: &str,
    tail: Option<u32>,
) -> Vec<String> {
    vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string(),
        "-p".to_string(),
        project.to_string(),
        "logs".to_string(),
        "--tail".to_string(),
        tail.unwrap_or(200).to_string(),
        service.to_string(),
    ]
}

/// `docker compose -f <path> -p <project> <verb> <service>` for
/// the per-service Stop/Start/Restart/Remove buttons on the SSG
/// services tab. `rm` is sent as `rm -s -f` so a stopped or
/// running service is force-removed in one call (matches the
/// semantics of `coast service rm` on regular instances).
pub fn build_inner_compose_service_action_argv(
    compose_path: &str,
    project: &str,
    verb: &str,
    service: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        compose_path.to_string(),
        "-p".to_string(),
        project.to_string(),
        verb.to_string(),
    ];
    if verb == "rm" {
        argv.push("-s".to_string());
        argv.push("-f".to_string());
    }
    argv.push(service.to_string());
    argv
}

/// `logs --tail N <container_id>` — the args passed to the host
/// `docker` CLI. The `docker` program name is NOT included.
pub fn build_host_docker_logs_argv(container_id: &str, tail: Option<u32>) -> Vec<String> {
    vec![
        "logs".to_string(),
        "--tail".to_string(),
        tail.unwrap_or(200).to_string(),
        container_id.to_string(),
    ]
}

// --- BollardSsgDockerOps --------------------------------------------

/// Production implementation backed by `bollard::Docker` +
/// [`coast_docker::dind::DindRuntime`]. Thin delegation — real
/// business logic stays in the `coast_docker` crate.
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

    async fn exec_inner(&self, container_id: &str, argv: &[String]) -> Result<SsgExecOutput> {
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let res = self.runtime.exec_in_coast(container_id, &refs).await?;
        Ok(SsgExecOutput::from_coast_exec(res))
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

    async fn wait_for_inner_daemon(&self, container_id: &str, timeout_s: u64) -> Result<()> {
        let manager = ContainerManager::with_timeout(
            DindRuntime::with_client(self.docker.clone()),
            timeout_s,
        );
        manager.wait_for_inner_daemon(container_id).await
    }

    async fn exec_in_container(
        &self,
        container_id: &str,
        argv: &[String],
    ) -> Result<SsgExecOutput> {
        self.exec_inner(container_id, argv).await
    }

    async fn pull_and_cache_image(&self, image: &str, cache_dir: &Path) -> Result<PathBuf> {
        coast_docker::image_cache::pull_and_cache_image(&self.docker, image, cache_dir).await
    }

    async fn load_images_into_inner(
        &self,
        container_id: &str,
        inner_tarball_paths: &[String],
    ) -> Result<u32> {
        let mut loaded = 0u32;
        for path in inner_tarball_paths {
            let argv = vec![
                "docker".to_string(),
                "load".to_string(),
                "-i".to_string(),
                path.clone(),
            ];
            let out = self.exec_inner(container_id, &argv).await?;
            if !out.success() {
                return Err(CoastError::docker(format!(
                    "docker load failed for tarball '{}' (exit {}). stderr: {}",
                    path, out.exit_code, out.stderr
                )));
            }
            loaded += 1;
        }
        Ok(loaded)
    }

    async fn list_inner_images(&self, container_id: &str) -> Result<HashSet<String>> {
        let argv = vec![
            "docker".to_string(),
            "images".to_string(),
            "--format".to_string(),
            "{{.Repository}}:{{.Tag}}".to_string(),
        ];
        match self.exec_inner(container_id, &argv).await {
            Ok(out) if out.success() => Ok(out
                .stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != "<none>:<none>")
                .collect()),
            _ => Ok(HashSet::new()),
        }
    }

    async fn remove_inner_volumes(&self, container_id: &str, label_filter: &str) -> Result<u32> {
        let list_argv = vec![
            "docker".to_string(),
            "volume".to_string(),
            "ls".to_string(),
            "-q".to_string(),
            "--filter".to_string(),
            format!("label={label_filter}"),
        ];
        let list = match self.exec_inner(container_id, &list_argv).await {
            Ok(out) => out,
            Err(e) => {
                tracing::warn!(error = %e, "failed to list inner named volumes; skipping volume removal");
                return Ok(0);
            }
        };
        let mut removed = 0u32;
        for vol in list.stdout.lines().filter(|l| !l.trim().is_empty()) {
            let vol = vol.trim();
            let argv = vec![
                "docker".to_string(),
                "volume".to_string(),
                "rm".to_string(),
                vol.to_string(),
            ];
            match self.exec_inner(container_id, &argv).await {
                Ok(out) if out.success() => removed += 1,
                Ok(out) => {
                    tracing::warn!(volume = %vol, exit = out.exit_code, stderr = %out.stderr, "failed to remove inner named volume");
                }
                Err(e) => {
                    tracing::warn!(error = %e, volume = %vol, "failed to remove inner named volume");
                }
            }
        }
        Ok(removed)
    }

    async fn inner_compose_up(
        &self,
        container_id: &str,
        compose_path: &str,
        extra_compose_files: &[String],
        project: &str,
    ) -> Result<()> {
        let argv = build_inner_compose_up_argv(compose_path, extra_compose_files, project);
        let out = self.exec_inner(container_id, &argv).await?;
        if !out.success() {
            return Err(CoastError::docker(format!(
                "docker compose up -d failed (exit {}). stderr: {}",
                out.exit_code, out.stderr
            )));
        }
        Ok(())
    }

    async fn inner_compose_down(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
    ) -> Result<()> {
        let argv = build_inner_compose_down_argv(compose_path, project);
        let out = self.exec_inner(container_id, &argv).await?;
        if !out.success() {
            return Err(CoastError::docker(format!(
                "docker compose down failed (exit {}). stderr: {}",
                out.exit_code, out.stderr
            )));
        }
        Ok(())
    }

    async fn inner_compose_exec(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        service: &str,
        argv: &[String],
    ) -> Result<SsgExecOutput> {
        let full = build_inner_compose_exec_argv(compose_path, project, service, argv);
        self.exec_inner(container_id, &full).await
    }

    async fn inner_compose_service_action(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        verb: &str,
        service: &str,
    ) -> Result<()> {
        // Reject anything that isn't a known compose verb to
        // prevent the SPA from sending arbitrary docker compose
        // subcommands through this path.
        if !matches!(verb, "stop" | "start" | "restart" | "rm") {
            return Err(CoastError::docker(format!(
                "inner_compose_service_action: unsupported verb '{verb}' \
                 (expected stop|start|restart|rm)"
            )));
        }
        let argv = build_inner_compose_service_action_argv(compose_path, project, verb, service);
        let out = self.exec_inner(container_id, &argv).await?;
        if !out.success() {
            return Err(CoastError::docker(format!(
                "docker compose {verb} {service} failed (exit {}). stderr: {}",
                out.exit_code, out.stderr,
            )));
        }
        Ok(())
    }

    async fn inner_compose_logs(
        &self,
        container_id: &str,
        compose_path: &str,
        project: &str,
        service: &str,
        tail: Option<u32>,
    ) -> Result<String> {
        let argv = build_inner_compose_logs_argv(compose_path, project, service, tail);
        let out = self.exec_inner(container_id, &argv).await?;
        Ok(if out.stdout.is_empty() {
            out.stderr
        } else {
            out.stdout
        })
    }

    async fn host_container_logs(&self, container_id: &str, tail: Option<u32>) -> Result<String> {
        let args = build_host_docker_logs_argv(container_id, tail);
        let output = tokio::process::Command::new("docker")
            .args(&args)
            .output()
            .await
            .map_err(|e| CoastError::Docker {
                message: format!("failed to spawn `docker {}`: {e}", args.join(" ")),
                source: Some(Box::new(e)),
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Ok(if stdout.is_empty() { stderr } else { stdout })
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

// --- MockSsgDockerOps -----------------------------------------------
//
// Exposed at `#[cfg(test)]` inside the `coast-ssg` crate AND at
// `#[cfg(any(test, feature = "test-support"))]` so
// `coast-daemon`'s unit tests can construct one too. The mock is
// kept handwritten (no `mockall` dep) so test authors can read and
// tweak it without new macro magic.

#[cfg(any(test, feature = "test-support"))]
pub use mock::{MockCall, MockSsgDockerOps};

#[cfg(any(test, feature = "test-support"))]
mod mock {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum MockCall {
        CreateContainer {
            project: String,
            instance_name: String,
            image: String,
            container_name_override: Option<String>,
            published_ports: Vec<(u16, u16)>,
        },
        StartContainer(String),
        StopContainer(String),
        RemoveContainer(String),
        WaitForInnerDaemon {
            container_id: String,
            timeout_s: u64,
        },
        ExecInContainer {
            container_id: String,
            argv: Vec<String>,
        },
        PullAndCacheImage {
            image: String,
            cache_dir: PathBuf,
        },
        LoadImagesIntoInner {
            container_id: String,
            tarballs: Vec<String>,
        },
        ListInnerImages {
            container_id: String,
        },
        RemoveInnerVolumes {
            container_id: String,
            label_filter: String,
        },
        InnerComposeUp {
            container_id: String,
            compose_path: String,
            extra_compose_files: Vec<String>,
            project: String,
        },
        InnerComposeDown {
            container_id: String,
            compose_path: String,
            project: String,
        },
        InnerComposeExec {
            container_id: String,
            compose_path: String,
            project: String,
            service: String,
            argv: Vec<String>,
        },
        InnerComposeLogs {
            container_id: String,
            compose_path: String,
            project: String,
            service: String,
            tail: Option<u32>,
        },
        InnerComposeServiceAction {
            container_id: String,
            compose_path: String,
            project: String,
            verb: String,
            service: String,
        },
        HostContainerLogs {
            container_id: String,
            tail: Option<u32>,
        },
    }

    /// Test double for `SsgDockerOps`. All methods record their call
    /// in `log` and return scripted values from per-method queues, or
    /// sensible defaults when the queue is empty.
    #[derive(Default)]
    pub struct MockSsgDockerOps {
        log: Mutex<Vec<MockCall>>,

        // --- scripted per-method response queues ---
        create_ids: Mutex<std::collections::VecDeque<Result<String>>>,
        start_results: Mutex<std::collections::VecDeque<Result<()>>>,
        stop_results: Mutex<std::collections::VecDeque<Result<()>>>,
        remove_results: Mutex<std::collections::VecDeque<Result<()>>>,
        wait_results: Mutex<std::collections::VecDeque<Result<()>>>,
        exec_results: Mutex<std::collections::VecDeque<Result<SsgExecOutput>>>,
        pull_results: Mutex<std::collections::VecDeque<Result<PathBuf>>>,
        load_results: Mutex<std::collections::VecDeque<Result<u32>>>,
        list_images_results: Mutex<std::collections::VecDeque<Result<HashSet<String>>>>,
        remove_volumes_results: Mutex<std::collections::VecDeque<Result<u32>>>,
        compose_up_results: Mutex<std::collections::VecDeque<Result<()>>>,
        compose_down_results: Mutex<std::collections::VecDeque<Result<()>>>,
        compose_exec_results: Mutex<std::collections::VecDeque<Result<SsgExecOutput>>>,
        compose_logs_results: Mutex<std::collections::VecDeque<Result<String>>>,
        compose_service_action_results: Mutex<std::collections::VecDeque<Result<()>>>,
        host_logs_results: Mutex<std::collections::VecDeque<Result<String>>>,
    }

    impl MockSsgDockerOps {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn calls(&self) -> Vec<MockCall> {
            self.log.lock().unwrap().clone()
        }

        pub fn push_create_id(&self, r: Result<String>) {
            self.create_ids.lock().unwrap().push_back(r);
        }
        pub fn push_start_result(&self, r: Result<()>) {
            self.start_results.lock().unwrap().push_back(r);
        }
        pub fn push_stop_result(&self, r: Result<()>) {
            self.stop_results.lock().unwrap().push_back(r);
        }
        pub fn push_remove_result(&self, r: Result<()>) {
            self.remove_results.lock().unwrap().push_back(r);
        }
        pub fn push_wait_result(&self, r: Result<()>) {
            self.wait_results.lock().unwrap().push_back(r);
        }
        pub fn push_exec_result(&self, r: Result<SsgExecOutput>) {
            self.exec_results.lock().unwrap().push_back(r);
        }
        pub fn push_pull_result(&self, r: Result<PathBuf>) {
            self.pull_results.lock().unwrap().push_back(r);
        }
        pub fn push_load_result(&self, r: Result<u32>) {
            self.load_results.lock().unwrap().push_back(r);
        }
        pub fn push_list_images_result(&self, r: Result<HashSet<String>>) {
            self.list_images_results.lock().unwrap().push_back(r);
        }
        pub fn push_remove_volumes_result(&self, r: Result<u32>) {
            self.remove_volumes_results.lock().unwrap().push_back(r);
        }
        pub fn push_compose_up_result(&self, r: Result<()>) {
            self.compose_up_results.lock().unwrap().push_back(r);
        }
        pub fn push_compose_down_result(&self, r: Result<()>) {
            self.compose_down_results.lock().unwrap().push_back(r);
        }
        pub fn push_compose_exec_result(&self, r: Result<SsgExecOutput>) {
            self.compose_exec_results.lock().unwrap().push_back(r);
        }
        pub fn push_compose_logs_result(&self, r: Result<String>) {
            self.compose_logs_results.lock().unwrap().push_back(r);
        }
        pub fn push_compose_service_action_result(&self, r: Result<()>) {
            self.compose_service_action_results
                .lock()
                .unwrap()
                .push_back(r);
        }
        pub fn push_host_logs_result(&self, r: Result<String>) {
            self.host_logs_results.lock().unwrap().push_back(r);
        }
    }

    fn default_exec_output() -> SsgExecOutput {
        SsgExecOutput {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[async_trait]
    impl SsgDockerOps for MockSsgDockerOps {
        async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
            let published_ports: Vec<(u16, u16)> = config
                .published_ports
                .iter()
                .map(|p| (p.host_port, p.container_port))
                .collect();
            self.log.lock().unwrap().push(MockCall::CreateContainer {
                project: config.project.clone(),
                instance_name: config.instance_name.clone(),
                image: config.image.clone(),
                container_name_override: config.container_name_override.clone(),
                published_ports,
            });
            self.create_ids
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok("mock-cid".to_string()))
        }

        async fn start_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::StartContainer(cid.to_string()));
            self.start_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn stop_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::StopContainer(cid.to_string()));
            self.stop_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn remove_container(&self, cid: &str) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::RemoveContainer(cid.to_string()));
            self.remove_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn wait_for_inner_daemon(&self, cid: &str, timeout_s: u64) -> Result<()> {
            self.log.lock().unwrap().push(MockCall::WaitForInnerDaemon {
                container_id: cid.to_string(),
                timeout_s,
            });
            self.wait_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn exec_in_container(&self, cid: &str, argv: &[String]) -> Result<SsgExecOutput> {
            self.log.lock().unwrap().push(MockCall::ExecInContainer {
                container_id: cid.to_string(),
                argv: argv.to_vec(),
            });
            self.exec_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(default_exec_output()))
        }

        async fn pull_and_cache_image(&self, image: &str, cache_dir: &Path) -> Result<PathBuf> {
            self.log.lock().unwrap().push(MockCall::PullAndCacheImage {
                image: image.to_string(),
                cache_dir: cache_dir.to_path_buf(),
            });
            self.pull_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(cache_dir.join("mock.tar")))
        }

        async fn load_images_into_inner(&self, cid: &str, tarballs: &[String]) -> Result<u32> {
            let tarball_count = tarballs.len() as u32;
            self.log
                .lock()
                .unwrap()
                .push(MockCall::LoadImagesIntoInner {
                    container_id: cid.to_string(),
                    tarballs: tarballs.to_vec(),
                });
            self.load_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(tarball_count))
        }

        async fn list_inner_images(&self, cid: &str) -> Result<HashSet<String>> {
            self.log.lock().unwrap().push(MockCall::ListInnerImages {
                container_id: cid.to_string(),
            });
            self.list_images_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(HashSet::new()))
        }

        async fn remove_inner_volumes(&self, cid: &str, label_filter: &str) -> Result<u32> {
            self.log.lock().unwrap().push(MockCall::RemoveInnerVolumes {
                container_id: cid.to_string(),
                label_filter: label_filter.to_string(),
            });
            self.remove_volumes_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(0))
        }

        async fn inner_compose_up(
            &self,
            cid: &str,
            compose_path: &str,
            extra_compose_files: &[String],
            project: &str,
        ) -> Result<()> {
            self.log.lock().unwrap().push(MockCall::InnerComposeUp {
                container_id: cid.to_string(),
                compose_path: compose_path.to_string(),
                extra_compose_files: extra_compose_files.to_vec(),
                project: project.to_string(),
            });
            self.compose_up_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn inner_compose_down(
            &self,
            cid: &str,
            compose_path: &str,
            project: &str,
        ) -> Result<()> {
            self.log.lock().unwrap().push(MockCall::InnerComposeDown {
                container_id: cid.to_string(),
                compose_path: compose_path.to_string(),
                project: project.to_string(),
            });
            self.compose_down_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn inner_compose_exec(
            &self,
            cid: &str,
            compose_path: &str,
            project: &str,
            service: &str,
            argv: &[String],
        ) -> Result<SsgExecOutput> {
            self.log.lock().unwrap().push(MockCall::InnerComposeExec {
                container_id: cid.to_string(),
                compose_path: compose_path.to_string(),
                project: project.to_string(),
                service: service.to_string(),
                argv: argv.to_vec(),
            });
            self.compose_exec_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(default_exec_output()))
        }

        async fn inner_compose_logs(
            &self,
            cid: &str,
            compose_path: &str,
            project: &str,
            service: &str,
            tail: Option<u32>,
        ) -> Result<String> {
            self.log.lock().unwrap().push(MockCall::InnerComposeLogs {
                container_id: cid.to_string(),
                compose_path: compose_path.to_string(),
                project: project.to_string(),
                service: service.to_string(),
                tail,
            });
            self.compose_logs_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }

        async fn inner_compose_service_action(
            &self,
            cid: &str,
            compose_path: &str,
            project: &str,
            verb: &str,
            service: &str,
        ) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push(MockCall::InnerComposeServiceAction {
                    container_id: cid.to_string(),
                    compose_path: compose_path.to_string(),
                    project: project.to_string(),
                    verb: verb.to_string(),
                    service: service.to_string(),
                });
            self.compose_service_action_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        async fn host_container_logs(&self, cid: &str, tail: Option<u32>) -> Result<String> {
            self.log.lock().unwrap().push(MockCall::HostContainerLogs {
                container_id: cid.to_string(),
                tail,
            });
            self.host_logs_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let declared = vec!["zeta:1", "alpha:2", "mongo:7"];
        let loaded: Vec<&str> = vec![];
        let missing =
            compute_missing_inner_images(declared.iter().copied(), loaded.iter().copied());
        assert_eq!(missing, vec!["alpha:2", "mongo:7", "zeta:1"]);
    }

    #[test]
    fn compute_missing_dedupes_declared_duplicates() {
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

    #[test]
    fn ssg_exec_output_to_coast_exec_round_trips() {
        let src = SsgExecOutput {
            exit_code: 7,
            stdout: "out".to_string(),
            stderr: "err".to_string(),
        };
        let dst = src.to_coast_exec();
        assert_eq!(dst.exit_code, 7);
        assert_eq!(dst.stdout, "out");
        assert_eq!(dst.stderr, "err");
    }

    // --- argv builders ---

    #[test]
    fn build_inner_compose_service_action_argv_stop_is_stable() {
        assert_eq!(
            build_inner_compose_service_action_argv(
                "/coast-artifact/compose.yml",
                "cg-ssg",
                "stop",
                "postgres",
            ),
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                "cg-ssg",
                "stop",
                "postgres",
            ]
        );
    }

    #[test]
    fn build_inner_compose_service_action_argv_rm_includes_force_and_stop_flags() {
        // `rm` must be `rm -s -f` so a still-running service is
        // force-removed in one call. Mirrors the semantics of
        // `coast service rm` on regular instances.
        assert_eq!(
            build_inner_compose_service_action_argv(
                "/coast-artifact/compose.yml",
                "cg-ssg",
                "rm",
                "redis",
            ),
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                "cg-ssg",
                "rm",
                "-s",
                "-f",
                "redis",
            ]
        );
    }

    #[test]
    fn build_inner_compose_up_argv_is_stable() {
        assert_eq!(
            build_inner_compose_up_argv("/coast-artifact/compose.yml", &[], "coast-ssg"),
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                "coast-ssg",
                "up",
                "-d",
                "--remove-orphans",
            ]
        );
    }

    #[test]
    fn build_inner_compose_up_argv_layers_extra_files_in_order() {
        // Phase 33: per-run override file gets layered on top of
        // the immutable artifact compose file via additional `-f`
        // pairs. Multiple extras flow through in order so callers
        // can stack overrides if needed.
        let extras = vec![
            "/coast-runtime/compose.override.yml".to_string(),
            "/coast-runtime/compose.override2.yml".to_string(),
        ];
        assert_eq!(
            build_inner_compose_up_argv("/coast-artifact/compose.yml", &extras, "cg-ssg"),
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-f",
                "/coast-runtime/compose.override.yml",
                "-f",
                "/coast-runtime/compose.override2.yml",
                "-p",
                "cg-ssg",
                "up",
                "-d",
                "--remove-orphans",
            ]
        );
    }

    #[test]
    fn build_inner_compose_down_argv_is_stable() {
        assert_eq!(
            build_inner_compose_down_argv("/p/compose.yml", "coast-ssg"),
            vec![
                "docker",
                "compose",
                "-f",
                "/p/compose.yml",
                "-p",
                "coast-ssg",
                "down",
            ]
        );
    }

    #[test]
    fn build_inner_compose_exec_argv_appends_user_argv_after_service() {
        let argv = build_inner_compose_exec_argv(
            "/c.yml",
            "coast-ssg",
            "postgres",
            &["psql".to_string(), "-U".to_string(), "coast".to_string()],
        );
        assert_eq!(
            argv,
            vec![
                "docker",
                "compose",
                "-f",
                "/c.yml",
                "-p",
                "coast-ssg",
                "exec",
                "-T",
                "postgres",
                "psql",
                "-U",
                "coast",
            ]
        );
    }

    #[test]
    fn build_inner_compose_logs_argv_honors_tail_default_200() {
        let argv = build_inner_compose_logs_argv("/c.yml", "coast-ssg", "redis", None);
        assert_eq!(
            argv,
            vec![
                "docker",
                "compose",
                "-f",
                "/c.yml",
                "-p",
                "coast-ssg",
                "logs",
                "--tail",
                "200",
                "redis",
            ]
        );
        let argv_50 = build_inner_compose_logs_argv("/c.yml", "coast-ssg", "redis", Some(50));
        assert_eq!(argv_50[8], "50");
    }

    #[test]
    fn build_host_docker_logs_argv_default_tail_200() {
        let argv = build_host_docker_logs_argv("cid-xyz", None);
        assert_eq!(argv, vec!["logs", "--tail", "200", "cid-xyz"]);
        let argv_10 = build_host_docker_logs_argv("cid", Some(10));
        assert_eq!(argv_10, vec!["logs", "--tail", "10", "cid"]);
    }

    // --- Mock sanity ---

    use mock::{MockCall, MockSsgDockerOps};

    #[tokio::test]
    async fn mock_records_every_lifecycle_call() {
        let mock = MockSsgDockerOps::new();
        mock.start_container("cid").await.unwrap();
        mock.stop_container("cid").await.unwrap();
        mock.remove_container("cid").await.unwrap();
        mock.wait_for_inner_daemon("cid", 120).await.unwrap();
        assert_eq!(
            mock.calls(),
            vec![
                MockCall::StartContainer("cid".to_string()),
                MockCall::StopContainer("cid".to_string()),
                MockCall::RemoveContainer("cid".to_string()),
                MockCall::WaitForInnerDaemon {
                    container_id: "cid".to_string(),
                    timeout_s: 120,
                },
            ]
        );
    }

    #[tokio::test]
    async fn mock_create_container_records_real_config_fields() {
        let mock = MockSsgDockerOps::new();
        let mut config = ContainerConfig::new("coast", "ssg", "docker:dind");
        config.container_name_override = Some("coast-ssg".to_string());
        config
            .published_ports
            .push(coast_docker::runtime::PortPublish {
                host_port: 60000,
                container_port: 5432,
            });
        mock.create_container(&config).await.unwrap();
        let calls = mock.calls();
        match &calls[0] {
            MockCall::CreateContainer {
                project,
                instance_name,
                image,
                container_name_override,
                published_ports,
            } => {
                assert_eq!(project, "coast");
                assert_eq!(instance_name, "ssg");
                assert_eq!(image, "docker:dind");
                assert_eq!(container_name_override.as_deref(), Some("coast-ssg"));
                assert_eq!(published_ports, &vec![(60000u16, 5432u16)]);
            }
            other => panic!("unexpected call: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_exec_returns_pushed_result_then_default() {
        let mock = MockSsgDockerOps::new();
        mock.push_exec_result(Ok(SsgExecOutput {
            exit_code: 42,
            stdout: "hi".to_string(),
            stderr: "err".to_string(),
        }));

        let first = mock
            .exec_in_container("cid", &["ls".to_string()])
            .await
            .unwrap();
        assert_eq!(first.exit_code, 42);
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
        let mock = MockSsgDockerOps::new();
        mock.push_create_id(Ok("custom-cid-123".to_string()));
        let config = ContainerConfig::new("coast", "ssg", "docker:dind");

        let first = mock.create_container(&config).await.unwrap();
        assert_eq!(first, "custom-cid-123");

        let second = mock.create_container(&config).await.unwrap();
        assert_eq!(second, "mock-cid");
    }

    #[tokio::test]
    async fn mock_pull_records_image_and_cache_dir() {
        let mock = MockSsgDockerOps::new();
        let out = mock
            .pull_and_cache_image("postgres:16", Path::new("/cache"))
            .await
            .unwrap();
        assert_eq!(out, PathBuf::from("/cache/mock.tar"));
        assert_eq!(
            mock.calls(),
            vec![MockCall::PullAndCacheImage {
                image: "postgres:16".to_string(),
                cache_dir: PathBuf::from("/cache"),
            }]
        );
    }

    #[tokio::test]
    async fn mock_load_images_default_returns_tarball_count() {
        let mock = MockSsgDockerOps::new();
        let n = mock
            .load_images_into_inner(
                "cid",
                &[
                    "/a.tar".to_string(),
                    "/b.tar".to_string(),
                    "/c.tar".to_string(),
                ],
            )
            .await
            .unwrap();
        assert_eq!(n, 3);
    }

    #[tokio::test]
    async fn mock_list_inner_images_default_returns_empty_set() {
        let mock = MockSsgDockerOps::new();
        let set = mock.list_inner_images("cid").await.unwrap();
        assert!(set.is_empty());
    }

    #[tokio::test]
    async fn mock_remove_inner_volumes_default_returns_zero() {
        let mock = MockSsgDockerOps::new();
        let n = mock
            .remove_inner_volumes("cid", "com.docker.compose.project=coast-ssg")
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn mock_inner_compose_up_records_args() {
        let mock = MockSsgDockerOps::new();
        mock.inner_compose_up("cid", "/c.yml", &[], "coast-ssg")
            .await
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![MockCall::InnerComposeUp {
                container_id: "cid".to_string(),
                compose_path: "/c.yml".to_string(),
                extra_compose_files: vec![],
                project: "coast-ssg".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn mock_inner_compose_up_records_extra_files() {
        // Phase 33: when the run path layers a per-run override
        // file, the extras list flows through unchanged.
        let mock = MockSsgDockerOps::new();
        let extras = vec!["/coast-runtime/compose.override.yml".to_string()];
        mock.inner_compose_up("cid", "/c.yml", &extras, "coast-ssg")
            .await
            .unwrap();
        assert_eq!(
            mock.calls(),
            vec![MockCall::InnerComposeUp {
                container_id: "cid".to_string(),
                compose_path: "/c.yml".to_string(),
                extra_compose_files: extras,
                project: "coast-ssg".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn mock_inner_compose_down_records_args() {
        let mock = MockSsgDockerOps::new();
        mock.inner_compose_down("cid", "/c.yml", "coast-ssg")
            .await
            .unwrap();
        match &mock.calls()[0] {
            MockCall::InnerComposeDown { container_id, .. } => {
                assert_eq!(container_id, "cid");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_inner_compose_exec_records_all_args() {
        let mock = MockSsgDockerOps::new();
        mock.inner_compose_exec(
            "cid",
            "/c.yml",
            "coast-ssg",
            "postgres",
            &["psql".to_string(), "-c".to_string(), "SELECT 1".to_string()],
        )
        .await
        .unwrap();
        match &mock.calls()[0] {
            MockCall::InnerComposeExec { service, argv, .. } => {
                assert_eq!(service, "postgres");
                assert_eq!(
                    argv,
                    &vec!["psql".to_string(), "-c".to_string(), "SELECT 1".to_string()]
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_inner_compose_logs_records_args() {
        let mock = MockSsgDockerOps::new();
        mock.push_compose_logs_result(Ok("log text".to_string()));
        let out = mock
            .inner_compose_logs("cid", "/c.yml", "coast-ssg", "redis", Some(50))
            .await
            .unwrap();
        assert_eq!(out, "log text");
        match &mock.calls()[0] {
            MockCall::InnerComposeLogs { service, tail, .. } => {
                assert_eq!(service, "redis");
                assert_eq!(*tail, Some(50));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_host_container_logs_records_args() {
        let mock = MockSsgDockerOps::new();
        mock.push_host_logs_result(Ok("host log text".to_string()));
        let out = mock.host_container_logs("cid-xyz", None).await.unwrap();
        assert_eq!(out, "host log text");
        match &mock.calls()[0] {
            MockCall::HostContainerLogs { container_id, tail } => {
                assert_eq!(container_id, "cid-xyz");
                assert!(tail.is_none());
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_scripted_error_propagates() {
        let mock = MockSsgDockerOps::new();
        mock.push_start_result(Err(CoastError::docker("simulated Docker failure")));
        let err = mock.start_container("cid").await.unwrap_err();
        assert!(err.to_string().contains("simulated Docker failure"));
    }
}
