//! SSG lifecycle verbs: run, start, stop, restart, rm.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §9.3`, `§9.4`.
//!
//! The SSG is a singleton — any of these verbs operates on the one
//! SSG container (named `coast-ssg`). `run` allocates dynamic ports
//! and populates `ssg_services` in the daemon state database.
//! `rm` preserves data by default; `--with-data` also removes inner
//! named volumes.

// TODO(ssg-phase-3): SsgRuntime::run / start / stop / restart / rm.
