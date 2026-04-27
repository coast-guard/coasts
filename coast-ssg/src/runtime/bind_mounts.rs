//! Bind mount plumbing for SSG-owned services.
//!
//! Phase: ssg-phase-3. See `DESIGN.md §10`.
//!
//! SSG services that declare host bind mounts use the **same path string**
//! on both mount hops (`DESIGN.md §10.2`):
//!
//! ```text
//! host:/var/coast-data/postgres
//!   -> outer DinD bind  (--mount type=bind,src=$path,dst=$path)
//!   -> inner compose bind ($path:/var/lib/postgresql/data)
//! ```
//!
//! This module:
//!
//! - Collects the set of host source directories declared across every
//!   service in an [`SsgCoastfile`] and emits one [`coast_docker::runtime::BindMount`]
//!   per distinct host path for the outer DinD container.
//! - Ensures each host source directory exists on disk before the outer
//!   DinD is created (so docker doesn't silently auto-create a
//!   root-owned empty dir).
//!
//! Inner named volumes (`SsgVolumeEntry::InnerNamedVolume`) are **not**
//! surfaced here — they live entirely inside the SSG DinD's inner docker
//! daemon and are created/torn down by `docker compose up -d` /
//! `docker volume rm` via [`crate::runtime::lifecycle`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use coast_core::error::{CoastError, Result};
use coast_docker::runtime::BindMount;

use crate::coastfile::{SsgCoastfile, SsgVolumeEntry};

/// Compute the outer-DinD bind mounts needed for every declared host
/// bind volume in this SSG Coastfile.
///
/// Each distinct host path is emitted once, even if multiple services
/// reference it. Host paths are sorted for deterministic output so
/// build artifacts / test assertions remain stable.
///
/// We leave `propagation` as `None` so `coast-docker` emits the mount
/// via the legacy `-v host:container:rw` form. The bollard `Mount`
/// API with an explicit propagation mode misbehaves in some Docker
/// configurations (DinD-in-DinD in particular), where writes made by
/// the inner service's bind mount don't reach the outer host through
/// a chained `rprivate` mount.
pub fn outer_bind_mounts(cf: &SsgCoastfile) -> Vec<BindMount> {
    let mut dedup: BTreeMap<PathBuf, ()> = BTreeMap::new();

    for svc in &cf.services {
        for vol in &svc.volumes {
            if let SsgVolumeEntry::HostBindMount { host_path, .. } = vol {
                dedup.insert(host_path.clone(), ());
            }
        }
    }

    dedup
        .into_keys()
        .map(|path| BindMount {
            container_path: path.display().to_string(),
            host_path: path,
            read_only: false,
            propagation: None,
        })
        .collect()
}

/// Ensure every host bind source referenced in the Coastfile exists as a
/// directory on disk. Creates missing directories (parent-inclusive) so
/// docker does not auto-create them as root-owned empty dirs when the
/// outer DinD starts.
///
/// Leaves existing directories untouched. Leaves files alone — if a
/// declared host source already exists as a regular file the caller
/// will see docker fail to bind-mount it, which matches Coast's existing
/// bind-mount semantics (we deliberately do not overwrite).
pub fn ensure_host_bind_dirs_exist(cf: &SsgCoastfile) -> Result<()> {
    for host_path in collect_host_bind_sources(cf) {
        ensure_dir(&host_path)?;
    }
    Ok(())
}

fn collect_host_bind_sources(cf: &SsgCoastfile) -> Vec<PathBuf> {
    let mut sources: BTreeMap<PathBuf, ()> = BTreeMap::new();
    for svc in &cf.services {
        for vol in &svc.volumes {
            if let SsgVolumeEntry::HostBindMount { host_path, .. } = vol {
                sources.insert(host_path.clone(), ());
            }
        }
    }
    sources.into_keys().collect()
}

fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if path.exists() {
        return Err(CoastError::coastfile(format!(
            "SSG bind source '{}' exists but is not a directory",
            path.display()
        )));
    }
    std::fs::create_dir_all(path).map_err(|e| CoastError::Io {
        message: format!(
            "failed to create SSG bind source directory '{}'",
            path.display()
        ),
        path: path.to_path_buf(),
        source: Some(e),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coastfile::SsgCoastfile;

    fn parse(toml: &str) -> SsgCoastfile {
        SsgCoastfile::parse(toml, Path::new("/tmp")).expect("valid SSG Coastfile")
    }

    #[test]
    fn outer_bind_mounts_single_service_single_host_bind() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/pg:/var/lib/postgresql/data"]
"#,
        );
        let mounts = outer_bind_mounts(&cf);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].host_path, PathBuf::from("/var/coast-data/pg"));
        assert_eq!(mounts[0].container_path, "/var/coast-data/pg");
        assert!(!mounts[0].read_only);
        assert!(mounts[0].propagation.is_none());
    }

    #[test]
    fn outer_bind_mounts_ignores_inner_named_volumes() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = [
    "/var/coast-data/pg:/var/lib/postgresql/data",
    "pg_wal:/var/lib/postgresql/wal",
]
"#,
        );
        let mounts = outer_bind_mounts(&cf);
        assert_eq!(
            mounts.len(),
            1,
            "only the host bind surfaces in outer mounts"
        );
        assert_eq!(mounts[0].host_path, PathBuf::from("/var/coast-data/pg"));
    }

    #[test]
    fn outer_bind_mounts_dedup_across_services() {
        let cf = parse(
            r#"
[shared_services.pg_primary]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/pg-shared:/var/lib/postgresql/data"]

[shared_services.pg_replica]
image = "postgres:16"
ports = [5433]
volumes = ["/var/coast-data/pg-shared:/var/lib/postgresql/data"]
"#,
        );
        let mounts = outer_bind_mounts(&cf);
        assert_eq!(mounts.len(), 1, "shared host path dedups to one mount");
    }

    #[test]
    fn outer_bind_mounts_stable_order() {
        let cf = parse(
            r#"
[shared_services.redis]
image = "redis:7"
ports = [6379]
volumes = ["/var/coast-data/redis:/data"]

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["/var/coast-data/pg:/var/lib/postgresql/data"]
"#,
        );
        let mounts = outer_bind_mounts(&cf);
        let hosts: Vec<_> = mounts.iter().map(|m| m.host_path.clone()).collect();
        let mut expected = vec![
            PathBuf::from("/var/coast-data/pg"),
            PathBuf::from("/var/coast-data/redis"),
        ];
        expected.sort();
        assert_eq!(hosts, expected);
    }

    #[test]
    fn outer_bind_mounts_empty_when_only_named_volumes() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_wal:/var/lib/postgresql/wal"]
"#,
        );
        assert!(outer_bind_mounts(&cf).is_empty());
    }

    #[test]
    fn outer_bind_mounts_propagation_is_none_by_default() {
        let cf = parse(
            r#"
[shared_services.pg]
image = "postgres:16"
ports = [5432]
volumes = ["/tmp/ssg-propagation-test:/data"]
"#,
        );
        let mounts = outer_bind_mounts(&cf);
        assert!(mounts[0].propagation.is_none());
    }

    #[test]
    fn ensure_host_bind_dirs_creates_missing_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("pg-data");
        let coastfile = format!(
            r#"
[shared_services.pg]
image = "postgres:16"
ports = [5432]
volumes = ["{}:/var/lib/postgresql/data"]
"#,
            host.display()
        );
        let cf = parse(&coastfile);
        assert!(!host.exists());
        ensure_host_bind_dirs_exist(&cf).expect("ensure");
        assert!(host.is_dir());
    }

    #[test]
    fn ensure_host_bind_dirs_leaves_existing_dirs_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("pg-data");
        std::fs::create_dir_all(&host).unwrap();
        let marker = host.join("marker");
        std::fs::write(&marker, b"keep").unwrap();

        let coastfile = format!(
            r#"
[shared_services.pg]
image = "postgres:16"
ports = [5432]
volumes = ["{}:/var/lib/postgresql/data"]
"#,
            host.display()
        );
        let cf = parse(&coastfile);
        ensure_host_bind_dirs_exist(&cf).expect("ensure");
        assert_eq!(std::fs::read(&marker).unwrap(), b"keep");
    }

    #[test]
    fn ensure_host_bind_dirs_errors_if_path_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"hello").unwrap();
        let coastfile = format!(
            r#"
[shared_services.pg]
image = "postgres:16"
ports = [5432]
volumes = ["{}:/var/lib/postgresql/data"]
"#,
            file.display()
        );
        let cf = parse(&coastfile);
        let err = ensure_host_bind_dirs_exist(&cf).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("not a directory"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn ensure_host_bind_dirs_noop_when_only_named_volumes() {
        let cf = parse(
            r#"
[shared_services.pg]
image = "postgres:16"
ports = [5432]
volumes = ["pg_wal:/var/lib/postgresql/wal"]
"#,
        );
        ensure_host_bind_dirs_exist(&cf).expect("ensure");
    }
}
