use std::path::PathBuf;

use coast_core::coastfile::Coastfile;
use coast_core::protocol::BuildProgressEvent;
use coast_docker::compose_build::{self, ComposeParseResult};

pub(super) struct ComposeAnalysis {
    pub content: Option<String>,
    pub dir: Option<PathBuf>,
    pub parse_result: Option<ComposeParseResult>,
}

impl ComposeAnalysis {
    pub(super) fn from_coastfile(coastfile: &Coastfile) -> Self {
        let content = coastfile
            .compose
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok());
        let dir = coastfile
            .compose
            .as_ref()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
        let parse_result = content.as_ref().and_then(|compose_content| {
            compose_build::parse_compose_file_filtered(
                compose_content,
                &coastfile.name,
                &coastfile.omit.services,
            )
            .ok()
        });

        Self {
            content,
            dir,
            parse_result,
        }
    }

    pub(super) fn has_build_directives(&self) -> bool {
        self.parse_result
            .as_ref()
            .is_some_and(|result| !result.build_directives.is_empty())
    }

    pub(super) fn has_image_refs(&self) -> bool {
        self.parse_result
            .as_ref()
            .is_some_and(|result| !result.image_refs.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BuildPlan {
    steps: Vec<String>,
}

impl BuildPlan {
    pub(super) fn from_inputs(
        has_secrets: bool,
        has_build_directives: bool,
        has_image_refs: bool,
        has_setup: bool,
    ) -> Self {
        let mut steps = vec!["Parsing Coastfile".to_string()];
        if has_secrets {
            steps.push("Extracting secrets".to_string());
        }
        steps.push("Creating artifact".to_string());
        if has_build_directives {
            steps.push("Building images".to_string());
        }
        if has_image_refs {
            steps.push("Pulling images".to_string());
        }
        if has_setup {
            steps.push("Building coast image".to_string());
        }
        steps.push("Writing manifest".to_string());
        Self { steps }
    }

    #[cfg(test)]
    pub(super) fn steps(&self) -> &[String] {
        &self.steps
    }

    pub(super) fn total_steps(&self) -> u32 {
        self.steps.len() as u32
    }

    pub(super) fn step_number(&self, name: &str) -> u32 {
        self.steps
            .iter()
            .position(|step| step == name)
            .map(|idx| (idx + 1) as u32)
            .expect("step not in plan")
    }

    pub(super) fn build_plan_event(&self) -> BuildProgressEvent {
        BuildProgressEvent::build_plan(self.steps.clone())
    }

    pub(super) fn started(&self, name: &str) -> BuildProgressEvent {
        BuildProgressEvent::started(name, self.step_number(name), self.total_steps())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_plan_includes_optional_steps_in_order() {
        let plan = BuildPlan::from_inputs(true, true, true, true);
        assert_eq!(
            plan.steps(),
            &[
                "Parsing Coastfile".to_string(),
                "Extracting secrets".to_string(),
                "Creating artifact".to_string(),
                "Building images".to_string(),
                "Pulling images".to_string(),
                "Building coast image".to_string(),
                "Writing manifest".to_string(),
            ]
        );
        assert_eq!(plan.step_number("Pulling images"), 5);
        assert_eq!(plan.total_steps(), 7);
    }

    #[test]
    fn test_build_plan_minimal_shape() {
        let plan = BuildPlan::from_inputs(false, false, false, false);
        assert_eq!(
            plan.steps(),
            &[
                "Parsing Coastfile".to_string(),
                "Creating artifact".to_string(),
                "Writing manifest".to_string(),
            ]
        );
        let event = plan.started("Creating artifact");
        assert_eq!(event.step, "Creating artifact");
        assert_eq!(event.status, "started");
        assert_eq!(event.step_number, Some(2));
        assert_eq!(event.total_steps, Some(3));
    }

    #[test]
    fn test_compose_analysis_detects_builds_and_images() {
        let dir = tempfile::tempdir().unwrap();
        let compose_path = dir.path().join("docker-compose.yml");
        std::fs::write(
            &compose_path,
            r#"services:
  app:
    build: .
  db:
    image: postgres:16
"#,
        )
        .unwrap();

        let coastfile = Coastfile::parse(
            r#"
[coast]
name = "plan-test"
compose = "./docker-compose.yml"
"#,
            dir.path(),
        )
        .unwrap();

        let analysis = ComposeAnalysis::from_coastfile(&coastfile);
        assert!(analysis.content.is_some());
        assert_eq!(analysis.dir, Some(dir.path().to_path_buf()));
        assert!(analysis.has_build_directives());
        assert!(analysis.has_image_refs());
    }
}
