/// Handler for the `coast rerun-extractors` command.
///
/// Re-runs secret extraction using a cached Coastfile from the build artifact
/// and applies refreshed secrets to all matching instances.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{
    BuildProgressEvent, RerunExtractorsRequest, RerunExtractorsResponse, StartRequest, StopRequest,
};
use coast_core::types::{InjectType, InstanceStatus};
use coast_docker::runtime::Runtime;
use coast_secrets::inject::ResolvedSecret;

use crate::server::AppState;

/// Send a progress event, ignoring send errors (client may have disconnected).
fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

fn project_images_dir(home: &Path, project: &str) -> PathBuf {
    home.join(".coast").join("images").join(project)
}

fn artifact_coastfile_path(home: &Path, project: &str, build_id: Option<&str>) -> PathBuf {
    let mut base = project_images_dir(home, project);
    if let Some(build_id) = build_id {
        base = base.join(build_id);
    } else {
        base = base.join("latest");
    }
    base.join("coastfile.toml")
}

fn resolve_cached_coastfile_path(
    project: &str,
    requested_build_id: Option<&str>,
) -> Result<(PathBuf, Option<String>)> {
    let home = dirs::home_dir().ok_or_else(|| {
        CoastError::io_simple("cannot determine home directory. Set $HOME and try again.")
    })?;

    // Explicit build target takes precedence and must exist.
    if let Some(build_id) = requested_build_id {
        let path = artifact_coastfile_path(&home, project, Some(build_id));
        if !path.exists() {
            return Err(CoastError::state(format!(
                "Build '{}' not found for project '{}'. Run `coast builds ls --project {}` to inspect available build IDs.",
                build_id, project, project
            )));
        }
        return Ok((path, Some(build_id.to_string())));
    }

    // Default path: resolve the `latest` symlink target so we can filter instances by build_id.
    if let Some(latest_build_id) = crate::handlers::run::resolve_latest_build_id(project, None) {
        let path = artifact_coastfile_path(&home, project, Some(&latest_build_id));
        if path.exists() {
            return Ok((path, Some(latest_build_id)));
        }
    }

    // Fallback to legacy latest path and legacy flat layout.
    let latest_path = artifact_coastfile_path(&home, project, None);
    if latest_path.exists() {
        return Ok((latest_path, None));
    }
    let legacy_path = project_images_dir(&home, project).join("coastfile.toml");
    if legacy_path.exists() {
        return Ok((legacy_path, None));
    }

    Err(CoastError::state(format!(
        "No build found for project '{}'. Run `coast build` first.",
        project
    )))
}

fn instance_matches_build(
    instance: &coast_core::types::CoastInstance,
    build_id: Option<&str>,
) -> bool {
    match build_id {
        Some(build_id) => instance.build_id.as_deref() == Some(build_id),
        None => true,
    }
}

fn load_resolved_secrets_for_instance(
    keystore: &coast_secrets::keystore::Keystore,
    base_secret_key: &str,
    project: &str,
    instance_name: &str,
) -> Result<Vec<ResolvedSecret>> {
    let mut by_name: BTreeMap<String, coast_secrets::keystore::StoredSecret> = BTreeMap::new();

    let base_secrets = keystore.get_all_secrets(base_secret_key)?;
    for secret in base_secrets {
        by_name.insert(secret.secret_name.clone(), secret);
    }

    let override_key = format!("{project}/{instance_name}");
    let override_secrets = keystore.get_all_secrets(&override_key)?;
    for secret in override_secrets {
        by_name.insert(secret.secret_name.clone(), secret);
    }

    Ok(by_name
        .into_values()
        .map(|secret| ResolvedSecret {
            name: secret.secret_name,
            inject_type: secret.inject_type,
            inject_target: secret.inject_target,
            value: secret.value,
        })
        .collect())
}

fn shell_quote_single(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn has_compose_for_build(project: &str, build_id: Option<&str>) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let preferred = artifact_coastfile_path(&home, project, build_id);
    if preferred.exists() {
        return coast_core::coastfile::Coastfile::from_file(&preferred)
            .ok()
            .map(|cf| cf.compose.is_some())
            .unwrap_or(true);
    }

    let legacy = project_images_dir(&home, project).join("coastfile.toml");
    if legacy.exists() {
        return coast_core::coastfile::Coastfile::from_file(&legacy)
            .ok()
            .map(|cf| cf.compose.is_some())
            .unwrap_or(true);
    }

    true
}

