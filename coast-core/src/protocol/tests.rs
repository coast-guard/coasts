use std::path::PathBuf;

use crate::types::{InstanceStatus, PortMapping, RuntimeType, SharedServicePort};

use super::*;

fn roundtrip_request(req: Request) {
    let encoded = encode_request(&req).unwrap();
    // Remove trailing newline for decode
    let decoded = decode_request(&encoded[..encoded.len() - 1]).unwrap();
    let re_encoded = encode_request(&decoded).unwrap();
    assert_eq!(encoded, re_encoded);
}

fn roundtrip_response(resp: Response) {
    let encoded = encode_response(&resp).unwrap();
    let decoded = decode_response(&encoded[..encoded.len() - 1]).unwrap();
    let re_encoded = encode_response(&decoded).unwrap();
    assert_eq!(encoded, re_encoded);
}

#[test]
fn test_build_request_roundtrip() {
    roundtrip_request(Request::Build(BuildRequest {
        coastfile_path: PathBuf::from("/home/user/Coastfile"),
        refresh: true,
    }));
}

#[test]
fn test_rerun_extractors_request_roundtrip() {
    roundtrip_request(Request::RerunExtractors(RerunExtractorsRequest {
        project: "my-app".to_string(),
        build_id: None,
    }));
}

#[test]
fn test_rerun_extractors_request_with_build_id_roundtrip() {
    roundtrip_request(Request::RerunExtractors(RerunExtractorsRequest {
        project: "my-app".to_string(),
        build_id: Some("a3c7d783".to_string()),
    }));
}

#[test]
fn test_run_request_roundtrip() {
    roundtrip_request(Request::Run(RunRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        branch: Some("feature/oauth".to_string()),
        commit_sha: Some("abc123def456".to_string()),
        worktree: None,
        build_id: None,
        coastfile_type: None,
        force_remove_dangling: false,
    }));
}

#[test]
fn test_run_request_without_commit_sha_roundtrip() {
    roundtrip_request(Request::Run(RunRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        branch: Some("feature/oauth".to_string()),
        commit_sha: None,
        worktree: None,
        build_id: None,
        coastfile_type: None,
        force_remove_dangling: false,
    }));
}

#[test]
fn test_run_request_with_worktree_roundtrip() {
    roundtrip_request(Request::Run(RunRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
        branch: None,
        commit_sha: None,
        worktree: Some("feature/my-worktree".to_string()),
        build_id: None,
        coastfile_type: None,
        force_remove_dangling: false,
    }));
}

#[test]
fn test_assign_request_roundtrip() {
    roundtrip_request(Request::Assign(AssignRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
        worktree: "feature/oauth".to_string(),
        commit_sha: Some("deadbeef".to_string()),
        explain: false,
        force_sync: false,
    }));
}

#[test]
fn test_assign_request_without_commit_sha_roundtrip() {
    roundtrip_request(Request::Assign(AssignRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
        worktree: "feature/oauth".to_string(),
        commit_sha: None,
        explain: false,
        force_sync: false,
    }));
}

