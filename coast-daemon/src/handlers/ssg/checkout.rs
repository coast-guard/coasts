//! `coast ssg checkout` / `uncheckout` orchestrator.
//!
//! Phase: ssg-phase-6. See `coast-ssg/DESIGN.md §12`.
//!
//! Binds (or unbinds) a host-side socat listener on an SSG service's
//! canonical port. Displaces known coast holders with a clear
//! warning; errors out on unknown-process strangers. All mutating
//! ops acquire `AppState.ssg_mutex` to serialize with `ssg run /
//! stop / rm`.
//!
//! The pure planner lives in
//! [`coast_ssg::runtime::port_checkout`]; this file handles the
//! daemon-side side effects (socat spawn / kill, state DB writes).
//!
//! DESIGN non-goals:
//! - No auto-restore of a displaced coast on `uncheckout`.
//! - No `--force` flag; displacement is always visible.

use std::sync::Arc;

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::port::PortBindStatus;
use coast_core::protocol::{SsgPortInfo, SsgResponse};
use coast_ssg::runtime::port_checkout::{plan_checkouts, SsgCheckoutPlan, SsgCheckoutTarget};
use coast_ssg::state::{SsgPortCheckoutRecord, SsgStateExt};

use crate::port_manager;
use crate::server::AppState;

/// A known-to-coast holder of a canonical port that displacement
/// must kill before the SSG checkout can bind. Not exposed outside
/// this module.
#[derive(Debug, Clone)]
struct CoastHolder {
    source: HolderSource,
    socat_pid: u32,
    /// Human-readable tag for the displacement warning.
    label: String,
    /// What to clear from state after we kill the socat.
    clear_target: ClearTarget,
}

#[derive(Debug, Clone)]
enum HolderSource {
    /// A `port_allocations` row with a non-null `socat_pid`.
    CoastInstance,
    /// A previous SSG checkout for a different service on the same
    /// canonical port. Shouldn't happen in practice since the
    /// primary key is `canonical_port` — if we see this, something
    /// else (a rebuild) is re-using the port.
    SsgPrevious,
}

#[derive(Debug, Clone)]
enum ClearTarget {
    /// Set `port_allocations.socat_pid = NULL` for this (project,
    /// instance, logical_name).
    CoastAllocation {
        project: String,
        instance_name: String,
        logical_name: String,
    },
    /// Delete the `ssg_port_checkouts` row.
    SsgCheckout { canonical_port: u16 },
}

