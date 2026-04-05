/// Main integration tests for the Coast project.
///
/// Tests that require a running Docker daemon are gated with `#[ignore]`.
/// Tests that only need filesystem access run without being ignored.
///
/// Run ignored tests: `cargo test -p coast-integration-tests -- --ignored`
/// Run non-ignored tests: `cargo test -p coast-integration-tests`
use std::path::{Path, PathBuf};

use coast_core::artifact;
use coast_core::coastfile::Coastfile;
use coast_core::protocol::*;
use coast_core::types::*;
use coast_core::volume;

// ---------------------------------------------------------------------------
// Helper: sample Coastfile TOML content
// ---------------------------------------------------------------------------

fn minimal_coastfile_toml() -> &'static str {
    r#"
[coast]
name = "test-project"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
web = 3000
postgres = 5432
"#
}

fn full_coastfile_toml() -> &'static str {
    r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"


[ports]
web = 3000
postgres = 5432
redis = 6379

[secrets.api_key]
extractor = "env"
var = "MY_API_KEY"
inject = "env:API_KEY"

[secrets.gcp_creds]
extractor = "file"
path = "/tmp/coast-test-gcp.json"
inject = "file:/run/secrets/gcp.json"

[inject]
env = ["NODE_ENV", "DEBUG"]
files = []

[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.redis_data]
strategy = "shared"
service = "redis"
mount = "/data"

[volumes.seed_data]
strategy = "isolated"
snapshot_source = "coast_seed_pg_data"
service = "db"
mount = "/var/lib/postgresql/data"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["coast_shared_pg:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "dev" }
auto_create_db = true
inject = "env:DATABASE_URL"
"#
}

// ---------------------------------------------------------------------------
// Coastfile parsing integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_minimal_coastfile() {
    let coastfile = Coastfile::parse(minimal_coastfile_toml(), Path::new("/tmp/project")).unwrap();
    assert_eq!(coastfile.name, "test-project");
    assert_eq!(
        coastfile.compose,
        Some(PathBuf::from("/tmp/project/docker-compose.yml"))
    );
    assert_eq!(coastfile.runtime, RuntimeType::Dind);
    assert_eq!(coastfile.ports.len(), 2);
    assert_eq!(coastfile.ports.get("web"), Some(&3000));
    assert_eq!(coastfile.ports.get("postgres"), Some(&5432));
}

#[test]
fn test_parse_full_coastfile() {
    let coastfile =
        Coastfile::parse(full_coastfile_toml(), Path::new("/home/user/dev/my-app")).unwrap();

    assert_eq!(coastfile.name, "my-app");
    assert_eq!(coastfile.runtime, RuntimeType::Dind);
    assert_eq!(coastfile.ports.len(), 3);
    assert_eq!(coastfile.secrets.len(), 2);
    assert_eq!(coastfile.volumes.len(), 3);
    assert_eq!(coastfile.shared_services.len(), 1);
    assert_eq!(coastfile.inject.env, vec!["NODE_ENV", "DEBUG"]);
}

#[test]
fn test_coastfile_from_file_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile_path = dir.path().join("Coastfile");
    let compose_path = dir.path().join("docker-compose.yml");

    std::fs::write(&coastfile_path, minimal_coastfile_toml()).unwrap();
    std::fs::write(&compose_path, "version: '3'\nservices:\n  web: {}\n").unwrap();

    let coastfile = Coastfile::from_file(&coastfile_path).unwrap();
    assert_eq!(coastfile.name, "test-project");
    assert_eq!(coastfile.project_root, dir.path());
}

// ---------------------------------------------------------------------------
// Volume strategy tests
// ---------------------------------------------------------------------------

#[test]
fn test_volume_strategies_isolation() {
    let vol = VolumeConfig {
        name: "postgres_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "db".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: None,
    };

    let name1 = volume::resolve_volume_name(&vol, "instance-1", "my-app");
    let name2 = volume::resolve_volume_name(&vol, "instance-2", "my-app");

    assert_ne!(
        name1, name2,
        "isolated volumes must have different names per instance"
    );
    assert_eq!(name1, "coast--instance-1--postgres_data");
    assert_eq!(name2, "coast--instance-2--postgres_data");
}