#[test]
fn test_unassign_request_roundtrip() {
    roundtrip_request(Request::Unassign(UnassignRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_unassign_response_roundtrip() {
    roundtrip_response(Response::Unassign(UnassignResponse {
        name: "dev-1".to_string(),
        worktree: "main".to_string(),
        previous_worktree: Some("feature/oauth".to_string()),
        time_elapsed_ms: 3456,
    }));
}

#[test]
fn test_unassign_response_no_previous_roundtrip() {
    roundtrip_response(Response::Unassign(UnassignResponse {
        name: "dev-1".to_string(),
        worktree: "main".to_string(),
        previous_worktree: None,
        time_elapsed_ms: 100,
    }));
}

#[test]
fn test_unassign_progress_roundtrip() {
    roundtrip_response(Response::UnassignProgress(BuildProgressEvent::started(
        "Validating instance",
        1,
        7,
    )));
}

#[test]
fn test_assign_response_roundtrip() {
    roundtrip_response(Response::Assign(AssignResponse {
        name: "dev-1".to_string(),
        worktree: "feature/oauth".to_string(),
        previous_worktree: Some("feature/billing".to_string()),
        time_elapsed_ms: 1234,
    }));
}

#[test]
fn test_assign_response_no_previous_worktree_roundtrip() {
    roundtrip_response(Response::Assign(AssignResponse {
        name: "dev-1".to_string(),
        worktree: "feature/oauth".to_string(),
        previous_worktree: None,
        time_elapsed_ms: 567,
    }));
}

#[test]
fn test_assign_progress_roundtrip() {
    roundtrip_response(Response::AssignProgress(BuildProgressEvent::started(
        "Validating instance",
        1,
        7,
    )));
}

#[test]
fn test_stop_request_roundtrip() {
    roundtrip_request(Request::Stop(StopRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_start_request_roundtrip() {
    roundtrip_request(Request::Start(StartRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_rm_request_roundtrip() {
    roundtrip_request(Request::Rm(RmRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_checkout_request_roundtrip() {
    roundtrip_request(Request::Checkout(CheckoutRequest {
        name: Some("feature-oauth".to_string()),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_checkout_none_request_roundtrip() {
    roundtrip_request(Request::Checkout(CheckoutRequest {
        name: None,
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_ports_list_request_roundtrip() {
    roundtrip_request(Request::Ports(PortsRequest::List {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_ports_set_primary_request_roundtrip() {
    roundtrip_request(Request::Ports(PortsRequest::SetPrimary {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        service: "web".to_string(),
    }));
}

#[test]
fn test_ports_unset_primary_request_roundtrip() {
    roundtrip_request(Request::Ports(PortsRequest::UnsetPrimary {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_exec_request_roundtrip() {
    roundtrip_request(Request::Exec(ExecRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        service: Some("web".to_string()),
        root: true,
        command: vec!["bash".to_string()],
    }));
}

#[test]
fn test_logs_request_roundtrip() {
    roundtrip_request(Request::Logs(LogsRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        service: Some("web".to_string()),
        tail: Some(50),
        tail_all: false,
        follow: true,
    }));
}

#[test]
fn test_logs_request_backward_compat_without_tail() {
    let payload = br#"{"type":"Logs","name":"feature-oauth","project":"my-app","service":"web","follow":true}"#;
    let req = decode_request(payload).unwrap();
    match req {
        Request::Logs(logs) => {
            assert_eq!(logs.name, "feature-oauth");
            assert_eq!(logs.project, "my-app");
            assert_eq!(logs.service.as_deref(), Some("web"));
            assert_eq!(logs.tail, None);
            assert!(!logs.tail_all);
            assert!(logs.follow);
        }
        _ => panic!("expected logs request"),
    }
}

#[test]
fn test_ps_request_roundtrip() {
    roundtrip_request(Request::Ps(PsRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_ls_request_roundtrip() {
    roundtrip_request(Request::Ls(LsRequest {
        project: Some("my-app".to_string()),
    }));
}

#[test]
fn test_docs_request_roundtrip() {
    roundtrip_request(Request::Docs(DocsRequest {
        path: Some("coastfiles/COASTFILE".to_string()),
        language: Some("ja".to_string()),
    }));
}

#[test]
fn test_search_docs_request_roundtrip() {
    roundtrip_request(Request::SearchDocs(SearchDocsRequest {
        query: "shared services".to_string(),
        limit: Some(10),
        language: Some("en".to_string()),
    }));
}

#[test]
fn test_secret_set_request_roundtrip() {
    roundtrip_request(Request::Secret(SecretRequest::Set {
        instance: "feature-oauth".to_string(),
        project: "my-app".to_string(),
        name: "API_KEY".to_string(),
        value: "secret123".to_string(),
    }));
}

#[test]
fn test_secret_list_request_roundtrip() {
    roundtrip_request(Request::Secret(SecretRequest::List {
        instance: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_shared_ps_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Ps {
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_shared_stop_single_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Stop {
        project: "my-app".to_string(),
        service: Some("postgres".to_string()),
    }));
}

#[test]
fn test_shared_stop_all_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Stop {
        project: "my-app".to_string(),
        service: None,
    }));
}

#[test]
fn test_shared_start_all_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Start {
        project: "my-app".to_string(),
        service: None,
    }));
}

#[test]
fn test_shared_restart_single_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Restart {
        project: "my-app".to_string(),
        service: Some("redis".to_string()),
    }));
}

#[test]
fn test_shared_rm_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Rm {
        project: "my-app".to_string(),
        service: "postgres".to_string(),
    }));
}

#[test]
fn test_build_progress_roundtrip() {
    roundtrip_response(Response::BuildProgress(BuildProgressEvent::item(
        "Extracting secrets",
        "macos-keychain -> claude.json",
        "ok",
    )));
}

#[test]
fn test_build_progress_with_step_numbers() {
    roundtrip_response(Response::BuildProgress(BuildProgressEvent::started(
        "Parsing Coastfile",
        1,
        5,
    )));
}

#[test]
fn test_build_plan_roundtrip() {
    roundtrip_response(Response::BuildProgress(BuildProgressEvent::build_plan(
        vec![
            "Parsing Coastfile".into(),
            "Extracting secrets".into(),
            "Creating artifact".into(),
        ],
    )));
}

#[test]
fn test_build_progress_verbose_roundtrip() {
    roundtrip_response(Response::BuildProgress(
        BuildProgressEvent::item("Building images", "app", "ok")
            .with_verbose("Step 1/5: FROM node:20-alpine\n..."),
    ));
}

#[test]
fn test_rerun_extractors_progress_roundtrip() {
    roundtrip_response(Response::RerunExtractorsProgress(
        BuildProgressEvent::started("Extracting secrets", 1, 1),
    ));
}

#[test]
fn test_build_response_roundtrip() {
    roundtrip_response(Response::Build(BuildResponse {
        project: "my-app".to_string(),
        artifact_path: PathBuf::from("/home/user/.coast/images/my-app"),
        images_cached: 3,
        images_built: 1,
        secrets_extracted: 2,
        coast_image: None,
        warnings: vec!["shared volume warning".to_string()],
        coastfile_type: None,
    }));
}

#[test]
fn test_build_response_with_coast_image_roundtrip() {
    roundtrip_response(Response::Build(BuildResponse {
        project: "coast-claude".to_string(),
        artifact_path: PathBuf::from("/home/user/.coast/images/coast-claude"),
        images_cached: 1,
        images_built: 0,
        secrets_extracted: 1,
        coast_image: Some("coast-image/coast-claude:latest".to_string()),
        warnings: vec![],
        coastfile_type: None,
    }));
}

#[test]
fn test_rerun_extractors_response_roundtrip() {
    roundtrip_response(Response::RerunExtractors(RerunExtractorsResponse {
        project: "my-app".to_string(),
        secrets_extracted: 2,
        warnings: vec!["warning".to_string()],
    }));
}

#[test]
fn test_run_response_roundtrip() {
    roundtrip_response(Response::Run(RunResponse {
        name: "feature-oauth".to_string(),
        container_id: "abc123def".to_string(),
        ports: vec![PortMapping {
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            is_primary: false,
        }],
    }));
}

#[test]
fn test_stop_response_roundtrip() {
    roundtrip_response(Response::Stop(StopResponse {
        name: "feature-oauth".to_string(),
    }));
}

#[test]
fn test_start_response_roundtrip() {
    roundtrip_response(Response::Start(StartResponse {
        name: "feature-oauth".to_string(),
        ports: vec![],
    }));
}

#[test]
fn test_rm_response_roundtrip() {
    roundtrip_response(Response::Rm(RmResponse {
        name: "feature-oauth".to_string(),
    }));
}

#[test]
fn test_rebuild_request_roundtrip() {
    roundtrip_request(Request::Rebuild(RebuildRequest {
        name: "feature-oauth".to_string(),
        project: "my-app".to_string(),
    }));
}

#[test]
fn test_rebuild_response_roundtrip() {
    roundtrip_response(Response::Rebuild(RebuildResponse {
        name: "feature-oauth".to_string(),
        services_rebuilt: vec!["web".to_string(), "worker".to_string()],
    }));
}

#[test]
fn test_checkout_response_roundtrip() {
    roundtrip_response(Response::Checkout(CheckoutResponse {
        checked_out: Some("feature-oauth".to_string()),
        ports: vec![PortMapping {
            logical_name: "web".to_string(),
            canonical_port: 3000,
            dynamic_port: 52340,
            is_primary: false,
        }],
    }));
}

#[test]
fn test_ports_response_roundtrip() {
    roundtrip_response(Response::Ports(PortsResponse {
        name: "feature-oauth".to_string(),
        ports: vec![
            PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: true,
            },
            PortMapping {
                logical_name: "postgres".to_string(),
                canonical_port: 5432,
                dynamic_port: 52341,
                is_primary: false,
            },
        ],
        message: None,
        subdomain_host: None,
    }));
}

#[test]
fn test_ports_response_with_message_roundtrip() {
    roundtrip_response(Response::Ports(PortsResponse {
        name: "feat-a".to_string(),
        ports: vec![],
        message: Some("Primary service set to 'web'".to_string()),
        subdomain_host: None,
    }));
}

#[test]
fn test_exec_response_roundtrip() {
    roundtrip_response(Response::Exec(ExecResponse {
        exit_code: 0,
        stdout: "hello world\n".to_string(),
        stderr: String::new(),
    }));
}

#[test]
fn test_logs_response_roundtrip() {
    roundtrip_response(Response::Logs(LogsResponse {
        output: "web_1 | Server started on :3000\n".to_string(),
    }));
}

#[test]
fn test_logs_progress_response_roundtrip() {
    roundtrip_response(Response::LogsProgress(LogsResponse {
        output: "web_1 | chunk\n".to_string(),
    }));
}

#[test]
fn test_ps_response_roundtrip() {
    roundtrip_response(Response::Ps(PsResponse {
        name: "feature-oauth".to_string(),
        services: vec![ServiceStatus {
            name: "web".to_string(),
            status: "Up 5 minutes".to_string(),
            ports: "0.0.0.0:3000->3000/tcp".to_string(),
            image: "myapp-web:latest".to_string(),
            kind: Some("compose".to_string()),
        }],
    }));
}

#[test]
fn test_ls_response_roundtrip() {
    roundtrip_response(Response::Ls(LsResponse {
        instances: vec![InstanceSummary {
            name: "main".to_string(),
            project: "my-app".to_string(),
            status: InstanceStatus::Running,
            branch: Some("main".to_string()),
            runtime: RuntimeType::Dind,
            checked_out: true,
            project_root: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            port_count: 3,
            primary_port_service: None,
            primary_port_canonical: None,
            primary_port_dynamic: None,
            primary_port_url: None,
            down_service_count: 0,
            remote_name: None,
        }],
        known_projects: Vec::new(),
    }));
}

#[test]
fn test_docs_response_roundtrip() {
    roundtrip_response(Response::Docs(DocsResponse {
        locale: "en".to_string(),
        tree: vec![
            DocsNode {
                name: "README.md".to_string(),
                path: "README.md".to_string(),
                kind: DocsNodeKind::File,
                children: Vec::new(),
            },
            DocsNode {
                name: "coastfiles".to_string(),
                path: "coastfiles".to_string(),
                kind: DocsNodeKind::Dir,
                children: vec![DocsNode {
                    name: "COASTFILE.md".to_string(),
                    path: "coastfiles/COASTFILE.md".to_string(),
                    kind: DocsNodeKind::File,
                    children: Vec::new(),
                }],
            },
        ],
        path: Some("README.md".to_string()),
        content: Some("# Coast Docs".to_string()),
    }));
}

#[test]
fn test_search_docs_response_roundtrip() {
    roundtrip_response(Response::SearchDocs(SearchDocsResponse {
        query: "coastfile runtime".to_string(),
        locale: "en".to_string(),
        strategy: "hybrid_keyword_semantic".to_string(),
        results: vec![SearchDocsResult {
            path: "coastfiles/COASTFILE.md".to_string(),
            route: "/docs/coastfiles/COASTFILE".to_string(),
            heading: "Coastfile Reference".to_string(),
            snippet: "The Coastfile is a TOML configuration file...".to_string(),
            score: 1.2345,
        }],
    }));
}

#[test]
fn test_secret_response_roundtrip() {
    roundtrip_response(Response::Secret(SecretResponse {
        message: "Secret set".to_string(),
        secrets: vec![SecretInfo {
            name: "API_KEY".to_string(),
            extractor: "env".to_string(),
            inject: "env:API_KEY".to_string(),
            is_override: false,
        }],
    }));
}

#[test]
fn test_shared_response_roundtrip() {
    roundtrip_response(Response::Shared(SharedResponse {
        message: "Shared services".to_string(),
        services: vec![SharedServiceInfo {
            name: "postgres".to_string(),
            container_id: Some("abc123".to_string()),
            status: "running".to_string(),
            image: Some("postgres:15".to_string()),
            ports: Some("0.0.0.0:5432->5432/tcp".to_string()),
        }],
    }));
}

#[test]
fn test_error_response_roundtrip() {
    roundtrip_response(Response::Error(ErrorResponse {
        error: "instance not found".to_string(),
    }));
}

#[test]
fn test_builds_ls_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::Ls { project: None }));
}

#[test]
fn test_builds_ls_project_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::Ls {
        project: Some("my-app".to_string()),
    }));
}

#[test]
fn test_builds_inspect_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::Inspect {
        project: "my-app".to_string(),
        build_id: Some("a3c7d783".to_string()),
    }));
}

#[test]
fn test_builds_images_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::Images {
        project: "my-app".to_string(),
        build_id: None,
    }));
}

#[test]
fn test_builds_docker_images_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::DockerImages {
        project: "my-app".to_string(),
        build_id: None,
    }));
}

#[test]
fn test_builds_inspect_docker_image_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::InspectDockerImage {
        project: "my-app".to_string(),
        image: "coast-image/my-app:latest".to_string(),
    }));
}