pub(super) async fn handle_checkout(
    project: &str,
    state: &Arc<AppState>,
    service: Option<String>,
    all: bool,
) -> Result<SsgResponse> {
    let _guard = state.ssg_mutex.lock().await;
    let target = normalize_target(service, all)?;

    let (services, existing_checkouts) = {
        let db = state.db.lock().await;
        let services = db.list_ssg_services(project)?;
        let checkouts = db.list_ssg_port_checkouts(project)?;
        (services, checkouts)
    };
    let plans = plan_checkouts(&services, &target)?;

    let mut warnings: Vec<String> = Vec::new();

    for plan in &plans {
        // Idempotence: if this exact canonical port is already
        // checked out for the SAME service and its socat is alive,
        // skip. A mismatch (different service) triggers displacement.
        if let Some(existing) = existing_checkouts
            .iter()
            .find(|c| c.canonical_port == plan.canonical_port)
        {
            if existing.service_name == plan.service_name && existing.socat_pid.is_some() {
                warnings.push(format!(
                    "Service '{}' already checked out on canonical port {}; \
                     leaving existing forwarder in place.",
                    plan.service_name, plan.canonical_port,
                ));
                continue;
            }
        }

        // 1. Find any coast-side holder (scoped to this project's
        // checkouts; cross-project host-port collisions surface as
        // "unknown host process" in step 2 since we don't touch
        // another project's rows).
        let holder = find_coast_holder(project, state, plan.canonical_port).await?;

        // 2. Reject non-coast strangers (only if we haven't already
        //    identified a known holder to displace).
        if holder.is_none() {
            match port_manager::inspect_port_binding(plan.canonical_port) {
                PortBindStatus::Available => { /* free, proceed */ }
                PortBindStatus::InUse => return Err(unknown_holder_error(plan.canonical_port)),
                PortBindStatus::PermissionDenied => {
                    return Err(permission_denied_error(plan.canonical_port));
                }
                PortBindStatus::UnexpectedError(err) => {
                    return Err(CoastError::port(format!(
                        "cannot inspect canonical port {}: {err}",
                        plan.canonical_port
                    )));
                }
            }
        }

        // 3. Displace if needed.
        if let Some(holder) = holder {
            displace_coast_holder(project, state, &holder).await?;
            warnings.push(format!(
                "Displaced {} from canonical port {}. Run `coast checkout <instance>` if you \
                 want to re-bind it later.",
                holder.label, plan.canonical_port,
            ));
        }

        // 4. Spawn the new socat + upsert the row.
        let pid = spawn_checkout_socat(plan)?;
        let db = state.db.lock().await;
        db.upsert_ssg_port_checkout(&SsgPortCheckoutRecord {
            project: project.to_string(),
            canonical_port: plan.canonical_port,
            service_name: plan.service_name.clone(),
            socat_pid: Some(pid as i32),
            created_at: chrono::Utc::now().to_rfc3339(),
        })?;
        drop(db);
        info!(
            service = %plan.service_name,
            canonical = plan.canonical_port,
            dynamic = plan.dynamic_host_port,
            pid = pid,
            "SSG checkout: canonical port bound via socat"
        );
    }

    Ok(build_checkout_response(&plans, &warnings, &target))
}

pub(super) async fn handle_uncheckout(
    project: &str,
    state: &Arc<AppState>,
    service: Option<String>,
    all: bool,
) -> Result<SsgResponse> {
    let _guard = state.ssg_mutex.lock().await;
    let target = normalize_target(service, all)?;

    let existing = {
        let db = state.db.lock().await;
        db.list_ssg_port_checkouts(project)?
    };
    if existing.is_empty() {
        return Ok(SsgResponse {
            message: "No SSG checkouts active; nothing to uncheck out.".to_string(),
            status: None,
            services: Vec::new(),
            ports: Vec::new(),
            findings: Vec::new(),
            listings: Vec::new(),
            builds: Vec::new(),
        });
    }

    let to_remove = select_uncheckout_rows(&existing, &target);

    if to_remove.is_empty() {
        return Ok(build_uncheckout_none_matching_response(&target));
    }

    for row in &to_remove {
        if let Some(pid) = row.socat_pid {
            if let Err(err) = port_manager::kill_socat(pid as u32) {
                warn!(
                    pid = pid,
                    canonical = row.canonical_port,
                    error = %err,
                    "uncheckout: failed to kill socat (already dead?)",
                );
            }
        }
        let db = state.db.lock().await;
        db.delete_ssg_port_checkout(project, row.canonical_port)?;
    }

    let names: Vec<String> = to_remove
        .iter()
        .map(|c| format!("{} ({})", c.service_name, c.canonical_port))
        .collect();
    Ok(SsgResponse {
        message: format!("SSG uncheckout complete: {}.", names.join(", ")),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    })
}

/// Pure helper: given the `ssg_port_checkouts` table contents and
/// an uncheckout target, return the subset that should be removed.
/// Extracted so tests can drive the matcher without needing a real
/// `StateDb`.
fn select_uncheckout_rows(
    existing: &[SsgPortCheckoutRecord],
    target: &SsgCheckoutTarget,
) -> Vec<SsgPortCheckoutRecord> {
    match target {
        SsgCheckoutTarget::All => existing.to_vec(),
        SsgCheckoutTarget::Service(name) => existing
            .iter()
            .filter(|c| c.service_name == *name)
            .cloned()
            .collect(),
    }
}

