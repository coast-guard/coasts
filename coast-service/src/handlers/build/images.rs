use std::path::Path;

use bollard::image::CreateImageOptions;
use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use futures_util::StreamExt;
use tracing::{info, warn};

pub struct ImageCacheOutput {
    pub images_cached: usize,
    pub images_built: usize,
    pub warnings: Vec<String>,
}

struct ComposeTargets {
    image_refs: Vec<String>,
    build_directives: Vec<coast_docker::compose_build::ComposeBuildDirective>,
    compose_dir: std::path::PathBuf,
    warnings: Vec<String>,
}

fn collect_compose_targets(coastfile: &Coastfile) -> Result<ComposeTargets> {
    let mut targets = ComposeTargets {
        image_refs: Vec::new(),
        build_directives: Vec::new(),
        compose_dir: std::path::PathBuf::from("."),
        warnings: Vec::new(),
    };

    if let Some(ref compose_path) = coastfile.compose {
        targets.compose_dir = compose_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf();

        match std::fs::read_to_string(compose_path) {
            Ok(content) => {
                let parsed = coast_docker::compose_build::parse_compose_file_filtered(
                    &content,
                    &coastfile.name,
                    &coastfile.omit.services,
                )?;
                targets.image_refs = parsed.image_refs;
                targets.build_directives = parsed.build_directives;
            }
            Err(e) => {
                targets.warnings.push(format!(
                    "compose file '{}' not readable for image parsing: {e}",
                    compose_path.display()
                ));
            }
        }
    }

    Ok(targets)
}

async fn cache_build_directives(
    directives: &[coast_docker::compose_build::ComposeBuildDirective],
    compose_dir: &Path,
    cache_dir: &Path,
) -> (usize, usize, Vec<String>) {
    let mut built = 0usize;
    let mut cached = 0usize;
    let mut warnings = Vec::new();

    for directive in directives {
        info!(
            service = %directive.service_name,
            tag = %directive.coast_image_tag,
            "building image from compose build: directive"
        );
        match coast_docker::compose_build::build_and_cache_image(directive, compose_dir, cache_dir)
            .await
        {
            Ok(_) => {
                built += 1;
                cached += 1;
                info!(
                    service = %directive.service_name,
                    tag = %directive.coast_image_tag,
                    "built and cached image"
                );
            }
            Err(e) => {
                warnings.push(format!(
                    "failed to build image for service '{}': {e}",
                    directive.service_name
                ));
                warn!(
                    service = %directive.service_name,
                    error = %e,
                    "image build failed"
                );
            }
        }
    }

    (built, cached, warnings)
}

pub async fn cache_images(
    docker: &bollard::Docker,
    coastfile: &Coastfile,
    home: &Path,
) -> Result<ImageCacheOutput> {
    let cache_dir = home.join("image-cache");
    std::fs::create_dir_all(&cache_dir).map_err(|e| CoastError::Io {
        message: format!("failed to create image cache dir: {e}"),
        path: cache_dir.clone(),
        source: Some(e),
    })?;

    let targets = collect_compose_targets(coastfile)?;
    let mut warnings = targets.warnings;

    let (built, mut cached, build_warnings) =
        cache_build_directives(&targets.build_directives, &targets.compose_dir, &cache_dir).await;
    warnings.extend(build_warnings);

    for image_ref in &targets.image_refs {
        match pull_and_cache(docker, image_ref, &cache_dir).await {
            Ok(()) => {
                cached += 1;
                info!(image = %image_ref, "cached image");
            }
            Err(e) => {
                warnings.push(format!("failed to cache image '{image_ref}': {e}"));
                warn!(image = %image_ref, error = %e, "image cache failed");
            }
        }
    }

    Ok(ImageCacheOutput {
        images_cached: cached,
        images_built: built,
        warnings,
    })
}

fn parse_image_ref(image_ref: &str) -> (String, String) {
    match image_ref.rsplit_once(':') {
        Some((img, t)) if !img.contains('/') || !t.contains('/') => {
            (img.to_string(), t.to_string())
        }
        _ => (image_ref.to_string(), "latest".to_string()),
    }
}

fn tar_path_for_image(cache_dir: &Path, image_ref: &str) -> std::path::PathBuf {
    let safe_name = safe_filename(image_ref);
    cache_dir.join(format!("{safe_name}.tar"))
}