#[test]
fn test_builds_compose_request_roundtrip() {
    roundtrip_request(Request::Builds(BuildsRequest::Compose {
        project: "my-app".to_string(),
        build_id: None,
    }));
}

#[test]
fn test_builds_ls_response_roundtrip() {
    roundtrip_response(Response::Builds(Box::new(BuildsResponse::Ls(
        BuildsLsResponse {
            builds: vec![BuildSummary {
                project: "my-app".to_string(),
                build_id: Some("a3c7d783".to_string()),
                is_latest: true,
                project_root: Some("/home/user/my-app".to_string()),
                build_timestamp: Some("2026-01-01T00:00:00Z".to_string()),
                images_cached: 10,
                images_built: 3,
                secrets_count: 2,
                coast_image: Some("coast-image/my-app:latest".to_string()),
                cache_size_bytes: 1_073_741_824,
                instance_count: 3,
                running_count: 2,
                archived: false,
                instances_using: 2,
                coastfile_type: None,
            }],
        },
    ))));
}

#[test]
fn test_builds_inspect_response_roundtrip() {
    roundtrip_response(Response::Builds(Box::new(BuildsResponse::Inspect(
        Box::new(BuildsInspectResponse {
            project: "my-app".to_string(),
            build_id: Some("a3c7d783".to_string()),
            project_root: Some("/home/user/my-app".to_string()),
            build_timestamp: Some("2026-01-01T00:00:00Z".to_string()),
            coastfile_hash: Some("abc123".to_string()),
            coast_image: Some("coast-image/my-app:latest".to_string()),
            artifact_path: "/home/user/.coast/images/my-app".to_string(),
            artifact_size_bytes: 16384,
            images_cached: 10,
            images_built: 3,
            cache_size_bytes: 1_073_741_824,
            secrets: vec!["db_password".to_string()],
            built_services: vec!["backend".to_string()],
            pulled_images: vec!["postgres:15".to_string()],
            base_images: vec!["golang:1.24-alpine".to_string()],
            omitted_services: vec!["debug".to_string()],
            omitted_volumes: vec![],
            mcp_servers: vec![McpBuildInfo {
                name: "context7".to_string(),
                proxy: None,
                command: Some("npx".to_string()),
                args: vec!["-y".to_string(), "@upstash/context7-mcp".to_string()],
            }],
            mcp_clients: vec![McpClientBuildInfo {
                name: "claude-code".to_string(),
                format: Some("claude-code".to_string()),
                config_path: Some("/root/.claude/mcp_servers.json".to_string()),
            }],
            shared_services: vec![SharedServiceBuildInfo {
                name: "postgres".to_string(),
                image: "postgres:15".to_string(),
                ports: vec![SharedServicePort::same(5432)],
                auto_create_db: false,
            }],
            volumes: vec![VolumeBuildInfo {
                name: "go_modules_cache".to_string(),
                strategy: "shared".to_string(),
                service: "backend".to_string(),
                mount: "/go/pkg/mod".to_string(),
                snapshot_source: None,
            }],
            instances: vec![],
            docker_images: vec![],
            coastfile_type: None,
        }),
    ))));
}