/// Base64-encode a byte slice using the standard alphabet.
fn base64_encode_bytes(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 0x03) << 4 | b1 >> 4) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((b1 & 0x0f) << 2 | b2 >> 6) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

async fn apply_secrets_to_container(
    state: &AppState,
    project: &str,
    instance_name: &str,
    container_id: &str,
    build_id: Option<&str>,
    should_restart_compose: bool,
    resolved_secrets: &[ResolvedSecret],
) -> Result<()> {
    let docker = state
        .docker
        .as_ref()
        .ok_or_else(|| CoastError::docker("Docker client is not available"))?;
    let runtime = coast_docker::dind::DindRuntime::with_client(docker.clone());

    let home = dirs::home_dir().ok_or_else(|| {
        CoastError::io_simple("cannot determine home directory. Set $HOME and try again.")
    })?;
    let tmpfs_base = home
        .join(".coast")
        .join("secrets-tmpfs")
        .join(instance_name);
    let plan = coast_secrets::inject::build_injection_plan(resolved_secrets, &tmpfs_base)?;

    let values_by_name: HashMap<String, Vec<u8>> = resolved_secrets
        .iter()
        .map(|secret| (secret.name.clone(), secret.value.clone()))
        .collect();

    // File-injected secrets: write refreshed content directly into the DinD container.
    for file_mount in &plan.file_mounts {
        let secret_name = file_mount
            .host_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let Some(secret_bytes) = values_by_name.get(secret_name) else {
            continue;
        };
        let container_path = file_mount.container_path.to_string_lossy().to_string();
        let parent = file_mount
            .container_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string());
        let b64 = base64_encode_bytes(secret_bytes);
        let cmd = format!(
            "mkdir -p {parent} && echo {b64} | base64 -d > {path}",
            parent = shell_quote_single(&parent),
            b64 = shell_quote_single(&b64),
            path = shell_quote_single(&container_path),
        );
        let result = runtime
            .exec_in_coast(container_id, &["sh", "-c", &cmd])
            .await?;
        if !result.success() {
            return Err(CoastError::docker(format!(
                "Failed to write refreshed file secret in instance '{}': {}",
                instance_name, result.stderr
            )));
        }
    }

    // Env-injected secrets: restart compose with exported values so interpolation picks
    // up the refreshed values for recreated service containers.
    if should_restart_compose {
        let mut compose_cmd = crate::handlers::compose_context_for_build(project, build_id)
            .compose_shell("up -d --force-recreate --remove-orphans");
        if !plan.env_vars.is_empty()
            && compose_cmd.len() == 3
            && compose_cmd[0] == "sh"
            && compose_cmd[1] == "-c"
        {
            let mut keys: Vec<&String> = plan.env_vars.keys().collect();
            keys.sort();
            let mut exports = String::new();
            for key in keys {
                if !is_valid_env_name(key) {
                    warn!(
                        key = %key,
                        "skipping invalid env var name during secret apply"
                    );
                    continue;
                }
                if let Some(value) = plan.env_vars.get(key) {
                    exports.push_str("export ");
                    exports.push_str(key);
                    exports.push('=');
                    exports.push_str(&shell_quote_single(value));
                    exports.push_str("; ");
                }
            }
            compose_cmd[2] = format!("{exports}{}", compose_cmd[2]);
        }

        let compose_refs: Vec<&str> = compose_cmd
            .iter()
            .map(std::string::String::as_str)
            .collect();
        let compose_result = runtime.exec_in_coast(container_id, &compose_refs).await?;
        if !compose_result.success() {
            return Err(CoastError::docker(format!(
                "Failed to recreate compose services in instance '{}': {}",
                instance_name, compose_result.stderr
            )));
        }
    }

    Ok(())
}

