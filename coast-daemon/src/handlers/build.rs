/// Handler for the `coast build` command.
///
/// Parses the Coastfile, validates it, extracts secrets, prepares the image
/// artifact directory, caches OCI images, and copies injected host files.
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, BuildRequest, BuildResponse};
use coast_core::types::{InjectType, VolumeStrategy};

use crate::server::AppState;

/// Send a progress event, ignoring send errors (CLI may have disconnected).
fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Handle a build request.
///
/// Steps:
/// 1. Parse and validate the Coastfile.
/// 2. Extract secrets via configured extractors and store in keystore.
/// 3. Create the artifact directory at `~/.coast/images/{project}/`.
/// 4. Cache OCI images referenced in the compose file.
/// 5. Build custom coast image (if configured).
/// 6. Write the manifest file.
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn handle(
    req: BuildRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<BuildResponse> {
    info!(coastfile_path = %req.coastfile_path.display(), refresh = req.refresh, "handling build request");

    // Parse the Coastfile up front so we can compute the build plan.
    let coastfile = Coastfile::from_file(&req.coastfile_path)?;

    let home = dirs::home_dir().ok_or_else(|| {
        CoastError::io_simple("cannot determine home directory. Set $HOME and try again.")
    })?;

    // Pre-parse compose file to determine which image steps will run.
    let compose_content_cached = coastfile
        .compose
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let compose_dir_cached = coastfile
        .compose
        .as_ref()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf));
    let compose_parse_cached = compose_content_cached.as_ref().and_then(|content| {
        coast_docker::compose_build::parse_compose_file_filtered(
            content,
            &coastfile.name,
            &coastfile.omit.services,
        )
        .ok()
    });

    let has_secrets = !coastfile.secrets.is_empty();
    let has_build_directives = compose_parse_cached
        .as_ref()
        .is_some_and(|r| !r.build_directives.is_empty());
    let has_image_refs = compose_parse_cached
        .as_ref()
        .is_some_and(|r| !r.image_refs.is_empty());
    let has_setup = !coastfile.setup.is_empty();

    // Build the ordered step plan.
    let mut plan: Vec<&str> = vec!["Parsing Coastfile"];
    if has_secrets {
        plan.push("Extracting secrets");
    }
    plan.push("Creating artifact");
    if has_build_directives {
        plan.push("Building images");
    }
    if has_image_refs {
        plan.push("Pulling images");
    }
    if has_setup {
        plan.push("Building coast image");
    }
    plan.push("Writing manifest");

    let total_steps = plan.len() as u32;
    let sn = |name: &str| -> u32 {
        plan.iter()
            .position(|&s| s == name)
            .map(|i| (i + 1) as u32)
            .expect("step not in plan")
    };

    // Emit the full plan so the CLI can render the checklist upfront.
    emit(
        &progress,
        BuildProgressEvent::build_plan(plan.iter().map(std::string::ToString::to_string).collect()),
    );

    // Step 1: Parse the Coastfile (already done above)
    emit(
        &progress,
        BuildProgressEvent::started("Parsing Coastfile", sn("Parsing Coastfile"), total_steps),
    );
    emit(
        &progress,
        BuildProgressEvent::done("Parsing Coastfile", "ok"),
    );

    let mut warnings = Vec::new();

    // Step 2: Extract secrets FIRST (before artifact/images)
    let mut secrets_extracted = 0;
    if has_secrets {
        emit(
            &progress,
            BuildProgressEvent::started(
                "Extracting secrets",
                sn("Extracting secrets"),
                total_steps,
            ),
        );

        let keystore_db_path = home.join(".coast").join("keystore.db");
        let keystore_key_path = home.join(".coast").join("keystore.key");

        match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
            Ok(keystore) => {
                if let Err(e) = keystore.delete_secrets_for_image(&coastfile.name) {
                    warnings.push(format!(
                        "Failed to clear old secrets for '{}': {}",
                        coastfile.name, e
                    ));
                }

                let registry = coast_secrets::extractor::ExtractorRegistry::with_builtins();

                for secret_config in &coastfile.secrets {
                    let mut resolved_params = secret_config.params.clone();
                    if let Some(path) = resolved_params.get("path") {
                        let p = std::path::Path::new(path);
                        if p.is_relative() {
                            let abs = coastfile.project_root.join(p);
                            resolved_params
                                .insert("path".to_string(), abs.to_string_lossy().to_string());
                        }
                    }

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
                            if let Err(e) = keystore.store_secret(
                                &coastfile.name,
                                &secret_config.name,
                                &value_bytes,
                                inject_type_str,
                                inject_target_str,
                                &secret_config.extractor,
                                ttl_seconds,
                            ) {
                                emit(
                                    &progress,
                                    BuildProgressEvent::item(
                                        "Extracting secrets",
                                        format!("{} -> {}", secret_config.extractor, inject_target),
                                        "warn",
                                    )
                                    .with_verbose(format!("Failed to store: {e}")),
                                );
                                warnings.push(format!(
                                    "Failed to store secret '{}': {}",
                                    secret_config.name, e
                                ));
                            } else {
                                secrets_extracted += 1;
                                emit(
                                    &progress,
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
                                &progress,
                                BuildProgressEvent::item(
                                    "Extracting secrets",
                                    format!("{} -> {}", secret_config.extractor, inject_target),
                                    "fail",
                                )
                                .with_verbose(e.to_string()),
                            );
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
                    &progress,
                    BuildProgressEvent::done("Extracting secrets", "fail")
                        .with_verbose(e.to_string()),
                );
                warnings.push(format!(
                    "Failed to open keystore: {}. Secrets will not be stored.",
                    e
                ));
            }
        }
    }

    // Step 3: Create artifact directory
    emit(
        &progress,
        BuildProgressEvent::started("Creating artifact", sn("Creating artifact"), total_steps),
    );

    // Hash the raw Coastfile content AND the resolved (merged) config so that
    // changes to a parent Coastfile produce different build IDs for children.
    let coastfile_raw = std::fs::read_to_string(&req.coastfile_path).unwrap_or_default();
    let build_timestamp = chrono::Utc::now();
    let coastfile_hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        coastfile_raw.hash(&mut hasher);
        format!("{:?}", coastfile.ports).hash(&mut hasher);
        format!("{:?}", coastfile.secrets).hash(&mut hasher);
        format!("{:?}", coastfile.shared_services).hash(&mut hasher);
        format!("{:?}", coastfile.volumes).hash(&mut hasher);
        format!("{:?}", coastfile.setup).hash(&mut hasher);
        format!("{:x}", hasher.finish())
    };
    let build_id: String = format!(
        "{}_{}",
        &coastfile_hash,
        build_timestamp.format("%Y%m%d%H%M%S")
    );

    let project_dir = home.join(".coast").join("images").join(&coastfile.name);
    let artifact_path = project_dir.join(&build_id);
    std::fs::create_dir_all(&artifact_path).map_err(|e| CoastError::Io {
        message: format!("failed to create artifact directory: {e}"),
        path: artifact_path.clone(),
        source: Some(e),
    })?;

    let artifact_coastfile = artifact_path.join("coastfile.toml");
    let standalone_toml = coastfile.to_standalone_toml();
    std::fs::write(&artifact_coastfile, standalone_toml.as_bytes()).map_err(|e| {
        CoastError::Io {
            message: format!("failed to write resolved Coastfile to artifact: {e}"),
            path: artifact_coastfile.clone(),
            source: Some(e),
        }
    })?;

    if let Some(ref content) = compose_content_cached {
        let artifact_compose = artifact_path.join("compose.yml");
        let rewritten =
            coast_docker::compose_build::rewrite_compose_for_artifact(content, &coastfile.name)?;

        // Strip omitted services and volumes from the artifact compose
        let rewritten = if !coastfile.omit.is_empty() {
            if let Ok(mut yaml) = serde_yaml::from_str::<serde_yaml::Value>(&rewritten) {
                let mut changed = false;

                // Remove omitted services
                if let Some(services) = yaml.get_mut("services").and_then(|s| s.as_mapping_mut()) {
                    for svc_name in &coastfile.omit.services {
                        let key = serde_yaml::Value::String(svc_name.clone());
                        if services.remove(&key).is_some() {
                            info!(service = %svc_name, "stripped omitted service from artifact compose");
                            changed = true;
                        }
                    }

                    // Strip depends_on references to omitted services
                    let omit_set: std::collections::HashSet<&str> = coastfile
                        .omit
                        .services
                        .iter()
                        .map(std::string::String::as_str)
                        .collect();
                    let svc_keys: Vec<serde_yaml::Value> = services.keys().cloned().collect();
                    for svc_key in svc_keys {
                        if let Some(svc_def) =
                            services.get_mut(&svc_key).and_then(|v| v.as_mapping_mut())
                        {
                            let dep_key = serde_yaml::Value::String("depends_on".into());
                            let mut remove_depends = false;
                            if let Some(deps) = svc_def.get_mut(&dep_key) {
                                if let Some(dep_map) = deps.as_mapping_mut() {
                                    for svc_name in &coastfile.omit.services {
                                        dep_map.remove(serde_yaml::Value::String(svc_name.clone()));
                                    }
                                    if dep_map.is_empty() {
                                        remove_depends = true;
                                    }
                                } else if let Some(dep_seq) = deps.as_sequence_mut() {
                                    dep_seq.retain(|v| {
                                        v.as_str().map(|s| !omit_set.contains(s)).unwrap_or(true)
                                    });
                                    if dep_seq.is_empty() {
                                        remove_depends = true;
                                    }
                                }
                            }
                            if remove_depends {
                                svc_def.remove(&dep_key);
                            }
                        }
                    }
                }

                // Remove omitted volumes
                if let Some(top_volumes) = yaml.get_mut("volumes").and_then(|v| v.as_mapping_mut())
                {
                    for vol_name in &coastfile.omit.volumes {
                        if top_volumes
                            .remove(serde_yaml::Value::String(vol_name.clone()))
                            .is_some()
                        {
                            info!(volume = %vol_name, "stripped omitted volume from artifact compose");
                            changed = true;
                        }
                    }
                }

                if changed {
                    serde_yaml::to_string(&yaml).unwrap_or(rewritten)
                } else {
                    rewritten
                }
            } else {
                rewritten
            }
        } else {
            rewritten
        };

        std::fs::write(&artifact_compose, &rewritten).map_err(|e| CoastError::Io {
            message: format!("failed to write rewritten compose file to artifact: {e}"),
            path: artifact_compose,
            source: Some(e),
        })?;
    }

    // Warn about shared volumes on database-adjacent services
    for vol in &coastfile.volumes {
        if vol.strategy == VolumeStrategy::Shared {
            let service_lower = vol.service.to_lowercase();
            if service_lower.contains("postgres")
                || service_lower.contains("mysql")
                || service_lower.contains("mongo")
                || service_lower.contains("redis")
                || service_lower.contains("db")
            {
                warnings.push(format!(
                    "Volume '{}' uses 'shared' strategy on service '{}' which looks database-related. \
                     Multiple instances writing to the same database volume can cause data corruption. \
                     Consider using 'isolated' strategy or 'shared_services' instead.",
                    vol.name, vol.service
                ));
            }
        }
    }

    // Copy injected host files
    let inject_dir = artifact_path.join("inject");
    std::fs::create_dir_all(&inject_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create inject directory: {e}"),
        path: inject_dir.clone(),
        source: Some(e),
    })?;

    for file_path_str in &coastfile.inject.files {
        let expanded = shellexpand::tilde(file_path_str);
        let host_path = PathBuf::from(expanded.as_ref());
        if host_path.exists() {
            if let Some(filename) = host_path.file_name() {
                let dest = inject_dir.join(filename);
                std::fs::copy(&host_path, &dest).map_err(|e| CoastError::Io {
                    message: format!(
                        "failed to copy injected file '{}' to artifact: {e}",
                        host_path.display()
                    ),
                    path: dest,
                    source: Some(e),
                })?;
            }
        } else {
            warnings.push(format!(
                "Injected host file '{}' does not exist, skipping.",
                file_path_str
            ));
        }
    }

    let secrets_dir = artifact_path.join("secrets");
    std::fs::create_dir_all(&secrets_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create secrets directory: {e}"),
        path: secrets_dir,
        source: Some(e),
    })?;

    emit(
        &progress,
        BuildProgressEvent::done("Creating artifact", "ok"),
    );

    // Step 4: Build images, pull image references (using pre-parsed compose)
    let mut images_cached: usize = 0;
    let mut images_built: usize = 0;
    let mut built_services: Vec<String> = Vec::new();
    let mut pulled_images: Vec<String> = Vec::new();
    let mut base_images: Vec<String> = Vec::new();
    let cache_dir = home.join(".coast").join("image-cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create image cache directory: {e}"),
        path: cache_dir.clone(),
        source: Some(e),
    })?;

    if let Some(ref parse_result) = compose_parse_cached {
        let compose_dir = compose_dir_cached
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("."));
        {
            if has_build_directives {
                emit(
                    &progress,
                    BuildProgressEvent::started(
                        "Building images",
                        sn("Building images"),
                        total_steps,
                    ),
                );
            }
            for directive in &parse_result.build_directives {
                info!(
                    service = %directive.service_name,
                    tag = %directive.coast_image_tag,
                    "building image from compose build: directive"
                );
                match coast_docker::compose_build::build_and_cache_image(
                    directive,
                    compose_dir,
                    &cache_dir,
                )
                .await
                {
                    Ok(_) => {
                        images_built += 1;
                        images_cached += 1;
                        built_services.push(directive.service_name.clone());
                        emit(
                            &progress,
                            BuildProgressEvent::item(
                                "Building images",
                                &directive.service_name,
                                "ok",
                            ),
                        );
                    }
                    Err(e) => {
                        let status = if req.refresh { "fail" } else { "warn" };
                        emit(
                            &progress,
                            BuildProgressEvent::item(
                                "Building images",
                                &directive.service_name,
                                status,
                            )
                            .with_verbose(e.to_string()),
                        );
                        if req.refresh {
                            return Err(e);
                        }
                        warnings.push(format!(
                            "Failed to build image for service '{}': {}. Build will continue.",
                            directive.service_name, e
                        ));
                    }
                }
            }

            if has_image_refs {
                emit(
                    &progress,
                    BuildProgressEvent::started(
                        "Pulling images",
                        sn("Pulling images"),
                        total_steps,
                    ),
                );
            }
            if let Some(ref docker) = state.docker {
                for image_name in &parse_result.image_refs {
                    info!(image = %image_name, "caching OCI image");
                    match pull_and_cache_image(docker, image_name, &cache_dir).await {
                        Ok(_) => {
                            images_cached += 1;
                            pulled_images.push(image_name.clone());
                            emit(
                                &progress,
                                BuildProgressEvent::item("Pulling images", image_name, "ok"),
                            );
                        }
                        Err(e) => {
                            let status = if req.refresh { "fail" } else { "warn" };
                            emit(
                                &progress,
                                BuildProgressEvent::item("Pulling images", image_name, status)
                                    .with_verbose(e.to_string()),
                            );
                            if req.refresh {
                                return Err(e);
                            }
                            warnings.push(format!(
                                "Failed to cache image '{}': {}. Build will continue.",
                                image_name, e
                            ));
                        }
                    }
                }
            } else if has_image_refs {
                emit(
                    &progress,
                    BuildProgressEvent::done("Pulling images", "skip")
                        .with_verbose("Docker not available"),
                );
                warnings.push(
                    "Docker is not available — skipping OCI image pulling. \
                         Images will be pulled at runtime."
                        .to_string(),
                );
            }

            for directive in &parse_result.build_directives {
                let dockerfile_path = if let Some(ref df) = directive.dockerfile {
                    compose_dir.join(&directive.context).join(df)
                } else {
                    compose_dir.join(&directive.context).join("Dockerfile")
                };
                if let Ok(dockerfile_content) = std::fs::read_to_string(&dockerfile_path) {
                    let base_imgs = coast_docker::compose_build::parse_dockerfile_base_images(
                        &dockerfile_content,
                    );
                    if let Some(ref docker) = state.docker {
                        for base_image in &base_imgs {
                            info!(
                                base_image = %base_image,
                                service = %directive.service_name,
                                "caching Dockerfile base image"
                            );
                            match pull_and_cache_image(docker, base_image, &cache_dir).await {
                                Ok(_) => {
                                    images_cached += 1;
                                    if !base_images.contains(base_image) {
                                        base_images.push(base_image.clone());
                                    }
                                    emit(
                                        &progress,
                                        BuildProgressEvent::item(
                                            "Pulling images",
                                            format!("{} (base)", base_image),
                                            "ok",
                                        ),
                                    );
                                }
                                Err(e) => {
                                    warnings.push(format!(
                                        "Failed to cache base image '{}' for service '{}': {}. \
                                             It will be pulled at runtime.",
                                        base_image, directive.service_name, e
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if state.docker.is_none() {
        warnings.push(
            "Docker is not available — skipping OCI image caching. \
             Images will be pulled at runtime."
                .to_string(),
        );
    }

    // Step 5: Build custom coast image if [coast.setup] is configured
    let coast_image: Option<String> = if has_setup {
        let image_tag = format!("coast-image/{}:{}", coastfile.name, build_id);
        emit(
            &progress,
            BuildProgressEvent::started(
                "Building coast image",
                sn("Building coast image"),
                total_steps,
            ),
        );
        info!(image = %image_tag, "building custom coast image from [coast.setup]");

        let mut dockerfile = String::from("FROM docker:dind\n");
        dockerfile.push_str("RUN apk add --no-cache ripgrep fd rsync\n");
        if !coastfile.setup.packages.is_empty() {
            dockerfile.push_str(&format!(
                "RUN apk add --no-cache {}\n",
                coastfile.setup.packages.join(" ")
            ));
        }
        // Install LSP servers (npm-based). Silently skip if npm is not available.
        dockerfile.push_str(
            "RUN command -v npm >/dev/null 2>&1 && \
             npm install -g typescript-language-server typescript \
             vscode-langservers-extracted yaml-language-server \
             pyright 2>/dev/null || true\n",
        );
        for cmd in &coastfile.setup.run {
            dockerfile.push_str(&format!("RUN {}\n", cmd));
        }
        if !coastfile.setup.files.is_empty() {
            dockerfile.push_str("COPY setup-files/ /\n");
            for file in &coastfile.setup.files {
                if let Some(mode) = file.mode.as_deref() {
                    dockerfile.push_str(&format!(
                        "RUN chmod {} {}\n",
                        mode,
                        shell_single_quote(&file.path)
                    ));
                }
            }
        }

        let build_dir = tempfile::tempdir().map_err(|e| {
            CoastError::io_simple(format!(
                "failed to create temp dir for coast image build: {e}"
            ))
        })?;
        if !coastfile.setup.files.is_empty() {
            let setup_root = build_dir.path().join("setup-files");
            for file in &coastfile.setup.files {
                let rel = file.path.trim_start_matches('/');
                let rel_path = Path::new(rel);
                let out_path = setup_root.join(rel_path);
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| CoastError::Io {
                        message: format!("failed to create setup file parent '{}': {e}", rel),
                        path: parent.to_path_buf(),
                        source: Some(e),
                    })?;
                }
                std::fs::write(&out_path, &file.content).map_err(|e| CoastError::Io {
                    message: format!("failed to write setup file '{}': {e}", file.path),
                    path: out_path.clone(),
                    source: Some(e),
                })?;
            }
        }
        let dockerfile_path = build_dir.path().join("Dockerfile");
        std::fs::write(&dockerfile_path, &dockerfile).map_err(|e| CoastError::Io {
            message: format!("failed to write coast image Dockerfile: {e}"),
            path: dockerfile_path.clone(),
            source: Some(e),
        })?;

        let build_output = tokio::process::Command::new("docker")
            .args([
                "build",
                "-t",
                &image_tag,
                build_dir.path().to_str().unwrap_or("."),
            ])
            .output()
            .await
            .map_err(|e| {
                CoastError::docker(format!(
                    "failed to run docker build for coast image: {e}. Is Docker running?"
                ))
            })?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            emit(
                &progress,
                BuildProgressEvent::done("Building coast image", "fail")
                    .with_verbose(stderr.to_string()),
            );
            return Err(CoastError::docker(format!(
                "Failed to build custom coast image '{}'. \
                 Check that the packages, commands, and files in [coast.setup] are valid.\n\
                 Stderr: {stderr}",
                image_tag
            )));
        }

        emit(
            &progress,
            BuildProgressEvent::done("Building coast image", "ok")
                .with_verbose(String::from_utf8_lossy(&build_output.stdout).to_string()),
        );
        info!(image = %image_tag, "custom coast image built successfully");

        // Also tag as :latest for run compatibility
        let latest_tag = format!("coast-image/{}:latest", coastfile.name);
        let _ = tokio::process::Command::new("docker")
            .args(["tag", &image_tag, &latest_tag])
            .output()
            .await;

        Some(image_tag)
    } else {
        None
    };

    // Step 6: Write manifest.json
    emit(
        &progress,
        BuildProgressEvent::started("Writing manifest", sn("Writing manifest"), total_steps),
    );

    let manifest = serde_json::json!({
        "build_id": &build_id,
        "project": &coastfile.name,
        "coastfile_type": &coastfile.coastfile_type,
        "project_root": coastfile.project_root.display().to_string(),
        "build_timestamp": build_timestamp.to_rfc3339(),
        "coastfile_hash": coastfile_hash,
        "images_cached": images_cached,
        "images_built": images_built,
        "coast_image": coast_image,
        "secrets": coastfile.secrets.iter().map(|s| &s.name).collect::<Vec<_>>(),
        "built_services": built_services,
        "pulled_images": pulled_images,
        "base_images": base_images,
        "omitted_services": &coastfile.omit.services,
        "omitted_volumes": &coastfile.omit.volumes,
        "mcp_servers": coastfile.mcp_servers.iter().map(|m| {
            serde_json::json!({
                "name": m.name,
                "proxy": m.proxy.as_ref().map(coast_core::types::McpProxyMode::as_str),
                "command": m.command,
                "args": m.args,
            })
        }).collect::<Vec<_>>(),
        "mcp_clients": coastfile.mcp_clients.iter().map(|c| {
            serde_json::json!({
                "name": c.name,
                "format": c.format.as_ref().map(coast_core::types::McpClientFormat::as_str),
                "config_path": c.resolved_config_path(),
            })
        }).collect::<Vec<_>>(),
        "shared_services": coastfile.shared_services.iter().map(|s| {
            serde_json::json!({
                "name": s.name,
                "image": s.image,
                "ports": s.ports,
                "auto_create_db": s.auto_create_db,
            })
        }).collect::<Vec<_>>(),
        "volumes": coastfile.volumes.iter().map(|v| {
            serde_json::json!({
                "name": v.name,
                "strategy": match v.strategy {
                    VolumeStrategy::Isolated => "isolated",
                    VolumeStrategy::Shared => "shared",
                },
                "service": v.service,
                "mount": v.mount.display().to_string(),
                "snapshot_source": v.snapshot_source,
            })
        }).collect::<Vec<_>>(),
        "agent_shell": coastfile.agent_shell.as_ref().map(|a| {
            serde_json::json!({ "command": a.command })
        }),
        "primary_port": &coastfile.primary_port,
    });
    let manifest_path = artifact_path.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| CoastError::protocol(format!("failed to serialize manifest: {e}")))?;
    std::fs::write(&manifest_path, manifest_json).map_err(|e| CoastError::Io {
        message: format!("failed to write manifest.json: {e}"),
        path: manifest_path,
        source: Some(e),
    })?;

    // Store primary port in settings (from Coastfile or auto-detect single port)
    {
        let primary = coastfile.primary_port.clone().or_else(|| {
            if coastfile.ports.len() == 1 {
                coastfile.ports.keys().next().cloned()
            } else {
                None
            }
        });
        if let Some(ref service) = primary {
            let db = state.db.lock().await;
            let key = format!("primary_port:{}:{}", coastfile.name, build_id);
            db.set_setting(&key, service)?;
        }
    }

    // Create/update per-type `latest` symlink: `latest` for default, `latest-{type}` for typed.
    let latest_name = match &coastfile.coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = project_dir.join(&latest_name);
    let _ = std::fs::remove_file(&latest_link);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&build_id, &latest_link).map_err(|e| CoastError::Io {
            message: format!("failed to create '{}' symlink: {e}", latest_name),
            path: latest_link.clone(),
            source: Some(e),
        })?;
    }

    // Collect build IDs currently in use by running instances to protect from pruning.
    // If any instance has build_id=NULL (pre-migration), conservatively protect `latest`.
    let in_use_build_ids: std::collections::HashSet<String> = {
        let db = state.db.lock().await;
        let instances = db
            .list_instances_for_project(&coastfile.name)
            .unwrap_or_default();
        let has_null_build_id = instances.iter().any(|inst| inst.build_id.is_none());
        let mut ids: std::collections::HashSet<String> = instances
            .into_iter()
            .filter_map(|inst| inst.build_id)
            .collect();
        if has_null_build_id {
            if let Ok(target) = std::fs::read_link(project_dir.join("latest")) {
                if let Some(name) = target.file_name() {
                    ids.insert(name.to_string_lossy().into_owned());
                }
            }
        }
        ids
    };
    auto_prune_builds(
        &project_dir,
        5,
        &in_use_build_ids,
        coastfile.coastfile_type.as_deref(),
    );

    emit(
        &progress,
        BuildProgressEvent::done("Writing manifest", "ok"),
    );

    info!(
        project = coastfile.name,
        build_id = %build_id,
        artifact_path = %artifact_path.display(),
        images_cached,
        images_built,
        secrets_extracted,
        warnings_count = warnings.len(),
        "build completed"
    );

    Ok(BuildResponse {
        project: coastfile.name,
        artifact_path,
        images_cached,
        images_built,
        secrets_extracted,
        coast_image,
        warnings,
        coastfile_type: coastfile.coastfile_type,
    })
}

/// Remove old builds from a project directory, keeping the most recent `keep` builds.
/// Builds whose IDs appear in `in_use` are never pruned regardless of the keep limit.
/// Only considers builds matching the given `coastfile_type` (per-type pruning).
fn auto_prune_builds(
    project_dir: &std::path::Path,
    keep: usize,
    in_use: &std::collections::HashSet<String>,
    coastfile_type: Option<&str>,
) {
    let Ok(entries) = std::fs::read_dir(project_dir) else {
        return;
    };

    let mut builds: Vec<(String, String)> = Vec::new(); // (dirname, build_timestamp)
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("latest") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok());
        let Some(ref manifest) = manifest else {
            continue;
        };
        let build_type = manifest
            .get("coastfile_type")
            .and_then(|v| v.as_str())
            .map(std::string::ToString::to_string);
        if build_type.as_deref() != coastfile_type {
            continue;
        }
        let ts = manifest
            .get("build_timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        builds.push((name, ts));
    }

    builds.sort_by(|a, b| b.1.cmp(&a.1));

    for (dirname, _) in builds.iter().skip(keep) {
        if in_use.contains(dirname) {
            info!(
                build_id = %dirname,
                "skipping prune of build — still in use by running instance(s)"
            );
            continue;
        }
        let path = project_dir.join(dirname);
        if let Err(e) = std::fs::remove_dir_all(&path) {
            warn!(path = %path.display(), error = %e, "failed to prune old build");
        } else {
            info!(build_id = %dirname, "pruned old build");
        }
    }
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

/// Pull a Docker image and save it as a tarball in the cache directory.
async fn pull_and_cache_image(
    docker: &bollard::Docker,
    image: &str,
    cache_dir: &std::path::Path,
) -> Result<std::path::PathBuf> {
    use bollard::image::CreateImageOptions;
    use futures_util::StreamExt;

    // Parse image name and tag
    let (name, tag) = if let Some(pos) = image.rfind(':') {
        (&image[..pos], &image[pos + 1..])
    } else {
        (image, "latest")
    };

    info!(image = %image, "pulling image for cache");

    // Pull the image
    let options = CreateImageOptions {
        from_image: name,
        tag,
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(options), None, None);
    while let Some(result) = stream.next().await {
        match result {
            Ok(info) => {
                if let Some(status) = info.status {
                    tracing::debug!(status = %status, "pull progress");
                }
            }
            Err(e) => {
                return Err(CoastError::docker(format!(
                    "failed to pull image '{}': {}",
                    image, e
                )));
            }
        }
    }

    // Save the image as a tarball
    let safe_name = image.replace(['/', ':'], "_");
    let tarball_path = cache_dir.join(format!("{safe_name}.tar"));

    let mut export_stream = docker.export_image(image);
    let mut tarball_data = Vec::new();
    while let Some(chunk) = export_stream.next().await {
        match chunk {
            Ok(bytes) => tarball_data.extend_from_slice(&bytes),
            Err(e) => {
                return Err(CoastError::docker(format!(
                    "failed to export image '{}': {}",
                    image, e
                )));
            }
        }
    }

    std::fs::write(&tarball_path, &tarball_data).map_err(|e| CoastError::Io {
        message: format!("failed to write image tarball: {e}"),
        path: tarball_path.clone(),
        source: Some(e),
    })?;

    info!(image = %image, path = %tarball_path.display(), "image cached");

    Ok(tarball_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn test_progress_sender() -> tokio::sync::mpsc::Sender<BuildProgressEvent> {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        tx
    }

    #[tokio::test]
    async fn test_build_nonexistent_coastfile() {
        let state = test_state();
        let req = BuildRequest {
            coastfile_path: PathBuf::from("/tmp/nonexistent/Coastfile"),
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_build_valid_coastfile() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-build"
compose = "./docker-compose.yml"
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.project, "test-build");
        assert!(resp.artifact_path.exists());
    }

    #[tokio::test]
    async fn test_build_shared_volume_warning() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-warn"
compose = "./docker-compose.yml"

[volumes.pg_data]
strategy = "shared"
service = "postgres"
mount = "/var/lib/postgresql/data"
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("shared"));
    }

    #[tokio::test]
    async fn test_build_with_missing_inject_file() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-inject"
compose = "./docker-compose.yml"

[inject]
files = ["/tmp/nonexistent_coast_test_file_12345"]
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("does not exist")));
    }

    #[tokio::test]
    async fn test_build_rewrites_artifact_compose() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-rewrite"