#[test]
fn test_builds_content_response_roundtrip() {
    roundtrip_response(Response::Builds(Box::new(BuildsResponse::Content(
        BuildsContentResponse {
            content: "services:\n  web:\n    image: nginx\n".to_string(),
            file_type: "compose".to_string(),
        },
    ))));
}

#[test]
fn test_builds_docker_images_response_roundtrip() {
    roundtrip_response(Response::Builds(Box::new(BuildsResponse::DockerImages(
        BuildsDockerImagesResponse {
            images: vec![DockerImageInfo {
                id: "sha256:abc123".to_string(),
                repository: "coast-built/my-app/backend".to_string(),
                tag: "latest".to_string(),
                created: "2 hours ago".to_string(),
                size: "50 MB".to_string(),
                size_bytes: 52_428_800,
            }],
        },
    ))));
}

#[test]
fn test_agent_shell_ls_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::Ls {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
    }));
}

#[test]
fn test_agent_shell_activate_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::Activate {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        shell_id: 2,
    }));
}

#[test]
fn test_agent_shell_spawn_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::Spawn {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        activate: false,
    }));
}

#[test]
fn test_agent_shell_tty_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::Tty {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        shell_id: Some(1),
    }));
}

#[test]
fn test_agent_shell_tty_input_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::TtyInput {
        data: "ls -la\r".to_string(),
    }));
}

