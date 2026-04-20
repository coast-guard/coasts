//! Test-only helpers shared across `coast-ssg` test modules.
//!
//! This module exists to serialize tests that mutate the process-global
//! `COAST_HOME` env var. Before this module was introduced each test
//! module kept its own private mutex, which let tests from different
//! modules race (set COAST_HOME in module A, read it back in module B).
//!
//! Always use [`with_coast_home`] from any test that sets or depends on
//! `COAST_HOME`.

#![cfg(test)]

use std::path::Path;
use std::sync::Mutex;

/// Process-wide lock. All `with_coast_home` callers across the crate
/// share this single mutex so there is no window during which two
/// tests can see inconsistent `COAST_HOME` state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` with a fresh, temporary `COAST_HOME` directory. The previous
/// value (if any) is restored when `f` returns. Recovers from a poisoned
/// lock because we never rely on panic-unwind invariants here.
pub fn with_coast_home<F: FnOnce(&Path)>(f: F) {
    let guard = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tmp = tempfile::tempdir().unwrap();
    let prev = std::env::var_os("COAST_HOME");
    unsafe {
        std::env::set_var("COAST_HOME", tmp.path());
    }

    f(tmp.path());

    unsafe {
        match prev {
            Some(v) => std::env::set_var("COAST_HOME", v),
            None => std::env::remove_var("COAST_HOME"),
        }
    }
    drop(guard);
}
