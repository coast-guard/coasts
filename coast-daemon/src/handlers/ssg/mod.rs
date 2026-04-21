//! Handler for `coast ssg *` requests (non-streaming variants).
//!
//! Phase 2 landed `Ps`. Phase 3 lands the full lifecycle:
//! `Stop`, `Rm`, `Logs` (non-follow), `Exec`, `Ports`. The streaming
//! variants (`Build`, `Run`, `Start`, `Restart`, `Logs { follow: true }`)
//! never reach this handler — they are intercepted by the streaming
//! routers in `server.rs`.
//!
//! Mutating verbs acquire `AppState.ssg_mutex` before dispatching
//! into `coast_ssg::daemon_integration`. Read-only verbs do not.
//! See `coast-ssg/DESIGN.md §17-5` for mutex scope.
//!
//! Lifecycle functions do not touch the SQLite state DB themselves —
//! this handler reads the current state before the async Docker
//! section and applies writes afterwards. That split exists because
//! `StateDb` wraps a `!Sync` `rusqlite::Connection`, which would
//! otherwise reject the `Send` bound on streaming futures.

// ssg-phase-6: checkout / uncheckout orchestrator (host-side canonical
// port binding via socat). Lives in a sibling file to keep `mod.rs`
// focused on the dispatcher + non-checkout lifecycle verbs.
pub mod checkout;

// ssg-phase-8: host bind-mount permission doctor. See `doctor.rs` +
// `coast-ssg/src/doctor.rs` for the pure evaluator.
pub mod doctor;

// ssg-phase-11: refresh consumer socat forwarders after SSG lifecycle
// verbs reallocate dynamic ports. See
// `coast-ssg/DESIGN.md §0 Phase 11` + `§17-38 SETTLED`.
pub(crate) mod consumer_refresh;

use std::sync::Arc;

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SsgRequest, SsgResponse};
use coast_ssg::state::{SsgRecord, SsgStateExt};

use crate::server::AppState;

/// Dispatch a non-streaming SSG request.
///
/// `Build`, `Run`, `Start`, `Restart`, and `Logs { follow: true }`
/// never reach this handler — they are intercepted upstream.
pub async fn handle(state: Arc<AppState>, req: SsgRequest) -> Result<SsgResponse> {
    match req {
        SsgRequest::Ps => {
            let db = state.db.lock().await;
            coast_ssg::daemon_integration::ps_ssg(Some(&*db as &dyn SsgStateExt))
        }
        SsgRequest::Ports => {
            let db = state.db.lock().await;
            coast_ssg::daemon_integration::ports_ssg(&*db)
        }

        SsgRequest::Stop { force } => handle_stop(&state, force).await,
        SsgRequest::Rm { with_data, force } => handle_rm(&state, with_data, force).await,

        SsgRequest::Logs {
            service,
            tail,
            follow,
        } => {
            if follow {
                unreachable!(
                    "SsgRequest::Logs {{ follow: true }} handled by handle_ssg_logs_streaming"
                )
            }
            handle_logs(&state, service, tail).await
        }

        SsgRequest::Exec { service, command } => handle_exec(&state, service, command).await,

        SsgRequest::Run => {
            unreachable!("SsgRequest::Run handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Start => {
            unreachable!("SsgRequest::Start handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Restart => {
            unreachable!("SsgRequest::Restart handled by handle_ssg_lifecycle_streaming")
        }
        SsgRequest::Build { .. } => {
            unreachable!("SsgRequest::Build handled by handle_ssg_build_streaming")
        }

        SsgRequest::Checkout { service, all } => {
            checkout::handle_checkout(&state, service, all).await
        }
        SsgRequest::Uncheckout { service, all } => {
            checkout::handle_uncheckout(&state, service, all).await
        }

        SsgRequest::Doctor => doctor::handle_doctor(&state).await,
    }
}

async fn handle_stop(state: &Arc<AppState>, force: bool) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot stop the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };
    let Some(record) = record else {
        return Ok(build_stop_response_missing_record());
    };

    // Phase 4.5 gate: refuse to stop while remote shadow coasts are
    // currently consuming the SSG unless `--force` is set. With
    // `--force`, kill the reverse-tunnel ssh children first so the
    // shadow coast doesn't leak stale ssh processes.
    enforce_shadow_gate_and_maybe_tear_down(state, force, "stop").await?;

    coast_ssg::daemon_integration::stop_ssg(&docker, &record).await?;

    {
        let db = state.db.lock().await;
        db.upsert_ssg(
            "stopped",
            record.container_id.as_deref(),
            record.build_id.as_deref(),
        )?;
        for svc in db.list_ssg_services()? {
            db.update_ssg_service_status(&svc.service_name, "stopped")?;
        }
    }

    // Phase 6: preserve `ssg_port_checkouts` rows but null their
    // socat_pid columns and kill the live socats. Next `run / start`
    // re-spawns against the new dynamic ports.
    checkout::kill_active_checkout_socats_preserve_rows(state).await;

    Ok(build_stop_response_success())
}

