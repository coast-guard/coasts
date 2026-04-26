//! Phase 33: SSG secret extraction during `coast ssg build`.
//!
//! Mirrors the regular [`coast-daemon/src/handlers/build/secrets.rs`]
//! but keys the encrypted keystore under the sentinel
//! `coast_image = "ssg:<project>"` namespace so SSG-owned secrets
//! never collide with a regular instance image of the same project
//! name.
//!
//! Run lifecycle:
//!
//! 1. `coast ssg build` calls [`extract_ssg_secrets`] after the
//!    compose synthesis step. Existing keystore rows for
//!    `ssg:<project>` are deleted and re-extracted (rebuild
//!    semantics — re-prompts on every build, accepted in the design
//!    decision per `DESIGN.md §33`).
//! 2. `coast ssg run` reads them back via
//!    `coast_secrets::keystore::Keystore::list_secrets_for_image`
//!    and renders a per-run `compose.override.yml` (see
//!    [`crate::runtime::secrets_inject`]).
//! 3. Keystore entries are NEVER auto-purged. Only the explicit
//!    `coast ssg secrets clear` verb removes them; `coast ssg rm`
//!    (with or without `--with-data`) leaves them alone, mirroring
//!    the user's preference in the design discussion.
//!
//! Extractor errors are emitted as per-secret `fail` items but do
//! NOT abort the build — partial extraction is still useful for
//! services that don't depend on a missing secret. The build
//! response surfaces a `warnings` list so the user knows.

use tokio::sync::mpsc::Sender;

use coast_core::artifact::coast_home;
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::InjectType;

use crate::coastfile::SsgCoastfile;

/// Sentinel keystore namespace. Centralized here so the build
/// (extract), run (inject), clear, and doctor paths agree on the
/// exact string. Format: `"ssg:<project>"` — the colon is illegal
/// in a regular Coastfile project name (validated upstream), so a
/// regular instance image can never collide with an SSG entry.
pub fn keystore_image_key(project: &str) -> String {
    format!("ssg:{project}")
}

/// Outcome of [`extract_ssg_secrets`]. Mirrors the shape of
/// `SecretExtractionOutput` in the regular daemon — kept narrow so
/// the orchestrator caller can fold the warnings into the final
/// response without repeating field plumbing.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SsgSecretExtractionOutput {
    pub secrets_extracted: usize,
    pub warnings: Vec<String>,
}

