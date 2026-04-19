//! Image resolution, pulls, and tarball caching for SSG services.
//!
//! Phase: ssg-phase-2. See `DESIGN.md §9.1`.
//!
//! Reuses the shared `~/.coast/image-cache/` tarball pool so that an
//! image used by both an SSG service and a regular coast compose
//! service is cached once.

// TODO(ssg-phase-2): pull_and_cache_image(), load_into_inner_dind() helpers.
