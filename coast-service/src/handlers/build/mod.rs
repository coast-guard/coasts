mod artifact;
mod images;
mod manifest;
mod secrets;

use sha2::{Digest, Sha256};
use tracing::info;

use coast_core::coastfile::Coastfile;
use coast_core::error::Result;
use coast_core::protocol::{BuildRequest, BuildResponse};

use crate::state::ServiceState;

pub async fn handle(req: BuildRequest, state: &ServiceState) -> Result<BuildResponse> {
    info!(path = %req.coastfile_path.display(), refresh = req.refresh, "remote build request");

    let coastfile = Coastfile::from_file(&req.coastfile_path)?;
    let home = crate::state::service_home();

    let build_id = compute_build_id(&coastfile)?;
    info!(build_id = %build_id, project = %coastfile.name, "computed build id");

    let artifact_out = artifact::create_artifact(&coastfile, &build_id, &home)?;
    let mut warnings = artifact_out.warnings;

    let mut images_cached = 0usize;
    let mut images_built = 0usize;

    if let Some(ref docker) = state.docker {
        match images::cache_images(docker, &coastfile, &home).await {
            Ok(img_out) => {
                images_cached = img_out.images_cached;
                images_built = img_out.images_built;
                warnings.extend(img_out.warnings);
            }
            Err(e) => {
                warnings.push(format!("image caching failed: {e}"));
            }
        }
    } else {
        info!("docker not available, skipping image caching");
    }

    let secret_out = secrets::extract_secrets(&coastfile, &home);
    let secrets_extracted = secret_out.secrets_extracted;
    warnings.extend(secret_out.warnings);

    manifest::write_manifest(
        &coastfile,
        &build_id,
        &artifact_out.artifact_path,
        images_cached,
        images_built,
    )?;

    Ok(BuildResponse {
        project: coastfile.name.clone(),
        artifact_path: artifact_out.artifact_path,
        images_cached,
        images_built,
        secrets_extracted,
        coast_image: None,
        warnings,
        coastfile_type: coastfile.coastfile_type.clone(),
    })
}

fn compute_build_id(coastfile: &Coastfile) -> Result<String> {
    let toml_text = coastfile.to_standalone_toml();
    let hash = hex::encode(Sha256::digest(toml_text.as_bytes()));
    let short_hash = &hash[..12];
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    Ok(format!("{short_hash}_{timestamp}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ServiceDb, ServiceState};
    use std::path::Path;
    use std::sync::Arc;

    fn test_state() -> Arc<ServiceState> {
        Arc::new(ServiceState::new_for_testing(
            ServiceDb::open_in_memory().unwrap(),
        ))
    }

    #[test]
    fn test_build_id_format() {
        let tmp = tempfile::tempdir().unwrap();
        let toml = "[coast]\nname = \"test-proj\"\n";
        let cf = Coastfile::parse(toml, tmp.path()).unwrap();
        let id = compute_build_id(&cf).unwrap();

        let parts: Vec<&str> = id.splitn(2, '_').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 12);
        assert!(parts[0].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(parts[1].len(), 14); // YYYYMMDDHHmmss
    }

    #[test]
    fn test_artifact_path_layout() {
        let home = Path::new("/coast-service");
        let path = home.join("images").join("myproj").join("abc123_20260101");
        assert_eq!(
            path.to_str().unwrap(),
            "/coast-service/images/myproj/abc123_20260101"
        );
    }

    #[tokio::test]
    async fn test_full_build_no_docker() {
        let tmp = tempfile::tempdir().unwrap();
        let coastfile_path = tmp.path().join("Coastfile");
        std::fs::write(&coastfile_path, "[coast]\nname = \"integration\"\n").unwrap();

        std::env::set_var(
            "COAST_SERVICE_HOME",
            tmp.path().join("svc_home").to_str().unwrap(),
        );

        let state = test_state();
        let req = BuildRequest {
            coastfile_path: coastfile_path.clone(),
            refresh: false,
            remote: None,
        };

        let resp = handle(req, &state).await.unwrap();

        assert_eq!(resp.project, "integration");
        assert!(resp.artifact_path.exists());
        assert_eq!(resp.images_cached, 0);
        assert_eq!(resp.secrets_extracted, 0);
        assert!(resp.artifact_path.join("coastfile.toml").exists());
        assert!(resp.artifact_path.join("manifest.json").exists());
        assert!(resp.artifact_path.join("secrets").is_dir());

        std::env::remove_var("COAST_SERVICE_HOME");
    }
}