#[test]
fn test_volume_strategies_shared() {
    let vol = VolumeConfig {
        name: "redis_data".to_string(),
        strategy: VolumeStrategy::Shared,
        service: "redis".to_string(),
        mount: PathBuf::from("/data"),
        snapshot_source: None,
    };

    let name1 = volume::resolve_volume_name(&vol, "instance-1", "my-app");
    let name2 = volume::resolve_volume_name(&vol, "instance-2", "my-app");

    assert_eq!(
        name1, name2,
        "shared volumes must have the same name across instances"
    );
    assert_eq!(name1, "coast-shared--my-app--redis_data");
}

#[test]
fn test_volume_isolated_with_snapshot_source() {
    let vol = VolumeConfig {
        name: "seed_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "db".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: Some("coast_seed_pg_data".to_string()),
    };

    let name1 = volume::resolve_volume_name(&vol, "instance-1", "my-app");
    let name2 = volume::resolve_volume_name(&vol, "instance-2", "my-app");

    assert_ne!(
        name1, name2,
        "isolated volumes with snapshot_source are per-instance"
    );
    assert_eq!(name1, "coast--instance-1--seed_data");
    assert_eq!(name2, "coast--instance-2--seed_data");
}

#[test]
fn test_volumes_to_delete_excludes_shared() {
    let volumes = vec![
        VolumeConfig {
            name: "pg_data".to_string(),
            strategy: VolumeStrategy::Isolated,
            service: "db".to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "redis_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "redis".to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "seed".to_string(),
            strategy: VolumeStrategy::Isolated,
            service: "db".to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: Some("source_vol".to_string()),
        },
    ];

    let to_delete = volume::volumes_to_delete(&volumes, "feature-oauth");
    assert_eq!(to_delete.len(), 2);
    assert!(to_delete.contains(&"coast--feature-oauth--pg_data".to_string()));
    assert!(to_delete.contains(&"coast--feature-oauth--seed".to_string()));
    assert!(!to_delete.iter().any(|v| v.contains("redis_data")));
}

