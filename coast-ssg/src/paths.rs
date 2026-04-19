//! Filesystem path helpers for the SSG.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! All SSG state on disk lives under `~/.coast/ssg/`:
//!
//! ```text
//! ~/.coast/ssg/
//!   builds/
//!     {build_id}/
//!       manifest.json
//!       ssg-coastfile.toml
//!       compose.yml
//!   latest -> builds/{build_id}
//! ```
//!
//! Image tarballs are cached in the shared pool at
//! `~/.coast/image-cache/` (unchanged from the existing coast build
//! pipeline).

// TODO(ssg-phase-2): ssg_home(), ssg_builds_dir(), ssg_latest_link(),
// resolve_latest_build_id().
