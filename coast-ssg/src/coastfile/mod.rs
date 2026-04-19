//! `Coastfile.shared_service_groups` parser.
//!
//! Phase: ssg-phase-1. See `DESIGN.md §5`.
//!
//! Parses the SSG Coastfile into [`SsgCoastfile`], validating the
//! narrow schema (§5) and classifying volume entries into host bind
//! mounts vs inner named volumes per §10.1.
//!
//! The accepted schema is intentionally narrow: only `[ssg]` and
//! `[shared_services.*]` sections. All other top-level keys are
//! rejected by serde `deny_unknown_fields` at deserialization time.
//! Consumer Coastfile extensions for `from_group = true` live in
//! [`coast_core::coastfile`], not here.
//!
//! Uses [`coast_core::coastfile::interpolation::interpolate_env_vars`]
//! so SSG Coastfiles support the same `${VAR}` / `${VAR:-default}` /
//! `$${literal}` syntax as regular Coastfiles.

mod raw_types;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use coast_core::coastfile::interpolation::interpolate_env_vars;
use coast_core::error::{CoastError, Result};
use coast_core::types::RuntimeType;

use self::raw_types::{RawSsgCoastfile, RawSsgSection, RawSsgSharedServiceConfig};

/// A fully parsed and validated SSG Coastfile.
///
/// See `coast-ssg/DESIGN.md §5` for the accepted schema.
#[derive(Debug, Clone)]
pub struct SsgCoastfile {
    /// `[ssg]` section.
    pub section: SsgSection,
    /// `[shared_services.*]` entries, deterministically sorted by name.
    pub services: Vec<SsgSharedServiceConfig>,
    /// Directory the Coastfile was parsed from. Currently kept for
    /// parity with [`coast_core::coastfile::Coastfile::project_root`];
    /// v1 does not resolve any relative paths from it, but later
    /// phases (e.g. `[ssg.setup.files]`) will.
    pub project_root: PathBuf,
    /// Warnings for undefined `${VAR}` references that had no default.
    pub interpolation_warnings: Vec<String>,
}

/// `[ssg]` section.
#[derive(Debug, Clone)]
pub struct SsgSection {
    /// Container runtime. Defaults to [`RuntimeType::Dind`] when unset.
    pub runtime: RuntimeType,
}

/// A single `[shared_services.<name>]` entry after validation.
#[derive(Debug, Clone)]
pub struct SsgSharedServiceConfig {
    /// Service name (the TOML key).
    pub name: String,
    /// Docker image, e.g. `"postgres:16"`.
    pub image: String,
    /// Container-side ports the service listens on. May be empty when
    /// the service is a sidecar with no published ports.
    pub ports: Vec<u16>,
    /// Classified volume entries.
    pub volumes: Vec<SsgVolumeEntry>,
    /// Environment variables. Non-string TOML scalars (ints, floats,
    /// bools) are coerced to strings during validation per the Phase 1
    /// decision recorded in `DESIGN.md §5`.
    pub env: HashMap<String, String>,
    /// When `true`, a per-instance database is auto-created inside
    /// this service for each consumer coast. See `DESIGN.md §13`.
    pub auto_create_db: bool,
}

/// A classified volume entry.
///
/// See `DESIGN.md §10.1`. Either a host bind mount (absolute host
/// path source) or an inner named Docker volume (Docker volume name
/// source, lives inside the SSG DinD's inner daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsgVolumeEntry {
    /// Absolute host path bind-mounted to an absolute container path.
    ///
    /// Under the symmetric-path plan (`DESIGN.md §10.2`) the host path
    /// is used verbatim on both mount hops (outer DinD bind + inner
    /// service bind), so only one path string is needed here.
    HostBindMount {
        host_path: PathBuf,
        container_path: PathBuf,
    },
    /// Docker named volume (lives inside the SSG DinD's inner daemon,
    /// opaque to the host).
    InnerNamedVolume {
        name: String,
        container_path: PathBuf,
    },
}

impl SsgVolumeEntry {
    /// Return the container-side mount path regardless of source kind.
    /// Used for duplicate-target detection.
    pub fn container_path(&self) -> &Path {
        match self {
            Self::HostBindMount { container_path, .. }
            | Self::InnerNamedVolume { container_path, .. } => container_path,
        }
    }
}