#[test]
fn test_agent_shell_tty_detach_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::TtyDetach));
}

#[test]
fn test_agent_shell_read_last_lines_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::ReadLastLines {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        lines: 200,
        shell_id: None,
    }));
}

#[test]
fn test_agent_shell_read_output_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::ReadOutput {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        shell_id: Some(3),
    }));
}

#[test]
fn test_agent_shell_input_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::Input {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        input: "hello".to_string(),
        shell_id: Some(2),
    }));
}

#[test]
fn test_agent_shell_session_status_request_roundtrip() {
    roundtrip_request(Request::AgentShell(AgentShellRequest::SessionStatus {
        project: "my-app".to_string(),
        name: "dev-1".to_string(),
        shell_id: None,
    }));
}

#[test]
fn test_agent_shell_response_roundtrip() {
    roundtrip_response(Response::AgentShell(AgentShellResponse::Ls(
        AgentShellLsResponse {
            name: "dev-1".to_string(),
            shells: vec![
                AgentShellSummary {
                    shell_id: 1,
                    is_active: true,
                    status: "running".to_string(),
                    is_live: true,
                },
                AgentShellSummary {
                    shell_id: 2,
                    is_active: false,
                    status: "running".to_string(),
                    is_live: false,
                },
            ],
        },
    )));
}

