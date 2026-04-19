//! Bind mount plumbing for SSG-owned services.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §10`.
//!
//! SSG services that declare host bind mounts use the **same path string**
//! on both mount hops:
//!
//! ```text
//! host:/var/coast-data/postgres
//!   -> outer DinD bind (--mount type=bind,src=$path,dst=$path)
//!   -> inner compose bind ($path:/var/lib/postgresql/data)
//! ```
//!
//! This module validates declarations at parse time, ensures host
//! directories exist before the SSG runs, and builds the `bollard`
//! `Mount` specs for the outer DinD.

// TODO(ssg-phase-3): classify_volume_entry (bind | named), validate rules,
// ensure_host_dirs_exist, outer_bind_mounts, inner_compose_volume_strings.
