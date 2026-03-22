/// Handler for the `coast run` command.
///
/// Creates a new coast instance: records it in the state DB,
/// creates the coast container with project root bind-mounted,
/// loads cached images, starts the inner compose stack, and allocates ports.
mod compose_rewrite;
mod finalize;
mod host_builds;
mod image_loading;
mod mcp_setup;
mod paths;
mod provision;
mod secrets;
mod service_start;
mod shared_services_setup;
mod validate;

use tracing::{info, warn};

use coast_core::error::Result;
use coast_core::protocol::{BuildProgressEvent, RunRequest, RunResponse};
use coast_core::types::PortMapping;

use crate::server::AppState;

fn emit(tx: &tokio::sync::mpsc::Sender<BuildProgressEvent>, event: BuildProgressEvent) {
    let _ = tx.try_send(event);
}

/// Resolve the per-type `latest` symlink to get the actual build_id for a project.
///
/// For the default type (None), reads `latest`. For a named type, reads `latest-{type}`.
pub fn resolve_latest_build_id(project: &str, coastfile_type: Option<&str>) -> Option<String> {
    let home = paths::active_coast_home();
    let latest_name = match coastfile_type {
        Some(t) => format!("latest-{t}"),
        None => "latest".to_string(),
    };
    let latest_link = home.join("images").join(project).join(latest_name);
    std::fs::read_link(&latest_link)
        .ok()
        .and_then(|target| target.file_name().map(|f| f.to_string_lossy().into_owned()))
}

fn port_mappings_from_pre_allocated_ports(
    pre_allocated_ports: &[(String, u16, u16)],
) -> Vec<PortMapping> {
    pre_allocated_ports
        .iter()
        .map(|(logical_name, canonical, dynamic)| PortMapping {
            logical_name: logical_name.clone(),
            canonical_port: *canonical,
            dynamic_port: *dynamic,
            is_primary: false,
        })
        .collect()
}

fn merge_dynamic_port_env_vars(
    env_vars: &mut std::collections::HashMap<String, String>,
    pre_allocated_ports: &[(String, u16, u16)],
) {
    let mappings = port_mappings_from_pre_allocated_ports(pre_allocated_ports);
    let dynamic_env = super::ports::dynamic_port_env_vars_from_mappings(&mappings);
    for (key, value) in dynamic_env {
        if env_vars.contains_key(&key) {
            warn!(
                env_var = %key,
                "dynamic port env var conflicts with existing env var; preserving existing value"
            );
            continue;
        }
        env_vars.insert(key, value);
    }
}

/// Detect whether the project uses compose, bare services, or neither.
///
/// Reads the coastfile from the build artifact to determine the startup mode and
/// extract the compose-relative directory for project naming.
fn detect_coastfile_info(
    project: &str,
    resolved_build_id: Option<&str>,
) -> (
    bool,
    Option<String>,
    bool,
    Vec<coast_core::types::BareServiceConfig>,
) {
    let project_dir = paths::project_images_dir(project);
    let coastfile_path = resolved_build_id
        .map(|bid| project_dir.join(bid).join("coastfile.toml"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| project_dir.join("coastfile.toml"));
    if !coastfile_path.exists() {
        return (true, None, false, vec![]);
    }
    let raw_text = std::fs::read_to_string(&coastfile_path).unwrap_or_default();
    let has_autostart_false = raw_text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "autostart = false" || trimmed.starts_with("autostart = false ")
    });
    if has_autostart_false {
        return (false, None, false, vec![]);
    }
    match coast_core::coastfile::Coastfile::from_file(&coastfile_path) {
        Ok(cf) => {
            let svc_list = cf.services.clone();
            let has_svc = !svc_list.is_empty();
            let rel_dir = cf.compose.as_ref().and_then(|p| {
                let parent = p.parent()?;
                let artifact_parent = coastfile_path.parent()?;
                if parent == artifact_parent {
                    return None;
                }
                parent
                    .strip_prefix(artifact_parent)
                    .ok()
                    .and_then(|rel| rel.to_str())
                    .filter(|s| !s.is_empty())
                    .map(std::string::ToString::to_string)
            });
            (cf.compose.is_some(), rel_dir, has_svc, svc_list)
        }
        Err(_) => (true, None, false, vec![]),
    }
}