impl SsgCoastfile {
    /// Parse an SSG Coastfile from a TOML string.
    ///
    /// `project_root` is used to resolve relative paths (currently
    /// none are accepted, but kept for parity with
    /// [`coast_core::coastfile::Coastfile::parse`] and for future
    /// `[ssg.setup]` work).
    ///
    /// Runs the same env-var interpolation pipeline as regular
    /// Coastfiles before TOML parsing.
    pub fn parse(content: &str, project_root: &Path) -> Result<Self> {
        let interp = interpolate_env_vars(content);
        let raw: RawSsgCoastfile = toml::from_str(&interp.content)?;
        let mut cf = Self::validate_and_build(raw, project_root)?;
        cf.interpolation_warnings = interp.warnings;
        Ok(cf)
    }

    /// Parse an SSG Coastfile from disk.
    ///
    /// Resolves `project_root` to the file's parent directory.
    pub fn from_file(path: &Path) -> Result<Self> {
        let project_root = path
            .parent()
            .ok_or_else(|| CoastError::coastfile("SSG Coastfile path has no parent directory"))?;

        let content = std::fs::read_to_string(path).map_err(|e| CoastError::Io {
            message: format!("failed to read SSG Coastfile: {e}"),
            path: path.to_path_buf(),
            source: Some(e),
        })?;

        Self::parse(&content, project_root)
    }

    fn validate_and_build(raw: RawSsgCoastfile, project_root: &Path) -> Result<Self> {
        let section = Self::build_section(&raw.ssg)?;

        // Deterministic ordering: services are emitted sorted by name
        // regardless of TOML map iteration order.
        let mut service_entries: Vec<_> = raw.shared_services.into_iter().collect();
        service_entries.sort_by(|a, b| a.0.cmp(&b.0));

        let mut services = Vec::with_capacity(service_entries.len());
        for (name, raw_svc) in service_entries {
            services.push(Self::build_shared_service(&name, raw_svc)?);
        }

        Ok(Self {
            section,
            services,
            project_root: project_root.to_path_buf(),
            interpolation_warnings: Vec::new(),
        })
    }

    fn build_section(raw: &RawSsgSection) -> Result<SsgSection> {
        let runtime = match raw.runtime.as_deref() {
            Some(value) => RuntimeType::from_str_value(value).ok_or_else(|| {
                CoastError::coastfile(format!(
                    "ssg.runtime: invalid runtime '{value}'. Expected one of: dind, sysbox, podman"
                ))
            })?,
            None => RuntimeType::Dind,
        };
        Ok(SsgSection { runtime })
    }

    fn build_shared_service(
        name: &str,
        raw: RawSsgSharedServiceConfig,
    ) -> Result<SsgSharedServiceConfig> {
        if raw.image.is_empty() {
            return Err(CoastError::coastfile(format!(
                "shared_services.{name}: image cannot be empty"
            )));
        }

        for port in &raw.ports {
            if *port == 0 {
                return Err(CoastError::coastfile(format!(
                    "shared_services.{name}: port 0 is not valid"
                )));
            }
        }

        let mut volumes = Vec::with_capacity(raw.volumes.len());
        let mut seen_targets = HashSet::new();
        for raw_volume in &raw.volumes {
            let entry = parse_volume_entry(name, raw_volume)?;
            let target = entry.container_path().to_path_buf();
            if !seen_targets.insert(target.clone()) {
                return Err(CoastError::coastfile(format!(
                    "shared_services.{name}: duplicate container path '{}'",
                    target.display()
                )));
            }
            volumes.push(entry);
        }

        let env = coerce_env_map(name, raw.env)?;

        Ok(SsgSharedServiceConfig {
            name: name.to_string(),
            image: raw.image,
            ports: raw.ports,
            volumes,
            env,
            auto_create_db: raw.auto_create_db,
        })
    }
}

/// Parse and classify a single volume entry string.
///
/// Accepted forms (see `DESIGN.md §10.1`):
/// - `"/abs/host:/abs/container"` -> [`SsgVolumeEntry::HostBindMount`]
/// - `"name:/abs/container"` -> [`SsgVolumeEntry::InnerNamedVolume`]
///
/// Rejects: missing `:`, empty source or target, relative paths
/// (`./foo`), `..` components on either side, non-absolute container
/// target, source that is neither an absolute host path nor a valid
/// Docker volume name.
fn parse_volume_entry(service_name: &str, raw: &str) -> Result<SsgVolumeEntry> {
    let (source, target) = split_volume_entry(service_name, raw)?;

    if contains_parent_dir(source) {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}' must not contain '..'"
        )));
    }
    if contains_parent_dir(target) {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}' must not contain '..'"
        )));
    }

    if !target.starts_with('/') {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}': container path must be absolute"
        )));
    }

    if source.starts_with('/') {
        return Ok(SsgVolumeEntry::HostBindMount {
            host_path: PathBuf::from(source),
            container_path: PathBuf::from(target),
        });
    }

    // Not absolute. Reject anything path-like (starts with `.` or
    // contains `/`). Otherwise it must be a valid Docker volume name.
    if source.starts_with('.') || source.contains('/') {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}': source must be an absolute host \
             path or a Docker volume name (relative paths are not supported)"
        )));
    }

    if !is_valid_docker_volume_name(source) {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}': source must be an absolute host \
             path or a Docker volume name"
        )));
    }

    Ok(SsgVolumeEntry::InnerNamedVolume {
        name: source.to_string(),
        container_path: PathBuf::from(target),
    })
}

