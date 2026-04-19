//! SSG build artifact: manifest + on-disk layout.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! Mirrors the shape of the per-project coast build artifact
//! (`~/.coast/images/{project}/{build_id}/`): each SSG build gets a
//! `build_id` directory containing `manifest.json`, the interpolated
//! `Coastfile.shared_service_groups`, the synthesized inner `compose.yml`,
//! and references to cached image tarballs.

// TODO(ssg-phase-2): SsgBuildArtifact, SsgManifest, write_artifact(), resolve_latest().
