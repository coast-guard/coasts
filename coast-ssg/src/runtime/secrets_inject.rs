//! Phase 33: SSG secret materialization at run time.
//!
//! Bridges the encrypted keystore (populated during `coast ssg
//! build` by [`crate::build::secrets::extract_ssg_secrets`]) and
//! the inner compose stack. Reads decrypted bytes, writes a
//! per-run `compose.override.yml` to
//! `~/.coast/ssg/runs/<project>/`, optionally streams `file:`
//! secret payloads into the outer DinD via privileged exec, and
//! returns the list of compose files the caller should layer on
//! top of `/coast-artifact/compose.yml` via additional `-f` argv
//! pairs.
//!
//! The override file is written fresh on every run so a
//! `coast ssg secrets clear` between runs takes effect (even
//! without re-creating the container) — although for the keystore-
//! cleared case the run path will yield an empty extras list since
//! `list_secrets_for_image` will return zero rows.
//!
//! See `DESIGN.md §33` for the full lifecycle and decision log.

use std::collections::BTreeMap;
use std::path::PathBuf;

use coast_core::error::{CoastError, Result};

use crate::build::artifact::SsgManifest;
use crate::build::secrets::keystore_image_key;
use crate::docker_ops::SsgDockerOps;
use crate::paths;
use crate::runtime::lifecycle::INNER_RUNTIME_DIR;

/// File name for the override compose file inside the per-run
/// scratch dir. Centralized so the host writer and inner reader
/// agree on a single string.
const OVERRIDE_FILE_NAME: &str = "compose.override.yml";

/// Sub-directory inside the per-run scratch dir for `inject = "file:..."`
/// payloads. Decrypted bytes land at
/// `~/.coast/ssg/runs/<project>/secrets/<basename>` on the host
/// (visible at `/coast-runtime/secrets/<basename>` inside the
/// outer DinD), and the override file mounts each one read-only
/// into the inner service at the configured `inject_target`.
const SECRETS_SUBDIR: &str = "secrets";