/// Split a `"src:dst"` volume entry at the first colon. Both sides
/// must be non-empty.
fn split_volume_entry<'a>(service_name: &str, raw: &'a str) -> Result<(&'a str, &'a str)> {
    let Some((source, target)) = raw.split_once(':') else {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}' is missing ':' separator"
        )));
    };
    if source.is_empty() || target.is_empty() {
        return Err(CoastError::coastfile(format!(
            "shared_services.{service_name}: volume '{raw}' has an empty source or target"
        )));
    }
    Ok((source, target))
}

fn contains_parent_dir(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Docker volume name grammar: `[a-zA-Z0-9][a-zA-Z0-9_.-]*`.
fn is_valid_docker_volume_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

/// Coerce a `HashMap<String, toml::Value>` of env entries into a
/// `HashMap<String, String>`.
///
/// Per the Phase 1 settled decision (`DESIGN.md §5`), scalar TOML
/// values (int, float, bool, string) are coerced to strings. Arrays,
/// tables, and datetimes are rejected with a clear error.
fn coerce_env_map(
    service_name: &str,
    raw: HashMap<String, toml::Value>,
) -> Result<HashMap<String, String>> {
    let mut out = HashMap::with_capacity(raw.len());
    for (key, value) in raw {
        let coerced = coerce_env_value(service_name, &key, value)?;
        out.insert(key, coerced);
    }
    Ok(out)
}

fn coerce_env_value(service_name: &str, key: &str, value: toml::Value) -> Result<String> {
    match value {
        toml::Value::String(s) => Ok(s),
        toml::Value::Integer(n) => Ok(n.to_string()),
        toml::Value::Float(f) => Ok(f.to_string()),
        toml::Value::Boolean(b) => Ok(b.to_string()),
        other => Err(CoastError::coastfile(format!(
            "shared_services.{service_name}.env.{key}: value must be a scalar (string, \
             integer, float, or boolean); got {}",
            other.type_str()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_text: &str) -> Result<SsgCoastfile> {
        SsgCoastfile::parse(toml_text, Path::new("/tmp/ssg-test"))
    }

    fn assert_parse_err_contains(toml_text: &str, needle: &str) {
        let err = parse(toml_text).expect_err("expected parse to fail");
        let msg = err.to_string();
        assert!(
            msg.contains(needle),
            "error message did not contain {needle:?}; got: {msg}"
        );
    }

    // -----------------------------------------------------------
    // Happy paths
    // -----------------------------------------------------------

    #[test]
    fn parses_minimal_single_service() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
"#,
        )
        .unwrap();

        assert_eq!(cf.section.runtime, RuntimeType::Dind);
        assert_eq!(cf.services.len(), 1);
        let svc = &cf.services[0];
        assert_eq!(svc.name, "postgres");
        assert_eq!(svc.image, "postgres:16");
        assert!(svc.ports.is_empty());
        assert!(svc.volumes.is_empty());
        assert!(svc.env.is_empty());
        assert!(!svc.auto_create_db);
        assert_eq!(cf.project_root, Path::new("/tmp/ssg-test"));
    }

    #[test]
    fn parses_multi_service() {
        let cf = parse(
            r#"
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
env = { MONGO_INITDB_ROOT_USERNAME = "coast" }
"#,
        )
        .unwrap();

        assert_eq!(cf.services.len(), 3);
        // Deterministic ordering: alphabetical by name.
        assert_eq!(cf.services[0].name, "mongodb");
        assert_eq!(cf.services[1].name, "postgres");
        assert_eq!(cf.services[2].name, "redis");
    }

    #[test]
    fn host_bind_volume_is_classified() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/pg:/var/lib/postgresql/data"]
"#,
        )
        .unwrap();

        let svc = &cf.services[0];
        assert_eq!(svc.volumes.len(), 1);
        match &svc.volumes[0] {
            SsgVolumeEntry::HostBindMount {
                host_path,
                container_path,
            } => {
                assert_eq!(host_path, Path::new("/var/coast-data/pg"));
                assert_eq!(container_path, Path::new("/var/lib/postgresql/data"));
            }
            other => panic!("expected HostBindMount, got {other:?}"),
        }
    }

    #[test]
    fn inner_named_volume_is_classified() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["pg_wal:/var/lib/postgresql/wal"]
"#,
        )
        .unwrap();

        match &cf.services[0].volumes[0] {
            SsgVolumeEntry::InnerNamedVolume {
                name,
                container_path,
            } => {
                assert_eq!(name, "pg_wal");
                assert_eq!(container_path, Path::new("/var/lib/postgresql/wal"));
            }
            other => panic!("expected InnerNamedVolume, got {other:?}"),
        }
    }

    #[test]
    fn env_scalar_values_are_coerced_to_strings() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