#[test]
fn test_agent_shell_tty_output_response_roundtrip() {
    roundtrip_response(Response::AgentShell(AgentShellResponse::TtyOutput(
        AgentShellTtyOutputResponse {
            data: "output chunk".to_string(),
        },
    )));
}

#[test]
fn test_decode_invalid_json() {
    let result = decode_request(b"not json");
    assert!(result.is_err());
}

#[test]
fn test_decode_empty() {
    let result = decode_request(b"");
    assert!(result.is_err());
}

#[test]
fn test_set_language_request_roundtrip() {
    let req = Request::SetLanguage(SetLanguageRequest {
        language: "zh".to_string(),
    });
    let encoded = encode_request(&req).unwrap();
    let decoded = decode_request(&encoded[..encoded.len() - 1]).unwrap();
    match decoded {
        Request::SetLanguage(ref inner) => assert_eq!(inner.language, "zh"),
        _ => panic!("expected SetLanguage request"),
    }
    roundtrip_request(req);
}

#[test]
fn test_set_language_response_roundtrip() {
    let resp = Response::SetLanguage(SetLanguageResponse {
        language: "ja".to_string(),
    });
    let encoded = encode_response(&resp).unwrap();
    let decoded = decode_response(&encoded[..encoded.len() - 1]).unwrap();
    match decoded {
        Response::SetLanguage(ref inner) => assert_eq!(inner.language, "ja"),
        _ => panic!("expected SetLanguage response"),
    }
    roundtrip_response(resp);
}

#[test]
fn test_config_language_changed_event_serialization() {
    let event = CoastEvent::ConfigLanguageChanged {
        language: "es".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "config.language_changed");
    assert_eq!(json["language"], "es");
}

#[test]
fn test_docker_info_response_serialization() {
    let resp = DockerInfoResponse {
        connected: true,
        mem_total_bytes: 8_589_934_592,
        cpus: 4,
        os: "Docker Desktop".to_string(),
        server_version: "28.3.3".to_string(),
        can_adjust: true,
        provider: "docker-desktop".to_string(),
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["mem_total_bytes"], 8_589_934_592u64);
    assert_eq!(json["cpus"], 4);
    assert_eq!(json["os"], "Docker Desktop");
    assert_eq!(json["server_version"], "28.3.3");
    assert_eq!(json["can_adjust"], true);
}

#[test]
fn test_set_analytics_enable_request_roundtrip() {
    roundtrip_request(Request::SetAnalytics(SetAnalyticsRequest {
        action: AnalyticsAction::Enable,
    }));
}

#[test]
fn test_set_analytics_disable_request_roundtrip() {
    roundtrip_request(Request::SetAnalytics(SetAnalyticsRequest {
        action: AnalyticsAction::Disable,
    }));
}

#[test]
fn test_set_analytics_status_request_roundtrip() {
    roundtrip_request(Request::SetAnalytics(SetAnalyticsRequest {
        action: AnalyticsAction::Status,
    }));
}

#[test]
fn test_set_analytics_response_enabled_roundtrip() {
    roundtrip_response(Response::SetAnalytics(SetAnalyticsResponse {
        enabled: true,
    }));
}

#[test]
fn test_set_analytics_response_disabled_roundtrip() {
    roundtrip_response(Response::SetAnalytics(SetAnalyticsResponse {
        enabled: false,
    }));
}