#[test]
fn test_volume_warning_shared_database() {
    let volumes = vec![
        VolumeConfig {
            name: "pg_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "postgres".to_string(),
            mount: PathBuf::from("/var/lib/postgresql/data"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "app_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "app".to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: None,
        },
    ];

    let warnings = volume::generate_volume_warnings(&volumes);
    assert_eq!(
        warnings.len(),
        1,
        "only database-like shared volumes get warnings"
    );
    assert!(warnings[0].contains("pg_data"));
    assert!(warnings[0].contains("data corruption"));
}

#[test]
fn test_volume_no_warning_for_isolated_database() {
    let volumes = vec![VolumeConfig {
        name: "pg_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "postgres".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: None,
    }];

    let warnings = volume::generate_volume_warnings(&volumes);
    assert!(warnings.is_empty());
}

#[test]
fn test_snapshot_copy_command_structure() {
    let cmd = volume::snapshot_copy_command("source-vol", "dest-vol");
    assert_eq!(cmd[0], "docker");
    assert_eq!(cmd[1], "run");
    assert_eq!(cmd[2], "--rm");
    assert!(cmd.contains(&"source-vol:/src".to_string()));
    assert!(cmd.contains(&"dest-vol:/dst".to_string()));
    assert!(cmd.contains(&"alpine".to_string()));
}

#[test]
fn test_snapshot_copy_uses_correct_source_and_dest() {
    let vol = VolumeConfig {
        name: "postgres_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "postgres".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: Some("infra_pg_data".to_string()),
    };

    let dest = volume::resolve_volume_name(&vol, "snap-1", "myapp");
    assert_eq!(dest, "coast--snap-1--postgres_data");

    let cmd = volume::snapshot_copy_command("infra_pg_data", &dest);
    assert!(cmd.contains(&"infra_pg_data:/src".to_string()));
    assert!(cmd.contains(&"coast--snap-1--postgres_data:/dst".to_string()));
}

#[test]
fn test_snapshot_copy_not_needed_without_source() {
    let vol = VolumeConfig {
        name: "redis_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "redis".to_string(),
        mount: PathBuf::from("/data"),
        snapshot_source: None,
    };

    assert!(vol.snapshot_source.is_none());
    let dest = volume::resolve_volume_name(&vol, "snap-1", "myapp");
    assert_eq!(dest, "coast--snap-1--redis_data");
}

// ---------------------------------------------------------------------------
// Multi-instance isolation test
// ---------------------------------------------------------------------------

#[test]
fn test_multi_instance_isolation() {
    let instances = ["feature-oauth", "feature-billing", "feature-admin"];
    let project = "my-app";

    let vol = VolumeConfig {
        name: "postgres_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "db".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: None,
    };

    let volume_names: Vec<String> = instances
        .iter()
        .map(|inst| volume::resolve_volume_name(&vol, inst, project))
        .collect();

    for i in 0..volume_names.len() {
        for j in (i + 1)..volume_names.len() {
            assert_ne!(
                volume_names[i], volume_names[j],
                "instances '{}' and '{}' must have different volume names",
                instances[i], instances[j]
            );
        }
    }
}

#[test]
fn test_multi_instance_shared_volumes_are_same() {
    let instances = ["feature-oauth", "feature-billing", "feature-admin"];
    let project = "my-app";

    let vol = VolumeConfig {
        name: "shared_cache".to_string(),
        strategy: VolumeStrategy::Shared,
        service: "cache".to_string(),
        mount: PathBuf::from("/cache"),
        snapshot_source: None,
    };

    let volume_names: Vec<String> = instances
        .iter()
        .map(|inst| volume::resolve_volume_name(&vol, inst, project))
        .collect();

    // All shared volume names must be the same
    assert_eq!(volume_names[0], volume_names[1]);
    assert_eq!(volume_names[1], volume_names[2]);
}

// ---------------------------------------------------------------------------
// Protocol round-trip tests
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_roundtrip_all_request_variants() {
    let requests: Vec<Request> = vec![
        Request::Build(BuildRequest {
            coastfile_path: PathBuf::from("/home/user/Coastfile"),
            refresh: true,
        }),
        Request::Run(RunRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            branch: Some("feature/oauth".to_string()),
            commit_sha: None,
            worktree: None,
            build_id: None,
            coastfile_type: None,
            force_remove_dangling: false,
        }),
        Request::Stop(StopRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Start(StartRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Rm(RmRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Checkout(CheckoutRequest {
            name: Some("feature-oauth".to_string()),
            project: "my-app".to_string(),
        }),
        Request::Checkout(CheckoutRequest {
            name: None,
            project: "my-app".to_string(),
        }),
        Request::Ports(PortsRequest::List {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Exec(ExecRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            service: None,
            root: false,
            command: vec![
                "bash".to_string(),
                "-c".to_string(),
                "echo hello".to_string(),
            ],
        }),
        Request::Logs(LogsRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            service: Some("web".to_string()),
            tail: Some(100),
            tail_all: false,
            follow: true,
        }),
        Request::Ps(PsRequest {
            name: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Ls(LsRequest {
            project: Some("my-app".to_string()),
        }),
        Request::Ls(LsRequest { project: None }),
        Request::Secret(SecretRequest::Set {
            instance: "feature-oauth".to_string(),
            project: "my-app".to_string(),
            name: "API_KEY".to_string(),
            value: "secret123".to_string(),
        }),
        Request::Secret(SecretRequest::List {
            instance: "feature-oauth".to_string(),
            project: "my-app".to_string(),
        }),
        Request::Shared(SharedRequest::Ps {
            project: "my-app".to_string(),
        }),
        Request::Shared(SharedRequest::Rm {
            project: "my-app".to_string(),
            service: "postgres".to_string(),
        }),
    ];

    for req in &requests {
        let encoded = encode_request(req).unwrap();
        let decoded = decode_request(&encoded[..encoded.len() - 1]).unwrap();
        let re_encoded = encode_request(&decoded).unwrap();
        assert_eq!(
            encoded, re_encoded,
            "round-trip failed for request: {:?}",
            req
        );
    }
}

#[test]
fn test_protocol_roundtrip_all_response_variants() {
    let responses: Vec<Response> = vec![
        Response::Build(BuildResponse {
            project: "my-app".to_string(),
            artifact_path: PathBuf::from("/home/user/.coast/images/my-app"),
            images_cached: 3,
            images_built: 1,
            secrets_extracted: 2,
            coast_image: None,
            warnings: vec!["shared volume warning".to_string()],
            coastfile_type: None,
        }),
        Response::Run(RunResponse {
            name: "feature-oauth".to_string(),
            container_id: "abc123def456".to_string(),
            ports: vec![
                PortMapping {
                    logical_name: "web".to_string(),
                    canonical_port: 3000,
                    dynamic_port: 52340,
                    is_primary: false,
                },
                PortMapping {
                    logical_name: "postgres".to_string(),
                    canonical_port: 5432,
                    dynamic_port: 52341,
                    is_primary: false,
                },
            ],
        }),
        Response::Stop(StopResponse {
            name: "feature-oauth".to_string(),
        }),
        Response::Start(StartResponse {
            name: "feature-oauth".to_string(),
            ports: vec![],
        }),
        Response::Rm(RmResponse {
            name: "feature-oauth".to_string(),
        }),
        Response::Checkout(CheckoutResponse {
            checked_out: Some("feature-oauth".to_string()),
            ports: vec![PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: false,
            }],
        }),
        Response::Checkout(CheckoutResponse {
            checked_out: None,
            ports: vec![],
        }),
        Response::Ports(PortsResponse {
            name: "feature-oauth".to_string(),
            ports: vec![PortMapping {
                logical_name: "web".to_string(),
                canonical_port: 3000,
                dynamic_port: 52340,
                is_primary: false,
            }],
            message: None,
            subdomain_host: None,
        }),
        Response::Exec(ExecResponse {
            exit_code: 0,
            stdout: "hello world\n".to_string(),
            stderr: String::new(),
        }),
        Response::LogsProgress(LogsResponse {
            output: "web_1 | chunk\n".to_string(),
        }),
        Response::Logs(LogsResponse {
            output: "web_1 | Server started on :3000\n".to_string(),
        }),
        Response::Ps(PsResponse {
            name: "feature-oauth".to_string(),
            services: vec![ServiceStatus {
                name: "web".to_string(),
                status: "Up 5 minutes".to_string(),
                ports: "0.0.0.0:3000->3000/tcp".to_string(),
                image: "my-app-web:latest".to_string(),
                kind: Some("compose".to_string()),
            }],
        }),
        Response::Ls(LsResponse {
            instances: vec![
                InstanceSummary {
                    name: "main".to_string(),
                    project: "my-app".to_string(),
                    status: InstanceStatus::CheckedOut,
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
                },
                InstanceSummary {
                    name: "feature-oauth".to_string(),
                    project: "my-app".to_string(),
                    status: InstanceStatus::Running,
                    branch: Some("feature/oauth".to_string()),
                    runtime: RuntimeType::Dind,
                    checked_out: false,
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
                },
            ],
            known_projects: vec![],
        }),
        Response::Secret(SecretResponse {
            message: "Secret set".to_string(),
            secrets: vec![SecretInfo {
                name: "API_KEY".to_string(),
                extractor: "env".to_string(),
                inject: "env:API_KEY".to_string(),
                is_override: false,
            }],
        }),
        Response::Shared(SharedResponse {
            message: "Shared services".to_string(),
            services: vec![SharedServiceInfo {
                name: "postgres".to_string(),
                container_id: Some("abc123".to_string()),
                status: "running".to_string(),
                image: Some("postgres:16".to_string()),
                ports: Some("0.0.0.0:5432->5432/tcp".to_string()),
            }],
        }),
        Response::Error(ErrorResponse {
            error: "Instance not found".to_string(),
        }),
    ];

    for resp in &responses {
        let encoded = encode_response(resp).unwrap();
        let decoded = decode_response(&encoded[..encoded.len() - 1]).unwrap();
        let re_encoded = encode_response(&decoded).unwrap();
        assert_eq!(
            encoded, re_encoded,
            "round-trip failed for response: {:?}",
            resp
        );
    }
}

#[test]
fn test_protocol_decode_invalid_json() {
    let result = decode_request(b"not valid json");
    assert!(result.is_err());

    let result = decode_response(b"{}");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Artifact building integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_artifact_directory_structure() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    let compose_content = "version: '3'\nservices:\n  web:\n    image: nginx\n";
    let compose_path = project_dir.join("docker-compose.yml");
    std::fs::write(&compose_path, compose_content).unwrap();

    let artifact_dir = dir.path().join("artifact");
    std::fs::create_dir_all(artifact_dir.join("inject")).unwrap();

    // Copy coastfile
    let coastfile_content = minimal_coastfile_toml();
    artifact::copy_coastfile(coastfile_content, &artifact_dir).unwrap();
    assert!(artifact_dir.join("coastfile.toml").exists());

    // Copy compose file
    let filename = artifact::copy_compose_file(&compose_path, &artifact_dir).unwrap();
    assert_eq!(filename, "docker-compose.yml");
    assert!(artifact_dir.join("docker-compose.yml").exists());

    // Verify copied content matches
    let copied_compose = std::fs::read_to_string(artifact_dir.join("docker-compose.yml")).unwrap();
    assert_eq!(copied_compose, compose_content);
}

#[test]
fn test_manifest_write_and_read() {
    let dir = tempfile::tempdir().unwrap();

    let manifest = artifact::Manifest {
        build_timestamp: chrono::Utc::now(),
        coastfile_hash: "deadbeef1234".to_string(),
        project_name: "my-app".to_string(),
        cached_images: vec![
            artifact::CachedImage {
                reference: "postgres:16".to_string(),
                tarball_name: "postgres_16_abc.tar".to_string(),
                digest_short: Some("abc".to_string()),
            },
            artifact::CachedImage {
                reference: "redis:7".to_string(),
                tarball_name: "redis_7_def.tar".to_string(),
                digest_short: None,
            },
        ],
        resolved_secret_names: vec!["api_key".to_string(), "db_password".to_string()],
        volume_warnings: vec!["shared volume warning".to_string()],
        runtime: "dind".to_string(),
        injected_files: vec!["id_ed25519".to_string()],
        injected_env: vec!["NODE_ENV".to_string()],
    };

    artifact::write_manifest(&manifest, dir.path()).unwrap();
    assert!(dir.path().join("manifest.json").exists());

    let loaded = artifact::read_manifest(dir.path()).unwrap().unwrap();
    assert_eq!(loaded.project_name, "my-app");
    assert_eq!(loaded.coastfile_hash, "deadbeef1234");
    assert_eq!(loaded.cached_images.len(), 2);
    assert_eq!(loaded.resolved_secret_names.len(), 2);
    assert_eq!(loaded.runtime, "dind");
}

#[test]
fn test_needs_rebuild_logic() {
    let dir = tempfile::tempdir().unwrap();
    let content = minimal_coastfile_toml();
    let hash = artifact::hash_coastfile(content);

    // No manifest -> needs rebuild
    assert!(artifact::needs_rebuild(dir.path(), &hash, false).unwrap());

    // Write manifest with same hash
    let manifest = artifact::Manifest {
        build_timestamp: chrono::Utc::now(),
        coastfile_hash: hash.clone(),
        project_name: "test".to_string(),
        cached_images: vec![],
        resolved_secret_names: vec![],
        volume_warnings: vec![],
        runtime: "dind".to_string(),
        injected_files: vec![],
        injected_env: vec![],
    };
    artifact::write_manifest(&manifest, dir.path()).unwrap();

    // Same hash -> no rebuild
    assert!(!artifact::needs_rebuild(dir.path(), &hash, false).unwrap());

    // Different hash -> rebuild
    assert!(artifact::needs_rebuild(dir.path(), "different_hash", false).unwrap());

    // Force -> always rebuild
    assert!(artifact::needs_rebuild(dir.path(), &hash, true).unwrap());
}

#[test]
fn test_partial_build_finalize() {
    let dir = tempfile::tempdir().unwrap();

    let partial = artifact::PartialBuild {
        artifact_dir: dir.path().to_path_buf(),
        coastfile_hash: "hash123".to_string(),
        project_name: "test-project".to_string(),
        volume_warnings: vec!["warning".to_string()],
        injected_files: vec!["file1".to_string()],
        injected_env: vec!["ENV1".to_string()],
        runtime: "dind".to_string(),
    };

    let cached = vec![artifact::CachedImage {
        reference: "nginx:latest".to_string(),
        tarball_name: "nginx_latest_abc.tar".to_string(),
        digest_short: Some("abc".to_string()),
    }];
    let secrets = vec!["my_secret".to_string()];

    let manifest = partial.finalize(cached, secrets).unwrap();
    assert_eq!(manifest.project_name, "test-project");
    assert_eq!(manifest.cached_images.len(), 1);
    assert_eq!(manifest.resolved_secret_names, vec!["my_secret"]);
    assert_eq!(manifest.volume_warnings, vec!["warning"]);

    // Manifest should exist on disk
    assert!(dir.path().join("manifest.json").exists());
}

#[test]
fn test_image_reference_parsing() {
    assert_eq!(
        artifact::parse_image_reference("postgres:16"),
        ("postgres", "16")
    );
    assert_eq!(
        artifact::parse_image_reference("redis"),
        ("redis", "latest")
    );
    assert_eq!(
        artifact::parse_image_reference("library/node:20"),
        ("library/node", "20")
    );
    assert_eq!(
        artifact::parse_image_reference("registry.example.com:5000/myapp"),
        ("registry.example.com:5000/myapp", "latest")
    );
    assert_eq!(
        artifact::parse_image_reference("registry.example.com:5000/myapp:v2"),
        ("registry.example.com:5000/myapp", "v2")
    );
}

#[test]
fn test_tarball_filename_generation() {
    assert_eq!(
        artifact::tarball_filename("postgres", "16", "abc123"),
        "postgres_16_abc123.tar"
    );
    assert_eq!(
        artifact::tarball_filename("library/node", "20", "def"),
        "library_node_20_def.tar"
    );
}

#[test]
fn test_coastfile_hash_deterministic() {
    let content = full_coastfile_toml();
    let hash1 = artifact::hash_coastfile(content);
    let hash2 = artifact::hash_coastfile(content);
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 64); // SHA-256 in hex
}

#[test]
fn test_coastfile_hash_different_content() {
    let h1 = artifact::hash_coastfile("content a");
    let h2 = artifact::hash_coastfile("content b");
    assert_ne!(h1, h2);
}

// ---------------------------------------------------------------------------
// Type serialization integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_coast_instance_json_roundtrip() {
    let instance = CoastInstance {
        name: "feature-oauth".to_string(),
        status: InstanceStatus::Running,
        project: "my-app".to_string(),
        branch: Some("feature/oauth".to_string()),
        commit_sha: None,
        container_id: Some("abc123".to_string()),
        runtime: RuntimeType::Dind,
        created_at: chrono::Utc::now(),
        worktree_name: None,
        build_id: None,
        coastfile_type: None,
        remote_name: None,
    };

    let json = serde_json::to_string(&instance).unwrap();
    let deser: CoastInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.name, "feature-oauth");
    assert_eq!(deser.status, InstanceStatus::Running);
    assert_eq!(deser.project, "my-app");
    assert_eq!(deser.runtime, RuntimeType::Dind);
}

#[test]
fn test_inject_type_parsing() {
    let env = InjectType::parse("env:MY_VAR").unwrap();
    assert_eq!(env, InjectType::Env("MY_VAR".to_string()));

    let file = InjectType::parse("file:/run/secrets/key.json").unwrap();
    assert_eq!(
        file,
        InjectType::File(PathBuf::from("/run/secrets/key.json"))
    );

    // Error cases
    assert!(InjectType::parse("env:").is_err());
    assert!(InjectType::parse("file:").is_err());
    assert!(InjectType::parse("invalid:format").is_err());
    assert!(InjectType::parse("justtext").is_err());
}

#[test]
fn test_inject_type_roundtrip() {
    let env = InjectType::Env("PGPASSWORD".to_string());
    let roundtripped = InjectType::parse(&env.to_inject_string()).unwrap();
    assert_eq!(env, roundtripped);

    let file = InjectType::File(PathBuf::from("/run/secrets/gcp.json"));
    let roundtripped = InjectType::parse(&file.to_inject_string()).unwrap();
    assert_eq!(file, roundtripped);
}

#[test]
fn test_instance_status_db_roundtrip() {
    for status in &[
        InstanceStatus::Running,
        InstanceStatus::Stopped,
        InstanceStatus::CheckedOut,
    ] {
        let s = status.as_db_str();
        let parsed = InstanceStatus::from_db_str(s).unwrap();
        assert_eq!(&parsed, status);
    }

    assert!(InstanceStatus::from_db_str("invalid").is_none());
}

#[test]
fn test_runtime_type_roundtrip() {
    for rt in &[RuntimeType::Dind, RuntimeType::Sysbox, RuntimeType::Podman] {
        let s = rt.as_str();
        let parsed = RuntimeType::from_str_value(s).unwrap();
        assert_eq!(&parsed, rt);
    }

    assert!(RuntimeType::from_str_value("docker").is_none());
}

// ---------------------------------------------------------------------------
// Full lifecycle test (requires Docker)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore] // Requires running Docker daemon
async fn test_full_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let project_dir = dir.path().join("project");
    std::fs::create_dir_all(&project_dir).unwrap();

    // Create Coastfile
    let coastfile_content = r#"
[coast]
name = "lifecycle-test"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
web = 8080

[volumes.app_data]
strategy = "isolated"
service = "web"
mount = "/data"
"#;

    let coastfile_path = project_dir.join("Coastfile");
    let compose_path = project_dir.join("docker-compose.yml");
    std::fs::write(&coastfile_path, coastfile_content).unwrap();
    std::fs::write(
        &compose_path,
        "version: '3'\nservices:\n  web:\n    image: nginx:alpine\n    ports:\n      - '8080:80'\n",
    )
    .unwrap();

    // 1. Parse Coastfile
    let coastfile = Coastfile::from_file(&coastfile_path).unwrap();
    assert_eq!(coastfile.name, "lifecycle-test");

    // 2. Build artifact directory structure
    let artifact_dir = dir.path().join("artifact");
    std::fs::create_dir_all(artifact_dir.join("inject")).unwrap();
    artifact::copy_coastfile(coastfile_content, &artifact_dir).unwrap();
    artifact::copy_compose_file(&compose_path, &artifact_dir).unwrap();

    // 3. Verify artifact files
    assert!(artifact_dir.join("coastfile.toml").exists());
    assert!(artifact_dir.join("docker-compose.yml").exists());

    // 4. Verify volume name generation
    let vol = &coastfile.volumes[0];
    let vol_name = volume::resolve_volume_name(vol, "instance-1", &coastfile.name);
    assert_eq!(vol_name, "coast--instance-1--app_data");

    // 5. Verify manifest can be written
    let manifest = artifact::Manifest {
        build_timestamp: chrono::Utc::now(),
        coastfile_hash: artifact::hash_coastfile(coastfile_content),
        project_name: coastfile.name.clone(),
        cached_images: vec![],
        resolved_secret_names: vec![],
        volume_warnings: vec![],
        runtime: "dind".to_string(),
        injected_files: vec![],
        injected_env: vec![],
    };
    artifact::write_manifest(&manifest, &artifact_dir).unwrap();
    assert!(artifact_dir.join("manifest.json").exists());
}

// ---------------------------------------------------------------------------
// Multi-instance test (requires Docker)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore] // Requires running Docker daemon
async fn test_multi_instance_port_isolation() {
    let coastfile_content = r#"
[coast]
name = "multi-test"
compose = "./docker-compose.yml"

[ports]
web = 3000

[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
"#;

    let coastfile = Coastfile::parse(coastfile_content, Path::new("/tmp/project")).unwrap();

    let instances = vec![
        ("instance-1", Some("main")),
        ("instance-2", Some("feature/oauth")),
        ("instance-3", Some("feature/billing")),
    ];

    let vol = &coastfile.volumes[0];

    let mut volume_names = Vec::new();
    for (name, _branch) in &instances {
        let vn = volume::resolve_volume_name(vol, name, &coastfile.name);
        volume_names.push(vn);
    }

    // All volume names must be unique for isolated strategy
    assert_ne!(volume_names[0], volume_names[1]);
    assert_ne!(volume_names[1], volume_names[2]);
    assert_ne!(volume_names[0], volume_names[2]);
}
