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
    /// Phase 33: `[secrets.<name>]` blocks. Each entry mirrors the
    /// regular Coastfile's `[secrets.<name>]` shape — extractor +
    /// inject + extractor-specific params (flattened). The SSG
    /// build pipeline runs the same `coast_secrets::extractor`
    /// registry against these, encrypts results into the
    /// keystore under `coast_image = "ssg:<project>"`, and the
    /// run pipeline decrypts + injects via per-run
    /// `compose.override.yml`. See `DESIGN.md §33`.
    #[serde(default)]
    pub secrets: HashMap<String, RawSsgSecretConfig>,
    /// Phase 17: `[unset]` block (applied after inheritance merge).
    /// Scoped to `shared_services` and `secrets` — the only named
    /// collections in the SSG schema. See `DESIGN.md §17 SETTLED
    /// #42` and `§33`.
    #[serde(default)]
    pub unset: Option<RawSsgUnsetConfig>,
}

/// `[ssg]` section.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSsgSection {
    #[serde(default)]
    pub runtime: Option<String>,
    /// Phase 17: path to a parent SSG Coastfile to inherit from.
    /// Resolved relative to the containing file's parent dir;
    /// `.toml` tie-break applies.
    #[serde(default)]
    pub extends: Option<String>,
    /// Phase 17: list of fragment files to merge into this Coastfile.
    /// Resolved relative to the containing file's parent dir.
    /// Fragments themselves cannot use `extends` / `includes`.
    #[serde(default)]
    pub includes: Option<Vec<String>>,
    /// Phase 23: optional explicit project name. When set, must
    /// match the sibling `Coastfile`'s `[coast] name` (cross-check
    /// happens at the daemon layer where both Coastfiles are in
    /// scope — the parser only enforces that when the field is
    /// present it is a non-empty string). See `coast-ssg/DESIGN.md
    /// §23`.
    #[serde(default)]
    pub project: Option<String>,
}

/// `[unset]` block — list named `shared_services` and/or `secrets`
/// entries to drop after merging parents/includes. Only applied
/// when the current Coastfile uses `extends` or `includes`;
/// standalone files never reach the unset pass. Mirrors
/// [`coast_core::coastfile::raw_types::RawUnsetConfig`] (narrow
/// version for the SSG schema).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawSsgUnsetConfig {
    #[serde(default)]
    pub shared_services: Vec<String>,
    /// Phase 33: drop `[secrets.<name>]` entries inherited from a
    /// parent / fragment.
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// A single `[secrets.<name>]` entry in the SSG Coastfile.
///
/// Identical shape to [`coast_core::coastfile::raw_types::RawSecretConfig`]
/// (same `extractor` / `inject` / `ttl` + flattened extractor
/// params). Defined here in the SSG crate rather than re-exporting
/// from `coast-core` because the regular Coastfile module keeps
/// that struct `pub(super)` on purpose. `coast-secrets` consumes
/// the validated [`coast_core::types::SecretConfig`], so the two
/// raw shapes never need to share a type.
///
/// Note: no `#[serde(deny_unknown_fields)]` here — `#[serde(flatten)]`
/// on `params` is incompatible with `deny_unknown_fields`. The
/// regular Coastfile makes the same trade-off.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawSsgSecretConfig {
    pub extractor: String,
    pub inject: String,
    #[serde(default)]
    pub ttl: Option<String>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
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