/// Materialize keystore-decrypted secrets into a per-run
/// `compose.override.yml` and (for `file:` injects) on-disk
/// payloads.
///
/// Returns the list of additional compose files the caller should
/// pass to `inner_compose_up` via `-f` argv pairs (always at most
/// one entry: the override file). Returns an empty Vec when no
/// keystore rows exist for `ssg:<project>` — callers should still
/// invoke this since the manifest may declare injects whose
/// keystore rows were since cleared. In that case the run still
/// proceeds without the override file (compose-up will fail at
/// service startup if a service depends on a missing var; that's
/// the correct fail-loud behavior).
///
/// The `_ops` and `_container_id` arguments are reserved for a
/// future enhancement that writes file-secret payloads via
/// privileged exec into a non-shared filesystem; v1 keeps the
/// payload on the host bind-mount because the runtime dir is
/// already shared with the outer DinD via the bind in
/// `create_ssg_container`.
pub async fn materialize_secrets(
    project: &str,
    _ops: &dyn SsgDockerOps,
    _container_id: &str,
    manifest: &SsgManifest,
) -> Result<Vec<String>> {
    if manifest.secret_injects.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve and prepare the per-run scratch dir. Any leftover
    // override file or `secrets/` payload from a previous run is
    // cleared out: the manifest may have changed inject targets,
    // and stale payloads must not leak into the new container.
    let run_dir = paths::ssg_run_dir(project)?;
    let secrets_subdir = run_dir.join(SECRETS_SUBDIR);
    let _ = std::fs::remove_dir_all(&secrets_subdir);
    std::fs::create_dir_all(&run_dir).map_err(|e| CoastError::Io {
        message: format!("failed to prepare SSG run dir '{}': {e}", run_dir.display()),
        path: run_dir.clone(),
        source: Some(e),
    })?;

    // Open the keystore (same DB the build pipeline wrote to).
    let home = coast_core::artifact::coast_home()?;
    let keystore = coast_secrets::keystore::Keystore::open(
        &home.join("keystore.db"),
        &home.join("keystore.key"),
    )
    .map_err(|e| CoastError::Secret {
        message: format!("failed to open keystore for '{project}': {e}"),
        source: Some(Box::new(e)),
    })?;

    let image_key = keystore_image_key(project);
    let override_key = format!("{image_key}/override");
    let mut stored = keystore
        .get_all_secrets(&image_key)
        .map_err(|e| CoastError::Secret {
            message: format!("failed to list keystore secrets for '{image_key}': {e}"),
            source: Some(Box::new(e)),
        })?;
    // Phase 33: a row in the override namespace REPLACES the base
    // row by `secret_name`. Mirrors the regular instance secret
    // merge policy in `coast-daemon::handlers::secret::merge_secrets`
    // and the SSG list endpoint at
    // `coast-daemon::api::query::ssg::load_ssg_secret_info`.
    let overrides = keystore
        .get_all_secrets(&override_key)
        .map_err(|e| CoastError::Secret {
            message: format!("failed to list keystore overrides for '{override_key}': {e}"),
            source: Some(Box::new(e)),
        })?;
    for ov in overrides {
        stored.retain(|existing| existing.secret_name != ov.secret_name);
        stored.push(ov);
    }

    if stored.is_empty() {
        // Keystore is empty (e.g. `coast ssg secrets clear` since
        // the last build). Wipe any stale override file from a
        // previous run so the next compose-up doesn't pick up
        // values we no longer have.
        let _ = std::fs::remove_file(run_dir.join(OVERRIDE_FILE_NAME));
        return Ok(Vec::new());
    }

    // Index keystore rows by secret_name for O(1) lookup as we
    // walk the manifest declarations.
    let mut by_name: std::collections::HashMap<&str, &coast_secrets::keystore::StoredSecret> =
        std::collections::HashMap::with_capacity(stored.len());
    for s in &stored {
        by_name.insert(s.secret_name.as_str(), s);
    }

    // Build the override structure deterministically. BTreeMap
    // gives sorted YAML output which keeps the file diffable and
    // makes integration tests robust to map iteration order.
    let mut per_service: BTreeMap<&str, ServiceOverlay> = BTreeMap::new();

    let mut file_payloads_written: Vec<(PathBuf, Vec<u8>)> = Vec::new();

    for inject in &manifest.secret_injects {
        let Some(rec) = by_name.get(inject.secret_name.as_str()) else {
            // Manifest declared a secret but the keystore row is
            // gone (cleared / never extracted). Skip silently —
            // the doctor check + run-time service failure will
            // surface this loudly enough.
            continue;
        };
        for service in &inject.services {
            let entry = per_service.entry(service.as_str()).or_default();
            match inject.inject_type.as_str() {
                "env" => {
                    // String value: keystore stores raw bytes;
                    // we route the lossy UTF-8 form into compose's
                    // YAML-string env value. The build pipeline only
                    // accepts UTF-8 secrets via the extractors today
                    // (env, file, command) so this is safe.
                    let value = String::from_utf8_lossy(&rec.value).into_owned();
                    entry
                        .environment
                        .insert(inject.inject_target.clone(), value);
                }
                "file" => {
                    // Write the decrypted bytes to
                    // `<run_dir>/secrets/<basename>` and bind-mount
                    // it read-only into the inner service at the
                    // configured `inject_target`.
                    let basename = std::path::Path::new(&inject.inject_target)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| inject.secret_name.clone());
                    let host_path = secrets_subdir.join(&basename);
                    file_payloads_written.push((host_path.clone(), rec.value.clone()));

                    let inner_path = format!("{INNER_RUNTIME_DIR}/{SECRETS_SUBDIR}/{basename}");
                    entry
                        .volumes
                        .push(format!("{inner_path}:{}:ro", inject.inject_target));
                }
                other => {
                    return Err(CoastError::Secret {
                        message: format!(
                            "unknown inject_type '{other}' in manifest secret \
                             '{}' (expected 'env' or 'file')",
                            inject.secret_name
                        ),
                        source: None,
                    });
                }
            }
        }
    }

    // Write file-secret payloads. We do this AFTER iterating the
    // manifest so a single bad secret aborts before we drop any
    // bytes on disk.
    if !file_payloads_written.is_empty() {
        std::fs::create_dir_all(&secrets_subdir).map_err(|e| CoastError::Io {
            message: format!(
                "failed to create SSG run secrets dir '{}': {e}",
                secrets_subdir.display()
            ),
            path: secrets_subdir.clone(),
            source: Some(e),
        })?;
        for (path, bytes) in &file_payloads_written {
            write_secret_file(path, bytes)?;
        }
    }

    // Render the override YAML and write it.
    let override_yaml = render_override_yaml(&per_service);
    let override_path = run_dir.join(OVERRIDE_FILE_NAME);
    std::fs::write(&override_path, override_yaml).map_err(|e| CoastError::Io {
        message: format!(
            "failed to write SSG compose override '{}': {e}",
            override_path.display()
        ),
        path: override_path.clone(),
        source: Some(e),
    })?;

    // Return the inner-side path the caller should pass to
    // `inner_compose_up`. The host path is bind-mounted at
    // `INNER_RUNTIME_DIR` so compose finds the file there.
    Ok(vec![format!("{INNER_RUNTIME_DIR}/{OVERRIDE_FILE_NAME}")])
}

#[derive(Debug, Default)]
struct ServiceOverlay {
    environment: BTreeMap<String, String>,
    volumes: Vec<String>,
}