async fn apply_refreshed_secrets_to_instance(
    state: &AppState,
    project: &str,
    instance_name: &str,
    status: InstanceStatus,
    container_id: Option<String>,
    build_id: Option<&str>,
    has_compose: bool,
    resolved_secrets: &[ResolvedSecret],
) -> Result<()> {
    match status {
        InstanceStatus::Running | InstanceStatus::CheckedOut | InstanceStatus::Idle => {
            let container_id = container_id.ok_or_else(|| {
                CoastError::state(format!(
                    "Instance '{}' has no container ID; cannot apply refreshed secrets.",
                    instance_name
                ))
            })?;
            let should_restart_compose = has_compose && status != InstanceStatus::Idle;
            apply_secrets_to_container(
                state,
                project,
                instance_name,
                &container_id,
                build_id,
                should_restart_compose,
                resolved_secrets,
            )
            .await
        }
        InstanceStatus::Stopped => {
            crate::handlers::start::handle(
                StartRequest {
                    name: instance_name.to_string(),
                    project: project.to_string(),
                },
                state,
                None,
            )
            .await?;

            let started_instance = {
                let db = state.db.lock().await;
                db.get_instance(project, instance_name)?
                    .ok_or_else(|| CoastError::state("instance disappeared during secret apply"))?
            };
            let started_container_id = started_instance.container_id.clone().ok_or_else(|| {
                CoastError::state(format!(
                    "Instance '{}' has no container ID after start; cannot apply refreshed secrets.",
                    instance_name
                ))
            })?;
            let should_restart_compose =
                has_compose && started_instance.status != InstanceStatus::Idle;
            let apply_result = apply_secrets_to_container(
                state,
                project,
                instance_name,
                &started_container_id,
                started_instance.build_id.as_deref(),
                should_restart_compose,
                resolved_secrets,
            )
            .await;

            let stop_result = crate::handlers::stop::handle(
                StopRequest {
                    name: instance_name.to_string(),
                    project: project.to_string(),
                },
                state,
                None,
            )
            .await;

            match (apply_result, stop_result) {
                (Ok(()), Ok(_)) => Ok(()),
                (Err(apply_err), Ok(_)) => Err(apply_err),
                (Ok(()), Err(stop_err)) => Err(stop_err),
                (Err(apply_err), Err(stop_err)) => Err(CoastError::state(format!(
                    "Failed applying refreshed secrets to '{}': {}. Also failed to return instance to stopped state: {}",
                    instance_name, apply_err, stop_err
                ))),
            }
        }
        other => Err(CoastError::state(format!(
            "Instance '{}' is currently {} and cannot be refreshed right now.",
            instance_name, other
        ))),
    }
}

/// Resolve secret params, converting relative paths to absolute under `project_root`.
fn resolve_secret_params(
    params: &HashMap<String, String>,
    project_root: &Path,
) -> HashMap<String, String> {
    let mut resolved = params.clone();
    if let Some(path) = resolved.get("path") {
        let p = Path::new(path);
        if p.is_relative() {
            let abs = project_root.join(p);
            resolved.insert("path".to_string(), abs.to_string_lossy().to_string());
        }
    }
    resolved
}

