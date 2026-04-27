//! Phase 26 / 28 / §24.5: stable virtual-port allocator.
//!
//! Picks a host-owned port in the `[band_start, band_end]` range,
//! persists it in `ssg_virtual_ports` keyed by
//! `(project, service_name, container_port)`, and returns the same
//! port on subsequent calls for the same key. When the persisted port
//! is in use by something else (another program grabbed it between
//! daemon runs), falls forward to the next free port in the band and
//! repersists.
//!
//! Phase 28 amended the key from per-service to per-port so multi-port
//! services (e.g. minio's 9000+9001) get a stable virtual port per
//! published port — one host socat process per `ssg_services` row.
//!
//! This module is pure logic plus two capabilities:
//! - State access via the `SsgStateExt` trait (read/write the table).
//! - Port availability probing via `std::net::TcpListener::bind` —
//!   succeeds for a free port, fails with `EADDRINUSE` otherwise.
//!
//! The signature takes `&dyn SsgStateExt` (not `&AppState`) so tests
//! can drive an in-memory `StateDb` directly without constructing an
//! `AppState`. The rest of the daemon calls `state.db.lock().await`
//! at the call site (Phase 28 lifecycle wiring) and passes the
//! guarded `StateDb`.

use std::collections::HashSet;
use std::net::TcpListener;

use coast_core::error::{CoastError, Result};
use coast_ssg::state::SsgStateExt;

/// Default virtual-port band. Chosen to sit well above ephemeral
/// user ports yet below the dynamic/private range Docker and similar
/// tools draw from (49152+). 1000 ports is plenty for any realistic
/// number of services on a dev machine.
pub const DEFAULT_BAND_START: u16 = 42000;
pub const DEFAULT_BAND_END: u16 = 43000;

const ENV_BAND_START: &str = "COAST_VIRTUAL_PORT_BAND_START";
const ENV_BAND_END: &str = "COAST_VIRTUAL_PORT_BAND_END";

/// Allocator band configuration. Small wrapper so tests can pass
/// deliberately narrow ranges to exercise collision/exhaustion paths
/// without touching env vars.
#[derive(Debug, Clone, Copy)]
pub struct AllocatorConfig {
    pub band_start: u16,
    pub band_end: u16,
}

impl AllocatorConfig {
    /// Read the band from `COAST_VIRTUAL_PORT_BAND_{START,END}` env
    /// vars, falling back to [`DEFAULT_BAND_START`] /
    /// [`DEFAULT_BAND_END`]. Malformed env values silently fall back
    /// to defaults — the daemon continues to serve rather than
    /// refusing to start.
    #[must_use]
    pub fn from_env() -> Self {
        let band_start = std::env::var(ENV_BAND_START)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_BAND_START);
        let band_end = std::env::var(ENV_BAND_END)
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_BAND_END);
        Self {
            band_start,
            band_end,
        }
    }
}

impl Default for AllocatorConfig {
    fn default() -> Self {
        Self {
            band_start: DEFAULT_BAND_START,
            band_end: DEFAULT_BAND_END,
        }
    }
}