/// Pure helper: build the "no matching checkout" response for
/// `coast ssg uncheckout` when the target matches zero rows.
fn build_uncheckout_none_matching_response(target: &SsgCheckoutTarget) -> SsgResponse {
    SsgResponse {
        message: format!(
            "No active SSG checkout for service '{}'; nothing to uncheck out.",
            match target {
                SsgCheckoutTarget::Service(name) => name.as_str(),
                SsgCheckoutTarget::All => unreachable!(
                    "SsgCheckoutTarget::All cannot reach here — handle_uncheckout \
                     short-circuits on empty `existing` list before calling this helper."
                ),
            }
        ),
        status: None,
        services: Vec::new(),
        ports: Vec::new(),
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

/// Translate the `(service, all)` CLI pair into the planner's target
/// enum. Rejects mutually-exclusive usage.
fn normalize_target(service: Option<String>, all: bool) -> Result<SsgCheckoutTarget> {
    match (service, all) {
        (None, false) => Err(CoastError::coastfile(
            "coast ssg checkout requires either --service <name> or --all.",
        )),
        (Some(_), true) => Err(CoastError::coastfile(
            "coast ssg checkout: --service and --all are mutually exclusive.",
        )),
        (Some(name), false) if name.is_empty() => Err(CoastError::coastfile(
            "coast ssg checkout: --service name must not be empty.",
        )),
        (Some(name), false) => Ok(SsgCheckoutTarget::Service(name)),
        (None, true) => Ok(SsgCheckoutTarget::All),
    }
}

/// Consult `port_allocations` and `ssg_port_checkouts` for any
/// existing coast-side owner of `canonical_port`.
async fn find_coast_holder(
    project: &str,
    state: &Arc<AppState>,
    canonical_port: u16,
) -> Result<Option<CoastHolder>> {
    let (coast_alloc, ssg_previous) = {
        let db = state.db.lock().await;
        let alloc = db.find_port_allocation_holding_canonical(canonical_port)?;
        let previous = db
            .list_ssg_port_checkouts(project)?
            .into_iter()
            .find(|c| c.canonical_port == canonical_port && c.socat_pid.is_some());
        (alloc, previous)
    };

    // Prefer the coast-instance holder (more informative label); it's
    // also the one the existing `coast checkout` flow bookkeeping
    // tracks. If only an SSG previous row exists, that's a rebuild
    // artefact we should clean up.
    if let Some(rec) = coast_alloc {
        if let Some(pid) = rec.socat_pid {
            let pid = u32::try_from(pid).map_err(|_| {
                CoastError::port(format!(
                    "invalid negative socat_pid for coast '{}/{}': {}",
                    rec.project, rec.instance_name, pid
                ))
            })?;
            return Ok(Some(CoastHolder {
                source: HolderSource::CoastInstance,
                socat_pid: pid,
                label: format!(
                    "coast instance '{}/{}' ({})",
                    rec.project, rec.instance_name, rec.logical_name
                ),
                clear_target: ClearTarget::CoastAllocation {
                    project: rec.project.clone(),
                    instance_name: rec.instance_name.clone(),
                    logical_name: rec.logical_name.clone(),
                },
            }));
        }
    }

    if let Some(row) = ssg_previous {
        if let Some(pid) = row.socat_pid {
            let pid = u32::try_from(pid).map_err(|_| {
                CoastError::port(format!(
                    "invalid negative socat_pid for ssg_port_checkouts row: {pid}"
                ))
            })?;
            return Ok(Some(CoastHolder {
                source: HolderSource::SsgPrevious,
                socat_pid: pid,
                label: format!("previous SSG checkout for '{}'", row.service_name),
                clear_target: ClearTarget::SsgCheckout {
                    canonical_port: row.canonical_port,
                },
            }));
        }
    }

    Ok(None)
}

async fn displace_coast_holder(
    ssg_project: &str,
    state: &Arc<AppState>,
    holder: &CoastHolder,
) -> Result<()> {
    if let Err(err) = port_manager::kill_socat(holder.socat_pid) {
        // Warn but proceed; the socat may already have died.
        warn!(
            pid = holder.socat_pid,
            label = %holder.label,
            error = %err,
            "displacement: failed to kill socat (already dead?)",
        );
    }

    match &holder.clear_target {
        ClearTarget::CoastAllocation {
            project,
            instance_name,
            logical_name,
        } => {
            let db = state.db.lock().await;
            db.update_socat_pid(project, instance_name, logical_name, None)?;
        }
        ClearTarget::SsgCheckout { canonical_port } => {
            let db = state.db.lock().await;
            db.delete_ssg_port_checkout(ssg_project, *canonical_port)?;
        }
    }
    info!(
        source = ?holder.source,
        label = %holder.label,
        "displacement: cleared coast holder"
    );
    Ok(())
}

/// Spawn the canonical-port socat forwarding to `localhost:<dynamic>`.
/// Uses the verified spawner so bind-in-time failures surface
/// immediately rather than as confusing connection errors later.
fn spawn_checkout_socat(plan: &SsgCheckoutPlan) -> Result<u32> {
    let cmd = port_manager::socat_command_canonical(
        plan.canonical_port,
        "localhost",
        plan.dynamic_host_port,
    );
    port_manager::spawn_socat_verified(&cmd, plan.canonical_port).map_err(|e| {
        CoastError::port(format!(
            "Failed to bind canonical port {} for SSG service '{}': {e}",
            plan.canonical_port, plan.service_name
        ))
    })
}

/// Build the final SsgResponse for a successful checkout. Messages
/// contain the applied plans plus any displacement warnings, joined
/// with newlines for readable CLI rendering.
fn build_checkout_response(
    plans: &[SsgCheckoutPlan],
    warnings: &[String],
    target: &SsgCheckoutTarget,
) -> SsgResponse {
    let applied: Vec<String> = plans
        .iter()
        .map(|p| format!("{} on canonical {}", p.service_name, p.canonical_port))
        .collect();
    let header = match target {
        SsgCheckoutTarget::All => format!("SSG checkout (--all): {}.", applied.join(", ")),
        SsgCheckoutTarget::Service(_) => {
            format!("SSG checkout complete: {}.", applied.join(", "))
        }
    };
    let message = if warnings.is_empty() {
        header
    } else {
        format!("{}\n{}", header, warnings.join("\n"))
    };
    // Phase 31: checkout responses don't include the virtual port
    // because the checkout flow operates on canonical / dynamic
    // ports specifically. Consumers can call `coast ssg ports`
    // separately to see the join with `ssg_virtual_ports`.
    let ports = plans
        .iter()
        .map(|p| SsgPortInfo {
            service: p.service_name.clone(),
            canonical_port: p.canonical_port,
            dynamic_host_port: p.dynamic_host_port,
            virtual_port: None,
            checked_out: true,
        })
        .collect();
    SsgResponse {
        message,
        status: None,
        services: Vec::new(),
        ports,
        findings: Vec::new(),
        listings: Vec::new(),
        builds: Vec::new(),
    }
}

fn unknown_holder_error(canonical_port: u16) -> CoastError {
    CoastError::port(format!(
        "Canonical port {canonical_port} is already in use by a process outside Coast's \
         tracking. Free the port (e.g. stop the conflicting server) and retry. Coast will \
         only displace coast instances — never an unknown host process."
    ))
}

fn permission_denied_error(canonical_port: u16) -> CoastError {
    CoastError::port(format!(
        "Binding canonical port {canonical_port} requires elevated privileges on this host. \
         On Linux, ports below 1024 are restricted unless you raise \
         `net.ipv4.ip_unprivileged_port_start` or grant CAP_NET_BIND_SERVICE to the \
         forwarding binary."
    ))
}

// --- Phase 6 shared helpers used by the lifecycle hooks in `mod.rs` ---
//
// `handle_stop` / `handle_rm` need to tear down the socat PIDs without
// reloading the whole dispatch path. Exposing these keeps the state
// mutations centralized on the trait impls instead of duplicating
// them in every lifecycle handler.

/// Kill every active checkout socat and null its `socat_pid` column.
/// Used from `handle_stop`: preserves the `ssg_port_checkouts` rows so
/// `ssg run` / `start` can re-spawn against the new dynamic ports.
pub(super) async fn kill_active_checkout_socats_preserve_rows(
    project: &str,
    state: &Arc<AppState>,
) {
    let rows = {
        let db = state.db.lock().await;
        match db.list_ssg_port_checkouts(project) {
            Ok(rows) => rows,
            Err(err) => {
                warn!(error = %err, "stop: failed to list ssg_port_checkouts; skipping socat teardown");
                return;
            }
        }
    };
    for row in rows {
        if let Some(pid) = row.socat_pid {
            if let Err(err) = port_manager::kill_socat(pid as u32) {
                warn!(
                    pid = pid,
                    canonical = row.canonical_port,
                    error = %err,
                    "stop: failed to kill checkout socat (already dead?)",
                );
            }
        }
        let db = state.db.lock().await;
        if let Err(err) = db.update_ssg_port_checkout_socat_pid(project, row.canonical_port, None) {
            warn!(
                canonical = row.canonical_port,
                error = %err,
                "stop: failed to null socat_pid in ssg_port_checkouts",
            );
        }
    }
}

/// Kill every checkout socat AND delete all `ssg_port_checkouts` rows.
/// Used from `handle_rm` (destructive remove).
pub(super) async fn kill_and_clear_all_checkouts(project: &str, state: &Arc<AppState>) {
    let rows = {
        let db = state.db.lock().await;
        match db.list_ssg_port_checkouts(project) {
            Ok(rows) => rows,
            Err(err) => {
                warn!(error = %err, "rm: failed to list ssg_port_checkouts; skipping socat teardown");
                return;
            }
        }
    };
    for row in &rows {
        if let Some(pid) = row.socat_pid {
            if let Err(err) = port_manager::kill_socat(pid as u32) {
                warn!(
                    pid = pid,
                    canonical = row.canonical_port,
                    error = %err,
                    "rm: failed to kill checkout socat (already dead?)",
                );
            }
        }
    }
    let db = state.db.lock().await;
    if let Err(err) = db.clear_ssg_port_checkouts(project) {
        warn!(error = %err, "rm: failed to clear ssg_port_checkouts rows");
    }
}

/// Re-spawn every checkout socat against the CURRENT
/// `ssg_services.dynamic_host_port`, dropping rows whose service has
/// disappeared from the active build. Used from `handle_run` /
/// `handle_start` / `handle_restart` / daemon-restart recovery.
pub(crate) async fn respawn_checkouts_after_lifecycle(
    project: &str,
    state: &Arc<AppState>,
) -> Vec<String> {
    let mut messages = Vec::new();
    let Some((rows, services)) = load_respawn_inputs(project, state).await else {
        return messages;
    };
    for row in rows {
        respawn_one_checkout(project, state, &row, &services, &mut messages).await;
    }
    messages
}

async fn load_respawn_inputs(
    project: &str,
    state: &Arc<AppState>,
) -> Option<(
    Vec<coast_ssg::state::SsgPortCheckoutRecord>,
    Vec<coast_ssg::state::SsgServiceRecord>,
)> {
    let db = state.db.lock().await;
    let rows = match db.list_ssg_port_checkouts(project) {
        Ok(rows) => rows,
        Err(err) => {
            warn!(error = %err, "respawn: failed to list ssg_port_checkouts; nothing to restore");
            return None;
        }
    };
    let services = match db.list_ssg_services(project) {
        Ok(services) => services,
        Err(err) => {
            warn!(error = %err, "respawn: failed to list ssg_services; nothing to restore");
            return None;
        }
    };
    Some((rows, services))
}

async fn respawn_one_checkout(
    project: &str,
    state: &Arc<AppState>,
    row: &coast_ssg::state::SsgPortCheckoutRecord,
    services: &[coast_ssg::state::SsgServiceRecord],
    messages: &mut Vec<String>,
) {
    let matching = services
        .iter()
        .find(|s| s.service_name == row.service_name && s.container_port == row.canonical_port);
    let Some(svc) = matching else {
        drop_stale_checkout_row(project, state, row, messages).await;
        return;
    };

    // Previous PID (if any) belongs to a socat whose upstream dynamic
    // port is now stale. Kill it before we spawn afresh.
    if let Some(pid) = row.socat_pid {
        let _ = port_manager::kill_socat(pid as u32);
    }

    let plan = SsgCheckoutPlan {
        service_name: svc.service_name.clone(),
        canonical_port: svc.container_port,
        dynamic_host_port: svc.dynamic_host_port,
    };
    match spawn_checkout_socat(&plan) {
        Ok(pid) => record_respawn_success(project, state, &plan, pid).await,
        Err(err) => record_respawn_failure(project, state, &plan, err, messages).await,
    }
}

async fn drop_stale_checkout_row(
    project: &str,
    state: &Arc<AppState>,
    row: &coast_ssg::state::SsgPortCheckoutRecord,
    messages: &mut Vec<String>,
) {
    let db = state.db.lock().await;
    let _ = db.delete_ssg_port_checkout(project, row.canonical_port);
    messages.push(format!(
        "Dropping stale checkout for '{}' on canonical {}: service no longer in the \
         active SSG build.",
        row.service_name, row.canonical_port,
    ));
}

async fn record_respawn_success(
    project: &str,
    state: &Arc<AppState>,
    plan: &SsgCheckoutPlan,
    pid: u32,
) {
    let db = state.db.lock().await;
    if let Err(err) =
        db.update_ssg_port_checkout_socat_pid(project, plan.canonical_port, Some(pid as i32))
    {
        warn!(
            canonical = plan.canonical_port,
            error = %err,
            "respawn: failed to update socat_pid",
        );
    }
    info!(
        service = %plan.service_name,
        canonical = plan.canonical_port,
        dynamic = plan.dynamic_host_port,
        pid = pid,
        "re-spawned SSG checkout socat against new dynamic port"
    );
}

async fn record_respawn_failure(
    project: &str,
    state: &Arc<AppState>,
    plan: &SsgCheckoutPlan,
    err: CoastError,
    messages: &mut Vec<String>,
) {
    messages.push(format!(
        "Failed to re-spawn checkout for '{}' on canonical {}: {err}",
        plan.service_name, plan.canonical_port,
    ));
    let db = state.db.lock().await;
    if let Err(err) = db.update_ssg_port_checkout_socat_pid(project, plan.canonical_port, None) {
        warn!(
            canonical = plan.canonical_port,
            error = %err,
            "respawn: failed to null socat_pid after spawn failure",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_target_service_ok() {
        assert_eq!(
            normalize_target(Some("postgres".into()), false).unwrap(),
            SsgCheckoutTarget::Service("postgres".into())
        );
    }

    #[test]
    fn normalize_target_all_ok() {
        assert_eq!(
            normalize_target(None, true).unwrap(),
            SsgCheckoutTarget::All
        );
    }

    #[test]
    fn normalize_target_rejects_neither() {
        let err = normalize_target(None, false).unwrap_err();
        assert!(err.to_string().contains("--service"));
        assert!(err.to_string().contains("--all"));
    }

    #[test]
    fn normalize_target_rejects_both() {
        let err = normalize_target(Some("pg".into()), true).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn normalize_target_rejects_empty_service_name() {
        let err = normalize_target(Some(String::new()), false).unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn unknown_holder_error_mentions_port_and_coast_only_policy() {
        let err = unknown_holder_error(5432);
        let msg = err.to_string();
        assert!(msg.contains("5432"));
        assert!(msg.contains("never an unknown host process"));
    }

    #[test]
    fn build_checkout_response_single_service_no_warnings() {
        let plan = SsgCheckoutPlan {
            service_name: "postgres".into(),
            canonical_port: 5432,
            dynamic_host_port: 60001,
        };
        let resp = build_checkout_response(
            &[plan.clone()],
            &[],
            &SsgCheckoutTarget::Service("postgres".into()),
        );
        assert!(resp.message.starts_with("SSG checkout complete"));
        assert!(resp.message.contains("postgres on canonical 5432"));
        assert_eq!(resp.ports.len(), 1);
        assert!(resp.ports[0].checked_out);
        assert_eq!(resp.ports[0].canonical_port, 5432);
    }

    #[test]
    fn build_checkout_response_all_label_differs() {
        let plan = SsgCheckoutPlan {
            service_name: "redis".into(),
            canonical_port: 6379,
            dynamic_host_port: 60002,
        };
        let resp = build_checkout_response(&[plan], &[], &SsgCheckoutTarget::All);
        assert!(resp.message.starts_with("SSG checkout (--all)"));
    }

    #[test]
    fn build_checkout_response_includes_warnings_on_new_line() {
        let plan = SsgCheckoutPlan {
            service_name: "postgres".into(),
            canonical_port: 5432,
            dynamic_host_port: 60001,
        };
        let warnings = vec!["Displaced coast 'proj/dev-a' (db) ...".to_string()];
        let resp = build_checkout_response(
            &[plan],
            &warnings,
            &SsgCheckoutTarget::Service("postgres".into()),
        );
        let lines: Vec<&str> = resp.message.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("checkout complete"));
        assert!(lines[1].starts_with("Displaced coast"));
    }

    // --- Phase 9 pure helpers for uncheckout ---

    const TEST_PROJECT: &str = "test-proj";

    fn checkout_row(canonical: u16, service: &str) -> SsgPortCheckoutRecord {
        SsgPortCheckoutRecord {
            project: TEST_PROJECT.to_string(),
            canonical_port: canonical,
            service_name: service.to_string(),
            socat_pid: None,
            created_at: "2026-04-20T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn select_uncheckout_rows_all_returns_everything() {
        let existing = vec![checkout_row(5432, "postgres"), checkout_row(6379, "redis")];
        let picked = select_uncheckout_rows(&existing, &SsgCheckoutTarget::All);
        assert_eq!(picked.len(), 2);
    }

    #[test]
    fn select_uncheckout_rows_service_filters_to_matching_name() {
        let existing = vec![
            checkout_row(5432, "postgres"),
            checkout_row(6379, "redis"),
            checkout_row(27017, "mongo"),
        ];
        let picked = select_uncheckout_rows(&existing, &SsgCheckoutTarget::Service("redis".into()));
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].service_name, "redis");
        assert_eq!(picked[0].canonical_port, 6379);
    }

    #[test]
    fn select_uncheckout_rows_service_no_match_returns_empty() {
        let existing = vec![checkout_row(5432, "postgres")];
        let picked = select_uncheckout_rows(&existing, &SsgCheckoutTarget::Service("nope".into()));
        assert!(picked.is_empty());
    }

    #[test]
    fn select_uncheckout_rows_on_empty_input_returns_empty() {
        let picked = select_uncheckout_rows(&[], &SsgCheckoutTarget::All);
        assert!(picked.is_empty());
        let picked = select_uncheckout_rows(&[], &SsgCheckoutTarget::Service("anything".into()));
        assert!(picked.is_empty());
    }

    #[test]
    fn uncheckout_none_matching_response_names_the_service() {
        let resp =
            build_uncheckout_none_matching_response(&SsgCheckoutTarget::Service("redis".into()));
        assert!(resp.message.contains("'redis'"));
        assert!(resp.message.contains("nothing to uncheck out"));
        assert!(resp.status.is_none());
        assert!(resp.ports.is_empty());
    }

    // --- Phase 9 Pattern C: in-memory AppState end-to-end uncheckout ---
    //
    // Exercises `handle_uncheckout` against a real in-memory StateDb
    // (via `AppState::new_for_testing`). socat_pid is None on every
    // row so we don't actually try to kill any processes.

    fn in_memory_app_state() -> Arc<AppState> {
        use crate::state::StateDb;
        let db = StateDb::open_in_memory().expect("in-memory statedb");
        Arc::new(AppState::new_for_testing(db))
    }

    #[tokio::test]
    async fn uncheckout_empty_table_says_nothing_to_uncheck_out() {
        let state = in_memory_app_state();
        let resp = handle_uncheckout(TEST_PROJECT, &state, None, true)
            .await
            .unwrap();
        assert!(resp.message.contains("nothing to uncheck out"));
    }

    #[tokio::test]
    async fn uncheckout_all_removes_every_row() {
        let state = in_memory_app_state();
        {
            let db = state.db.lock().await;
            db.upsert_ssg_port_checkout(&checkout_row(5432, "postgres"))
                .unwrap();
            db.upsert_ssg_port_checkout(&checkout_row(6379, "redis"))
                .unwrap();
        }

        let resp = handle_uncheckout(TEST_PROJECT, &state, None, true)
            .await
            .unwrap();
        assert!(resp.message.contains("SSG uncheckout complete"));
        assert!(resp.message.contains("postgres (5432)"));
        assert!(resp.message.contains("redis (6379)"));

        let db = state.db.lock().await;
        assert!(db.list_ssg_port_checkouts(TEST_PROJECT).unwrap().is_empty());
    }

    #[tokio::test]
    async fn uncheckout_one_service_keeps_others() {
        let state = in_memory_app_state();
        {
            let db = state.db.lock().await;
            db.upsert_ssg_port_checkout(&checkout_row(5432, "postgres"))
                .unwrap();
            db.upsert_ssg_port_checkout(&checkout_row(6379, "redis"))
                .unwrap();
        }

        let resp = handle_uncheckout(TEST_PROJECT, &state, Some("postgres".into()), false)
            .await
            .unwrap();
        assert!(resp.message.contains("postgres (5432)"));
        assert!(!resp.message.contains("redis"));

        let db = state.db.lock().await;
        let remaining = db.list_ssg_port_checkouts(TEST_PROJECT).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].service_name, "redis");
    }

    #[tokio::test]
    async fn uncheckout_missing_service_leaves_rows_alone() {
        let state = in_memory_app_state();
        {
            let db = state.db.lock().await;
            db.upsert_ssg_port_checkout(&checkout_row(5432, "postgres"))
                .unwrap();
        }

        let resp = handle_uncheckout(TEST_PROJECT, &state, Some("nope".into()), false)
            .await
            .unwrap();
        assert!(resp.message.contains("No active SSG checkout"));
        assert!(resp.message.contains("'nope'"));

        let db = state.db.lock().await;
        assert_eq!(db.list_ssg_port_checkouts(TEST_PROJECT).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn uncheckout_invalid_target_errors() {
        let state = in_memory_app_state();
        // Neither --service nor --all.
        let err = handle_uncheckout(TEST_PROJECT, &state, None, false)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("--service"));
        assert!(err.to_string().contains("--all"));
    }
}
