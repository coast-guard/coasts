use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request to build a coast image.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildRequest {
    /// Path to the Coastfile.
    pub coastfile_path: PathBuf,
    /// Whether to refresh (re-extract secrets, re-pull images).
    pub refresh: bool,
    /// Which registered remote to build on (for remote types).
    #[serde(default)]
    pub remote: Option<String>,
}

/// Request to re-run secret extractors using the cached build Coastfile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RerunExtractorsRequest {
    /// Project name whose cached Coastfile should be used.
    pub project: String,
    /// Optional build ID to target. When omitted, daemon resolves `latest`.
    #[serde(default)]
    pub build_id: Option<String>,
}

/// A progress event emitted during a build. Streamed to the CLI as individual
/// JSON lines before the final `BuildResponse`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildProgressEvent {
    /// Step name (e.g., "Extracting secrets", "Pulling images").
    pub step: String,
    /// Per-item detail (e.g., "macos-keychain -> claude.json").
    #[serde(default)]
    pub detail: Option<String>,
    /// Status: "plan", "started", "ok", "warn", "fail", "skip".
    pub status: String,
    /// Extra detail shown only in --verbose mode.
    #[serde(default)]
    pub verbose_detail: Option<String>,
    /// 1-based step number within the build plan.
    #[serde(default)]
    pub step_number: Option<u32>,
    /// Total number of steps in the build plan.
    #[serde(default)]
    pub total_steps: Option<u32>,
    /// Ordered list of step names, sent once with status "plan".
    #[serde(default)]
    pub plan: Option<Vec<String>>,
}

impl BuildProgressEvent {
    pub fn build_plan(steps: Vec<String>) -> Self {
        Self {
            step: String::new(),
            detail: None,
            status: "plan".into(),
            verbose_detail: None,
            step_number: None,
            total_steps: Some(steps.len() as u32),
            plan: Some(steps),
        }
    }

    pub fn started(step: impl Into<String>, number: u32, total: u32) -> Self {
        Self {
            step: step.into(),
            detail: None,
            status: "started".into(),
            verbose_detail: None,
            step_number: Some(number),
            total_steps: Some(total),
            plan: None,
        }
    }

    pub fn done(step: impl Into<String>, status: &str) -> Self {
        Self {
            step: step.into(),
            detail: None,
            status: status.into(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        }
    }

    pub fn item(step: impl Into<String>, detail: impl Into<String>, status: &str) -> Self {
        Self {
            step: step.into(),
            detail: Some(detail.into()),
            status: status.into(),
            verbose_detail: None,
            step_number: None,
            total_steps: None,
            plan: None,
        }
    }

    pub fn ok(step: impl Into<String>, number: u32, total: u32) -> Self {
        Self {
            step: step.into(),
            detail: None,
            status: "ok".into(),
            verbose_detail: None,
            step_number: Some(number),
            total_steps: Some(total),
            plan: None,
        }
    }

    pub fn ok_with_detail(
        step: impl Into<String>,
        number: u32,
        total: u32,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            step: step.into(),
            detail: Some(detail.into()),
            status: "ok".into(),
            verbose_detail: None,
            step_number: Some(number),
            total_steps: Some(total),
            plan: None,
        }
    }

    pub fn skip(step: impl Into<String>, number: u32, total: u32) -> Self {
        Self {
            step: step.into(),
            detail: None,
            status: "skip".into(),
            verbose_detail: None,
            step_number: Some(number),
            total_steps: Some(total),
            plan: None,
        }
    }

    pub fn with_verbose(mut self, detail: impl Into<String>) -> Self {
        self.verbose_detail = Some(detail.into());
        self
    }
}

/// Response after a successful build.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuildResponse {
    pub project: String,
    pub artifact_path: PathBuf,
    pub images_cached: usize,
    #[serde(default)]
    pub images_built: usize,
    pub secrets_extracted: usize,
    #[serde(default)]
    pub coast_image: Option<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub coastfile_type: Option<String>,
}

/// Response after successfully re-running secret extractors.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RerunExtractorsResponse {
    pub project: String,
    pub secrets_extracted: usize,
    pub warnings: Vec<String>,
}