/// Run every `[secrets.<name>]` extractor declared in `cf` and
/// persist the encrypted results into the shared keystore.
///
/// `step` is the build-pipeline step number to use in the emitted
/// `Extracting secrets` `started` event; `total` is the total step
/// count (already accounting for this step via
/// [`crate::daemon_integration::total_steps`] when secrets are
/// present).
///
/// Returns an `SsgSecretExtractionOutput` regardless of whether the
/// keystore could be opened; per-secret failures and keystore-open
/// failures are recorded as warnings rather than propagated as
/// errors. This matches the regular build's behaviour and keeps a
/// failed extractor (e.g. macOS keychain locked) from blocking the
/// rest of the build.
pub async fn extract_ssg_secrets(
    project: &str,
    cf: &SsgCoastfile,
    progress: &Sender<BuildProgressEvent>,
    step: u32,
    total: u32,
) -> SsgSecretExtractionOutput {
    let mut output = SsgSecretExtractionOutput::default();
    if cf.secrets.is_empty() {
        return output;
    }

    let _ = progress
        .send(BuildProgressEvent::started(
            "Extracting secrets",
            step,
            total,
        ))
        .await;

    // Resolve the keystore file paths the same way the regular
    // build does. `coast_home()` honors `COAST_HOME` so dev mode
    // (`~/.coast-dev/`) Just Works.
    let home = match coast_home() {
        Ok(p) => p,
        Err(e) => {
            let _ = progress
                .send(
                    BuildProgressEvent::done("Extracting secrets", "fail")
                        .with_verbose(e.to_string()),
                )
                .await;
            output
                .warnings
                .push(format!("failed to resolve coast home for keystore: {e}"));
            return output;
        }
    };
    let keystore_db_path = home.join("keystore.db");
    let keystore_key_path = home.join("keystore.key");
    let image_key = keystore_image_key(project);

    let keystore =
        match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
            Ok(k) => k,
            Err(e) => {
                let _ = progress
                    .send(
                        BuildProgressEvent::done("Extracting secrets", "fail")
                            .with_verbose(e.to_string()),
                    )
                    .await;
                output.warnings.push(format!(
                    "failed to open keystore: {e}. SSG secrets will not be stored."
                ));
                return output;
            }
        };

    // Wipe existing rows for this project so a removed `[secrets]`
    // entry on rebuild doesn't leave a stale ghost in the keystore.
    if let Err(e) = keystore.delete_secrets_for_image(&image_key) {
        output.warnings.push(format!(
            "failed to clear old SSG secrets for '{image_key}': {e}"
        ));
    }

    let registry = coast_secrets::extractor::ExtractorRegistry::with_builtins();

    for sec in &cf.secrets {
        // Resolve relative `path = "..."` params against the
        // Coastfile's project_root (mirrors the regular build:
        // `coast-daemon/src/handlers/build/secrets.rs:46-53`).
        // Other params pass through verbatim.
        let mut resolved_params = sec.params.clone();
        if let Some(path_value) = resolved_params.get("path") {
            let p = std::path::Path::new(path_value);
            if p.is_relative() {
                let abs = cf.project_root.join(p);
                resolved_params.insert("path".to_string(), abs.to_string_lossy().to_string());
            }
        }

        let inject_label = match &sec.inject {
            InjectType::Env(name) => name.clone(),
            InjectType::File(path) => path.display().to_string(),
        };

        match registry.extract(&sec.extractor, &resolved_params) {
            Ok(value) => {
                let value_bytes = value.as_bytes().to_vec();
                let (inject_type_str, inject_target_str) = match &sec.inject {
                    InjectType::Env(name) => ("env", name.as_str()),
                    InjectType::File(path) => ("file", path.to_str().unwrap_or("")),
                };
                let ttl_seconds = sec.ttl.as_deref().and_then(parse_ttl_to_seconds);

                if let Err(e) = keystore.store_secret(&coast_secrets::keystore::StoreSecretParams {
                    coast_image: &image_key,
                    secret_name: &sec.name,
                    value: &value_bytes,
                    inject_type: inject_type_str,
                    inject_target: inject_target_str,
                    extractor: &sec.extractor,
                    ttl_seconds,
                }) {
                    let _ = progress
                        .send(
                            BuildProgressEvent::item(
                                "Extracting secrets",
                                format!("{} -> {}", sec.extractor, inject_label),
                                "warn",
                            )
                            .with_verbose(format!("Failed to store: {e}")),
                        )
                        .await;
                    output
                        .warnings
                        .push(format!("failed to store secret '{}': {e}", sec.name));
                } else {
                    output.secrets_extracted += 1;
                    let _ = progress
                        .send(BuildProgressEvent::item(
                            "Extracting secrets",
                            format!("{} -> {}", sec.extractor, inject_label),
                            "ok",
                        ))
                        .await;
                }
            }
            Err(e) => {
                let _ = progress
                    .send(
                        BuildProgressEvent::item(
                            "Extracting secrets",
                            format!("{} -> {}", sec.extractor, inject_label),
                            "fail",
                        )
                        .with_verbose(e.to_string()),
                    )
                    .await;
                output.warnings.push(format!(
                    "failed to extract secret '{}' using extractor '{}': {e}",
                    sec.name, sec.extractor
                ));
            }
        }
    }

    let summary = format!("{} extracted", output.secrets_extracted);
    let _ = progress
        .send(BuildProgressEvent::done("Extracting secrets", &summary))
        .await;

    output
}

/// Pure helper. Inlined from `coast-daemon::handlers::build::utils`
/// so `coast-ssg` doesn't depend on `coast-daemon` (which would
/// create a cycle — coast-daemon already depends on coast-ssg).
fn parse_ttl_to_seconds(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix('s') {
        num.trim().parse::<i64>().ok()
    } else if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<i64>().ok().map(|n| n * 60)
    } else if let Some(num) = s.strip_suffix('h') {
        num.trim().parse::<i64>().ok().map(|n| n * 3600)
    } else if let Some(num) = s.strip_suffix('d') {
        num.trim().parse::<i64>().ok().map(|n| n * 86400)
    } else {
        s.parse::<i64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystore_image_key_format_is_stable() {
        assert_eq!(keystore_image_key("cg"), "ssg:cg");
        assert_eq!(keystore_image_key("my-app"), "ssg:my-app");
    }

    #[test]
    fn parse_ttl_to_seconds_supports_suffixes() {
        assert_eq!(parse_ttl_to_seconds("45"), Some(45));
        assert_eq!(parse_ttl_to_seconds("45s"), Some(45));
        assert_eq!(parse_ttl_to_seconds("2m"), Some(120));
        assert_eq!(parse_ttl_to_seconds("3h"), Some(10800));
        assert_eq!(parse_ttl_to_seconds("1d"), Some(86400));
        assert_eq!(parse_ttl_to_seconds(""), None);
        assert_eq!(parse_ttl_to_seconds("garbage"), None);
    }

    #[tokio::test]
    async fn extract_ssg_secrets_returns_empty_when_no_secrets() {
        // Skips both the started/done events and the keystore work.
        let cf = SsgCoastfile::parse(
            r#"
[shared_services.postgres]
image = "postgres:16"
"#,
            std::path::Path::new("/tmp"),
        )
        .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<BuildProgressEvent>(8);
        let out = extract_ssg_secrets("p", &cf, &tx, 4, 7).await;
        assert_eq!(out.secrets_extracted, 0);
        assert!(out.warnings.is_empty());
        // No events should have been emitted.
        assert!(rx.try_recv().is_err());
    }
}
