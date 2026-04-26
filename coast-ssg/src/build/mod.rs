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

// ssg-phase-2: SsgManifest, write_artifact, flip_latest, auto_prune.
pub mod artifact;

// ssg-phase-2: image resolution + pull + tarball cache helpers.
pub mod images;

// ssg-phase-33: build-time secret extractor pass. Mirrors the
// regular daemon's `handlers::build::secrets::extract_secrets` but
// keys the keystore under `ssg:<project>` so SSG and instance
// secrets never collide. See `DESIGN.md §33`.
pub mod secrets;

pub use artifact::{
    auto_prune, build_manifest, coastfile_hash_for, compute_build_id, flip_latest, write_artifact,
    SsgManifest, SsgManifestSecretInject, SsgManifestService,
};
pub use secrets::{extract_ssg_secrets, keystore_image_key, SsgSecretExtractionOutput};