async fn handle_rm(state: &Arc<AppState>, with_data: bool, force: bool) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot remove the SSG."))?;
    let _ssg_guard = state.ssg_mutex.lock().await;

    let record = {
        let db = state.db.lock().await;
        db.get_ssg()?
    };
    let Some(record) = record else {
        return Ok(build_rm_response_missing_record());
    };

    enforce_shadow_gate_and_maybe_tear_down(state, force, "remove").await?;

    // Phase 6: tear down checkouts before the SSG itself. Doing it
    // first means if the subsequent Docker rm fails and the user
    // retries, we don't end up with dangling checkout rows pointing
    // at a partially-removed SSG.
    checkout::kill_and_clear_all_checkouts(state).await;

    coast_ssg::daemon_integration::rm_ssg(&docker, &record, with_data).await?;

    let db = state.db.lock().await;
    db.clear_ssg()?;
    db.clear_ssg_services()?;

    Ok(build_rm_response_success(with_data))
}

async fn handle_logs(
    state: &Arc<AppState>,
    service: Option<String>,
    tail: Option<u32>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot tail SSG logs."))?;

    let record = fetch_required_record(state).await?;
    let text = coast_ssg::daemon_integration::logs_ssg(&docker, &record, service, tail).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    })
}

async fn handle_exec(
    state: &Arc<AppState>,
    service: Option<String>,
    command: Vec<String>,
) -> Result<SsgResponse> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker is unavailable; cannot exec against the SSG."))?;

    let record = fetch_required_record(state).await?;
    let text = coast_ssg::daemon_integration::exec_ssg(&docker, &record, service, command).await?;

    Ok(SsgResponse {
        message: text,
        status: Some(record.status),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    })
}

async fn fetch_required_record(state: &Arc<AppState>) -> Result<SsgRecord> {
    let db = state.db.lock().await;
    db.get_ssg()?.ok_or_else(|| {
        CoastError::coastfile("SSG has not been created. Run `coast ssg run` first.")
    })
}

/// Identifier for a shadow instance that is currently consuming the SSG.
#[derive(Debug, Clone)]
struct ShadowUsingSsg {
    project: String,
    instance: String,
    remote_host: String,
}

impl std::fmt::Display for ShadowUsingSsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}@{}", self.project, self.instance, self.remote_host)
    }
}

/// Enforce the Phase 4.5 §20.6 block: refuse `coast ssg stop/rm` while
/// any remote shadow instance references the SSG, unless `force` is
/// set. With `force`, kill the tracked reverse-tunnel PIDs for each
/// blocking shadow before returning so the caller can proceed.
async fn enforce_shadow_gate_and_maybe_tear_down(
    state: &Arc<AppState>,
    force: bool,
    verb: &str,
) -> Result<()> {
    let shadows = collect_remote_shadows_using_ssg(state).await?;
    if shadows.is_empty() {
        return Ok(());
    }

    if !force {
        return Err(CoastError::state(format_shadow_gate_error(&shadows, verb)));
    }

    // --force: tear down recorded reverse-tunnel PIDs for each shadow.
    let mut map = state.shared_service_tunnel_pids.lock().await;
    for shadow in &shadows {
        if let Some(pids) = map.remove(&(shadow.project.clone(), shadow.instance.clone())) {
            for pid in pids {
                kill_ssh_tunnel_pid(pid);
            }
        }
    }
    Ok(())
}