/// Resolve the branch name: use explicit value if provided, otherwise detect from git HEAD.
async fn resolve_branch(
    explicit_branch: Option<&str>,
    project: &str,
    resolved_build_id: Option<&str>,
) -> Option<String> {
    if let Some(b) = explicit_branch {
        return Some(b.to_string());
    }
    let home = dirs::home_dir().unwrap_or_default();
    let project_dir = home.join(".coast").join("images").join(project);
    let manifest_path = resolved_build_id
        .map(|bid| project_dir.join(bid).join("manifest.json"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| project_dir.join("manifest.json"));
    let project_root = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("project_root")?
                    .as_str()
                    .map(std::path::PathBuf::from)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    } else {
        std::env::current_dir().unwrap_or_default()
    };
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&project_root)
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let b = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if b.is_empty() || b == "HEAD" {
                None
            } else {
                Some(b)
            }
        }
        _ => None,
    }
}

/// Handle a run request.
pub async fn handle(
    req: RunRequest,
    state: &AppState,
    progress: tokio::sync::mpsc::Sender<BuildProgressEvent>,
) -> Result<RunResponse> {
    info!(name = %req.name, project = %req.project, branch = ?req.branch, "handling run request");

    if state.docker.is_none() {
        return Err(coast_core::error::CoastError::docker(
            "Host Docker is not available. `coast run` requires a Docker-compatible host engine. \
             If you use Docker contexts (OrbStack, Colima, Rancher Desktop, Docker Desktop), \
             ensure coastd can resolve the active context and then restart the daemon.",
        ));
    }

    // Phase 1: Validate, resolve build_id, insert instance record
    let validated = validate::validate_and_insert(&req, state, &progress).await?;

    // Phase 2: Docker provisioning (container, images, services)
    let mut container_id = format!("{}-coasts-{}", req.project, req.name);
    let mut pre_allocated_ports: Vec<(String, u16, u16)> = Vec::new();

    if let Some(ref docker) = state.docker {
        let result =
            provision::provision_instance(docker, &validated, &req, state, &progress).await?;
        container_id = result.container_id;
        pre_allocated_ports = result.pre_allocated_ports;
    }

    // Phase 3: Finalize (port allocations, status transition)
    let ports = finalize::finalize_instance(
        state,
        &req.project,
        &req.name,
        &container_id,
        validated.build_id.as_deref(),
        &pre_allocated_ports,
        &validated.final_status,
        validated.total_steps,
        &progress,
    )
    .await?;

    // Phase 4: Optional worktree assignment
    if let Some(ref worktree_name) = req.worktree {
        assign_worktree(&req, worktree_name, state, &progress, validated.total_steps).await;
    }

    Ok(RunResponse {
        name: req.name,
        container_id,
        ports,
    })
}