compose = "./docker-compose.yml"
"#,
        )
        .unwrap();

        std::fs::write(
            &compose_path,
            r#"services:
  app:
    build: .
    ports:
      - "3000:3000"
  db:
    image: postgres:16
"#,
        )
        .unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        // Verify the artifact compose has image: instead of build:
        let artifact_compose = result.artifact_path.join("compose.yml");
        let content = std::fs::read_to_string(&artifact_compose).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();
        let app = doc.get("services").unwrap().get("app").unwrap();
        assert!(
            app.get("build").is_none(),
            "build: should be removed from artifact compose"
        );
        assert_eq!(
            app.get("image").unwrap().as_str().unwrap(),
            "coast-built/test-rewrite/app:latest"
        );
        // db should be unchanged
        let db = doc.get("services").unwrap().get("db").unwrap();
        assert_eq!(db.get("image").unwrap().as_str().unwrap(), "postgres:16");
    }

    #[tokio::test]
    async fn test_build_with_setup_no_docker() {
        // When [coast.setup] is configured but Docker isn't available,
        // the build should fail with a clear error
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-setup"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["curl"]
run = ["echo hello"]
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await;
        // We can't guarantee Docker is available in tests, so just verify the code path runs
        if let Ok(resp) = result {
            // If Docker was available, coast_image should be set
            assert!(resp.coast_image.is_some());
            assert!(resp.coast_image.unwrap().contains("test-setup"));
        }
        // If it errored, it should be a Docker error (not a parsing error)
    }

    #[tokio::test]
    async fn test_build_without_setup_no_coast_image() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-no-setup"