env = { A = "hello", B = 42, C = 3.5, D = true }
"#,
        )
        .unwrap();

        let env = &cf.services[0].env;
        assert_eq!(env.get("A").map(String::as_str), Some("hello"));
        assert_eq!(env.get("B").map(String::as_str), Some("42"));
        assert_eq!(env.get("C").map(String::as_str), Some("3.5"));
        assert_eq!(env.get("D").map(String::as_str), Some("true"));
    }

    #[test]
    fn empty_and_missing_ports_are_both_accepted() {
        let explicit = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = []
"#,
        )
        .unwrap();
        assert!(explicit.services[0].ports.is_empty());

        let implicit = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
"#,
        )
        .unwrap();
        assert!(implicit.services[0].ports.is_empty());
    }

    #[test]
    fn auto_create_db_round_trips() {
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
auto_create_db = true
"#,
        )
        .unwrap();
        assert!(cf.services[0].auto_create_db);
    }

    #[test]
    fn interpolation_resolves_env_vars_in_volumes() {
        // We avoid std::env mutation (racy in parallel tests) by
        // reading an env var we know exists in every environment:
        // PATH. Its existence is sufficient; we do not assert on its
        // value.
        let cf = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["${COAST_SSG_UNDEFINED_VAR:-/tmp/pg}:/var/lib/postgresql/data"]
"#,
        )
        .unwrap();

        match &cf.services[0].volumes[0] {
            SsgVolumeEntry::HostBindMount { host_path, .. } => {
                assert_eq!(host_path, Path::new("/tmp/pg"));
            }
            other => panic!("expected HostBindMount, got {other:?}"),
        }
        // No warning expected since `${VAR:-default}` form is used.
        assert!(cf.interpolation_warnings.is_empty());
    }

    #[test]
    fn interpolation_warns_for_undefined_var_without_default() {
        let cf = parse(
            r#"
[shared_services.redis]
image = "redis:${COAST_SSG_UNDEFINED_XYZ_123}"
"#,
        )
        .unwrap();

        // The undefined var interpolates to empty string; the image
        // becomes "redis:" which is a valid non-empty string so the
        // parse succeeds. The warning records the undefined var.
        assert_eq!(cf.services[0].image, "redis:");
        assert_eq!(cf.interpolation_warnings.len(), 1);
        assert!(cf.interpolation_warnings[0].contains("COAST_SSG_UNDEFINED_XYZ_123"));
    }

    #[test]
    fn services_are_sorted_by_name_regardless_of_toml_order() {
        let cf = parse(
            r#"
[shared_services.zeta]
image = "zeta:1"

[shared_services.alpha]
image = "alpha:1"

[shared_services.mu]
image = "mu:1"
"#,
        )
        .unwrap();

        let names: Vec<&str> = cf.services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    }

    // -----------------------------------------------------------
    // Rejections
    // -----------------------------------------------------------

    #[test]
    fn rejects_unknown_top_level_section_coast() {
        assert_parse_err_contains(
            r#"
[coast]
name = "my-app"

[shared_services.postgres]
image = "postgres:16"
"#,
            "unknown field",
        );
    }

    #[test]
    fn rejects_unknown_top_level_section_ports() {
        assert_parse_err_contains(
            r#"
[ports]
web = 3000

[shared_services.postgres]
image = "postgres:16"
"#,
            "unknown field",
        );
    }

    #[test]
    fn rejects_unknown_field_in_ssg_section() {
        assert_parse_err_contains(
            r#"
[ssg]
runtime = "dind"
mystery = "value"
"#,
            "unknown field",
        );
    }

    #[test]
    fn rejects_from_group_on_ssg_service() {
        // `from_group` is a consumer-Coastfile concept, not an SSG
        // Coastfile concept. `deny_unknown_fields` catches this.
        let err = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
from_group = true
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field"), "got: {err}");
        assert!(err.contains("from_group"), "got: {err}");
    }

    #[test]
    fn rejects_inject_on_ssg_service() {
        let err = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
inject = "env:DATABASE_URL"
"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unknown field"), "got: {err}");
        assert!(err.contains("inject"), "got: {err}");
    }

    #[test]
    fn rejects_missing_image() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
