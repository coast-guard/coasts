use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::query::InstanceSummary;
use crate::types::SharedServicePort;

/// Request to inspect build artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum BuildsRequest {
    /// List builds. If project is set, list all builds for that project; otherwise list latest per project.
    Ls { project: Option<String> },
    /// Detailed info about a specific build. build_id defaults to "latest".
    Inspect {
        project: String,
        build_id: Option<String>,
    },
    /// List cached image tarballs for a build.
    Images {
        project: String,
        build_id: Option<String>,
    },
    /// List live Docker images on the host daemon for a build.
    DockerImages {
        project: String,
        build_id: Option<String>,
    },
    /// Inspect a specific Docker image on the host daemon.
    InspectDockerImage { project: String, image: String },
    /// Show the rewritten compose.yml.
    Compose {
        project: String,
        build_id: Option<String>,
    },
    /// Show the raw manifest.json.
    Manifest {
        project: String,
        build_id: Option<String>,
    },
    /// Show the stored coastfile.toml.
    Coastfile {
        project: String,
        build_id: Option<String>,
    },
}

/// Response for build inspection requests.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(tag = "kind")]
pub enum BuildsResponse {
    /// List of all builds.
    Ls(BuildsLsResponse),
    /// Detailed inspect output.
    Inspect(Box<BuildsInspectResponse>),
    /// Cached image tarballs.
    Images(BuildsImagesResponse),
    /// Live Docker images.
    DockerImages(BuildsDockerImagesResponse),
    /// Raw Docker inspect JSON.
    DockerImageInspect { data: serde_json::Value },
    /// File content (compose, manifest, coastfile).
    Content(BuildsContentResponse),
}

/// Summary of all builds for `coast builds ls`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildsLsResponse {
    pub builds: Vec<BuildSummary>,
}

/// Per-project summary row.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildSummary {
    pub project: String,
    #[serde(default)]
    pub build_id: Option<String>,
    #[serde(default)]
    pub is_latest: bool,
    pub project_root: Option<String>,
    pub build_timestamp: Option<String>,
    pub images_cached: usize,
    pub images_built: usize,
    pub secrets_count: usize,
    pub coast_image: Option<String>,
    pub cache_size_bytes: u64,
    pub instance_count: usize,
    pub running_count: usize,
    pub archived: bool,
    #[serde(default)]
    pub instances_using: usize,
    #[serde(default)]
    pub coastfile_type: Option<String>,
    /// Architecture of the build (e.g. "x86_64", "aarch64"). Present for remote builds.
    #[serde(default)]
    pub arch: Option<String>,
    /// Whether this build targets a remote machine.
    #[serde(default)]
    pub is_remote: bool,
}

/// MCP server info stored in the build manifest.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpBuildInfo {
    pub name: String,
    pub proxy: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// MCP client connector info stored in the build manifest.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpClientBuildInfo {
    pub name: String,
    pub format: Option<String>,
    pub config_path: Option<String>,
}

/// Shared service info stored in the build manifest.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharedServiceBuildInfo {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub ports: Vec<SharedServicePort>,
    #[serde(default)]
    pub auto_create_db: bool,
}

/// Volume strategy info stored in the build manifest.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VolumeBuildInfo {
    pub name: String,
    pub strategy: String,
    pub service: String,
    pub mount: String,
    pub snapshot_source: Option<String>,
}

/// Full build inspection output.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildsInspectResponse {
    pub project: String,
    #[serde(default)]
    pub build_id: Option<String>,
    pub project_root: Option<String>,
    pub build_timestamp: Option<String>,
    pub coastfile_hash: Option<String>,
    pub coast_image: Option<String>,
    pub artifact_path: String,
    pub artifact_size_bytes: u64,
    pub images_cached: usize,
    pub images_built: usize,
    pub cache_size_bytes: u64,
    pub secrets: Vec<String>,
    pub built_services: Vec<String>,
    pub pulled_images: Vec<String>,
    pub base_images: Vec<String>,
    pub omitted_services: Vec<String>,
    pub omitted_volumes: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<McpBuildInfo>,
    #[serde(default)]
    pub mcp_clients: Vec<McpClientBuildInfo>,
    #[serde(default)]
    pub shared_services: Vec<SharedServiceBuildInfo>,
    #[serde(default)]
    pub volumes: Vec<VolumeBuildInfo>,
    pub instances: Vec<InstanceSummary>,
    pub docker_images: Vec<DockerImageInfo>,
    #[serde(default)]
    pub coastfile_type: Option<String>,
}

/// A cached image tarball in the image cache.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CachedImageInfo {
    pub reference: String,
    pub filename: String,
    pub size_bytes: u64,
    pub image_type: String,
    pub modified: Option<String>,
}

/// A live Docker image on the host daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DockerImageInfo {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub created: String,
    pub size: String,
    pub size_bytes: i64,
}

/// Cached image tarballs response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildsImagesResponse {
    pub images: Vec<CachedImageInfo>,
    pub total_size_bytes: u64,
}

/// Live Docker images response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildsDockerImagesResponse {
    pub images: Vec<DockerImageInfo>,
}

/// Raw file content response (compose, manifest, coastfile).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildsContentResponse {
    pub content: String,
    pub file_type: String,
}
