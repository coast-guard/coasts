//! Shared test-only helpers. Compiled only under `#[cfg(test)]`.
//!
//! The daemon has several test modules that mutate process-global
//! env vars (`COAST_HOME` in particular). Cargo runs tests across
//! threads, so mutations from one module race with reads from
//! another unless every mutator serialises through THE SAME mutex.
//!
//! Historically each module carried its own file-local `ENV_LOCK`,
//! which serialised within the file but NOT across files. This
//! module centralises the lock so every site that touches
//! `COAST_HOME` acquires the same guard.

#![cfg(test)]

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Process-wide mutex serialising every test that mutates
/// `COAST_HOME`. Call sites in this crate MUST acquire this lock
/// before touching the env var, and hold it for the full duration
/// of their logic (not just the `set_var` call).
pub(crate) fn coast_home_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