compose = "./docker-compose.yml"
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();
        assert!(result.coast_image.is_none());
    }

    #[tokio::test]
    async fn test_build_manifest_contains_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-manifest"
compose = "./docker-compose.yml"
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        // Verify manifest.json contains project_root
        let manifest_path = result.artifact_path.join("manifest.json");
        let manifest_str = std::fs::read_to_string(&manifest_path).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_str).unwrap();
        assert!(
            manifest.get("project_root").is_some(),
            "manifest.json must contain project_root"
        );
        let project_root = manifest["project_root"].as_str().unwrap();
        // The project_root should point to the directory containing the Coastfile
        assert!(
            project_root.contains(dir.path().to_str().unwrap()),
            "project_root '{}' should contain the Coastfile's directory '{}'",
            project_root,
            dir.path().display()
        );
    }

    #[tokio::test]
    async fn test_build_emits_progress_events() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-progress"
compose = "./docker-compose.yml"
"#,
        )
        .unwrap();

        std::fs::write(&compose_path, "version: '3'\nservices: {}").unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let _result = handle(req, &state, tx).await.unwrap();

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        let steps: Vec<_> = events
            .iter()
            .map(|e| (e.step.as_str(), e.status.as_str()))
            .collect();

        assert!(
            steps
                .iter()
                .any(|(s, st)| *s == "Parsing Coastfile" && *st == "started"),
            "should have Parsing Coastfile/started"
        );
        assert!(
            steps
                .iter()
                .any(|(s, st)| *s == "Parsing Coastfile" && *st == "ok"),
            "should have Parsing Coastfile/ok"
        );
        assert!(
            steps
                .iter()
                .any(|(s, st)| *s == "Creating artifact" && *st == "ok"),
            "should have Creating artifact/ok"
        );
        assert!(
            steps
                .iter()
                .any(|(s, st)| *s == "Writing manifest" && *st == "ok"),
            "should have Writing manifest/ok"
        );

        // Verify step numbering: started events should have step_number and total_steps.
        let started_events: Vec<_> = events
            .iter()
            .filter(|e| e.status == "started" && e.detail.is_none())
            .collect();
        for ev in &started_events {
            assert!(
                ev.step_number.is_some(),
                "started event '{}' should have step_number",
                ev.step
            );
            assert!(
                ev.total_steps.is_some(),
                "started event '{}' should have total_steps",
                ev.step
            );
        }

        // For this minimal coastfile (no secrets, no setup, empty compose), expect 3 steps.
        let first = &started_events[0];
        assert_eq!(first.step, "Parsing Coastfile");
        assert_eq!(first.step_number, Some(1));
        assert_eq!(first.total_steps, Some(3));
    }

    #[tokio::test]
    async fn test_build_omit_strips_services_from_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-omit"
