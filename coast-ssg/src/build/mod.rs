//! SSG build pipeline.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1` and `§9.2`.
//!
//! `coast ssg build` parses `Coastfile.shared_service_groups`, pulls
//! every declared image (caching tarballs in the shared
//! `~/.coast/image-cache/`), synthesizes an inner compose file, and
//! writes a build artifact at `~/.coast/ssg/builds/{build_id}/` before
//! flipping `~/.coast/ssg/latest`.
//!
//! Layout:
//! - [`artifact`] — manifest + on-disk artifact structure.
//! - [`images`] — image resolution, pulls, tarball caching.

// TODO(ssg-phase-2): SsgBuildArtifact, SsgManifest, build_ssg() entrypoint.
mod artifact;

// TODO(ssg-phase-2): image resolution + pull + tarball cache helpers.
mod images;
