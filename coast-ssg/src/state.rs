//! SSG state: SQLite schema and CRUD.
//!
//! Phase: ssg-phase-2 (migrations) / ssg-phase-3 (row writes). See `DESIGN.md §8`.
//!
//! Three new tables, added via migration in
//! `coast-daemon/src/state/mod.rs`:
//!
//! ```sql
//! CREATE TABLE ssg (
//!     id              INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton
//!     container_id    TEXT,
//!     status          TEXT NOT NULL,
//!     build_id        TEXT,
//!     created_at      TEXT NOT NULL
//! );
//!
//! CREATE TABLE ssg_services (
//!     service_name        TEXT PRIMARY KEY,
//!     container_port      INTEGER NOT NULL,
//!     dynamic_host_port   INTEGER NOT NULL,
//!     status              TEXT NOT NULL
//! );
//!
//! CREATE TABLE ssg_port_checkouts (
//!     canonical_port  INTEGER PRIMARY KEY,
//!     service_name    TEXT NOT NULL,
//!     socat_pid       INTEGER,
//!     created_at      TEXT NOT NULL
//! );
//! ```
//!
//! This module exposes an `SsgStateExt` trait that `coast-daemon`'s
//! `StateDb` implements. Keeps SSG DB logic in one place without
//! bloating `coast-daemon/src/state/`.

// TODO(ssg-phase-2 / 3): SsgStateExt trait, SsgRecord / SsgServiceRecord /
// SsgPortCheckoutRecord types, migration SQL constants.