/// Open the keystore, extract each secret from the coastfile, and store results.
///
/// Returns `(secrets_extracted, warnings, step_status)`.
async fn extract_and_store_secrets(
    coastfile: &coast_core::coastfile::Coastfile,
    keystore_db_path: &Path,
    keystore_key_path: &Path,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> (usize, Vec<String>, &'static str) {
    let mut warnings = Vec::new();
    let mut secrets_extracted = 0usize;
    let mut step_status = "ok";

    match coast_secrets::keystore::Keystore::open(keystore_db_path, keystore_key_path) {
        Ok(keystore) => {
            if let Err(e) = keystore.delete_secrets_for_image(&coastfile.name) {
                warnings.push(format!(
                    "Failed to clear old secrets for '{}': {}",
                    coastfile.name, e
                ));
            }

            let registry = coast_secrets::extractor::ExtractorRegistry::with_builtins();
            for secret_config in &coastfile.secrets {
                let resolved_params =
                    resolve_secret_params(&secret_config.params, &coastfile.project_root);

                let inject_target = match &secret_config.inject {
                    InjectType::Env(name) => name.clone(),
                    InjectType::File(path) => path.display().to_string(),
                };

                match registry.extract(&secret_config.extractor, &resolved_params) {
                    Ok(value) => {
                        let value_bytes = value.as_bytes().to_vec();
                        let (inject_type_str, inject_target_str) = match &secret_config.inject {
                            InjectType::Env(name) => ("env", name.as_str()),
                            InjectType::File(path) => ("file", path.to_str().unwrap_or("")),
                        };
                        let ttl_seconds =
                            secret_config.ttl.as_deref().and_then(parse_ttl_to_seconds);
                        if let Err(e) =
                            keystore.store_secret(&coast_secrets::keystore::StoreSecretParams {
                                coast_image: &coastfile.name,
                                secret_name: &secret_config.name,
                                value: &value_bytes,
                                inject_type: inject_type_str,
                                inject_target: inject_target_str,
                                extractor: &secret_config.extractor,
                                ttl_seconds,
                            })
                        {
                            emit(
                                progress,
                                BuildProgressEvent::item(
                                    "Extracting secrets",
                                    format!("{} -> {}", secret_config.extractor, inject_target),
                                    "warn",
                                )
                                .with_verbose(format!("Failed to store: {e}")),
                            );
                            step_status = "warn";
                            warnings.push(format!(
                                "Failed to store secret '{}': {}",
                                secret_config.name, e
                            ));
                        } else {
                            secrets_extracted += 1;
                            emit(
                                progress,
                                BuildProgressEvent::item(
                                    "Extracting secrets",
                                    format!("{} -> {}", secret_config.extractor, inject_target),
                                    "ok",
                                ),
                            );
                        }
                    }
                    Err(e) => {
                        emit(
                            progress,
                            BuildProgressEvent::item(
                                "Extracting secrets",
                                format!("{} -> {}", secret_config.extractor, inject_target),
                                "fail",
                            )
                            .with_verbose(e.to_string()),
                        );
                        step_status = "warn";
                        warnings.push(format!(
                            "Failed to extract secret '{}' using extractor '{}': {}",
                            secret_config.name, secret_config.extractor, e
                        ));
                    }
                }
            }
        }
        Err(e) => {
            emit(
                progress,
                BuildProgressEvent::done("Extracting secrets", "fail").with_verbose(e.to_string()),
            );
            step_status = "fail";
            warnings.push(format!(
                "Failed to open keystore: {}. Secrets were not stored.",
                e
            ));
        }
    }

    (secrets_extracted, warnings, step_status)
}

/// Load resolved secrets from the keystore and apply them to all matching instances.
///
/// Returns `(warnings, step_status)`.
async fn apply_secrets_to_instances(
    state: &AppState,
    coastfile: &coast_core::coastfile::Coastfile,
    project: &str,
    target_build_id: Option<&str>,
    keystore_db_path: &Path,
    keystore_key_path: &Path,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<(Vec<String>, &'static str)> {
    let mut warnings = Vec::new();
    let mut step_status = "ok";

    if target_build_id.is_none() {
        step_status = "warn";
        warnings.push(
            "Could not resolve a specific latest build_id; applying refreshed secrets to all instances in the project."
                .to_string(),
        );
    }

    let target_instances = {
        let db = state.db.lock().await;
        db.list_instances_for_project(project)?
            .into_iter()
            .filter(|instance| instance_matches_build(instance, target_build_id))
            .collect::<Vec<_>>()
    };

    if target_instances.is_empty() {
        emit(
            progress,
            BuildProgressEvent::item(
                "Applying refreshed secrets",
                "no matching instances",
                "skip",
            ),
        );
        return Ok((warnings, "skip"));
    }

    if state.docker.is_none() {
        warnings.push(
            "Docker is not available; refreshed secrets were stored but could not be applied to instances."
                .to_string(),
        );
        return Ok((warnings, "warn"));
    }

    let instance_secrets: Option<HashMap<String, Vec<ResolvedSecret>>> =
        match coast_secrets::keystore::Keystore::open(keystore_db_path, keystore_key_path) {
            Ok(keystore) => {
                let mut per_instance = HashMap::new();
                for instance in &target_instances {
                    match load_resolved_secrets_for_instance(
                        &keystore,
                        &coastfile.name,
                        project,
                        &instance.name,
                    ) {
                        Ok(resolved) => {
                            per_instance.insert(instance.name.clone(), resolved);
                        }
                        Err(e) => {
                            step_status = "warn";
                            warnings.push(format!(
                                "Failed loading merged secrets for instance '{}': {}",
                                instance.name, e
                            ));
                        }
                    }
                }
                Some(per_instance)
            }
            Err(e) => {
                step_status = "warn";
                warnings.push(format!(
                    "Failed to reopen keystore for applying refreshed secrets: {}",
                    e
                ));
                None
            }
        };

    if let Some(instance_secrets) = instance_secrets {
        for instance in target_instances {
            let resolved = instance_secrets
                .get(&instance.name)
                .cloned()
                .unwrap_or_default();
            if resolved.is_empty() {
                emit(
                    progress,
                    BuildProgressEvent::item(
                        "Applying refreshed secrets",
                        format!("{} (no injected secrets)", instance.name),
                        "skip",
                    ),
                );
                continue;
            }

            emit(
                progress,
                BuildProgressEvent::item(
                    "Applying refreshed secrets",
                    format!("{} (status: {})", instance.name, instance.status),
                    "started",
                ),
            );

            let has_compose = has_compose_for_build(project, instance.build_id.as_deref());
            match apply_refreshed_secrets_to_instance(
                state,
                project,
                &instance.name,
                instance.status,
                instance.container_id,
                instance.build_id.as_deref(),
                has_compose,
                &resolved,
            )
            .await
            {
                Ok(()) => {
                    emit(
                        progress,
                        BuildProgressEvent::item(
                            "Applying refreshed secrets",
                            format!("{} applied", instance.name),
                            "ok",
                        ),
                    );
                }
                Err(e) => {
                    step_status = "warn";
                    warnings.push(format!(
                        "Failed applying refreshed secrets to instance '{}': {}",
                        instance.name, e
                    ));
                    emit(
                        progress,
                        BuildProgressEvent::item(
                            "Applying refreshed secrets",
                            format!("{} failed", instance.name),
                            "warn",
                        )
                        .with_verbose(e.to_string()),
                    );
                }
            }
        }
    }

    Ok((warnings, step_status))
}

/// Handle a rerun-extractors request.
pub async fn handle(
    req: RerunExtractorsRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<RerunExtractorsResponse> {
    info!(
        project = %req.project,
        build_id = ?req.build_id,
        "handling rerun-extractors request"
    );

    let plan = vec![
        "Resolving cached Coastfile".to_string(),
        "Extracting secrets".to_string(),
        "Applying refreshed secrets".to_string(),
    ];
    let total_steps = plan.len() as u32;
    emit(&progress, BuildProgressEvent::build_plan(plan));

    // Step 1: Resolve cached Coastfile.
    emit(
        &progress,
        BuildProgressEvent::started("Resolving cached Coastfile", 1, total_steps),
    );
    let (coastfile_path, resolved_build_id) =
        resolve_cached_coastfile_path(&req.project, req.build_id.as_deref())?;
    let coastfile = coast_core::coastfile::Coastfile::from_file(&coastfile_path)?;
    emit(
        &progress,
        BuildProgressEvent::done("Resolving cached Coastfile", "ok")
            .with_verbose(coastfile_path.display().to_string()),
    );

    let home = dirs::home_dir().ok_or_else(|| {
        CoastError::io_simple("cannot determine home directory. Set $HOME and try again.")
    })?;
    let keystore_db_path = home.join(".coast").join("keystore.db");
    let keystore_key_path = home.join(".coast").join("keystore.key");

    // Step 2: Extract secrets.
    emit(
        &progress,
        BuildProgressEvent::started("Extracting secrets", 2, total_steps),
    );
    if coastfile.secrets.is_empty() {
        emit(
            &progress,
            BuildProgressEvent::skip("Extracting secrets", 2, total_steps),
        );
        emit(
            &progress,
            BuildProgressEvent::skip("Applying refreshed secrets", 3, total_steps),
        );
        return Ok(RerunExtractorsResponse {
            project: req.project,
            secrets_extracted: 0,
            warnings: Vec::new(),
        });
    }

    let (secrets_extracted, extract_warnings, extraction_step_status) =
        extract_and_store_secrets(&coastfile, &keystore_db_path, &keystore_key_path, &progress)
            .await;
    let mut warnings = extract_warnings;
    emit(
        &progress,
        BuildProgressEvent::done("Extracting secrets", extraction_step_status),
    );

    // Step 3: Apply refreshed secrets to instances.
    emit(
        &progress,
        BuildProgressEvent::started("Applying refreshed secrets", 3, total_steps),
    );
    let (apply_warnings, apply_step_status) = apply_secrets_to_instances(
        state,
        &coastfile,
        &req.project,
        resolved_build_id.as_deref(),
        &keystore_db_path,
        &keystore_key_path,
        &progress,
    )
    .await?;
    warnings.extend(apply_warnings);
    emit(
        &progress,
        BuildProgressEvent::done("Applying refreshed secrets", apply_step_status),
    );

    Ok(RerunExtractorsResponse {
        project: req.project,
        secrets_extracted,
        warnings,
    })
}

/// Parse a TTL duration string (e.g., "1h", "30m", "3600s", "3600") into seconds.
///
/// Supported suffixes: `s` (seconds), `m` (minutes), `h` (hours), `d` (days).
/// If no suffix is provided, the value is treated as seconds.
/// Returns `None` if the string cannot be parsed.
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
    use coast_core::types::{CoastInstance, RuntimeType};

    fn sample_instance(
        name: &str,
        build_id: Option<&str>,
        status: InstanceStatus,
    ) -> CoastInstance {
        CoastInstance {
            name: name.to_string(),
            project: "my-app".to_string(),
            status,
            branch: Some("main".to_string()),
            commit_sha: None,
            container_id: Some("container-1".to_string()),
            runtime: RuntimeType::Dind,
            created_at: chrono::Utc::now(),
            worktree_name: None,
            build_id: build_id.map(str::to_string),
            coastfile_type: None,
        }
    }

    #[test]
    fn test_parse_ttl_to_seconds() {
        assert_eq!(parse_ttl_to_seconds(""), None);
        assert_eq!(parse_ttl_to_seconds("3600"), Some(3600));
        assert_eq!(parse_ttl_to_seconds("30s"), Some(30));
        assert_eq!(parse_ttl_to_seconds("2m"), Some(120));
        assert_eq!(parse_ttl_to_seconds("1h"), Some(3600));
        assert_eq!(parse_ttl_to_seconds("1d"), Some(86400));
        assert_eq!(parse_ttl_to_seconds("bad"), None);
    }

    #[test]
    fn test_artifact_coastfile_path_latest() {
        let home = Path::new("/tmp/home");
        let path = artifact_coastfile_path(home, "my-app", None);
        assert_eq!(
            path,
            Path::new("/tmp/home/.coast/images/my-app/latest/coastfile.toml")
        );
    }

    #[test]
    fn test_artifact_coastfile_path_build_id() {
        let home = Path::new("/tmp/home");
        let path = artifact_coastfile_path(home, "my-app", Some("a3c7d783"));
        assert_eq!(
            path,
            Path::new("/tmp/home/.coast/images/my-app/a3c7d783/coastfile.toml")
        );
    }

    #[test]
    fn test_instance_matches_build() {
        let inst = sample_instance("dev-1", Some("a3c7d783"), InstanceStatus::Running);
        assert!(instance_matches_build(&inst, Some("a3c7d783")));
        assert!(!instance_matches_build(&inst, Some("other-build")));
        assert!(instance_matches_build(&inst, None));
    }

    #[test]
    fn test_instance_matches_build_includes_stopped() {
        let stopped = sample_instance("dev-2", Some("a3c7d783"), InstanceStatus::Stopped);
        assert!(instance_matches_build(&stopped, Some("a3c7d783")));
    }

    #[test]
    fn test_shell_quote_single() {
        assert_eq!(shell_quote_single("abc"), "'abc'");
        assert_eq!(shell_quote_single("a'b"), "'a'\"'\"'b'");
    }

    #[test]
    fn test_is_valid_env_name() {
        assert!(is_valid_env_name("API_KEY"));
        assert!(is_valid_env_name("_SECRET"));
        assert!(!is_valid_env_name(""));
        assert!(!is_valid_env_name("1BAD"));
        assert!(!is_valid_env_name("BAD-NAME"));
    }

    #[test]
    fn test_resolve_secret_params_relative_path() {
        let mut params = HashMap::new();
        params.insert("path".to_string(), "secrets/api.key".to_string());
        let root = Path::new("/home/user/my-project");
        let resolved = resolve_secret_params(&params, root);
        assert_eq!(
            resolved.get("path").unwrap(),
            "/home/user/my-project/secrets/api.key"
        );
    }

    #[test]
    fn test_resolve_secret_params_absolute_path() {
        let mut params = HashMap::new();
        params.insert("path".to_string(), "/etc/secrets/api.key".to_string());
        let root = Path::new("/home/user/my-project");
        let resolved = resolve_secret_params(&params, root);
        assert_eq!(resolved.get("path").unwrap(), "/etc/secrets/api.key");
    }

    #[test]
    fn test_resolve_secret_params_no_path() {
        let mut params = HashMap::new();
        params.insert("key".to_string(), "value".to_string());
        let root = Path::new("/home/user/my-project");
        let resolved = resolve_secret_params(&params, root);
        assert_eq!(resolved.get("key").unwrap(), "value");
        assert!(!resolved.contains_key("path"));
    }
}