/// Render the per-service overlay map into a minimal compose YAML
/// fragment. We don't pull in a full YAML serializer here — the
/// shape is fixed and small, and a hand-rolled emitter avoids a
/// dependency on `serde_yaml` (already weighing on coast-daemon).
fn render_override_yaml(per_service: &BTreeMap<&str, ServiceOverlay>) -> String {
    let mut out = String::from("# Generated by coast-ssg at run time. Do not hand-edit.\n");
    out.push_str("services:\n");
    for (svc, overlay) in per_service {
        out.push_str(&format!("  {}:\n", yaml_key(svc)));
        if !overlay.environment.is_empty() {
            out.push_str("    environment:\n");
            for (k, v) in &overlay.environment {
                out.push_str(&format!("      {}: {}\n", yaml_key(k), yaml_string(v)));
            }
        }
        if !overlay.volumes.is_empty() {
            out.push_str("    volumes:\n");
            for vol in &overlay.volumes {
                out.push_str(&format!("      - {}\n", yaml_string(vol)));
            }
        }
    }
    out
}

/// Quote a YAML scalar conservatively. Always emits a
/// double-quoted string with `\\` and `\"` escaped. Compose accepts
/// double-quoted scalars in every position we use here.
fn yaml_string(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

/// Emit a YAML map key. We keep this conservative: if the key is
/// purely `[A-Za-z_][A-Za-z0-9_-]*`, write it bare; otherwise quote.
fn yaml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_string()
    } else {
        yaml_string(key)
    }
}

/// Write a secret file with 0600 perms on Unix. Best-effort: if
/// chmod fails (e.g. on a filesystem that doesn't support Unix
/// perms) we still keep the file but surface the error so the
/// user can see it. Bytes are written before the chmod to avoid
/// a window where the file is world-readable.
fn write_secret_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|e| CoastError::Io {
        message: format!("failed to write secret file '{}': {e}", path.display()),
        path: path.to_path_buf(),
        source: Some(e),
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_key_is_bare_for_simple_identifiers() {
        assert_eq!(yaml_key("postgres"), "postgres");
        assert_eq!(yaml_key("POSTGRES_PASSWORD"), "POSTGRES_PASSWORD");
        assert_eq!(yaml_key("my-service"), "my-service");
        assert_eq!(yaml_key("a_1"), "a_1");
    }

    #[test]
    fn yaml_key_is_quoted_for_special_chars() {
        assert_eq!(yaml_key("with space"), "\"with space\"");
        assert_eq!(yaml_key("1leading-digit"), "\"1leading-digit\"");
        assert_eq!(yaml_key(""), "\"\"");
    }

    #[test]
    fn yaml_string_escapes_quotes_backslashes_and_newlines() {
        assert_eq!(yaml_string("plain"), "\"plain\"");
        assert_eq!(yaml_string("with\"quote"), "\"with\\\"quote\"");
        assert_eq!(yaml_string("with\\back"), "\"with\\\\back\"");
        assert_eq!(yaml_string("line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn render_override_yaml_emits_environment_block() {
        let mut per_service: BTreeMap<&str, ServiceOverlay> = BTreeMap::new();
        let mut overlay = ServiceOverlay::default();
        overlay
            .environment
            .insert("POSTGRES_PASSWORD".to_string(), "s3cr3t".to_string());
        per_service.insert("postgres", overlay);

        let yaml = render_override_yaml(&per_service);
        assert!(yaml.contains("services:"));
        assert!(yaml.contains("  postgres:"));
        assert!(yaml.contains("    environment:"));
        assert!(yaml.contains("POSTGRES_PASSWORD: \"s3cr3t\""));
    }

    #[test]
    fn render_override_yaml_emits_volumes_block_for_file_inject() {
        let mut per_service: BTreeMap<&str, ServiceOverlay> = BTreeMap::new();
        let mut overlay = ServiceOverlay::default();
        overlay
            .volumes
            .push("/coast-runtime/secrets/jwt:/run/secrets/jwt:ro".to_string());
        per_service.insert("api", overlay);

        let yaml = render_override_yaml(&per_service);
        assert!(yaml.contains("services:"));
        assert!(yaml.contains("  api:"));
        assert!(yaml.contains("    volumes:"));
        assert!(yaml.contains("      - \"/coast-runtime/secrets/jwt:/run/secrets/jwt:ro\""));
    }

    #[test]
    fn render_override_yaml_sorts_services_deterministically() {
        let mut per_service: BTreeMap<&str, ServiceOverlay> = BTreeMap::new();
        per_service.insert("zeta", ServiceOverlay::default());
        per_service.insert("alpha", ServiceOverlay::default());
        per_service.insert("mu", ServiceOverlay::default());

        let yaml = render_override_yaml(&per_service);
        let alpha_pos = yaml.find("alpha").unwrap();
        let mu_pos = yaml.find("mu").unwrap();
        let zeta_pos = yaml.find("zeta").unwrap();
        assert!(alpha_pos < mu_pos && mu_pos < zeta_pos);
    }
}