ports = [5432]
"#,
            "image",
        );
    }

    #[test]
    fn rejects_empty_image() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = ""
"#,
            "image cannot be empty",
        );
    }

    #[test]
    fn rejects_port_zero() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [0]
"#,
            "port 0 is not valid",
        );
    }

    #[test]
    fn rejects_host_container_port_string() {
        // "5433:5432" cannot deserialize into a Vec<u16>. Serde emits
        // a TOML deserialization error before our validation runs.
        let err = parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = ["5433:5432"]
"#,
        )
        .unwrap_err()
        .to_string();
        // Accept any TOML/serde phrasing.
        assert!(
            err.contains("TOML parse error") || err.contains("invalid type"),
            "expected a TOML/serde error; got: {err}"
        );
    }

    #[test]
    fn rejects_relative_volume_source() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["./data:/data"]
"#,
            "relative paths are not supported",
        );
    }

    #[test]
    fn rejects_volume_missing_colon() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["just-a-name"]
"#,
            "missing ':' separator",
        );
    }

    #[test]
    fn rejects_parent_dir_in_host_path() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/../escape:/data"]
"#,
            "must not contain '..'",
        );
    }

    #[test]
    fn rejects_parent_dir_in_container_path() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/pg:/var/lib/../escape"]
"#,
            "must not contain '..'",
        );
    }

    #[test]
    fn rejects_source_with_slash_but_not_absolute() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["relative/path:/data"]
"#,
            "absolute host path or a Docker volume name",
        );
    }

    #[test]
    fn rejects_non_absolute_container_path() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/pg:relative-target"]
"#,
            "container path must be absolute",
        );
    }

    #[test]
    fn rejects_duplicate_container_paths() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = [
    "/var/pg1:/var/lib/postgresql/data",
    "/var/pg2:/var/lib/postgresql/data",
]
"#,
            "duplicate container path",
        );
    }

    #[test]
    fn rejects_invalid_runtime() {
        assert_parse_err_contains(
            r#"
[ssg]
runtime = "kvm"

[shared_services.postgres]
image = "postgres:16"
"#,
            "invalid runtime 'kvm'",
        );
    }

    #[test]
    fn rejects_env_array_value() {
        assert_parse_err_contains(
            r#"
[shared_services.postgres]
image = "postgres:16"
env = { FOO = [1, 2] }
"#,
            "must be a scalar",
        );
    }

    // -----------------------------------------------------------
    // File I/O
    // -----------------------------------------------------------

    #[test]
    fn from_file_reads_and_parses_happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Coastfile.shared_service_groups");
        std::fs::write(
            &path,
            r#"
[shared_services.postgres]
image = "postgres:16"
ports = [5432]
"#,
        )
        .unwrap();

        let cf = SsgCoastfile::from_file(&path).unwrap();
        assert_eq!(cf.services[0].name, "postgres");
        assert_eq!(cf.project_root, dir.path());
    }

    #[test]
    fn from_file_errors_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");

        let err = SsgCoastfile::from_file(&path).unwrap_err();
        assert!(
            matches!(err, CoastError::Io { .. }),
            "expected Io error, got: {err}"
        );
    }

    // -----------------------------------------------------------
    // Volume name classifier
    // -----------------------------------------------------------

    #[test]
    fn docker_volume_name_accepts_valid_names() {
        assert!(is_valid_docker_volume_name("pg_wal"));
        assert!(is_valid_docker_volume_name("pgdata"));
        assert!(is_valid_docker_volume_name("data-1"));
        assert!(is_valid_docker_volume_name("v1.2.3"));
        assert!(is_valid_docker_volume_name("a"));
    }

    #[test]
    fn docker_volume_name_rejects_invalid_names() {
        assert!(!is_valid_docker_volume_name(""));
        assert!(!is_valid_docker_volume_name("_leading_underscore"));
        assert!(!is_valid_docker_volume_name("-leading-dash"));
        assert!(!is_valid_docker_volume_name(".leading.dot"));
        assert!(!is_valid_docker_volume_name("has space"));
        assert!(!is_valid_docker_volume_name("has/slash"));
    }
}
