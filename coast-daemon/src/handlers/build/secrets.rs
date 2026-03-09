use std::path::Path;

use coast_core::coastfile::Coastfile;
use coast_core::protocol::BuildProgressEvent;
use coast_core::types::InjectType;

use super::emit;
use super::plan::BuildPlan;
use super::utils::parse_ttl_to_seconds;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SecretExtractionOutput {
    pub secrets_extracted: usize,
    pub warnings: Vec<String>,
}

pub(super) fn extract_secrets(
    coastfile: &Coastfile,
    home: &Path,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    plan: &BuildPlan,
) -> SecretExtractionOutput {
    if coastfile.secrets.is_empty() {
        return SecretExtractionOutput::default();
    }

    emit(progress, plan.started("Extracting secrets"));

    let mut output = SecretExtractionOutput::default();
    let keystore_db_path = home.join(".coast").join("keystore.db");
    let keystore_key_path = home.join(".coast").join("keystore.key");

    match coast_secrets::keystore::Keystore::open(&keystore_db_path, &keystore_key_path) {
        Ok(keystore) => {
            if let Err(error) = keystore.delete_secrets_for_image(&coastfile.name) {
                output.warnings.push(format!(
                    "Failed to clear old secrets for '{}': {}",
                    coastfile.name, error
                ));
            }

            let registry = coast_secrets::extractor::ExtractorRegistry::with_builtins();

            for secret_config in &coastfile.secrets {
                let mut resolved_params = secret_config.params.clone();
                if let Some(path) = resolved_params.get("path") {
                    let path = std::path::Path::new(path);
                    if path.is_relative() {
                        let abs = coastfile.project_root.join(path);
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
                        if let Err(error) = keystore.store_secret(
                            &coastfile.name,
                            &secret_config.name,
                            &value_bytes,
                            inject_type_str,
                            inject_target_str,
                            &secret_config.extractor,
                            ttl_seconds,
                        ) {
                            emit(
                                progress,
                                BuildProgressEvent::item(
                                    "Extracting secrets",
                                    format!("{} -> {}", secret_config.extractor, inject_target),
                                    "warn",
                                )
                                .with_verbose(format!("Failed to store: {error}")),
                            );
                            output.warnings.push(format!(
                                "Failed to store secret '{}': {}",
                                secret_config.name, error
                            ));
                        } else {
                            output.secrets_extracted += 1;
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
                    Err(error) => {
                        emit(
                            progress,
                            BuildProgressEvent::item(
                                "Extracting secrets",
                                format!("{} -> {}", secret_config.extractor, inject_target),
                                "fail",
                            )
                            .with_verbose(error.to_string()),
                        );
                        output.warnings.push(format!(
                            "Failed to extract secret '{}' using extractor '{}': {}",
                            secret_config.name, secret_config.extractor, error
                        ));
                    }
                }
            }
        }
        Err(error) => {
            emit(
                progress,
                BuildProgressEvent::done("Extracting secrets", "fail")
                    .with_verbose(error.to_string()),
            );
            output.warnings.push(format!(
                "Failed to open keystore: {}. Secrets will not be stored.",
                error
            ));
        }
    }

    output
}