#[test]
fn test_update_safety_request_roundtrip() {
    roundtrip_request(Request::IsSafeToUpdate(UpdateSafetyRequest::default()));
}

#[test]
fn test_prepare_for_update_request_roundtrip() {
    roundtrip_request(Request::PrepareForUpdate(PrepareForUpdateRequest {
        timeout_ms: Some(45_000),
        close_sessions: true,
        stop_running_instances: false,
        stop_shared_services: false,
    }));
}

#[test]
fn test_update_safety_response_roundtrip() {
    roundtrip_response(Response::UpdateSafety(UpdateSafetyResponse {
        safe: false,
        quiescing: true,
        blockers: vec![UpdateSafetyIssue {
            kind: UpdateSafetyIssueKind::ActiveOperation,
            project: Some("my-app".to_string()),
            instance: Some("dev-1".to_string()),
            operation: Some("assign".to_string()),
            summary: "Assign is still in progress".to_string(),
            suggested_action: Some("Wait for the operation to finish.".to_string()),
        }],
        warnings: vec![UpdateSafetyIssue {
            kind: UpdateSafetyIssueKind::InteractiveSession,
            project: Some("my-app".to_string()),
            instance: Some("dev-1".to_string()),
            operation: None,
            summary: "An exec session will be disconnected during update.".to_string(),
            suggested_action: None,
        }],
    }));
}

#[test]
fn test_prepare_for_update_response_roundtrip() {
    roundtrip_response(Response::PrepareForUpdate(PrepareForUpdateResponse {
        ready: true,
        quiescing: true,
        timed_out: false,
        actions: vec![
            "Blocked new mutating requests.".to_string(),
            "Closed 2 interactive session(s).".to_string(),
        ],
        report: UpdateSafetyResponse {
            safe: true,
            quiescing: true,
            blockers: Vec::new(),
            warnings: Vec::new(),
        },
    }));
}

#[test]
fn test_config_analytics_changed_event_serialization() {
    let event = CoastEvent::ConfigAnalyticsChanged { enabled: true };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "config.analytics_changed");
    assert_eq!(json["enabled"], true);
}

#[test]
fn test_config_analytics_changed_event_disabled_serialization() {
    let event = CoastEvent::ConfigAnalyticsChanged { enabled: false };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "config.analytics_changed");
    assert_eq!(json["enabled"], false);
}

#[test]
fn test_open_docker_settings_response_serialization() {
    let resp = OpenDockerSettingsResponse { success: true };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["success"], true);
}

#[test]
fn test_shared_start_single_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Start {
        project: "my-app".to_string(),
        service: Some("postgres".to_string()),
    }));
}

#[test]
fn test_shared_restart_all_request_roundtrip() {
    roundtrip_request(Request::Shared(SharedRequest::Restart {
        project: "my-app".to_string(),
        service: None,
    }));
}

#[test]
fn test_shared_response_empty_fields_roundtrip() {
    roundtrip_response(Response::Shared(SharedResponse {
        message: "ok".to_string(),
        services: vec![SharedServiceInfo {
            name: "redis".to_string(),
            container_id: None,
            status: "stopped".to_string(),
            image: None,
            ports: None,
        }],
    }));
}

#[test]
fn test_shared_service_starting_event_serialization() {
    let event = CoastEvent::SharedServiceStarting {
        project: "my-app".to_string(),
        service: "postgres".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.starting");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "postgres");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceStarting { .. }
    ));
}

#[test]
fn test_shared_service_started_event_serialization() {
    let event = CoastEvent::SharedServiceStarted {
        project: "my-app".to_string(),
        service: "postgres".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.started");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "postgres");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceStarted { .. }
    ));
}

#[test]
fn test_shared_service_stopped_event_serialization() {
    let event = CoastEvent::SharedServiceStopped {
        project: "my-app".to_string(),
        service: "redis".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.stopped");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "redis");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceStopped { .. }
    ));
}

#[test]
fn test_shared_service_restarted_event_serialization() {
    let event = CoastEvent::SharedServiceRestarted {
        project: "my-app".to_string(),
        service: "postgres".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.restarted");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "postgres");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceRestarted { .. }
    ));
}

#[test]
fn test_shared_service_removed_event_serialization() {
    let event = CoastEvent::SharedServiceRemoved {
        project: "my-app".to_string(),
        service: "redis".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.removed");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "redis");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceRemoved { .. }
    ));
}

