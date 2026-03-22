use std::path::Path;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::BareServiceConfig;
use coast_docker::runtime::{ExecResult, Runtime};

use super::emit;

const ARTIFACT_COMPOSE_PATH: &str = "/coast-artifact/compose.yml";
const MERGED_COMPOSE_PATH: &str = "/coast-override/docker-compose.coast.yml";

trait ExecRuntime: Send + Sync {
    async fn exec_in_coast(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult>;
}

impl ExecRuntime for coast_docker::dind::DindRuntime {
    async fn exec_in_coast(&self, container_id: &str, cmd: &[&str]) -> Result<ExecResult> {
        Runtime::exec_in_coast(self, container_id, cmd).await
    }
}

pub(super) struct StartServicesRequest<'a> {
    pub container_id: &'a str,
    pub instance_name: &'a str,
    pub project: &'a str,
    pub has_compose: bool,
    pub has_services: bool,
    pub compose_rel_dir: Option<&'a str>,
    pub artifact_dir_opt: Option<&'a Path>,
    pub bare_services: &'a [BareServiceConfig],
    pub total_steps: u32,
    pub progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
}

impl StartServicesRequest<'_> {
    fn starting_step(&self) -> u32 {
        self.total_steps - 1
    }
}

pub(super) async fn start_and_wait_for_services(
    docker: &bollard::Docker,
    request: StartServicesRequest<'_>,
) -> Result<()> {
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());
    start_and_wait_for_services_with_runtime(&runtime, &request).await
}

async fn start_and_wait_for_services_with_runtime<R: ExecRuntime>(
    runtime: &R,
    request: &StartServicesRequest<'_>,
) -> Result<()> {
    if request.has_compose {
        run_compose_services(runtime, request).await?;
    }

    if request.has_services {
        run_bare_services(runtime, request).await?;
    }

    if !request.has_compose && !request.has_services {
        info!(
            instance = %request.instance_name,
            "no compose file configured — skipping compose up. Instance is Idle."
        );
    }

    Ok(())
}

fn health_poll_interval(elapsed: tokio::time::Duration) -> tokio::time::Duration {
    if elapsed.as_secs() < 5 {
        tokio::time::Duration::from_millis(500)
    } else if elapsed.as_secs() < 30 {
        tokio::time::Duration::from_secs(1)
    } else {
        tokio::time::Duration::from_secs(2)
    }
}

fn compose_project_dir(compose_rel_dir: Option<&str>) -> String {
    if let Some(dir) = compose_rel_dir {
        format!("/workspace/{dir}")
    } else {
        "/workspace".to_string()
    }
}

fn compose_project_name(project: &str, compose_rel_dir: Option<&str>) -> String {
    compose_rel_dir
        .map(std::string::ToString::to_string)
        .unwrap_or_else(|| format!("coast-{project}"))
}

fn compose_base_args(
    has_merged_override: bool,
    artifact_compose_exists: bool,
    compose_project_name: &str,
    project_dir: &str,
) -> Vec<String> {
    if has_merged_override {
        return vec![
            "docker".into(),
            "compose".into(),
            "-p".into(),
            compose_project_name.to_string(),
            "-f".into(),
            MERGED_COMPOSE_PATH.into(),
            "--project-directory".into(),
            project_dir.to_string(),
        ];
    }

    if artifact_compose_exists {
        return vec![
            "docker".into(),
            "compose".into(),
            "-p".into(),
            compose_project_name.to_string(),
            "-f".into(),
            ARTIFACT_COMPOSE_PATH.into(),
            "--project-directory".into(),
            project_dir.to_string(),
        ];
    }

    vec![
        "docker".into(),
        "compose".into(),
        "-p".into(),
        compose_project_name.to_string(),
    ]
}

async fn build_compose_base_args<R: ExecRuntime>(
    runtime: &R,
    request: &StartServicesRequest<'_>,
) -> Vec<String> {
    let project_dir = compose_project_dir(request.compose_rel_dir);
    let project_name = compose_project_name(request.project, request.compose_rel_dir);
    let has_merged_override = has_merged_override(runtime, request.container_id).await;

    compose_base_args(
        has_merged_override,
        request.artifact_dir_opt.is_some(),
        &project_name,
        &project_dir,
    )
}

