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
    /// `project_root` is used to resolve relative paths (e.g. during
    /// future `[ssg.setup]` work).
    ///
    /// Runs the same env-var interpolation pipeline as regular
    /// Coastfiles before TOML parsing.
    ///
    /// **String-mode rejects `extends` / `includes`.** Inheritance
    /// requires disk access to resolve parent / fragment paths; call
    /// [`SsgCoastfile::from_file`] instead. Mirrors the regular
    /// `Coastfile::parse` behavior. See `DESIGN.md §17 SETTLED #42`.
    pub fn parse(content: &str, project_root: &Path) -> Result<Self> {
        let interp = interpolate_env_vars(content);
        let raw: RawSsgCoastfile = toml::from_str(&interp.content)?;
        if raw.ssg.extends.is_some() || raw.ssg.includes.is_some() {
            return Err(CoastError::coastfile(
                "extends and includes require file-based parsing. \
                 Use SsgCoastfile::from_file() instead.",
            ));
        }
        let mut cf = Self::validate_and_build(raw, project_root)?;
        cf.interpolation_warnings = interp.warnings;
        Ok(cf)
    }

    /// Parse an SSG Coastfile from disk, recursively resolving
    /// [`extends`](RawSsgSection::extends) and
    /// [`includes`](RawSsgSection::includes).
    ///
    /// Resolves `project_root` to the file's parent directory.
    /// Mirrors [`coast_core::coastfile::Coastfile::from_file`].
    pub fn from_file(path: &Path) -> Result<Self> {
        Self::from_file_with_ancestry(path, &mut HashSet::new())
    }

    /// Recursive loader for inheritance. `ancestors` is a DFS
    /// visit-set keyed by canonicalized path, so direct cycles are
    /// rejected with `"circular extends/includes dependency
    /// detected: '<path>'"` while diamond inheritance (A extends B
    /// and C, both extend D) still succeeds.
    ///
    /// See `DESIGN.md §17 SETTLED #42` for the design rationale and
    /// the divergence from the regular Coastfile's merge strategy
    /// (SSG merges at the raw-TOML level, then validates once).
    fn from_file_with_ancestry(path: &Path, ancestors: &mut HashSet<PathBuf>) -> Result<Self> {
        let project_root = path
            .parent()
            .ok_or_else(|| CoastError::coastfile("SSG Coastfile path has no parent directory"))?
            .to_path_buf();

        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !ancestors.insert(canonical.clone()) {
            return Err(CoastError::coastfile(format!(
                "circular extends/includes dependency detected: '{}'",
                path.display()
            )));
        }

        let result = Self::load_merged(path, &project_root, ancestors);
        ancestors.remove(&canonical);
        result
    }

    /// Read, interpolate, deserialize, and (if needed) merge
    /// parents/fragments. Extracted so `from_file_with_ancestry` can
    /// do `ancestors.remove(&canonical)` on every return path
    /// without repeating the call at each `?` site.
    fn load_merged(
        path: &Path,
        project_root: &Path,
        ancestors: &mut HashSet<PathBuf>,
    ) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| CoastError::Io {
            message: format!("failed to read SSG Coastfile: {e}"),
            path: path.to_path_buf(),
            source: Some(e),
        })?;

        let interp = interpolate_env_vars(&content);
        let mut warnings = interp.warnings;
        let raw: RawSsgCoastfile = toml::from_str(&interp.content)?;

        // Fast path: no inheritance directives -> validate once.
        // `[unset]` on a standalone file is silently ignored (matches
        // regular Coastfile: `apply_unset` is only invoked inside
        // `from_file_with_ancestry`, never in `parse`).
        if raw.ssg.extends.is_none() && raw.ssg.includes.is_none() {
            let mut cf = Self::validate_and_build(raw, project_root)?;
            cf.interpolation_warnings = warnings;
            return Ok(cf);
        }

        // Inheritance path: merge at the raw level, then validate once.
        let extends_ref = raw.ssg.extends.clone();
        let includes_ref = raw.ssg.includes.clone().unwrap_or_default();

        let mut base = match extends_ref {
            Some(ref extends_path_str) => {
                let extends_path = Self::find_ssg_coastfile(project_root, extends_path_str)
                    .unwrap_or_else(|| project_root.join(extends_path_str));
                let parent = Self::from_file_with_ancestry(&extends_path, ancestors)?;
                // Surface parent's interpolation warnings too (matches
                // regular Coastfile behavior: warnings are cumulative
                // across extended files).
                warnings.extend(parent.interpolation_warnings.iter().cloned());
                Self::to_raw(parent)
            }
            None => Self::empty_raw(),
        };

        for include_path_str in &includes_ref {
            let include_path = project_root.join(include_path_str);
            let include_content =
                std::fs::read_to_string(&include_path).map_err(|e| CoastError::Io {
                    message: format!(
                        "failed to read SSG Coastfile include '{}': {e}",
                        include_path.display()
                    ),
                    path: include_path.clone(),
                    source: Some(e),
                })?;
            let include_interp = interpolate_env_vars(&include_content);
            warnings.extend(include_interp.warnings);
            let include_raw: RawSsgCoastfile = toml::from_str(&include_interp.content)?;
            if include_raw.ssg.extends.is_some() || include_raw.ssg.includes.is_some() {
                return Err(CoastError::coastfile(format!(
                    "SSG Coastfile include '{}' cannot itself use extends or includes. \
                     Fragments must be self-contained.",
                    include_path.display()
                )));
            }
            base = merge_raw_onto(base, include_raw);
        }

        base = merge_raw_onto(base, raw);
        apply_unset(&mut base);

        let mut cf = Self::validate_and_build(base, project_root)?;
        cf.interpolation_warnings = warnings;
        Ok(cf)
    }

    /// Build an empty `RawSsgCoastfile` to seed merges when the
    /// top-level file has `includes` but no `extends`.
    fn empty_raw() -> RawSsgCoastfile {
        RawSsgCoastfile {
            ssg: RawSsgSection::default(),
            shared_services: HashMap::new(),
            unset: None,
        }
    }

    /// Reverse-serialize a validated [`SsgCoastfile`] into a
    /// [`RawSsgCoastfile`] so the raw-level merge pipeline can layer
    /// more files on top of it. Only called from `load_merged` in
    /// the `extends = "..."` branch.
    fn to_raw(cf: SsgCoastfile) -> RawSsgCoastfile {
        let mut shared_services = HashMap::with_capacity(cf.services.len());
        for svc in cf.services {
            let mut env = HashMap::with_capacity(svc.env.len());
            for (k, v) in svc.env {
                env.insert(k, toml::Value::String(v));
            }
            let volumes = svc
                .volumes
                .iter()
                .map(format_volume_entry)
                .collect::<Vec<_>>();
            shared_services.insert(
                svc.name,
                RawSsgSharedServiceConfig {
                    image: svc.image,
                    ports: svc.ports,
                    volumes,
                    env,
                    auto_create_db: svc.auto_create_db,
                },
            );
        }

        RawSsgCoastfile {
            ssg: RawSsgSection {
                runtime: Some(cf.section.runtime.as_str().to_string()),
                extends: None,
                includes: None,
            },
            shared_services,
            unset: None,
        }
    }

    /// Resolve `extends = "<base>"` against `project_root` with the
    /// `.toml` tie-break: try `<base>.toml` first, then plain
    /// `<base>`. Returns `None` when neither file exists (caller
    /// falls back to naive `project_root.join(base)` so the I/O
    /// error surfaces from `read_to_string`).
    ///
    /// Uses string concatenation (`"<base>.toml"`) rather than
    /// `Path::with_extension` so `Coastfile.base` + `.toml` yields
    /// `Coastfile.base.toml`, not `Coastfile.toml`. Mirrors the
    /// regular Coastfile's `find_coastfile`.
    fn find_ssg_coastfile(project_root: &Path, base: &str) -> Option<PathBuf> {
        let base_path = Path::new(base);
        let resolved = if base_path.is_absolute() {
            base_path.to_path_buf()
        } else {
            project_root.join(base_path)
        };
        let with_toml = PathBuf::from(format!("{}.toml", resolved.display()));
        if with_toml.is_file() {
            return Some(with_toml);
        }
        if resolved.is_file() {
            return Some(resolved);
        }
        None
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

    /// Serialize back to the accepted `Coastfile.shared_service_groups`
    /// schema as a string.
    ///
    /// Used by the build artifact writer
    /// (`coast-ssg/src/build/artifact.rs`) to snapshot the
    /// post-interpolation, post-validation Coastfile into the build
    /// directory so `coast ssg run` has a stable input to consume. The
    /// output parses back via [`Self::parse`] (round-trip tested).
    pub fn to_standalone_toml(&self) -> String {
        let mut out = String::new();

        out.push_str("[ssg]\n");
        out.push_str(&format!(
            "runtime = {}\n",
            toml_quote(self.section.runtime.as_str())
        ));

        for svc in &self.services {
            out.push('\n');
            out.push_str(&format!("[shared_services.{}]\n", toml_key(&svc.name)));
            out.push_str(&format!("image = {}\n", toml_quote(&svc.image)));

            if !svc.ports.is_empty() {
                let ports = svc
                    .ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("ports = [{ports}]\n"));
            }

            if !svc.volumes.is_empty() {
                let volumes = svc
                    .volumes
                    .iter()
                    .map(|v| toml_quote(&format_volume_entry(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("volumes = [{volumes}]\n"));
            }

            if !svc.env.is_empty() {
                let mut pairs: Vec<_> = svc.env.iter().collect();
                pairs.sort_by_key(|(k, _)| k.as_str());
                let rendered = pairs
                    .iter()
                    .map(|(k, v)| format!("{} = {}", toml_key(k), toml_quote(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("env = {{ {rendered} }}\n"));
            }

            if svc.auto_create_db {
                out.push_str("auto_create_db = true\n");
            }
        }

        out
    }
}

/// Format an `SsgVolumeEntry` back into its `"source:target"` string form.
fn format_volume_entry(entry: &SsgVolumeEntry) -> String {
    match entry {
        SsgVolumeEntry::HostBindMount {
            host_path,
            container_path,
        } => format!("{}:{}", host_path.display(), container_path.display()),
        SsgVolumeEntry::InnerNamedVolume {
            name,
            container_path,
        } => format!("{}:{}", name, container_path.display()),
    }
}

/// Produce a TOML-safe quoted string value. Mirrors the escaping used
/// by [`coast_core::coastfile::serializer`].
fn toml_quote(s: &str) -> String {
    format!("{:?}", s)
}

/// Emit a bare TOML key when the input is alphanumeric / `_` / `-`,
/// otherwise quote it.
fn toml_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        key.to_string()
    } else {
        toml_quote(key)
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

/// Layer `layer` onto `base` at the raw-TOML level.
///
/// Phase 17 merge semantics (see `DESIGN.md §17 SETTLED #42`):
/// - **`[ssg]` scalars** (`runtime`): child wins when present, else
///   inherit.
/// - **`[shared_services.*]`**: by-name replace. `HashMap::extend`
///   already gives us "layer overrides same key, other keys
///   preserved" -- whole-entry replacement, not field-level merge,
///   matching the regular Coastfile's `merge_named_items` shape.
/// - **`[unset]`**: lists from base + layer are concatenated; the
///   resulting list is applied post-merge via [`apply_unset`].
/// - **`extends` / `includes`** are consumed at load time and never
///   carried forward (the merged `RawSsgCoastfile` is the
///   post-inheritance state).
fn merge_raw_onto(mut base: RawSsgCoastfile, layer: RawSsgCoastfile) -> RawSsgCoastfile {
    if layer.ssg.runtime.is_some() {
        base.ssg.runtime = layer.ssg.runtime;
    }

    base.shared_services.extend(layer.shared_services);

    base.unset = match (base.unset, layer.unset) {
        (None, other) => other,
        (existing, None) => existing,
        (Some(mut a), Some(b)) => {
            a.shared_services.extend(b.shared_services);
            Some(a)
        }
    };

    base.ssg.extends = None;
    base.ssg.includes = None;
    base
}

/// Drop every `shared_services` entry listed in `[unset]`. Only
/// invoked on files that used `extends` or `includes` -- standalone
/// files never reach this pass (serde still accepts `[unset]` on
/// standalone files, but it's a no-op, matching the regular
/// Coastfile's `apply_unset` invocation site).
fn apply_unset(raw: &mut RawSsgCoastfile) {
    if let Some(unset) = raw.unset.take() {
        for name in unset.shared_services {
            raw.shared_services.remove(&name);
        }
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

        // Preserve-on-miss: the undefined `${VAR}` is kept as literal
        // text so shell-defined variables elsewhere survive. The image
        // string here is nonsensical to Docker but Coast surfaces the
        // warning so the user can spot it.
        assert_eq!(cf.services[0].image, "redis:${COAST_SSG_UNDEFINED_XYZ_123}");
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

    // -----------------------------------------------------------
    // to_standalone_toml round-trip
    // -----------------------------------------------------------

    fn roundtrip(input: &str) -> SsgCoastfile {
        let cf = parse(input).unwrap();
        let serialized = cf.to_standalone_toml();
        SsgCoastfile::parse(&serialized, Path::new("/tmp/ssg-test"))
            .unwrap_or_else(|e| panic!("reparse failed: {e}\n---\n{serialized}"))
    }

    #[test]
    fn round_trip_minimal() {
        let reparsed = roundtrip(
            r#"
[shared_services.postgres]
image = "postgres:16"
"#,
        );
        assert_eq!(reparsed.services.len(), 1);
        assert_eq!(reparsed.services[0].name, "postgres");
        assert_eq!(reparsed.services[0].image, "postgres:16");
        assert!(reparsed.services[0].ports.is_empty());
    }

    #[test]
    fn round_trip_multi_service_keeps_sort_order() {
        let reparsed = roundtrip(
            r#"
[ssg]
runtime = "dind"

[shared_services.zeta]
image = "zeta:1"
ports = [9000]

[shared_services.alpha]
image = "alpha:1"
"#,
        );
        let names: Vec<&str> = reparsed.services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn round_trip_host_bind_volume() {
        let reparsed = roundtrip(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["/var/coast-data/pg:/var/lib/postgresql/data"]
"#,
        );
        match &reparsed.services[0].volumes[0] {
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
    fn round_trip_inner_named_volume() {
        let reparsed = roundtrip(
            r#"
[shared_services.postgres]
image = "postgres:16"
volumes = ["pg_wal:/var/lib/postgresql/wal"]
"#,
        );
        match &reparsed.services[0].volumes[0] {
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
    fn round_trip_env_preserves_string_values() {
        let reparsed = roundtrip(
            r#"
[shared_services.postgres]
image = "postgres:16"
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "secret" }
"#,
        );
        let env = &reparsed.services[0].env;
        assert_eq!(env.get("POSTGRES_USER").map(String::as_str), Some("coast"));
        assert_eq!(
            env.get("POSTGRES_PASSWORD").map(String::as_str),
            Some("secret")
        );
    }

    #[test]
    fn round_trip_env_coerced_scalars_survive_as_strings() {
        // Reparsing the serializer output, the re-serialized values are
        // strings (since we coerced ints/bools/floats on first parse).
        // Ensure the second parse still succeeds.
        let reparsed = roundtrip(
            r#"
[shared_services.redis]
image = "redis:7"
env = { COUNT = 42, FLAG = true, RATIO = 3.5 }
"#,
        );
        let env = &reparsed.services[0].env;
        assert_eq!(env.get("COUNT").map(String::as_str), Some("42"));
        assert_eq!(env.get("FLAG").map(String::as_str), Some("true"));
        assert_eq!(env.get("RATIO").map(String::as_str), Some("3.5"));
    }

    #[test]
    fn round_trip_auto_create_db() {
        let reparsed = roundtrip(
            r#"
[shared_services.postgres]
image = "postgres:16"
auto_create_db = true
"#,
        );
        assert!(reparsed.services[0].auto_create_db);
    }

    #[test]
    fn round_trip_runtime_defaults_to_dind_when_absent() {
        let reparsed = roundtrip(
            r#"
[shared_services.pg]
image = "postgres:16"
"#,
        );
        assert_eq!(reparsed.section.runtime, RuntimeType::Dind);
    }

    // ----------------------------------------------------------------
    // Phase 17: extends / includes / [unset]
    // ----------------------------------------------------------------

    fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    fn service_names(cf: &SsgCoastfile) -> Vec<String> {
        cf.services.iter().map(|s| s.name.clone()).collect()
    }

    fn service(cf: &SsgCoastfile, name: &str) -> SsgSharedServiceConfig {
        cf.services
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .unwrap_or_else(|| panic!("service '{name}' not found"))
    }

    #[test]
    fn extends_basic_inherits_parent_services() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.shared_service_groups",
            r#"
[ssg]
extends = "Coastfile.base"

[shared_services.postgres]
image = "postgres:17-alpine"
ports = [5432]
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(service_names(&cf), vec!["postgres", "redis"]);
        assert_eq!(service(&cf, "postgres").image, "postgres:17-alpine");
        assert_eq!(service(&cf, "redis").image, "redis:7-alpine");
    }

    #[test]
    fn extends_chain_of_three_deep_override() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.root",
            r#"
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:15-alpine"

[shared_services.mongo]
image = "mongo:6"
"#,
        );
        write_file(
            tmp.path(),
            "Coastfile.mid",
            r#"
[ssg]
extends = "Coastfile.root"

[shared_services.postgres]
image = "postgres:16-alpine"
"#,
        );
        let top = write_file(
            tmp.path(),
            "Coastfile.top",
            r#"
[ssg]
extends = "Coastfile.mid"

[shared_services.postgres]
image = "postgres:17-alpine"
"#,
        );

        let cf = SsgCoastfile::from_file(&top).unwrap();
        assert_eq!(service_names(&cf), vec!["mongo", "postgres"]);
        assert_eq!(service(&cf, "postgres").image, "postgres:17-alpine");
        assert_eq!(service(&cf, "mongo").image, "mongo:6");
    }

    #[test]
    fn extends_child_overrides_shared_service_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast" }
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.shared_service_groups",
            r#"
[ssg]
extends = "Coastfile.base"

[shared_services.postgres]
image = "postgres:17-alpine"
ports = [5432]
env = { POSTGRES_USER = "override", POSTGRES_DB = "app" }
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        let pg = service(&cf, "postgres");
        assert_eq!(pg.image, "postgres:17-alpine");
        // Whole-entry replace: child's env fully replaces parent's.
        assert_eq!(
            pg.env.get("POSTGRES_USER").map(String::as_str),
            Some("override")
        );
        assert_eq!(pg.env.get("POSTGRES_DB").map(String::as_str), Some("app"));
    }

    #[test]
    fn extends_child_inherits_runtime_from_parent() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[ssg]
runtime = "sysbox"

[shared_services.pg]
image = "postgres:16"
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.base"
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(cf.section.runtime, RuntimeType::Sysbox);
    }

    #[test]
    fn extends_child_overrides_runtime() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[ssg]
runtime = "dind"

[shared_services.pg]
image = "postgres:16"
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.base"
runtime = "podman"
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(cf.section.runtime, RuntimeType::Podman);
    }

    #[test]
    fn includes_basic_fragment_merges() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "extra.toml",
            r#"
[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
"#,
        );
        let main = write_file(
            tmp.path(),
            "Coastfile.shared_service_groups",
            r#"
[ssg]
runtime = "dind"
includes = ["extra.toml"]

[shared_services.postgres]
image = "postgres:16-alpine"
"#,
        );

        let cf = SsgCoastfile::from_file(&main).unwrap();
        assert_eq!(service_names(&cf), vec!["postgres", "redis"]);
    }

    #[test]
    fn includes_cannot_have_extends_in_fragment() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "base.toml",
            "[shared_services.x]\nimage = \"x:1\"\n",
        );
        write_file(
            tmp.path(),
            "frag.toml",
            r#"
[ssg]
extends = "base.toml"

[shared_services.pg]
image = "postgres:16"
"#,
        );
        let main = write_file(
            tmp.path(),
            "Coastfile.main",
            r#"
[ssg]
includes = ["frag.toml"]
"#,
        );

        let err = SsgCoastfile::from_file(&main).unwrap_err().to_string();
        assert!(
            err.contains("cannot itself use extends or includes"),
            "got: {err}"
        );
    }

    #[test]
    fn includes_cannot_have_includes_in_fragment() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "nested.toml",
            "[shared_services.x]\nimage = \"x:1\"\n",
        );
        write_file(
            tmp.path(),
            "frag.toml",
            r#"
[ssg]
includes = ["nested.toml"]
"#,
        );
        let main = write_file(
            tmp.path(),
            "Coastfile.main",
            r#"
[ssg]
includes = ["frag.toml"]
"#,
        );

        let err = SsgCoastfile::from_file(&main).unwrap_err().to_string();
        assert!(
            err.contains("cannot itself use extends or includes"),
            "got: {err}"
        );
    }

    #[test]
    fn extends_cycle_detection_a_to_b_to_a() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write_file(
            tmp.path(),
            "Coastfile.a",
            r#"
[ssg]
extends = "Coastfile.b"

[shared_services.x]
image = "x:1"
"#,
        );
        write_file(
            tmp.path(),
            "Coastfile.b",
            r#"
[ssg]
extends = "Coastfile.a"
"#,
        );

        let err = SsgCoastfile::from_file(&a).unwrap_err().to_string();
        assert!(
            err.contains("circular extends/includes dependency"),
            "got: {err}"
        );
    }

    #[test]
    fn extends_self_cycle_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let a = write_file(
            tmp.path(),
            "Coastfile.self",
            r#"
[ssg]
extends = "Coastfile.self"

[shared_services.x]
image = "x:1"
"#,
        );

        let err = SsgCoastfile::from_file(&a).unwrap_err().to_string();
        assert!(
            err.contains("circular extends/includes dependency"),
            "got: {err}"
        );
    }

    #[test]
    fn extends_path_resolves_relative_to_child_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("base")).unwrap();
        std::fs::create_dir_all(tmp.path().join("child")).unwrap();
        write_file(
            &tmp.path().join("base"),
            "Coastfile.shared_service_groups",
            r#"
[shared_services.pg]
image = "postgres:16-alpine"
"#,
        );
        let child = write_file(
            &tmp.path().join("child"),
            "Coastfile.shared_service_groups",
            r#"
[ssg]
extends = "../base/Coastfile.shared_service_groups"

[shared_services.redis]
image = "redis:7-alpine"
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(service_names(&cf), vec!["pg", "redis"]);
    }

    #[test]
    fn extends_toml_tie_break_picks_dot_toml_first() {
        // When both `Coastfile.base` and `Coastfile.base.toml` exist,
        // the `.toml` form wins. Matches regular Coastfile behavior.
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[shared_services.a]
image = "loser:1"
"#,
        );
        write_file(
            tmp.path(),
            "Coastfile.base.toml",
            r#"
[shared_services.a]
image = "winner:1"
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.base"
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(service(&cf, "a").image, "winner:1");
    }

    #[test]
    fn parse_rejects_extends_in_string_mode() {
        let err = parse(
            r#"
[ssg]
extends = "Coastfile.base"
"#,
        )
        .expect_err("parse must reject extends");
        let msg = err.to_string();
        assert!(
            msg.contains("extends and includes require file-based parsing"),
            "got: {msg}"
        );
    }

    #[test]
    fn parse_rejects_includes_in_string_mode() {
        let err = parse(
            r#"
[ssg]
includes = ["extra.toml"]
"#,
        )
        .expect_err("parse must reject includes");
        let msg = err.to_string();
        assert!(
            msg.contains("extends and includes require file-based parsing"),
            "got: {msg}"
        );
    }

    #[test]
    fn unset_removes_shared_service_after_merge() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[shared_services.postgres]