#[test]
fn test_shared_service_error_event_serialization() {
    let event = CoastEvent::SharedServiceError {
        project: "my-app".to_string(),
        service: "postgres".to_string(),
        error: "connection refused".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "shared_service.error");
    assert_eq!(json["project"], "my-app");
    assert_eq!(json["service"], "postgres");
    assert_eq!(json["error"], "connection refused");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(
        deserialized,
        CoastEvent::SharedServiceError { .. }
    ));
}

// --- Lookup ---

#[test]
fn test_lookup_request_roundtrip_with_worktree() {
    roundtrip_request(Request::Lookup(LookupRequest {
        project: "my-app".to_string(),
        worktree: Some("feature-alpha".to_string()),
    }));
}

#[test]
fn test_lookup_request_roundtrip_without_worktree() {
    roundtrip_request(Request::Lookup(LookupRequest {
        project: "my-app".to_string(),
        worktree: None,
    }));
}

#[test]
fn test_lookup_response_roundtrip_with_instances() {
    roundtrip_response(Response::Lookup(LookupResponse {
        project: "my-app".to_string(),
        worktree: Some("feature-alpha".to_string()),
        project_root: Some("/home/user/dev/my-app".to_string()),
        instances: vec![
            LookupInstance {
                name: "dev-1".to_string(),
                status: InstanceStatus::Running,
                checked_out: false,
                branch: Some("feature-alpha".to_string()),
                primary_url: Some("http://localhost:52340".to_string()),
                ports: vec![
                    PortMapping {
                        logical_name: "web".to_string(),
                        canonical_port: 3000,
                        dynamic_port: 52340,
                        is_primary: true,
                    },
                    PortMapping {
                        logical_name: "db".to_string(),
                        canonical_port: 5432,
                        dynamic_port: 55681,
                        is_primary: false,
                    },
                ],
            },
            LookupInstance {
                name: "dev-2".to_string(),
                status: InstanceStatus::CheckedOut,
                checked_out: true,
                branch: Some("feature-alpha".to_string()),
                primary_url: None,
                ports: vec![],
            },
        ],
    }));
}

#[test]
fn test_lookup_response_roundtrip_empty_instances() {
    roundtrip_response(Response::Lookup(LookupResponse {
        project: "my-app".to_string(),
        worktree: None,
        project_root: None,
        instances: vec![],
    }));
}

#[test]
fn test_lookup_response_roundtrip_no_primary_url() {
    roundtrip_response(Response::Lookup(LookupResponse {
        project: "app".to_string(),
        worktree: Some("feat".to_string()),
        project_root: Some("/tmp/app".to_string()),
        instances: vec![LookupInstance {
            name: "dev-1".to_string(),
            status: InstanceStatus::Idle,
            checked_out: false,
            branch: None,
            primary_url: None,
            ports: vec![PortMapping {
                logical_name: "api".to_string(),
                canonical_port: 8080,
                dynamic_port: 63104,
                is_primary: false,
            }],
        }],
    }));
}

#[test]
fn test_assign_request_explain_roundtrip() {
    roundtrip_request(Request::Assign(AssignRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
        worktree: "feature/test".to_string(),
        commit_sha: None,
        explain: true,
        force_sync: false,
    }));
}

#[test]
fn test_assign_request_force_sync_roundtrip() {
    roundtrip_request(Request::Assign(AssignRequest {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
        worktree: "feature/test".to_string(),
        commit_sha: None,
        explain: false,
        force_sync: true,
    }));
}

#[test]
fn test_assign_explain_response_roundtrip() {
    roundtrip_response(Response::AssignExplain(AssignExplainResponse {
        name: "dev-1".to_string(),
        worktree: "feature/test".to_string(),
        current_branch: Some("main".to_string()),
        services: vec![
            AssignExplainService {
                name: "web".to_string(),
                action: "hot".to_string(),
            },
            AssignExplainService {
                name: "worker".to_string(),
                action: "rebuild".to_string(),
            },
        ],
        exclude_paths: vec!["docs".to_string(), ".yarn".to_string()],
        tracked_file_count: 1500,
        gitignored_file_count: 8000,
        worktree_exists: true,
        worktree_synced: false,
        has_bare_install: true,
        changed_files_count: 42,
    }));
}

#[test]
fn test_port_health_changed_event_serialization() {
    let event = CoastEvent::PortHealthChanged {
        name: "dev-1".to_string(),
        project: "my-app".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["event"], "port.health_changed");
    assert_eq!(json["name"], "dev-1");
    assert_eq!(json["project"], "my-app");
    let deserialized: CoastEvent = serde_json::from_value(json).unwrap();
    assert!(matches!(deserialized, CoastEvent::PortHealthChanged { .. }));
}
