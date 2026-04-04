use std::path::Path;

use tracing::info;

use coast_core::artifact::image_cache_dir;
use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::{BuildProgressEvent, BuildRequest};
use coast_docker::compose_build::ComposeBuildDirective;

use crate::server::AppState;

use super::emit;
use super::plan::{BuildPlan, ComposeAnalysis};
use super::utils::pull_and_cache_image;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct ImageBuildOutput {
    pub images_cached: usize,
    pub images_built: usize,
    pub built_services: Vec<String>,
    pub pulled_images: Vec<String>,
    pub base_images: Vec<String>,
    pub warnings: Vec<String>,
}

struct ImageBuildContext<'a> {
    refresh: bool,
    docker: Option<&'a bollard::Docker>,
    cache_dir: &'a Path,
    compose_dir: &'a Path,
    progress: &'a tokio::sync::mpsc::Sender<BuildProgressEvent>,
    plan: &'a BuildPlan,
}

pub(super) async fn cache_images(
    req: &BuildRequest,
    state: &AppState,
    _coastfile: &Coastfile,
    compose_analysis: &ComposeAnalysis,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    plan: &BuildPlan,
) -> Result<ImageBuildOutput> {
    let cache_dir = image_cache_dir()?;
    std::fs::create_dir_all(&cache_dir).map_err(|error| CoastError::Io {
        message: format!("failed to create image cache directory: {error}"),
        path: cache_dir.clone(),
        source: Some(error),
    })?;

    let mut output = ImageBuildOutput::default();
    let Some(parse_result) = compose_analysis.parse_result.as_ref() else {
        if state.docker.is_none() {
            output.warnings.push(
                "Docker is not available — skipping OCI image caching. \
                 Images will be pulled at runtime."
                    .to_string(),
            );
        }
        return Ok(output);
    };

    let compose_dir = compose_analysis
        .dir
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("."));
    let docker = state.docker.as_ref();
    let context = ImageBuildContext {
        refresh: req.refresh,
        docker: docker.as_ref(),
        cache_dir: &cache_dir,
        compose_dir,
        progress,
        plan,
    };

    cache_built_images(&context, &parse_result.build_directives, &mut output).await?;
    cache_referenced_images(&context, &parse_result.image_refs, &mut output).await?;
    cache_base_images(&context, &parse_result.build_directives, &mut output).await;
    Ok(output)
}

async fn cache_built_images(
    context: &ImageBuildContext<'_>,
    build_directives: &[ComposeBuildDirective],
    output: &mut ImageBuildOutput,
) -> Result<()> {
    if build_directives.is_empty() {
        return Ok(());
    }

    emit(context.progress, context.plan.started("Building images"));

    for directive in build_directives {
        info!(
            service = %directive.service_name,
            tag = %directive.coast_image_tag,
            "building image from compose build: directive"
        );
        match coast_docker::compose_build::build_and_cache_image(
            directive,
            context.compose_dir,
            context.cache_dir,
        )
        .await
        {
            Ok(_) => {
                output.images_built += 1;
                output.images_cached += 1;
                output.built_services.push(directive.service_name.clone());
                emit(
                    context.progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, "ok"),
                );
            }
            Err(error) => {
                let status = if context.refresh { "fail" } else { "warn" };
                emit(
                    context.progress,
                    BuildProgressEvent::item("Building images", &directive.service_name, status)
                        .with_verbose(error.to_string()),
                );
                if context.refresh {
                    return Err(error);
                }
                output.warnings.push(format!(
                    "Failed to build image for service '{}': {}. Build will continue.",
                    directive.service_name, error
                ));
            }
        }
    }

    Ok(())
}

async fn cache_referenced_images(
    context: &ImageBuildContext<'_>,
    image_refs: &[String],
    output: &mut ImageBuildOutput,
) -> Result<()> {
    if image_refs.is_empty() {
        return Ok(());
    }

    emit(context.progress, context.plan.started("Pulling images"));

    let Some(docker) = context.docker else {
        emit(
            context.progress,
            BuildProgressEvent::done("Pulling images", "skip").with_verbose("Docker not available"),
        );
        output.warnings.push(
            "Docker is not available — skipping OCI image pulling. \
                 Images will be pulled at runtime."
                .to_string(),
        );
        return Ok(());
    };

    for image_name in image_refs {
        info!(image = %image_name, "caching OCI image");
        match pull_and_cache_image(docker, image_name, context.cache_dir).await {
            Ok(_) => {
                output.images_cached += 1;
                output.pulled_images.push(image_name.clone());
                emit(
                    context.progress,
                    BuildProgressEvent::item("Pulling images", image_name, "ok"),
                );
            }
            Err(error) => {
                let status = if context.refresh { "fail" } else { "warn" };
                emit(
                    context.progress,
                    BuildProgressEvent::item("Pulling images", image_name, status)
                        .with_verbose(error.to_string()),
                );
                if context.refresh {
                    return Err(error);
                }
                output.warnings.push(format!(
                    "Failed to cache image '{}': {}. Build will continue.",
                    image_name, error
                ));
            }
        }
    }

    Ok(())
}

async fn cache_base_images(
    context: &ImageBuildContext<'_>,
    build_directives: &[ComposeBuildDirective],
    output: &mut ImageBuildOutput,
) {
    let Some(docker) = context.docker else {
        return;
    };

    for directive in build_directives {
        if let Ok(dockerfile_content) = read_dockerfile_content(context.compose_dir, directive) {
            let base_images =
                coast_docker::compose_build::parse_dockerfile_base_images(&dockerfile_content);
            pull_base_images(context, docker, directive, &base_images, output).await;
        }
    }
}

fn read_dockerfile_content(
    compose_dir: &Path,
    directive: &ComposeBuildDirective,
) -> std::io::Result<String> {
    let dockerfile_path = if let Some(ref dockerfile) = directive.dockerfile {
        compose_dir.join(&directive.context).join(dockerfile)
    } else {
        compose_dir.join(&directive.context).join("Dockerfile")
    };
    std::fs::read_to_string(dockerfile_path)
}

async fn pull_base_images(
    context: &ImageBuildContext<'_>,
    docker: &bollard::Docker,
    directive: &ComposeBuildDirective,
    base_images: &[String],
    output: &mut ImageBuildOutput,
) {
    for base_image in base_images {
        info!(
            base_image = %base_image,
            service = %directive.service_name,
            "caching Dockerfile base image"
        );
        match pull_and_cache_image(docker, base_image, context.cache_dir).await {
            Ok(_) => {
                output.images_cached += 1;
                if !output.base_images.contains(base_image) {
                    output.base_images.push(base_image.clone());
                }
                emit(
                    context.progress,
                    BuildProgressEvent::item(
                        "Pulling images",
                        format!("{} (base)", base_image),
                        "ok",
                    ),
                );
            }
            Err(error) => {
                output.warnings.push(format!(
                    "Failed to cache base image '{}' for service '{}': {}. \
                     It will be pulled at runtime.",
                    base_image, directive.service_name, error
                ));
            }
        }
    }
}