/// Probe whether `port` is currently free on all local IPv4
/// interfaces. Returns `true` when we can bind (and immediately
/// release) `0.0.0.0:port`. The drop of the `TcpListener` closes the
/// socket; the caller's subsequent spawn path (Phase 27) will
/// re-bind. TOCTOU between this probe and the actual bind is
/// unavoidable at this abstraction layer and is accepted — the real
/// socat spawn reprobes and raises a user-visible error if it loses
/// the race.
fn probe_port_free(port: u16) -> bool {
    TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Return the stable virtual port for
/// `(project, service_name, container_port)`.
///
/// 1. If a port is persisted AND still free, return it.
/// 2. Otherwise scan `[band_start, band_end]`. Skip ports already
///    assigned to any other `(project, service_name, container_port)`
///    triple and ports currently bound by some other process. First
///    match wins; persisted immediately; returned.
/// 3. If the band is exhausted, returns a clear error naming the
///    band bounds and the env var to widen them.
pub fn allocate_or_reuse(
    db: &dyn SsgStateExt,
    project: &str,
    service_name: &str,
    container_port: u16,
    config: &AllocatorConfig,
) -> Result<u16> {
    // Reuse path.
    if let Some(persisted) = db.get_ssg_virtual_port(project, service_name, container_port)? {
        // A port that's ALSO persisted for another
        // (project, service, container_port) should never happen
        // here (the allocator is the sole writer and enforces
        // uniqueness), but the probe still guards against a process
        // outside Coast grabbing it.
        if probe_port_free(persisted) {
            return Ok(persisted);
        }
        // Persisted but blocked — fall through to re-allocate.
    }

    // Fresh allocation path.
    let taken: HashSet<u16> = db
        .list_all_ssg_virtual_port_numbers()?
        .into_iter()
        .collect();

    for candidate in config.band_start..=config.band_end {
        if taken.contains(&candidate) {
            continue;
        }
        if !probe_port_free(candidate) {
            continue;
        }
        db.upsert_ssg_virtual_port(project, service_name, container_port, candidate)?;
        return Ok(candidate);
    }

    Err(CoastError::docker(format!(
        "virtual port band [{}-{}] exhausted; widen via {} / {}",
        config.band_start, config.band_end, ENV_BAND_START, ENV_BAND_END
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::StateDb;

    /// Reuse a narrow band of real, unassigned ports for most tests so
    /// we don't step on actual services — picked >= 42000 to match
    /// production defaults and reduce accidental overlap with other
    /// test suites.
    fn test_config() -> AllocatorConfig {
        AllocatorConfig {
            band_start: 42500,
            band_end: 42600,
        }
    }

    fn fresh_db() -> StateDb {
        StateDb::open_in_memory().expect("in-memory statedb")
    }

    /// Default container port used in tests where the value doesn't
    /// matter; postgres' canonical port is fine.
    const PG: u16 = 5432;

    #[test]
    fn stable_across_rebuild() {
        let db = fresh_db();
        let cfg = test_config();

        let first = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        let second = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();

        assert_eq!(first, second);
        assert!((cfg.band_start..=cfg.band_end).contains(&first));
    }

    #[test]
    fn distinct_within_project() {
        let db = fresh_db();
        let cfg = test_config();

        let pg = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        let redis = allocate_or_reuse(&db, "cg", "redis", 6379, &cfg).unwrap();

        assert_ne!(pg, redis);
    }

    #[test]
    fn distinct_across_projects() {
        let db = fresh_db();
        let cfg = test_config();

        let cg_pg = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        let fm_pg = allocate_or_reuse(&db, "filemap", "postgres", PG, &cfg).unwrap();

        // Different (project, service_name) pairs → distinct ports,
        // even though both are "postgres" on 5432.
        assert_ne!(cg_pg, fm_pg);
    }

    #[test]
    fn distinct_container_ports_within_one_service() {
        // Phase 28: a multi-port service like minio (9000 + 9001)
        // gets one virtual port per declared container port.
        let db = fresh_db();
        let cfg = test_config();

        let api = allocate_or_reuse(&db, "cg", "minio", 9000, &cfg).unwrap();
        let console = allocate_or_reuse(&db, "cg", "minio", 9001, &cfg).unwrap();

        assert_ne!(api, console);
        // Reuse remains stable per (service, container_port).
        let api_again = allocate_or_reuse(&db, "cg", "minio", 9000, &cfg).unwrap();
        assert_eq!(api, api_again);
    }

    #[test]
    fn collision_fallback_skips_in_use_port() {
        let db = fresh_db();
        // A very narrow band: only 2 candidates. Pre-bind the first
        // so the allocator must pick the second.
        let cfg = AllocatorConfig {
            band_start: 42700,
            band_end: 42701,
        };
        let _blocker = TcpListener::bind(("0.0.0.0", cfg.band_start)).expect("pre-bind first port");

        let got = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        assert_eq!(got, cfg.band_end);
    }

    #[test]
    fn band_exhaustion_error() {
        let db = fresh_db();
        let cfg = AllocatorConfig {
            band_start: 42800,
            band_end: 42801,
        };
        // Occupy both ports in the band. Hold them for the duration
        // of the allocation call.
        let _a = TcpListener::bind(("0.0.0.0", cfg.band_start)).unwrap();
        let _b = TcpListener::bind(("0.0.0.0", cfg.band_end)).unwrap();

        let err = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("exhausted"),
            "expected 'exhausted' in error: {msg}"
        );
        assert!(msg.contains("COAST_VIRTUAL_PORT_BAND_END"));
    }

    #[test]
    fn persisted_value_reused_via_state_db() {
        let db = fresh_db();
        let cfg = test_config();

        let first = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();

        // Simulate "daemon restart" by re-reading the persisted row
        // against a fresh allocator call. Same DB, same triple →
        // same port.
        let second = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        assert_eq!(first, second);

        // And confirm the DB layer returns it too.
        let via_db = db.get_ssg_virtual_port("cg", "postgres", PG).unwrap();
        assert_eq!(via_db, Some(first));
    }

    #[test]
    fn removed_service_recycles_into_band() {
        let db = fresh_db();
        let cfg = test_config();

        let first = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        db.clear_ssg_virtual_ports("cg").unwrap();

        let second = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        assert!((cfg.band_start..=cfg.band_end).contains(&second));
        // We don't assert first == second — the order of iteration
        // + probe races may land a different port in rare cases.
        // The contract is "port is in band", not "always the same".
        let _ = first;
    }

    #[test]
    fn project_scoped_clear_leaves_other_projects_alone() {
        let db = fresh_db();
        let cfg = test_config();

        let cg_pg = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        let fm_pg = allocate_or_reuse(&db, "filemap", "postgres", PG, &cfg).unwrap();

        db.clear_ssg_virtual_ports("cg").unwrap();

        // filemap's persisted port survives.
        assert_eq!(
            db.get_ssg_virtual_port("filemap", "postgres", PG).unwrap(),
            Some(fm_pg)
        );
        // cg's is gone.
        assert!(db
            .get_ssg_virtual_port("cg", "postgres", PG)
            .unwrap()
            .is_none());

        // Re-allocating cg must not accidentally steal filemap's port.
        let cg_pg_again = allocate_or_reuse(&db, "cg", "postgres", PG, &cfg).unwrap();
        assert_ne!(cg_pg_again, fm_pg);
        let _ = cg_pg;
    }

    #[test]
    fn allocator_config_from_env_parses_valid_values() {
        // Use a prefix unlikely to collide with another test.
        unsafe {
            std::env::set_var(ENV_BAND_START, "50000");
            std::env::set_var(ENV_BAND_END, "50010");
        }
        let cfg = AllocatorConfig::from_env();
        assert_eq!(cfg.band_start, 50000);
        assert_eq!(cfg.band_end, 50010);
        unsafe {
            std::env::remove_var(ENV_BAND_START);
            std::env::remove_var(ENV_BAND_END);
        }
    }
}
