//! Per-instance DB creation for SSG postgres/mysql services.
//!
//! Phase: ssg-phase-5. See `DESIGN.md §13`.
//!
//! When a consumer coast references an SSG service with
//! `auto_create_db = true`, the daemon runs a nested exec:
//!
//! ```text
//! docker exec <ssg-outer> \
//!   docker exec <inner-postgres> \
//!   psql -U postgres -c "... \\gexec"
//! ```
//!
//! The SQL command construction is shared with the inline shared
//! services path (`coast-daemon/src/shared_services.rs::create_db_command`).
//! This module owns only the nested-exec wrapper.

// TODO(ssg-phase-5): exec_in_ssg_service(service_name, command) -> ExecResult,
// create_instance_db_for_consumer(instance, service_name).
