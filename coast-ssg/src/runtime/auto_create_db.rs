//! Per-instance DB creation for SSG postgres/mysql services.
//!
//! Phase: ssg-phase-5. See `DESIGN.md §13`.
//!
//! When a consumer coast references an SSG service with
//! `auto_create_db = true`, the daemon runs a nested exec:
//!
//! ```text
//! docker exec <ssg-outer> \
//!   docker compose -f /coast-artifact/compose.yml -p coast-ssg exec -T <service> \
//!   psql -U postgres -c "... \\gexec"
//! ```
//!
//! The SQL command construction is shared with the inline shared
//! services path (`coast-daemon/src/shared_services.rs::create_db_command`).
//! This module owns only the nested-exec wrapper.

use bollard::Docker;

use coast_core::error::{CoastError, Result};
use coast_docker::dind::DindRuntime;
use coast_docker::runtime::{ExecResult, Runtime};

use crate::runtime::lifecycle::{inner_compose_path, SSG_COMPOSE_PROJECT};
use crate::state::SsgRecord;

/// Build the argv that runs `command` inside an inner SSG service
/// container, via `docker compose exec -T <service>` on the outer
/// DinD daemon.
///
/// Broken out from [`exec_in_ssg_service`] so it can be unit-tested
/// without a live Docker socket.
pub(crate) fn build_nested_compose_exec_argv(
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

    let mut argv: Vec<String> = vec![
        "docker".to_string(),
        "compose".to_string(),
        "-f".to_string(),
        inner_compose_path(),
        "-p".to_string(),
        SSG_COMPOSE_PROJECT.to_string(),
        "exec".to_string(),
        "-T".to_string(),
        service_name.to_string(),
    ];
    argv.extend(command.iter().cloned());
    Ok(argv)
}

/// Run `command` inside the inner `service_name` container of the SSG
/// singleton DinD.
///
/// Returns the raw [`ExecResult`]; the caller decides how to treat
/// non-zero exits. [`daemon_integration::create_instance_db_for_consumer`]
/// is the standard wrapper that promotes non-zero exits to errors.
pub async fn exec_in_ssg_service(
    docker: &Docker,
    record: &SsgRecord,
    service_name: &str,
    command: Vec<String>,
) -> Result<ExecResult> {
    let container_id = record.container_id.clone().ok_or_else(|| {
        CoastError::coastfile(
            "SSG record has no container id; cannot exec against an inner service.",
        )
    })?;

    let argv = build_nested_compose_exec_argv(service_name, &command)?;
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();

    let runtime = DindRuntime::with_client(docker.clone());
    runtime.exec_in_coast(&container_id, &refs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_argv_minimal_psql_command() {
        let argv = build_nested_compose_exec_argv(
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
                "coast-ssg",
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
        let err = build_nested_compose_exec_argv("", &["psql".to_string()]).unwrap_err();
        assert!(
            err.to_string().contains("service name is required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn build_argv_errors_on_empty_command() {
        let err = build_nested_compose_exec_argv("postgres", &[]).unwrap_err();
        assert!(
            err.to_string().contains("non-empty command"),
            "unexpected error: {err}"
        );
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
        let argv = build_nested_compose_exec_argv("db", &cmd).unwrap();

        // First 9 elements are the fixed prefix + service.
        assert_eq!(
            &argv[0..9],
            &[
                "docker",
                "compose",
                "-f",
                "/coast-artifact/compose.yml",
                "-p",
                "coast-ssg",
                "exec",
                "-T",
                "db",
            ]
        );
        // Remainder is the user command in original order.
        assert_eq!(&argv[9..], cmd.as_slice());
    }
}