async fn assign_worktree(
    req: &RunRequest,
    worktree_name: &str,
    state: &AppState,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    total_steps: u32,
) {
    info!(name = %req.name, worktree = %worktree_name, "auto-assigning worktree after provisioning");
    emit(
        progress,
        BuildProgressEvent::started("Assigning worktree", total_steps, total_steps),
    );

    let assign_req = coast_core::protocol::AssignRequest {
        name: req.name.clone(),
        project: req.project.clone(),
        worktree: worktree_name.to_string(),
        commit_sha: None,
        explain: false,
        force_sync: false,
    };

    match super::assign::handle(assign_req, state, progress.clone()).await {
        Ok(resp) => {
            emit(
                progress,
                BuildProgressEvent::done("Assigning worktree", "ok"),
            );
            state.emit_event(coast_core::protocol::CoastEvent::InstanceAssigned {
                name: req.name.clone(),
                project: req.project.clone(),
                worktree: resp.worktree,
            });
        }
        Err(e) => {
            emit(
                progress,
                BuildProgressEvent::item("Assigning worktree", format!("Warning: {e}"), "warn"),
            );
            emit(
                progress,
                BuildProgressEvent::done("Assigning worktree", "warn"),
            );
            warn!(
                name = %req.name, worktree = %worktree_name, error = %e,
                "post-provisioning worktree assignment failed (coast is still running)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDb;
    use coast_core::error::CoastError;
    use coast_core::types::{CoastInstance, InstanceStatus, RuntimeType};

    fn test_state() -> AppState {
        AppState::new_for_testing(StateDb::open_in_memory().unwrap())
    }

    fn test_state_with_docker() -> AppState {
        AppState::new_for_testing_with_docker(StateDb::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn test_run_without_docker_fails_before_inserting_instance() {
        let state = test_state();
        let req = RunRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            branch: Some("feature/oauth".to_string()),
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Host Docker is not available"));

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "feature-oauth").unwrap();
        assert!(instance.is_none());
    }

    #[tokio::test]
    async fn test_run_with_docker_stub_rejects_duplicate_instance() {
        let state = test_state_with_docker();
        {
            let db = state.db.lock().await;
            db.insert_instance(&CoastInstance {
                name: "dup".to_string(),
                project: "my-app".to_string(),
                status: InstanceStatus::Running,
                branch: None,
                commit_sha: None,
                container_id: Some("existing-container".to_string()),
                runtime: RuntimeType::Dind,
                created_at: chrono::Utc::now(),
                worktree_name: None,
                build_id: None,
                coastfile_type: None,
            })
            .unwrap();
        }

        let req = RunRequest {
            name: "dup".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));

        let db = state.db.lock().await;
        let instance = db.get_instance("my-app", "dup").unwrap().unwrap();
        assert_eq!(instance.status, InstanceStatus::Running);
    }

    #[test]
    fn test_port_mappings_from_pre_allocated_ports() {
        let pre_allocated = vec![
            ("web".to_string(), 3000, 52340),
            ("backend-test".to_string(), 8080, 52341),
        ];
        let mappings = port_mappings_from_pre_allocated_ports(&pre_allocated);
        assert_eq!(mappings.len(), 2);
        assert_eq!(mappings[0].logical_name, "web");
        assert_eq!(mappings[0].canonical_port, 3000);
        assert_eq!(mappings[0].dynamic_port, 52340);
        assert_eq!(mappings[1].logical_name, "backend-test");
        assert_eq!(mappings[1].canonical_port, 8080);
        assert_eq!(mappings[1].dynamic_port, 52341);
    }

    #[test]
    fn test_merge_dynamic_port_env_vars_inserts_vars() {
        let pre_allocated = vec![
            ("web".to_string(), 3000, 52340),
            ("backend-test".to_string(), 8080, 52341),
        ];
        let mut env = std::collections::HashMap::new();
        merge_dynamic_port_env_vars(&mut env, &pre_allocated);
        assert_eq!(env.get("WEB_DYNAMIC_PORT"), Some(&"52340".to_string()));
        assert_eq!(
            env.get("BACKEND_TEST_DYNAMIC_PORT"),
            Some(&"52341".to_string())
        );
    }

    #[test]
    fn test_merge_dynamic_port_env_vars_preserves_existing_key() {
        let pre_allocated = vec![("web".to_string(), 3000, 52340)];
        let mut env = std::collections::HashMap::new();
        env.insert("WEB_DYNAMIC_PORT".to_string(), "9999".to_string());
        merge_dynamic_port_env_vars(&mut env, &pre_allocated);
        assert_eq!(env.get("WEB_DYNAMIC_PORT"), Some(&"9999".to_string()));
    }

    #[test]
    fn test_expected_container_name_for_dangling_check() {
        let project = "my-app";
        let name = "dev-1";
        let expected = format!("{}-coasts-{}", project, name);
        assert_eq!(expected, "my-app-coasts-dev-1");
    }

    #[tokio::test]
    async fn test_run_with_force_remove_dangling_still_fails_without_docker() {
        let state = test_state();
        let req = RunRequest {
            name: "force-test".to_string(),
            project: "my-app".to_string(),
            branch: None,
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: true,
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let result = handle(req, &state, tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Host Docker is not available"));
    }

    #[test]
    fn test_dangling_container_error_is_actionable() {
        let err = CoastError::DanglingContainerDetected {
            name: "dev-1".to_string(),
            project: "my-app".to_string(),
            container_name: "my-app-coasts-dev-1".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("--force-remove-dangling"),
            "error should contain the flag hint"
        );
        assert!(
            msg.contains("coast run dev-1"),
            "error should contain the suggested command"
        );
    }

    #[test]
    fn test_dangling_cache_volume_name() {
        let vol = coast_docker::dind::dind_cache_volume_name("my-app", "dev-1");
        assert_eq!(vol, "coast-dind--my-app--dev-1");
    }
}