async fn has_merged_override<R: ExecRuntime>(runtime: &R, container_id: &str) -> bool {
    runtime
        .exec_in_coast(container_id, &["test", "-f", MERGED_COMPOSE_PATH])
        .await
        .map(|result| result.success())
        .unwrap_or(false)
}

fn extend_command(base: &[String], extra: &[&str]) -> Vec<String> {
    let mut command = base.to_vec();
    command.extend(extra.iter().map(|arg| (*arg).to_string()));
    command
}

async fn exec_string_command<R: ExecRuntime>(
    runtime: &R,
    container_id: &str,
    command: &[String],
) -> Result<ExecResult> {
    let command_refs: Vec<&str> = command.iter().map(std::string::String::as_str).collect();
    runtime.exec_in_coast(container_id, &command_refs).await
}

async fn run_compose_up<R: ExecRuntime>(
    runtime: &R,
    container_id: &str,
    compose_base_args: &[String],
) -> Result<ExecResult> {
    let compose_cmd = extend_command(compose_base_args, &["up", "-d", "--remove-orphans"]);
    exec_string_command(runtime, container_id, &compose_cmd).await
}

fn compose_ps_output_is_ready(output: &str) -> bool {
    !output.is_empty()
        && output
            .lines()
            .all(|line| line.contains("running") || line.contains("healthy"))
}

async fn wait_for_compose_health<R: ExecRuntime>(
    runtime: &R,
    container_id: &str,
    compose_base_args: &[String],
    instance_name: &str,
) -> Result<()> {
    wait_for_compose_health_with_timeout(
        runtime,
        container_id,
        compose_base_args,
        instance_name,
        tokio::time::Duration::from_secs(120),
    )
    .await
}

async fn wait_for_compose_health_with_timeout<R: ExecRuntime>(
    runtime: &R,
    container_id: &str,
    compose_base_args: &[String],
    instance_name: &str,
    timeout: tokio::time::Duration,
) -> Result<()> {
    let start_time = tokio::time::Instant::now();

    loop {
        if start_time.elapsed() >= timeout {
            let log_cmd = extend_command(compose_base_args, &["logs", "--tail", "50"]);
            let logs = exec_string_command(runtime, container_id, &log_cmd)
                .await
                .map(|result| result.stdout)
                .unwrap_or_default();
            return Err(CoastError::docker(format!(
                "Services in instance '{}' did not become healthy within 120s. \
                 Check the service logs below and fix any issues, then retry with \
                 `coast rm {} && coast run {}`.\nRecent logs:\n{}",
                instance_name, instance_name, instance_name, logs
            )));
        }

        let ps_cmd = extend_command(compose_base_args, &["ps", "--format", "json"]);
        if let Ok(result) = exec_string_command(runtime, container_id, &ps_cmd).await {
            if result.success() && compose_ps_output_is_ready(&result.stdout) {
                info!(instance = %instance_name, "all compose services are healthy");
                break;
            }
        }

        tokio::time::sleep(health_poll_interval(start_time.elapsed())).await;
    }

    Ok(())
}

async fn run_compose_services<R: ExecRuntime>(
    runtime: &R,
    request: &StartServicesRequest<'_>,
) -> Result<()> {
    emit(
        request.progress,
        BuildProgressEvent::started(
            "Starting services",
            request.starting_step(),
            request.total_steps,
        ),
    );

    let compose_base_args = build_compose_base_args(runtime, request).await;
    let compose_result = run_compose_up(runtime, request.container_id, &compose_base_args).await;
    if let Err(error) = &compose_result {
        warn!(error = %error, "docker compose up failed");
    }

    wait_for_compose_health(
        runtime,
        request.container_id,
        &compose_base_args,
        request.instance_name,
    )
    .await?;

    emit(
        request.progress,
        BuildProgressEvent::done("Starting services", "ok"),
    );

    Ok(())
}