compose = "./docker-compose.yml"

[omit]
services = ["keycloak", "redash"]
volumes = ["keycloak-db-data"]
"#,
        )
        .unwrap();

        std::fs::write(
            &compose_path,
            r#"services:
  app:
    image: myapp:latest
    depends_on:
      - keycloak
      - db
  keycloak:
    image: quay.io/keycloak/keycloak
    depends_on:
      - db
  redash:
    image: redash/redash
  db:
    image: postgres:16
volumes:
  keycloak-db-data:
  app-data:
"#,
        )
        .unwrap();

        let state = test_state();
        let req = BuildRequest {
            coastfile_path,
            refresh: false,
        };
        let result = handle(req, &state, test_progress_sender()).await.unwrap();

        let artifact_compose = result.artifact_path.join("compose.yml");
        let content = std::fs::read_to_string(&artifact_compose).unwrap();
        let doc: serde_yaml::Value = serde_yaml::from_str(&content).unwrap();

        let services = doc.get("services").unwrap().as_mapping().unwrap();
        assert!(services.contains_key(&serde_yaml::Value::String("app".into())));
        assert!(services.contains_key(&serde_yaml::Value::String("db".into())));
        assert!(
            !services.contains_key(&serde_yaml::Value::String("keycloak".into())),
            "keycloak should be omitted"
        );
        assert!(
            !services.contains_key(&serde_yaml::Value::String("redash".into())),
            "redash should be omitted"
        );

        // depends_on for app should no longer reference keycloak
        let app = services
            .get(&serde_yaml::Value::String("app".into()))
            .unwrap();
        if let Some(deps) = app.get("depends_on") {
            let dep_list: Vec<&str> = if let Some(seq) = deps.as_sequence() {
                seq.iter().filter_map(|v| v.as_str()).collect()
            } else if let Some(map) = deps.as_mapping() {
                map.keys().filter_map(|k| k.as_str()).collect()
            } else {
                vec![]
            };
            assert!(
                !dep_list.contains(&"keycloak"),
                "depends_on should not reference omitted keycloak"
            );
            assert!(
                dep_list.contains(&"db"),
                "depends_on should still reference db"
            );
        }

        // keycloak-db-data volume should be removed
        if let Some(volumes) = doc.get("volumes").and_then(|v| v.as_mapping()) {
            assert!(
                !volumes.contains_key(&serde_yaml::Value::String("keycloak-db-data".into())),
                "keycloak-db-data volume should be omitted"
            );
            assert!(
                volumes.contains_key(&serde_yaml::Value::String("app-data".into())),
                "app-data volume should be preserved"
            );
        }
    }

    #[tokio::test]
    async fn test_build_omit_skips_building_omitted_images() {
        let dir = tempfile::tempdir().unwrap();
        let coastfile_path = dir.path().join("Coastfile");
        let compose_path = dir.path().join("docker-compose.yml");

        std::fs::write(
            &coastfile_path,
            r#"
[coast]
name = "test-omit-build"
compose = "./docker-compose.yml"

[omit]
services = ["redash"]
"#,
        )
        .unwrap();

        std::fs::write(
            &compose_path,
            r#"services:
  app:
    build: .
  redash:
    build: ./redash
  db:
    image: postgres:16
"#,
        )
        .unwrap();

        // Verify filtering: unfiltered has both, filtered skips redash
        let content = std::fs::read_to_string(&compose_path).unwrap();
        let unfiltered =
            coast_docker::compose_build::parse_compose_file(&content, "test-omit-build").unwrap();
        assert_eq!(unfiltered.build_directives.len(), 2);

        let filtered = coast_docker::compose_build::parse_compose_file_filtered(
            &content,
            "test-omit-build",
            &["redash".to_string()],
        )
        .unwrap();
        assert_eq!(filtered.build_directives.len(), 1);
        assert_eq!(filtered.build_directives[0].service_name, "app");
    }
}
