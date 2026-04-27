//! Per-instance DB creation for SSG postgres/mysql services.
//!
//! Phase: ssg-phase-5. See `DESIGN.md §13`.
//!
//! When a consumer coast references an SSG service with
//! `auto_create_db = true`, the daemon runs a nested exec:
//!
//! ```text
//! docker exec <ssg-outer> \
//!   docker compose -f /coast-artifact/compose.yml -p {project}-ssg exec -T <service> \
//!   psql -U postgres -c "... \\gexec"
//! ```
//!
//! The SQL command construction is shared with the inline shared
//! services path (`coast-daemon/src/shared_services.rs::create_db_command`).
//! This module owns only the nested-exec wrapper.

use coast_core::error::{CoastError, Result};
use coast_docker::runtime::ExecResult;

use crate::docker_ops::{build_inner_compose_exec_argv, SsgDockerOps};
use crate::runtime::lifecycle::{inner_compose_path, ssg_compose_project};
use crate::state::SsgRecord;

/// Build the argv that runs `command` inside an inner SSG service
/// container, via `docker compose exec -T <service>` on the outer
/// DinD daemon.
///
/// Thin wrapper around [`build_inner_compose_exec_argv`] that adds
/// empty-argument validation (the pure builder in `docker_ops`
/// doesn't validate because it's also used for the happy-path
/// lifecycle `exec_ssg` where validation has already happened).
pub(crate) fn build_nested_compose_exec_argv(
    project: &str,
    service_name: &str,
    command: &[String],
) -> Result<Vec<String>> {
    if service_name.is_empty() {
        return Err(CoastError::coastfile(
            "SSG service name is required for nested exec (got empty string).",
        ));
    }
    if command.is_empty() {
        return Err(CoastError::coastfile(
            "Nested SSG exec requires a non-empty command vector.",
        ));
    }
    Ok(build_inner_compose_exec_argv(
        &inner_compose_path(),
        &ssg_compose_project(project),
        service_name,
        command,
    ))
}

/// Run `command` inside the inner `service_name` container of the SSG
/// singleton DinD.
///
/// Returns the raw [`ExecResult`]; the caller decides how to treat
/// non-zero exits.
/// [`crate::daemon_integration::create_instance_db_for_consumer`] is
/// the standard wrapper that promotes non-zero exits to errors.
pub async fn exec_in_ssg_service(
    ops: &dyn SsgDockerOps,
    record: &SsgRecord,
    service_name: &str,
    command: Vec<String>,
) -> Result<ExecResult> {
    let container_id = record.container_id.clone().ok_or_else(|| {
        CoastError::coastfile(
            "SSG record has no container id; cannot exec against an inner service.",
        )
    })?;

    // Validate via the nested-exec builder (keeps the existing
    // empty-argv error messages verbatim for Phase 5 callers).
    let _ = build_nested_compose_exec_argv(&record.project, service_name, &command)?;

    let out = ops
        .inner_compose_exec(
            &container_id,
            &inner_compose_path(),
            &ssg_compose_project(&record.project),
            service_name,
            &command,
        )
        .await?;
    Ok(out.to_coast_exec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_argv_minimal_psql_command() {
        let argv = build_nested_compose_exec_argv(
            "cg",
            "postgres",
            &[
                "psql".to_string(),
                "-U".to_string(),
                "postgres".to_string(),
                "-c".to_string(),
                "SELECT 1".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(
            argv,
            vec![
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                // Per-project SSG (§23): compose label derives from
                // the consumer project name, not a global constant.
                "cg-ssg",
                "exec",
                "-T",
                "postgres",
                "psql",
                "-U",
                "postgres",
                "-c",
                "SELECT 1",
            ]
        );
    }

    #[test]
    fn build_argv_errors_on_empty_service_name() {
        let err = build_nested_compose_exec_argv("cg", "", &["psql".to_string()]).unwrap_err();
        assert!(
            err.to_string().contains("service name is required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn build_argv_errors_on_empty_command() {
        let err = build_nested_compose_exec_argv("cg", "postgres", &[]).unwrap_err();
        assert!(
            err.to_string().contains("non-empty command"),
            "unexpected error: {err}"
        );
    }

    // --- Phase 12 exec_in_ssg_service tests (MockSsgDockerOps) ---

    use crate::docker_ops::{MockCall, MockSsgDockerOps, SsgExecOutput};

    fn sample_record(cid: Option<&str>) -> SsgRecord {
        SsgRecord {
            project: "test-proj".to_string(),
            status: "running".to_string(),
            container_id: cid.map(str::to_string),
            build_id: Some("b_test".to_string()),
            latest_build_id: Some("b_test".to_string()),
            created_at: "2026-04-20T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn exec_in_ssg_service_delegates_to_inner_compose_exec() {
        let mock = MockSsgDockerOps::new();
        mock.push_compose_exec_result(Ok(SsgExecOutput {
            exit_code: 0,
            stdout: "CREATE DATABASE".to_string(),
            stderr: String::new(),
        }));
        let record = sample_record(Some("cid-1"));
        let out = exec_in_ssg_service(
            &mock,
            &record,
            "postgres",
            vec!["psql".to_string(), "-U".to_string(), "postgres".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(out.exit_code, 0);
        assert_eq!(out.stdout, "CREATE DATABASE");
        match &mock.calls()[0] {
            MockCall::InnerComposeExec {
                container_id,
                service,
                argv,
                ..
            } => {
                assert_eq!(container_id, "cid-1");
                assert_eq!(service, "postgres");
                assert_eq!(
                    argv,
                    &vec!["psql".to_string(), "-U".to_string(), "postgres".to_string(),]
                );
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn exec_in_ssg_service_missing_container_id_errors() {
        let mock = MockSsgDockerOps::new();
        let record = sample_record(None);
        let err = exec_in_ssg_service(&mock, &record, "postgres", vec!["psql".to_string()])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no container id"));
    }

    #[test]
    fn build_argv_preserves_multi_token_command_order() {
        // `psql -c "SELECT ... \gexec"` from `create_db_command` is
        // passed as-is; verify the full sequence lands after the
        // `exec -T <service>` prefix in order.
        let cmd = vec![
            "psql".to_string(),
            "-U".to_string(),
            "postgres".to_string(),
            "-c".to_string(),
            "SELECT 'CREATE DATABASE \"foo\"' ... \\gexec".to_string(),
        ];
        let argv = build_nested_compose_exec_argv("cg", "db", &cmd).unwrap();

        // First 9 elements are the fixed prefix + service.
        assert_eq!(
            &argv[0..9],
            &[
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                "cg-ssg",
                "exec",
                "-T",
                "db",
            ]
        );
        // Remainder is the user command in original order.
        assert_eq!(&argv[9..], cmd.as_slice());
    }
}