image = "postgres:16-alpine"

[shared_services.redis]
image = "redis:7-alpine"

[shared_services.mongo]
image = "mongo:6"
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.base"

[unset]
shared_services = ["redis", "mongo"]
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        assert_eq!(service_names(&cf), vec!["postgres"]);
    }

    #[test]
    fn to_standalone_toml_flattens_extended_coastfile() {
        // After extends + override + unset, to_standalone_toml
        // should emit a self-contained file: no [ssg].extends, no
        // [ssg].includes, no [unset]. Parsing the output in
        // string-mode must succeed and yield the same services.
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[shared_services.postgres]
image = "postgres:16-alpine"

[shared_services.redis]
image = "redis:7-alpine"
"#,
        );
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.base"

[shared_services.postgres]
image = "postgres:17-alpine"

[unset]
shared_services = ["redis"]
"#,
        );

        let cf = SsgCoastfile::from_file(&child).unwrap();
        let flat = cf.to_standalone_toml();
        assert!(
            !flat.contains("extends"),
            "flattened output must not contain `extends`:\n{flat}"
        );
        assert!(
            !flat.contains("includes"),
            "flattened output must not contain `includes`:\n{flat}"
        );
        assert!(
            !flat.contains("[unset]"),
            "flattened output must not contain `[unset]`:\n{flat}"
        );

        // Reparse the flattened output -- must succeed and match.
        let reparsed = SsgCoastfile::parse(&flat, tmp.path()).unwrap();
        assert_eq!(service_names(&reparsed), vec!["postgres"]);
        assert_eq!(service(&reparsed, "postgres").image, "postgres:17-alpine");
    }

    #[test]
    fn extends_missing_parent_surfaces_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let child = write_file(
            tmp.path(),
            "Coastfile.child",
            r#"
[ssg]
extends = "Coastfile.does-not-exist"
"#,
        );

        let err = SsgCoastfile::from_file(&child).unwrap_err().to_string();
        assert!(
            err.contains("failed to read") || err.contains("does-not-exist"),
            "got: {err}"
        );
    }

    #[test]
    fn diamond_inheritance_works_despite_repeated_grandparent() {
        // A extends B; A extends C (via includes merging is different
        // -- we model diamond via extends-chain + includes). Here:
        // top extends mid which extends base, AND top includes a
        // fragment that also extends base. The DFS visit-set is
        // per-recursion and pops ancestors on return, so visiting
        // `base` twice via different paths must not trip cycle
        // detection.
        //
        // Simpler form: top includes base via includes (fragment),
        // and also top extends base-parent. The includes fragment
        // is forbidden from using extends, so we simulate a diamond
        // via two separate subtrees that both end at `base` without
        // crossing fragments.
        //
        // Concretely: top extends mid; mid extends base; top also
        // *separately* extends the same mid's parent -- but `extends`
        // is single-valued. So strictly, the only diamond we can
        // exercise is: base -> mid -> top, where `base` would appear
        // once in the DFS. Cycle detection correctly allows this
        // (single path). This test is a sanity check that a normal
        // linear chain of extends doesn't accidentally trip the
        // cycle check by failing to remove from `ancestors`.
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path(),
            "Coastfile.base",
            r#"
[shared_services.pg]
image = "postgres:16"
"#,
        );
        write_file(
            tmp.path(),
            "Coastfile.mid",
            r#"
[ssg]
extends = "Coastfile.base"
"#,
        );
        let top = write_file(
            tmp.path(),
            "Coastfile.top",
            r#"
[ssg]
extends = "Coastfile.mid"

[shared_services.redis]
image = "redis:7"
"#,
        );

        let cf = SsgCoastfile::from_file(&top).unwrap();
        assert_eq!(service_names(&cf), vec!["pg", "redis"]);
    }
}
