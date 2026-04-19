//! `Coastfile.shared_service_groups` parser.
//!
//! Phase: ssg-phase-1. See `DESIGN.md §5`.
//!
//! Public API will expose `SsgCoastfile` with the same discovery rules
//! as regular `coast build` (cwd lookup, `-f <path>`, `--working-dir`,
//! inline `--config`).
//!
//! Layout:
//! - [`raw_types`] — TOML deserialization structs (pre-validation shape).
//! - this module — `SsgCoastfile` + validation + public helpers.
//!
//! The accepted schema is narrow: only `[ssg]` and `[shared_services.*]`
//! sections. Consumer Coastfile extensions for `[shared_services.<name>]
//! from_group = true` live in [`coast_core::coastfile`], not here.

// TODO(ssg-phase-1): pub(crate) raw_types; + pub use crate::coastfile::SsgCoastfile;
mod raw_types;
