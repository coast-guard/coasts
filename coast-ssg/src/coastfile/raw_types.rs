//! Raw TOML deserialization types for the SSG Coastfile.
//!
//! Phase: ssg-phase-1. See `DESIGN.md §5`.
//!
//! Mirrors the pattern used by [`coast_core::coastfile::raw_types`] —
//! `serde` structs that map 1:1 to the on-disk TOML before validation
//! converts them into the public `SsgCoastfile` type.

// TODO(ssg-phase-1): RawSsgCoastfile, RawSsgSection, RawSsgSharedServiceConfig,
// RawSsgSharedServicePort (reusing the single-or-mapping pattern from
// coast_core::coastfile::raw_types::RawSharedServicePort).
