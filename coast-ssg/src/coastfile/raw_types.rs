//! Raw TOML deserialization types for the SSG Coastfile.
//!
//! Phase: ssg-phase-1. See `DESIGN.md §5`.
//!
//! Mirrors the pattern used by [`coast_core::coastfile`]: serde structs
//! that map 1:1 to the on-disk TOML before validation converts them
//! into the public `SsgCoastfile` type.
//!
//! All structs use `#[serde(deny_unknown_fields)]` so the accepted
//! schema is strictly enforced at the deserialization layer. Unknown
//! top-level sections (`[coast]`, `[ports]`, ...) or unknown fields on
//! SSG services (`inject`, `from_group`, ...) fail the parse with a
//! serde error identifying the rejected key.

use std::collections::HashMap;

use serde::Deserialize;

/// Top-level SSG Coastfile structure.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSsgCoastfile {
    #[serde(default)]
    pub ssg: RawSsgSection,
    #[serde(default)]
    pub shared_services: HashMap<String, RawSsgSharedServiceConfig>,
}

/// `[ssg]` section.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSsgSection {
    #[serde(default)]
    pub runtime: Option<String>,
}

/// A single `[shared_services.<name>]` entry.
///
/// `ports` is `Vec<u16>` (not an untagged string-or-int enum like the
/// regular Coastfile's `RawSharedServicePort`): this is what makes
/// strings such as `"5433:5432"` automatically fail deserialization
/// with a clear error. SSG services always publish on dynamic host
/// ports, so no `"HOST:CONTAINER"` mappings are meaningful here.
///
/// `env` is `HashMap<String, toml::Value>` so non-string scalars
/// (ints, floats, bools) can be coerced to strings during validation
/// per the settled Phase 1 decision.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSsgSharedServiceConfig {
    pub image: String,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, toml::Value>,
    #[serde(default)]
    pub auto_create_db: bool,
}
