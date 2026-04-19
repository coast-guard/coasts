//! Synthesize the inner `compose.yml` that the SSG DinD runs.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.2`.
//!
//! Takes a parsed `SsgCoastfile` and emits a `docker compose`-compatible
//! YAML document that:
//!
//! - Defines one service per `[shared_services.*]` entry using the
//!   declared image / env.
//! - Publishes each service's declared container port as `5432:5432`
//!   etc. inside the DinD. The outer DinD's `-p dyn:canonical`
//!   publication is set up separately by [`crate::runtime::lifecycle`].
//! - Rewrites volumes per the symmetric-path rules in [`crate::runtime::bind_mounts`]
//!   (host-path sources stay verbatim, named volumes get a top-level
//!   entry).

// TODO(ssg-phase-3): synth_inner_compose(ssg: &SsgCoastfile) -> String.