async fn pull_and_cache(docker: &bollard::Docker, image_ref: &str, cache_dir: &Path) -> Result<()> {
    let (from_image, tag) = parse_image_ref(image_ref);

    let opts = CreateImageOptions {
        from_image: from_image.clone(),
        tag: tag.clone(),
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(opts), None, None);
    while let Some(result) = stream.next().await {
        result.map_err(|e| CoastError::docker(format!("pull failed for '{image_ref}': {e}")))?;
    }

    let tar_path = tar_path_for_image(cache_dir, image_ref);

    let mut export_stream = docker.export_image(image_ref);
    let mut tar_data = Vec::new();
    while let Some(chunk) = export_stream.next().await {
        let bytes = chunk
            .map_err(|e| CoastError::docker(format!("export failed for '{image_ref}': {e}")))?;
        tar_data.extend_from_slice(&bytes);
    }

    std::fs::write(&tar_path, &tar_data).map_err(|e| CoastError::Io {
        message: format!("failed to write image tar for '{image_ref}': {e}"),
        path: tar_path,
        source: Some(e),
    })?;

    Ok(())
}

fn safe_filename(image_ref: &str) -> String {
    image_ref.replace(['/', ':'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_safe_filename() {
        assert_eq!(safe_filename("nginx:latest"), "nginx_latest");
        assert_eq!(
            safe_filename("registry.example.com/app:v1.2"),
            "registry.example.com_app_v1.2"
        );
        assert_eq!(safe_filename("postgres"), "postgres");
    }

    #[test]
    fn test_safe_filename_empty() {
        assert_eq!(safe_filename(""), "");
    }

    #[test]
    fn test_safe_filename_multiple_slashes() {
        assert_eq!(
            safe_filename("ghcr.io/org/repo/image:tag"),
            "ghcr.io_org_repo_image_tag"
        );
    }

    #[test]
    fn test_safe_filename_multiple_colons() {
        assert_eq!(safe_filename("img:tag:extra"), "img_tag_extra");
    }

    #[test]
    fn test_safe_filename_slash_and_colon_combined() {
        assert_eq!(safe_filename("a/b:c/d:e"), "a_b_c_d_e");
    }

    #[test]
    fn test_safe_filename_preserves_dots_and_dashes() {
        assert_eq!(
            safe_filename("my-app.io/image-v2.1:rc-1"),
            "my-app.io_image-v2.1_rc-1"
        );
    }

    #[test]
    fn test_parse_image_ref_simple_tag() {
        let (img, tag) = parse_image_ref("nginx:1.25");
        assert_eq!(img, "nginx");
        assert_eq!(tag, "1.25");
    }

    #[test]
    fn test_parse_image_ref_latest_implied() {
        let (img, tag) = parse_image_ref("postgres");
        assert_eq!(img, "postgres");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_with_registry() {
        let (img, tag) = parse_image_ref("ghcr.io/org/app:v2.0");
        assert_eq!(img, "ghcr.io/org/app");
        assert_eq!(tag, "v2.0");
    }

    #[test]
    fn test_parse_image_ref_registry_no_tag() {
        let (img, tag) = parse_image_ref("registry.example.com/myimage");
        assert_eq!(img, "registry.example.com/myimage");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_docker_hub_with_org() {
        let (img, tag) = parse_image_ref("library/nginx:alpine");
        assert_eq!(img, "library/nginx");
        assert_eq!(tag, "alpine");
    }

    #[test]
    fn test_tar_path_for_image_basic() {
        let cache = Path::new("/cache");
        let p = tar_path_for_image(cache, "nginx:latest");
        assert_eq!(p, PathBuf::from("/cache/nginx_latest.tar"));
    }

    #[test]
    fn test_tar_path_for_image_with_registry() {
        let cache = Path::new("/tmp/images");
        let p = tar_path_for_image(cache, "ghcr.io/org/app:v1");
        assert_eq!(p, PathBuf::from("/tmp/images/ghcr.io_org_app_v1.tar"));
    }

    #[test]
    fn test_tar_path_for_image_no_tag() {
        let cache = Path::new("/cache");
        let p = tar_path_for_image(cache, "redis");
        assert_eq!(p, PathBuf::from("/cache/redis.tar"));
    }
}