/// Read shadow instances from the state DB and return those whose
/// artifact Coastfile has at least one `shared_service_group_refs`
/// entry (i.e. those actively consuming SSG services).
///
/// Reads artifact Coastfiles with best-effort IO: if an artifact dir
/// is missing or parse-fails, the shadow is skipped rather than
/// failing the entire gate. This matches `provision::load_coastfile_resources`'s
/// lenient reading behavior (missing artifact -> empty resources).
async fn collect_remote_shadows_using_ssg(state: &Arc<AppState>) -> Result<Vec<ShadowUsingSsg>> {
    let shadow_rows = {
        let db = state.db.lock().await;
        db.list_instances()?
            .into_iter()
            .filter(|inst| inst.remote_host.is_some())
            .map(|inst| {
                (
                    inst.project.clone(),
                    inst.name.clone(),
                    inst.build_id.clone(),
                    inst.remote_host.clone().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
    };

    let mut result = Vec::new();
    for (project, instance, build_id, remote_host) in shadow_rows {
        let artifact_dir = resolve_artifact_dir(&project, build_id.as_deref());
        let coastfile_path = artifact_dir.join("coastfile.toml");
        if !coastfile_path.exists() {
            continue;
        }
        if coastfile_has_ssg_refs(&coastfile_path) {
            result.push(ShadowUsingSsg {
                project,
                instance,
                remote_host,
            });
        }
    }
    Ok(result)
}

/// Minimal TOML scan: does this Coastfile declare at least one
/// `[shared_services.*]` entry with `from_group = true`?
///
/// We cannot use the full `Coastfile::from_file` parser because
/// artifact coastfiles are always written to `coastfile.toml`, which
/// the parser rejects when a `[remote]` section is present (the
/// `[remote]` section is gated on the filename `Coastfile.remote*`).
/// For the shadow-gate we don't need validation — we only need the
/// single boolean "does this consumer reference the SSG?". A tiny
/// custom deserializer avoids that filename coupling.
fn coastfile_has_ssg_refs(path: &std::path::Path) -> bool {
    #[derive(serde::Deserialize)]
    struct MinimalCf {
        #[serde(default)]
        shared_services: std::collections::HashMap<String, MinimalSvc>,
    }
    #[derive(serde::Deserialize)]
    struct MinimalSvc {
        #[serde(default)]
        from_group: bool,
    }

    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(cf) = toml::from_str::<MinimalCf>(&contents) else {
        return false;
    };
    cf.shared_services.values().any(|s| s.from_group)
}

/// Mirror of `provision::resolve_artifact_dir`, kept inline so this
/// handler doesn't need `pub(super)` access into the run submodule.
fn resolve_artifact_dir(project: &str, build_id: Option<&str>) -> std::path::PathBuf {
    let project_images_dir = crate::handlers::run::paths::project_images_dir(project);
    if let Some(bid) = build_id {
        let resolved = project_images_dir.join(bid);
        if resolved.exists() {
            return resolved;
        }
    }
    project_images_dir.join("latest")
}

/// Send SIGTERM to a reverse-tunnel ssh child PID. Best-effort: a
/// missing PID (already died) is not an error.
fn kill_ssh_tunnel_pid(pid: u32) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    let Ok(signed_pid) = i32::try_from(pid) else {
        tracing::warn!(pid = %pid, "reverse-tunnel PID does not fit in i32; skipping kill");
        return;
    };
    match kill(Pid::from_raw(signed_pid), Signal::SIGTERM) {
        Ok(()) => {
            tracing::info!(pid = %pid, "killed reverse-tunnel ssh child on --force");
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Already gone.
        }
        Err(err) => {
            tracing::warn!(pid = %pid, error = %err, "failed to SIGTERM reverse-tunnel PID");
        }
    }
}

// --- Phase 9 pure response helpers -------------------------------------
//
// Each `handle_*` above is split into (a) state-read + Docker side-
// effects (remain async and Docker-dependent) and (b) pure response
// shaping (these functions). The pure halves are fully unit-tested.

/// Response for `coast ssg stop` when no SSG record exists.
fn build_stop_response_missing_record() -> SsgResponse {
    SsgResponse {
        message: "SSG has not been created. Nothing to stop.".to_string(),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    }
}

/// Response for `coast ssg stop` after a successful stop.
fn build_stop_response_success() -> SsgResponse {
    SsgResponse {
        message: "SSG stopped.".to_string(),
        status: Some("stopped".to_string()),
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    }
}

/// Response for `coast ssg rm` when no SSG record exists.
fn build_rm_response_missing_record() -> SsgResponse {
    SsgResponse {
        message: "SSG has not been created. Nothing to remove.".to_string(),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    }
}

/// Response for `coast ssg rm [--with-data]` after a successful remove.
fn build_rm_response_success(with_data: bool) -> SsgResponse {
    let suffix = if with_data { " (with data)" } else { "" };
    SsgResponse {
        message: format!("SSG removed{suffix}."),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
    }
}

/// Build the error message for the shadow-gate refusal. Pure function
/// of the blocking shadows + the verb being gated. Extracted so
/// tests can assert wording without synthesizing the full
/// `CoastError` + `AppState`.
fn format_shadow_gate_error(shadows: &[ShadowUsingSsg], verb: &str) -> String {
    let list = shadows
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SSG is currently serving remote coast(s) [{list}]. \
         Running `coast ssg {verb}` now will break their shared-service \
         connectivity. Stop those remotes first, or re-run with \
         --force to tear down their reverse tunnels and proceed.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_using_ssg_display_is_project_instance_at_remote() {
        let s = ShadowUsingSsg {
            project: "my-app".to_string(),
            instance: "dev-1".to_string(),
            remote_host: "host-a".to_string(),
        };
        assert_eq!(s.to_string(), "my-app/dev-1@host-a");
    }

    fn write_temp_coastfile(content: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn coastfile_has_ssg_refs_detects_from_group_true() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
from_group = true
"#,
        );
        assert!(coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_tolerates_remote_section() {
        // Artifact coastfiles for remote instances include [remote],
        // but are saved as `coastfile.toml`. The full parser rejects
        // that combination; our minimal scanner must accept it.
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
from_group = true

[remote]
"#,
        );
        assert!(coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_for_inline_only() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"

[shared_services.postgres]
image = "postgres:16-alpine"
"#,
        );
        assert!(!coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_when_no_shared_services() {
        let f = write_temp_coastfile(
            r#"
[coast]
name = "consumer"
"#,
        );
        assert!(!coastfile_has_ssg_refs(f.path()));
    }

    #[test]
    fn coastfile_has_ssg_refs_false_on_missing_file() {
        let nonexistent = std::path::PathBuf::from("/tmp/coast-nonexistent-xyz-404.toml");
        assert!(!coastfile_has_ssg_refs(&nonexistent));
    }

    // --- Phase 9 pure response helpers ---

    #[test]
    fn stop_response_missing_record_has_nothing_to_stop_message() {
        let r = build_stop_response_missing_record();
        assert_eq!(r.message, "SSG has not been created. Nothing to stop.");
        assert!(r.status.is_none());
        assert!(r.services.is_empty());
        assert!(r.ports.is_empty());
        assert!(r.findings.is_empty());
    }

    #[test]
    fn stop_response_success_reports_stopped_status() {
        let r = build_stop_response_success();
        assert_eq!(r.message, "SSG stopped.");
        assert_eq!(r.status.as_deref(), Some("stopped"));
    }

    #[test]
    fn rm_response_missing_record_has_nothing_to_remove_message() {
        let r = build_rm_response_missing_record();
        assert_eq!(r.message, "SSG has not been created. Nothing to remove.");
        assert!(r.status.is_none());
    }

    #[test]
    fn rm_response_success_without_data_has_no_suffix() {
        let r = build_rm_response_success(false);
        assert_eq!(r.message, "SSG removed.");
        assert!(r.status.is_none());
    }

    #[test]
    fn rm_response_success_with_data_appends_with_data_suffix() {
        let r = build_rm_response_success(true);
        assert_eq!(r.message, "SSG removed (with data).");
    }

    #[test]
    fn shadow_gate_error_lists_all_blocking_shadows() {
        let shadows = vec![
            ShadowUsingSsg {
                project: "app-a".to_string(),
                instance: "dev-1".to_string(),
                remote_host: "host-x".to_string(),
            },
            ShadowUsingSsg {
                project: "app-b".to_string(),
                instance: "dev-2".to_string(),
                remote_host: "host-y".to_string(),
            },
        ];
        let msg = format_shadow_gate_error(&shadows, "stop");
        assert!(
            msg.contains("app-a/dev-1@host-x"),
            "first shadow missing; got: {msg}"
        );
        assert!(
            msg.contains("app-b/dev-2@host-y"),
            "second shadow missing; got: {msg}"
        );
        assert!(msg.contains("--force"), "must mention --force; got: {msg}");
    }

    #[test]
    fn shadow_gate_error_names_the_verb_via_coast_ssg_cmd() {
        let shadows = vec![ShadowUsingSsg {
            project: "p".to_string(),
            instance: "i".to_string(),
            remote_host: "h".to_string(),
        }];
        let stop_msg = format_shadow_gate_error(&shadows, "stop");
        assert!(stop_msg.contains("`coast ssg stop`"), "got: {stop_msg}");
        let rm_msg = format_shadow_gate_error(&shadows, "remove");
        assert!(rm_msg.contains("`coast ssg remove`"), "got: {rm_msg}");
    }

    #[test]
    fn shadow_gate_error_with_single_shadow_has_no_comma() {
        let shadows = vec![ShadowUsingSsg {
            project: "only".to_string(),
            instance: "one".to_string(),
            remote_host: "h".to_string(),
        }];
        let msg = format_shadow_gate_error(&shadows, "stop");
        // The joined list should not contain ", " since only one shadow.
        let after_bracket = msg.split('[').nth(1).unwrap();
        let before_bracket = after_bracket.split(']').next().unwrap();
        assert_eq!(before_bracket, "only/one@h");
    }
}