async fn run_bare_services<R: ExecRuntime>(
    runtime: &R,
    request: &StartServicesRequest<'_>,
) -> Result<()> {
    if !request.has_compose {
        emit(
            request.progress,
            BuildProgressEvent::started(
                "Starting services",
                request.starting_step(),
                request.total_steps,
            ),
        );
    }

    let setup_cmd = crate::bare_services::generate_setup_and_start_command(request.bare_services);
    let setup_result = runtime
        .exec_in_coast(request.container_id, &["sh", "-c", &setup_cmd])
        .await
        .map_err(|error| {
            CoastError::docker(format!(
                "Failed to start bare services for instance '{}': {}",
                request.instance_name, error
            ))
        })?;

    if !setup_result.success() {
        return Err(CoastError::docker(format!(
            "Failed to start bare services for instance '{}' (exit code {}): {}",
            request.instance_name, setup_result.exit_code, setup_result.stderr
        )));
    }

    for service in request.bare_services {
        emit(
            request.progress,
            BuildProgressEvent::item(
                "Starting services",
                format!("{} ({})", service.name, service.command),
                "started",
            ),
        );
    }

    if !request.has_compose {
        emit(
            request.progress,
            BuildProgressEvent::done("Starting services", "ok"),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use coast_core::types::RestartPolicy;

    use super::*;

    #[derive(Clone, Default)]
    struct FakeRuntime {
        commands: Arc<Mutex<Vec<Vec<String>>>>,
        responses: Arc<Mutex<VecDeque<Result<ExecResult>>>>,
    }

    impl FakeRuntime {
        fn with_responses(responses: Vec<Result<ExecResult>>) -> Self {
            Self {
                commands: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses.into())),
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.lock().unwrap().clone()
        }
    }

    impl ExecRuntime for FakeRuntime {
        async fn exec_in_coast(&self, _container_id: &str, cmd: &[&str]) -> Result<ExecResult> {
            self.commands
                .lock()
                .unwrap()
                .push(cmd.iter().map(|arg| (*arg).to_string()).collect());

            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(success_result("", "")))
        }
    }

    fn success_result(stdout: &str, stderr: &str) -> ExecResult {
        ExecResult {
            exit_code: 0,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    fn failure_result(exit_code: i64, stderr: &str) -> ExecResult {
        ExecResult {
            exit_code,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    fn progress_channel() -> (
        tokio::sync::mpsc::Sender<BuildProgressEvent>,
        tokio::sync::mpsc::Receiver<BuildProgressEvent>,
    ) {
        tokio::sync::mpsc::channel(32)
    }

    fn collect_progress(
        receiver: &mut tokio::sync::mpsc::Receiver<BuildProgressEvent>,
    ) -> Vec<BuildProgressEvent> {
        let mut events = Vec::new();
        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }
        events
    }

    fn sample_bare_services() -> Vec<BareServiceConfig> {
        vec![BareServiceConfig {
            name: "api".to_string(),
            command: "python -m http.server 8080".to_string(),
            port: Some(8080),
            restart: RestartPolicy::No,
            install: Vec::new(),
            cache: Vec::new(),
        }]
    }

    fn sample_request<'a>(
        progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
        bare_services: &'a [BareServiceConfig],
    ) -> StartServicesRequest<'a> {
        StartServicesRequest {
            container_id: "container-123",
            instance_name: "dev-1",
            project: "proj",
            has_compose: false,
            has_services: false,
            compose_rel_dir: None,
            artifact_dir_opt: None,
            bare_services,
            total_steps: 5,
            progress,
        }
    }

    #[test]
    fn test_health_poll_interval_first_five_seconds() {
        assert_eq!(
            health_poll_interval(tokio::time::Duration::from_secs(0)),
            tokio::time::Duration::from_millis(500)
        );
        assert_eq!(
            health_poll_interval(tokio::time::Duration::from_secs(4)),
            tokio::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn test_health_poll_interval_mid_window() {
        assert_eq!(
            health_poll_interval(tokio::time::Duration::from_secs(5)),
            tokio::time::Duration::from_secs(1)
        );
        assert_eq!(
            health_poll_interval(tokio::time::Duration::from_secs(29)),
            tokio::time::Duration::from_secs(1)
        );
    }

    #[test]
    fn test_health_poll_interval_late_window() {
        assert_eq!(
            health_poll_interval(tokio::time::Duration::from_secs(30)),
            tokio::time::Duration::from_secs(2)
        );
    }

    #[test]
    fn test_compose_project_dir_no_rel_dir() {
        assert_eq!(compose_project_dir(None), "/workspace");
    }

    #[test]
    fn test_compose_project_dir_with_rel_dir() {
        assert_eq!(compose_project_dir(Some("apps/web")), "/workspace/apps/web");
    }

    #[test]
    fn test_compose_project_name_uses_relative_dir_when_present() {
        assert_eq!(compose_project_name("proj", Some("apps/web")), "apps/web");
        assert_eq!(compose_project_name("proj", None), "coast-proj");
    }

    #[test]
    fn test_compose_base_args_with_merged_override() {
        let args = compose_base_args(true, false, "apps/web", "/workspace/apps/web");
        assert_eq!(
            args,
            vec![
                "docker",
                "compose",
                "-p",
                "apps/web",
                "-f",
                MERGED_COMPOSE_PATH,
                "--project-directory",
                "/workspace/apps/web",
            ]
        );
    }

    #[test]
    fn test_compose_base_args_with_artifact_compose() {
        let args = compose_base_args(false, true, "coast-proj", "/workspace");
        assert_eq!(
            args,
            vec![
                "docker",
                "compose",
                "-p",
                "coast-proj",
                "-f",
                ARTIFACT_COMPOSE_PATH,
                "--project-directory",
                "/workspace",
            ]
        );
    }

    #[test]
    fn test_compose_base_args_merged_override_wins_over_artifact() {
        let args = compose_base_args(true, true, "coast-proj", "/workspace");
        assert!(
            args.contains(&MERGED_COMPOSE_PATH.to_string()),
            "merged override should take priority when both exist"
        );
        assert!(
            !args.contains(&ARTIFACT_COMPOSE_PATH.to_string()),
            "artifact compose should not appear when merged override exists"
        );
    }

    #[test]
    fn test_compose_base_args_plain_workspace() {
        let args = compose_base_args(false, false, "coast-proj", "/workspace");
        assert_eq!(args, vec!["docker", "compose", "-p", "coast-proj"]);
    }

    #[test]
    fn test_compose_project_dir_never_returns_tmp() {
        for rel_dir in [None, Some("apps/web"), Some("sub/dir")] {
            let dir = compose_project_dir(rel_dir);
            assert!(
                dir.starts_with("/workspace"),
                "compose project dir must always be under /workspace, got: {dir}"
            );
        }
    }

    #[test]
    fn test_compose_ps_output_is_ready_with_running_lines() {
        let output = r#"{"State":"running"}
{"State":"running"}"#;
        assert!(compose_ps_output_is_ready(output));
    }

    #[test]
    fn test_compose_ps_output_is_ready_matches_healthy_substring() {
        assert!(compose_ps_output_is_ready(r#"{"Health":"healthy"}"#));
    }

    #[test]
    fn test_compose_ps_output_is_ready_rejects_empty_or_unhealthy_lines() {
        assert!(!compose_ps_output_is_ready(""));
        assert!(!compose_ps_output_is_ready(r#"{"State":"exited"}"#));
    }

    #[tokio::test]
    async fn test_start_services_no_compose_and_no_bare_services_is_noop() {
        let (progress, mut receiver) = progress_channel();
        let request = sample_request(&progress, &[]);
        let runtime = FakeRuntime::default();

        start_and_wait_for_services_with_runtime(&runtime, &request)
            .await
            .unwrap();

        assert!(runtime.commands().is_empty());
        assert!(collect_progress(&mut receiver).is_empty());
    }

    #[tokio::test]
    async fn test_start_services_bare_only_emits_started_items_done() {
        let (progress, mut receiver) = progress_channel();
        let bare_services = sample_bare_services();
        let mut request = sample_request(&progress, &bare_services);
        request.has_services = true;
        let runtime = FakeRuntime::with_responses(vec![Ok(success_result("", ""))]);

        start_and_wait_for_services_with_runtime(&runtime, &request)
            .await
            .unwrap();

        let setup_cmd = crate::bare_services::generate_setup_and_start_command(&bare_services);
        assert_eq!(
            runtime.commands(),
            vec![vec!["sh".to_string(), "-c".to_string(), setup_cmd]]
        );

        let events = collect_progress(&mut receiver);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].step, "Starting services");
        assert_eq!(events[0].status, "started");
        assert_eq!(events[0].step_number, Some(4));
        assert_eq!(events[1].step, "Starting services");
        assert_eq!(events[1].status, "started");
        assert_eq!(
            events[1].detail,
            Some("api (python -m http.server 8080)".to_string())
        );
        assert_eq!(events[2].step, "Starting services");
        assert_eq!(events[2].status, "ok");
    }

    #[tokio::test]
    async fn test_start_services_compose_only_uses_merged_override_and_emits_started_done() {
        let (progress, mut receiver) = progress_channel();
        let mut request = sample_request(&progress, &[]);
        request.has_compose = true;
        request.compose_rel_dir = Some("apps/web");
        let artifact_dir = Path::new("/coast-artifact");
        request.artifact_dir_opt = Some(artifact_dir);
        let runtime = FakeRuntime::with_responses(vec![
            Ok(success_result("", "")),
            Ok(success_result("", "")),
            Ok(success_result(r#"{"State":"running"}"#, "")),
        ]);

        start_and_wait_for_services_with_runtime(&runtime, &request)
            .await
            .unwrap();

        assert_eq!(
            runtime.commands(),
            vec![
                vec![
                    "test".to_string(),
                    "-f".to_string(),
                    MERGED_COMPOSE_PATH.to_string()
                ],
                vec![
                    "docker".to_string(),
                    "compose".to_string(),
                    "-p".to_string(),
                    "apps/web".to_string(),
                    "-f".to_string(),
                    MERGED_COMPOSE_PATH.to_string(),
                    "--project-directory".to_string(),
                    "/workspace/apps/web".to_string(),
                    "up".to_string(),
                    "-d".to_string(),
                    "--remove-orphans".to_string(),
                ],
                vec![
                    "docker".to_string(),
                    "compose".to_string(),
                    "-p".to_string(),
                    "apps/web".to_string(),
                    "-f".to_string(),
                    MERGED_COMPOSE_PATH.to_string(),
                    "--project-directory".to_string(),
                    "/workspace/apps/web".to_string(),
                    "ps".to_string(),
                    "--format".to_string(),
                    "json".to_string(),
                ],
            ]
        );

        let events = collect_progress(&mut receiver);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step, "Starting services");
        assert_eq!(events[0].status, "started");
        assert_eq!(events[1].step, "Starting services");
        assert_eq!(events[1].status, "ok");
    }

    #[tokio::test]
    async fn test_start_services_compose_and_bare_services_emit_bare_items_after_compose_done() {
        let (progress, mut receiver) = progress_channel();
        let bare_services = sample_bare_services();
        let mut request = sample_request(&progress, &bare_services);
        request.has_compose = true;
        request.has_services = true;
        let runtime = FakeRuntime::with_responses(vec![
            Ok(failure_result(1, "")),
            Ok(success_result("", "")),
            Ok(success_result(r#"{"State":"running"}"#, "")),
            Ok(success_result("", "")),
        ]);

        start_and_wait_for_services_with_runtime(&runtime, &request)
            .await
            .unwrap();

        let events = collect_progress(&mut receiver);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].status, "started");
        assert_eq!(events[1].status, "ok");
        assert_eq!(events[2].status, "started");
        assert_eq!(
            events[2].detail,
            Some("api (python -m http.server 8080)".to_string())
        );
    }

    #[tokio::test]
    async fn test_wait_for_compose_health_timeout_includes_recent_logs() {
        let runtime = FakeRuntime::with_responses(vec![Ok(success_result("service logs", ""))]);
        let compose_base_args = compose_base_args(false, false, "coast-proj", "/workspace");

        let error = wait_for_compose_health_with_timeout(
            &runtime,
            "container-123",
            &compose_base_args,
            "dev-1",
            tokio::time::Duration::ZERO,
        )
        .await
        .unwrap_err();

        let message = error.to_string();
        assert!(message.contains("did not become healthy within 120s"));
        assert!(message.contains("service logs"));
        assert!(message.contains("coast rm dev-1 && coast run dev-1"));
        assert_eq!(
            runtime.commands(),
            vec![vec![
                "docker".to_string(),
                "compose".to_string(),
                "-p".to_string(),
                "coast-proj".to_string(),
                "logs".to_string(),
                "--tail".to_string(),
                "50".to_string(),
            ]]
        );
    }

    #[tokio::test]
    async fn test_run_bare_services_failure_includes_exit_code_and_stderr() {
        let (progress, _receiver) = progress_channel();
        let bare_services = sample_bare_services();
        let mut request = sample_request(&progress, &bare_services);
        request.has_services = true;
        let runtime = FakeRuntime::with_responses(vec![Ok(failure_result(17, "boom"))]);

        let error = run_bare_services(&runtime, &request).await.unwrap_err();
        let message = error.to_string();
        assert!(message.contains("Failed to start bare services for instance 'dev-1'"));
        assert!(message.contains("exit code 17"));
        assert!(message.contains("boom"));
    }
}
