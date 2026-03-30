use super::*;
use crate::types::{InjectType, SharedServicePort, VolumeStrategy};
use std::path::Path;

fn sample_coastfile() -> &'static str {
    r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
web = 3000
postgres = 5432
redis = 6379

[secrets.claude_api_key]
extractor = "macos-keychain"
item = "claude-code-api-key"
inject = "env:CLAUDE_API_KEY"

[secrets.db_password]
extractor = "file"
path = "~/.config/gcloud/application_default_credentials.json"
inject = "file:/run/secrets/gcp.json"

[secrets.aws_session]
extractor = "command"
run = "aws sts get-session-token --output json"
inject = "env:AWS_SESSION"
ttl = "1h"

[inject]
env = ["NODE_ENV", "DEBUG"]
files = ["~/.ssh/id_ed25519", "~/.gitconfig"]

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

#[test]
fn test_parse_valid_coastfile() {
    let root = Path::new("/home/user/dev/my-app");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.name, "my-app");
    assert_eq!(
        coastfile.compose,
        Some(PathBuf::from("/home/user/dev/my-app/docker-compose.yml"))
    );
    assert_eq!(coastfile.runtime, RuntimeType::Dind);
}

#[test]
fn test_parse_ports() {
    let root = Path::new("/tmp/project");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.ports.get("web"), Some(&3000));
    assert_eq!(coastfile.ports.get("postgres"), Some(&5432));
    assert_eq!(coastfile.ports.get("redis"), Some(&6379));
    assert_eq!(coastfile.ports.len(), 3);
}

#[test]
fn test_parse_secrets() {
    let root = Path::new("/tmp/project");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.secrets.len(), 3);

    let claude = coastfile
        .secrets
        .iter()
        .find(|s| s.name == "claude_api_key")
        .unwrap();
    assert_eq!(claude.extractor, "macos-keychain");
    assert_eq!(claude.inject, InjectType::Env("CLAUDE_API_KEY".to_string()));
    assert_eq!(claude.params.get("item").unwrap(), "claude-code-api-key");
    assert!(claude.ttl.is_none());

    let aws = coastfile
        .secrets
        .iter()
        .find(|s| s.name == "aws_session")
        .unwrap();
    assert_eq!(aws.ttl, Some("1h".to_string()));
}

#[test]
fn test_parse_inject() {
    let root = Path::new("/tmp/project");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.inject.env, vec!["NODE_ENV", "DEBUG"]);
    assert_eq!(
        coastfile.inject.files,
        vec!["~/.ssh/id_ed25519", "~/.gitconfig"]
    );
}

#[test]
fn test_parse_volumes() {
    let root = Path::new("/tmp/project");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.volumes.len(), 3);

    let pg = coastfile
        .volumes
        .iter()
        .find(|v| v.name == "postgres_data")
        .unwrap();
    assert_eq!(pg.strategy, VolumeStrategy::Isolated);
    assert_eq!(pg.service, "db");

    let seed = coastfile
        .volumes
        .iter()
        .find(|v| v.name == "seed_data")
        .unwrap();
    assert_eq!(seed.strategy, VolumeStrategy::Isolated);
    assert_eq!(seed.snapshot_source.as_deref(), Some("coast_seed_pg_data"));
}

#[test]
fn test_parse_shared_services() {
    let root = Path::new("/tmp/project");
    let coastfile = Coastfile::parse(sample_coastfile(), root).unwrap();

    assert_eq!(coastfile.shared_services.len(), 1);
    let pg = &coastfile.shared_services[0];
    assert_eq!(pg.name, "postgres");
    assert_eq!(pg.image, "postgres:16");
    assert_eq!(pg.ports, vec![SharedServicePort::same(5432)]);
    assert!(pg.auto_create_db);
    assert_eq!(pg.inject, Some(InjectType::Env("DATABASE_URL".to_string())));
}

#[test]
fn test_parse_shared_service_mapped_ports() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432", 6432]
"#;

    let coastfile = Coastfile::parse(toml, Path::new("/tmp/project")).unwrap();

    assert_eq!(coastfile.shared_services.len(), 1);
    assert_eq!(
        coastfile.shared_services[0].ports,
        vec![
            SharedServicePort::new(5433, 5432),
            SharedServicePort::same(6432),
        ]
    );
}

#[test]
fn test_parse_shared_service_invalid_mapped_port() {
    let toml = r#"
[coast]
name = "my-app"

[shared_services.postgis]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:not-a-port"]
"#;

    let error = Coastfile::parse(toml, Path::new("/tmp/project")).unwrap_err();

    assert!(error
        .to_string()
        .contains("shared_service 'postgis': invalid container port"));
}

#[test]
fn test_missing_name() {
    let toml = r#"
[coast]
compose = "./docker-compose.yml"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_empty_name() {
    let toml = r#"
[coast]
name = ""
compose = "./docker-compose.yml"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("name"));
}

#[test]
fn test_coastfile_without_compose() {
    let toml = r#"
[coast]
name = "my-app"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.name, "my-app");
    assert!(coastfile.compose.is_none());
}

#[test]
fn test_invalid_runtime() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "lxc"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("invalid runtime"));
}

#[test]
fn test_default_runtime() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.runtime, RuntimeType::Dind);
}

#[test]
fn test_invalid_port_zero() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 0
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("port"));
}

#[test]
fn test_primary_port_valid() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
primary_port = "web"

[ports]
web = 3000
postgres = 5432
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(cf.primary_port, Some("web".to_string()));
}

#[test]
fn test_primary_port_invalid_unknown_service() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
primary_port = "nonexistent"

[ports]
web = 3000
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("primary_port"));
    assert!(err.contains("nonexistent"));
}

#[test]
fn test_primary_port_none_by_default() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(cf.primary_port.is_none());
}

#[test]
fn test_invalid_inject_syntax() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[secrets.bad]
extractor = "file"
path = "/tmp/secret"
inject = "invalid:format"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("invalid inject format"));
}

#[test]
fn test_invalid_volume_strategy() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[volumes.data]
strategy = "replicated"
service = "app"
mount = "/data"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("invalid strategy"));
}

#[test]
fn test_snapshot_source_on_shared_rejected() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[volumes.data]
strategy = "shared"
snapshot_source = "some_volume"
service = "db"
mount = "/data"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("snapshot_source is only valid with strategy 'isolated'"));
}

#[test]
fn test_shared_service_invalid_inject() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.pg]
image = "postgres:16"
inject = "bad:format"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_shared_service_invalid_port() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.pg]
image = "postgres:16"
ports = [0]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_empty_coastfile() {
    let result = Coastfile::parse("", Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_invalid_toml() {
    let result = Coastfile::parse("{{not valid toml", Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_minimal_coastfile() {
    let toml = r#"
[coast]
name = "minimal"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.name, "minimal");
    assert!(coastfile.compose.is_none());
    assert!(coastfile.ports.is_empty());
    assert!(coastfile.secrets.is_empty());
    assert!(coastfile.volumes.is_empty());
    assert!(coastfile.shared_services.is_empty());
}

#[test]
fn test_minimal_coastfile_with_compose() {
    let toml = r#"
[coast]
name = "minimal"
compose = "docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.name, "minimal");
    assert_eq!(
        coastfile.compose,
        Some(PathBuf::from("/tmp/docker-compose.yml"))
    );
}

#[test]
fn test_compose_absolute_path() {
    let toml = r#"
[coast]
name = "my-app"
compose = "/absolute/path/docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(
        coastfile.compose,
        Some(PathBuf::from("/absolute/path/docker-compose.yml"))
    );
}

#[test]
fn test_compose_relative_path_resolved() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let root = Path::new("/home/user/project");
    let coastfile = Coastfile::parse(toml, root).unwrap();
    assert_eq!(
        coastfile.compose,
        Some(PathBuf::from("/home/user/project/docker-compose.yml"))
    );
}

#[test]
fn test_from_file_nonexistent() {
    let result = Coastfile::from_file(Path::new("/tmp/nonexistent/Coastfile"));
    assert!(result.is_err());
}

#[test]
fn test_from_file_valid() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile_path = dir.path().join("Coastfile");
    std::fs::write(
        &coastfile_path,
        r#"
[coast]
name = "test-app"
compose = "./docker-compose.yml"
"#,
    )
    .unwrap();

    let coastfile = Coastfile::from_file(&coastfile_path).unwrap();
    assert_eq!(coastfile.name, "test-app");
    assert!(coastfile.compose.is_some());
    assert_eq!(coastfile.project_root, dir.path());
}

#[test]
fn test_from_file_valid_without_compose() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile_path = dir.path().join("Coastfile");
    std::fs::write(
        &coastfile_path,
        r#"
[coast]
name = "test-app"
"#,
    )
    .unwrap();

    let coastfile = Coastfile::from_file(&coastfile_path).unwrap();
    assert_eq!(coastfile.name, "test-app");
    assert!(coastfile.compose.is_none());
}

#[test]
fn test_secret_with_ttl() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[secrets.aws]
extractor = "command"
run = "aws sts get-session-token"
inject = "env:AWS_SESSION"
ttl = "1h"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let aws = coastfile.secrets.iter().find(|s| s.name == "aws").unwrap();
    assert_eq!(aws.ttl, Some("1h".to_string()));
    assert_eq!(aws.params.get("run").unwrap(), "aws sts get-session-token");
}

#[test]
fn test_no_inject_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.inject.env.is_empty());
    assert!(coastfile.inject.files.is_empty());
}

#[test]
fn test_all_runtimes() {
    for runtime in &["dind", "sysbox", "podman"] {
        let toml = format!(
            r#"
[coast]
name = "app"
compose = "./dc.yml"
runtime = "{runtime}"
"#
        );
        let coastfile = Coastfile::parse(&toml, Path::new("/tmp")).unwrap();
        assert_eq!(coastfile.runtime.as_str(), *runtime);
    }
}

#[test]
fn test_parse_with_setup() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["nodejs", "npm", "git"]
run = [
    "npm install -g @anthropic-ai/claude-code",
    "echo 'done'",
]
[[coast.setup.files]]
path = "/root/.claude/settings.json"
content = "{\"skipDangerousModePermissionPrompt\":true}"
mode = "0600"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.setup.is_empty());
    assert_eq!(coastfile.setup.packages, vec!["nodejs", "npm", "git"]);
    assert_eq!(coastfile.setup.run.len(), 2);
    assert!(coastfile.setup.run[0].contains("claude-code"));
    assert_eq!(coastfile.setup.files.len(), 1);
    assert_eq!(coastfile.setup.files[0].path, "/root/.claude/settings.json");
    assert_eq!(coastfile.setup.files[0].mode.as_deref(), Some("0600"));
}

#[test]
fn test_parse_without_setup() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.setup.is_empty());
}

#[test]
fn test_parse_setup_only_packages() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
packages = ["curl", "jq"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.setup.is_empty());
    assert_eq!(coastfile.setup.packages, vec!["curl", "jq"]);
    assert!(coastfile.setup.run.is_empty());
    assert!(coastfile.setup.files.is_empty());
}

#[test]
fn test_parse_setup_only_run() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
run = ["echo hello"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.setup.is_empty());
    assert!(coastfile.setup.packages.is_empty());
    assert_eq!(coastfile.setup.run, vec!["echo hello"]);
    assert!(coastfile.setup.files.is_empty());
}

#[test]
fn test_parse_setup_only_files() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
[[coast.setup.files]]
path = "/etc/tool/config.json"
content = "{\"feature\":true}"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.setup.is_empty());
    assert!(coastfile.setup.packages.is_empty());
    assert!(coastfile.setup.run.is_empty());
    assert_eq!(coastfile.setup.files.len(), 1);
    assert_eq!(coastfile.setup.files[0].path, "/etc/tool/config.json");
}

#[test]
fn test_parse_setup_files_reject_relative_path() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
[[coast.setup.files]]
path = "etc/tool/config.json"
content = "x"
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("must be an absolute container path"));
}

#[test]
fn test_parse_setup_files_reject_invalid_mode() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[coast.setup]
[[coast.setup.files]]
path = "/etc/tool/config.json"
content = "x"
mode = "xyz"
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("must be a 3-4 digit octal string"));
}

#[test]
fn test_shared_service_no_optional_fields() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.redis]
image = "redis:7"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let redis = &coastfile.shared_services[0];
    assert_eq!(redis.name, "redis");
    assert_eq!(redis.image, "redis:7");
    assert!(redis.ports.is_empty());
    assert!(redis.volumes.is_empty());
    assert!(!redis.auto_create_db);
    assert!(redis.inject.is_none());
}

// --- [assign] section tests ---

#[test]
fn test_parse_without_assign_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(
        coastfile.assign.default,
        crate::types::AssignAction::Restart
    );
    assert!(coastfile.assign.services.is_empty());
    assert!(coastfile.assign.rebuild_triggers.is_empty());
}

#[test]
fn test_parse_assign_default_only() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign]
default = "none"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.assign.default, crate::types::AssignAction::None);
    assert!(coastfile.assign.services.is_empty());
}

#[test]
fn test_parse_assign_full() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign]
default = "none"

[assign.services]
api = "restart"
worker = "rebuild"
postgres = "none"

[assign.rebuild_triggers]
worker = ["Dockerfile", "package.json"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.assign.default, crate::types::AssignAction::None);
    assert_eq!(
        coastfile.assign.action_for_service("api"),
        crate::types::AssignAction::Restart
    );
    assert_eq!(
        coastfile.assign.action_for_service("worker"),
        crate::types::AssignAction::Rebuild
    );
    assert_eq!(
        coastfile.assign.action_for_service("postgres"),
        crate::types::AssignAction::None
    );
    // Unlisted service falls back to default
    assert_eq!(
        coastfile.assign.action_for_service("redis"),
        crate::types::AssignAction::None
    );

    let triggers = coastfile.assign.rebuild_triggers.get("worker").unwrap();
    assert_eq!(triggers, &vec!["Dockerfile", "package.json"]);
}

#[test]
fn test_parse_assign_invalid_default_action() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign]
default = "explode"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("assign.default"));
    assert!(err.contains("invalid action"));
}

#[test]
fn test_parse_assign_invalid_service_action() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign.services]
api = "turbo"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("assign.services.api"));
    assert!(err.contains("invalid action"));
}

#[test]
fn test_parse_assign_config_exclude_paths() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign]
default = "none"
exclude_paths = ["apps/ide", "apps/extension"]

[assign.services]
backend = "hot"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(
        coastfile.assign.exclude_paths,
        vec!["apps/ide".to_string(), "apps/extension".to_string()]
    );
}

#[test]
fn test_parse_assign_config_exclude_paths_default_empty() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[assign]
default = "restart"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.assign.exclude_paths.is_empty());
}

// --- root field tests ---

#[test]
fn test_root_absent_uses_coastfile_dir() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let root = Path::new("/home/user/dev/my-app");
    let coastfile = Coastfile::parse(toml, root).unwrap();
    assert_eq!(
        coastfile.project_root,
        PathBuf::from("/home/user/dev/my-app")
    );
}

#[test]
fn test_root_relative_resolves_to_coastfile_dir() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
root = ".."
"#;
    let root = Path::new("/home/user/dev/my-app/infra");
    let coastfile = Coastfile::parse(toml, root).unwrap();
    assert_eq!(
        coastfile.project_root,
        PathBuf::from("/home/user/dev/my-app/infra/..")
    );
}

#[test]
fn test_root_absolute_used_directly() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
root = "/opt/projects/my-app"
"#;
    let root = Path::new("/home/user/dev/my-app");
    let coastfile = Coastfile::parse(toml, root).unwrap();
    assert_eq!(
        coastfile.project_root,
        PathBuf::from("/opt/projects/my-app")
    );
}

// --- [egress] section tests ---

#[test]
fn test_parse_egress() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[egress]
host-api = 48080
postgres = 5432
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.egress.len(), 2);
    assert_eq!(coastfile.egress.get("host-api"), Some(&48080));
    assert_eq!(coastfile.egress.get("postgres"), Some(&5432));
}

#[test]
fn test_parse_no_egress_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.egress.is_empty());
}

#[test]
fn test_egress_invalid_port_zero() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[egress]
bad = 0
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("egress"));
}

#[test]
fn test_minimal_coastfile_egress_empty() {
    let toml = r#"
[coast]
name = "minimal"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.egress.is_empty());
}

// --- [omit] section tests ---

#[test]
fn test_parse_without_omit_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.omit.is_empty());
    assert!(coastfile.omit.services.is_empty());
    assert!(coastfile.omit.volumes.is_empty());
}

#[test]
fn test_parse_omit_services_only() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["keycloak", "redash", "nginx-proxy"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.omit.is_empty());
    assert_eq!(
        coastfile.omit.services,
        vec!["keycloak", "redash", "nginx-proxy"]
    );
    assert!(coastfile.omit.volumes.is_empty());
}

#[test]
fn test_parse_omit_volumes_only() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
volumes = ["keycloak-db-data", "redash-data"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.omit.is_empty());
    assert!(coastfile.omit.services.is_empty());
    assert_eq!(
        coastfile.omit.volumes,
        vec!["keycloak-db-data", "redash-data"]
    );
}

#[test]
fn test_parse_omit_both() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = ["keycloak", "langfuse"]
volumes = ["keycloak-db-data"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(!coastfile.omit.is_empty());
    assert_eq!(coastfile.omit.services, vec!["keycloak", "langfuse"]);
    assert_eq!(coastfile.omit.volumes, vec!["keycloak-db-data"]);
}

#[test]
fn test_parse_omit_empty_lists() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[omit]
services = []
volumes = []
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.omit.is_empty());
}

// --- [mcp] section tests ---

#[test]
fn test_parse_mcp_internal_with_install_and_source() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[mcp.custom-tool]
source = "./tools/my-mcp"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
env = { API_KEY = "secret" }
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 1);
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(mcp.name, "custom-tool");
    assert!(mcp.proxy.is_none());
    assert!(!mcp.is_host_proxied());
    assert_eq!(mcp.command, Some("node".to_string()));
    assert_eq!(mcp.args, vec!["dist/index.js"]);
    assert_eq!(mcp.env.get("API_KEY").unwrap(), "secret");
    assert_eq!(
        mcp.install,
        vec!["npm install".to_string(), "npm run build".to_string()]
    );
    assert_eq!(mcp.source, Some("./tools/my-mcp".to_string()));
}

#[test]
fn test_parse_mcp_internal_minimal() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.filesystem]
command = "npx"
args = ["@modelcontextprotocol/server-filesystem", "/workspace"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 1);
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(mcp.name, "filesystem");
    assert!(mcp.proxy.is_none());
    assert_eq!(mcp.command, Some("npx".to_string()));
    assert!(mcp.install.is_empty());
    assert!(mcp.source.is_none());
    assert!(mcp.env.is_empty());
}

#[test]
fn test_parse_mcp_host_proxied_with_command() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.company-tools]
proxy = "host"
command = "/usr/local/bin/company-mcp"
args = ["--mode", "production"]
env = { API_TOKEN = "tok123" }
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 1);
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(mcp.name, "company-tools");
    assert_eq!(mcp.proxy, Some(crate::types::McpProxyMode::Host));
    assert!(mcp.is_host_proxied());
    assert_eq!(mcp.command, Some("/usr/local/bin/company-mcp".to_string()));
    assert_eq!(mcp.args, vec!["--mode", "production"]);
    assert_eq!(mcp.env.get("API_TOKEN").unwrap(), "tok123");
}

#[test]
fn test_parse_mcp_host_proxied_by_name_only() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.postgres]
proxy = "host"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 1);
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(mcp.name, "postgres");
    assert!(mcp.is_host_proxied());
    assert!(mcp.command.is_none());
    assert!(mcp.args.is_empty());
    assert!(mcp.env.is_empty());
    assert!(mcp.install.is_empty());
    assert!(mcp.source.is_none());
}

#[test]
fn test_parse_mcp_multiple_servers() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.filesystem]
command = "npx"
args = ["@mcp/server-filesystem", "/workspace"]

[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]

[mcp.host-db]
proxy = "host"
command = "npx"
args = ["-y", "@mcp/server-postgres"]

[mcp.host-lookup]
proxy = "host"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 4);

    let internal_count = coastfile
        .mcp_servers
        .iter()
        .filter(|m| !m.is_host_proxied())
        .count();
    assert_eq!(internal_count, 2);

    let host_count = coastfile
        .mcp_servers
        .iter()
        .filter(|m| m.is_host_proxied())
        .count();
    assert_eq!(host_count, 2);

    let lookup = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == "host-lookup")
        .unwrap();
    assert!(lookup.command.is_none());
}

#[test]
fn test_parse_mcp_install_as_single_string() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.filesystem]
install = "npm install @modelcontextprotocol/server-filesystem"
command = "npx"
args = ["@modelcontextprotocol/server-filesystem"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(
        mcp.install,
        vec!["npm install @modelcontextprotocol/server-filesystem"]
    );
}

#[test]
fn test_parse_mcp_install_as_array() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.custom]
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let mcp = &coastfile.mcp_servers[0];
    assert_eq!(
        mcp.install,
        vec!["npm install".to_string(), "npm run build".to_string()]
    );
}

#[test]
fn test_parse_no_mcp_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.mcp_servers.is_empty());
}

#[test]
fn test_mcp_reject_internal_missing_command() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.broken]
install = ["npm install something"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp 'broken'"));
    assert!(err.contains("command"));
}

#[test]
fn test_mcp_reject_host_with_install() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.bad]
proxy = "host"
install = ["npm install something"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp 'bad'"));
    assert!(err.contains("install"));
    assert!(err.contains("proxy"));
}

#[test]
fn test_mcp_reject_host_with_source() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.bad]
proxy = "host"
source = "./tools/my-mcp"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp 'bad'"));
    assert!(err.contains("source"));
    assert!(err.contains("proxy"));
}

#[test]
fn test_mcp_reject_invalid_proxy_value() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.bad]
proxy = "cloud"
command = "some-cmd"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp 'bad'"));
    assert!(err.contains("invalid proxy"));
    assert!(err.contains("cloud"));
}

// --- [mcp_clients] section tests ---

#[test]
fn test_parse_mcp_clients_builtin_claude_code() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.claude-code]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_clients.len(), 1);
    let client = &coastfile.mcp_clients[0];
    assert_eq!(client.name, "claude-code");
    assert_eq!(
        client.format,
        Some(crate::types::McpClientFormat::ClaudeCode)
    );
    assert!(client.config_path.is_none());
    assert!(client.run.is_none());
    assert_eq!(
        client.resolved_config_path(),
        Some("/root/.claude/mcp_servers.json")
    );
}

#[test]
fn test_parse_mcp_clients_builtin_cursor() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.cursor]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_clients.len(), 1);
    let client = &coastfile.mcp_clients[0];
    assert_eq!(client.name, "cursor");
    assert_eq!(client.format, Some(crate::types::McpClientFormat::Cursor));
    assert_eq!(
        client.resolved_config_path(),
        Some("/workspace/.cursor/mcp.json")
    );
}

#[test]
fn test_parse_mcp_clients_builtin_with_path_override() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.claude-code]
config_path = "/custom/path/mcp.json"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let client = &coastfile.mcp_clients[0];
    assert_eq!(
        client.format,
        Some(crate::types::McpClientFormat::ClaudeCode)
    );
    assert_eq!(
        client.config_path,
        Some("/custom/path/mcp.json".to_string())
    );
    assert_eq!(client.resolved_config_path(), Some("/custom/path/mcp.json"));
}

#[test]
fn test_parse_mcp_clients_custom_format_with_path() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.my-fork]
format = "claude-code"
config_path = "/home/user/.my-fork/mcp.json"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let client = &coastfile.mcp_clients[0];
    assert_eq!(client.name, "my-fork");
    assert_eq!(
        client.format,
        Some(crate::types::McpClientFormat::ClaudeCode)
    );
    assert_eq!(
        client.config_path,
        Some("/home/user/.my-fork/mcp.json".to_string())
    );
}

#[test]
fn test_parse_mcp_clients_custom_run_command() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.exotic-tool]
run = "coast-mcp-connector-exotic --output /etc/exotic/mcp.conf"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let client = &coastfile.mcp_clients[0];
    assert_eq!(client.name, "exotic-tool");
    assert!(client.format.is_none());
    assert!(client.config_path.is_none());
    assert_eq!(
        client.run,
        Some("coast-mcp-connector-exotic --output /etc/exotic/mcp.conf".to_string())
    );
    assert!(client.is_command_based());
}

#[test]
fn test_parse_mcp_clients_multiple() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.claude-code]

[mcp_clients.cursor]
config_path = "/workspace/.cursor/mcp.json"

[mcp_clients.exotic]
run = "my-script"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_clients.len(), 3);
}

#[test]
fn test_parse_no_mcp_clients_section() {
    let toml = r#"
[coast]
name = "my-app"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(coastfile.mcp_clients.is_empty());
}

#[test]
fn test_mcp_clients_reject_run_with_format() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.bad]
run = "some-script"
format = "claude-code"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp_clients 'bad'"));
    assert!(err.contains("run"));
    assert!(err.contains("format"));
}

#[test]
fn test_mcp_clients_reject_run_with_config_path() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.bad]
run = "some-script"
config_path = "/some/path"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp_clients 'bad'"));
    assert!(err.contains("run"));
}

#[test]
fn test_mcp_clients_reject_unknown_format() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.my-tool]
format = "vscode"
config_path = "/some/path"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp_clients 'my-tool'"));
    assert!(err.contains("unknown format"));
    assert!(err.contains("vscode"));
}

// --- Coastfile type from path tests ---

#[test]
fn test_type_from_base_coastfile() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile")).unwrap();
    assert_eq!(t, None);
}

#[test]
fn test_type_from_typed_coastfile() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.light")).unwrap();
    assert_eq!(t, Some("light".to_string()));
}

#[test]
fn test_type_from_multi_part_suffix() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.ci.minimal")).unwrap();
    assert_eq!(t, Some("ci.minimal".to_string()));
}

#[test]
fn test_type_default_is_illegal() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.default"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Coastfile.default"));
}

#[test]
fn test_type_trailing_dot_is_illegal() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile."));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("empty"));
}

#[test]
fn test_type_non_coastfile_returns_none() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/docker-compose.yml")).unwrap();
    assert_eq!(t, None);
}

// --- .toml extension type-from-path tests ---

#[test]
fn test_type_from_coastfile_toml_is_default() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.toml")).unwrap();
    assert_eq!(t, None);
}

#[test]
fn test_type_from_coastfile_light_toml() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.light.toml")).unwrap();
    assert_eq!(t, Some("light".to_string()));
}

#[test]
fn test_type_from_coastfile_snap_toml() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.snap.toml")).unwrap();
    assert_eq!(t, Some("snap".to_string()));
}

#[test]
fn test_type_from_coastfile_test_toml() {
    let t = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.test.toml")).unwrap();
    assert_eq!(t, Some("test".to_string()));
}

#[test]
fn test_type_from_multi_part_suffix_toml() {
    let t =
        Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.ci.minimal.toml")).unwrap();
    assert_eq!(t, Some("ci.minimal".to_string()));
}

#[test]
fn test_type_toml_is_reserved() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.toml.toml"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("reserved"));
}

#[test]
fn test_type_default_toml_is_still_illegal() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.default.toml"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Coastfile.default"));
}

#[test]
fn test_type_trailing_dot_toml_is_illegal() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile..toml"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("empty"));
}

// --- find_coastfile tests ---

#[test]
fn test_find_coastfile_plain_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Coastfile"), "[coast]\nname = \"a\"\n").unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile");
    assert_eq!(found, Some(dir.path().join("Coastfile")));
}

#[test]
fn test_find_coastfile_toml_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Coastfile.toml"), "[coast]\nname = \"a\"\n").unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile");
    assert_eq!(found, Some(dir.path().join("Coastfile.toml")));
}

#[test]
fn test_find_coastfile_both_prefers_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Coastfile"), "[coast]\nname = \"a\"\n").unwrap();
    std::fs::write(dir.path().join("Coastfile.toml"), "[coast]\nname = \"a\"\n").unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile");
    assert_eq!(found, Some(dir.path().join("Coastfile.toml")));
}

#[test]
fn test_find_coastfile_neither_exists() {
    let dir = tempfile::tempdir().unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile");
    assert_eq!(found, None);
}

#[test]
fn test_find_coastfile_typed_plain_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile.light");
    assert_eq!(found, Some(dir.path().join("Coastfile.light")));
}

#[test]
fn test_find_coastfile_typed_toml_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light.toml"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile.light");
    assert_eq!(found, Some(dir.path().join("Coastfile.light.toml")));
}

#[test]
fn test_find_coastfile_typed_both_prefers_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light.toml"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let found = Coastfile::find_coastfile(dir.path(), "Coastfile.light");
    assert_eq!(found, Some(dir.path().join("Coastfile.light.toml")));
}

// --- find_coastfile_for_type tests ---

#[test]
fn test_find_coastfile_for_type_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Coastfile.toml"), "[coast]\nname = \"a\"\n").unwrap();
    let found = Coastfile::find_coastfile_for_type(dir.path(), None);
    assert_eq!(found, Some(dir.path().join("Coastfile.toml")));
}

#[test]
fn test_find_coastfile_for_type_named() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.snap.toml"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let found = Coastfile::find_coastfile_for_type(dir.path(), Some("snap"));
    assert_eq!(found, Some(dir.path().join("Coastfile.snap.toml")));
}

#[test]
fn test_find_coastfile_for_type_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let found = Coastfile::find_coastfile_for_type(dir.path(), Some("nonexistent"));
    assert_eq!(found, None);
}

// --- from_file integration tests with .toml extension ---

#[test]
fn test_from_file_coastfile_toml_is_default_type() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.toml"),
        "[coast]\nname = \"my-app\"\n",
    )
    .unwrap();
    let cf = Coastfile::from_file(&dir.path().join("Coastfile.toml")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.coastfile_type, None);
}

#[test]
fn test_from_file_coastfile_light_toml_has_type() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.toml"),
        "[coast]\nname = \"my-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light.toml"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light.toml")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
}

#[test]
fn test_extends_discovers_coastfile_toml() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile.toml"),
        "[coast]\nname = \"my-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
}

#[test]
fn test_extends_prefers_toml_over_plain() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        "[coast]\nname = \"plain-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.toml"),
        "[coast]\nname = \"toml-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light"),
        "[coast]\nextends = \"Coastfile\"\n",
    )
    .unwrap();
    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "toml-app");
}

#[test]
fn test_extends_subdir_discovers_coastfile_toml() {
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("subdir");
    std::fs::create_dir_all(&subdir).unwrap();
    std::fs::write(
        subdir.join("Coastfile.toml"),
        "[coast]\nname = \"sub-app\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("Coastfile.light"),
        "[coast]\nextends = \"subdir/Coastfile\"\n",
    )
    .unwrap();
    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "sub-app");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
}

// ---------------------------------------------------------------------------
// External worktree dir helpers
// ---------------------------------------------------------------------------

#[test]
fn test_is_external_worktree_dir_relative() {
    assert!(!Coastfile::is_external_worktree_dir(".worktrees"));
    assert!(!Coastfile::is_external_worktree_dir("my-worktrees"));
    assert!(!Coastfile::is_external_worktree_dir(".claude/worktrees"));
}

#[test]
fn test_is_external_worktree_dir_tilde() {
    assert!(Coastfile::is_external_worktree_dir("~/.codex/worktrees"));
    assert!(Coastfile::is_external_worktree_dir("~/worktrees"));
}

#[test]
fn test_is_external_worktree_dir_absolute() {
    assert!(Coastfile::is_external_worktree_dir("/tmp/worktrees"));
    assert!(Coastfile::is_external_worktree_dir(
        "/home/user/.codex/worktrees"
    ));
}

#[test]
fn test_is_external_worktree_dir_empty() {
    assert!(!Coastfile::is_external_worktree_dir(""));
}

#[test]
fn test_is_external_worktree_dir_tilde_alone() {
    assert!(!Coastfile::is_external_worktree_dir("~"));
}

#[test]
fn test_resolve_worktree_dir_relative() {
    let root = Path::new("/projects/my-app");
    let resolved = Coastfile::resolve_worktree_dir(root, ".worktrees");
    assert_eq!(resolved, Path::new("/projects/my-app/.worktrees"));
}

#[test]
fn test_resolve_worktree_dir_nested_relative() {
    let root = Path::new("/projects/my-app");
    let resolved = Coastfile::resolve_worktree_dir(root, ".claude/worktrees");
    assert_eq!(resolved, Path::new("/projects/my-app/.claude/worktrees"));
}

#[test]
fn test_resolve_worktree_dir_absolute() {
    let root = Path::new("/projects/my-app");
    let resolved = Coastfile::resolve_worktree_dir(root, "/tmp/worktrees");
    assert_eq!(resolved, Path::new("/tmp/worktrees"));
}

#[test]
fn test_resolve_worktree_dir_tilde_expansion() {
    let root = Path::new("/projects/my-app");
    let resolved = Coastfile::resolve_worktree_dir(root, "~/.codex/worktrees");
    let home = dirs::home_dir().unwrap();
    assert_eq!(resolved, home.join(".codex/worktrees"));
}

#[test]
fn test_external_worktree_dirs_mixed() {
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(
        r#"
[coast]
name = "test"
worktree_dir = [".worktrees", "~/.codex/worktrees", "/tmp/ext-wt"]
"#,
        dir.path(),
    )
    .unwrap();

    let externals = cf.external_worktree_dirs();
    assert_eq!(externals.len(), 2);
    assert_eq!(externals[0].0, 1);
    assert_eq!(externals[1].0, 2);
    assert_eq!(externals[1].1, Path::new("/tmp/ext-wt"));
}

#[test]
fn test_external_worktree_dirs_all_local() {
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(
        r#"
[coast]
name = "test"
worktree_dir = [".worktrees", ".claude/worktrees"]
"#,
        dir.path(),
    )
    .unwrap();

    let externals = cf.external_worktree_dirs();
    assert!(externals.is_empty());
}

#[test]
fn test_external_worktree_dirs_all_external() {
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(
        r#"
[coast]
name = "test"
worktree_dir = ["~/.codex/worktrees", "/opt/worktrees"]
"#,
        dir.path(),
    )
    .unwrap();

    let externals = cf.external_worktree_dirs();
    assert_eq!(externals.len(), 2);
    assert_eq!(externals[0].0, 0);
    assert_eq!(externals[1].0, 1);
}

#[test]
fn test_external_worktree_dirs_empty() {
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(
        r#"
[coast]
name = "test"
"#,
        dir.path(),
    )
    .unwrap();

    let externals = cf.external_worktree_dirs();
    assert!(externals.is_empty());
}

#[test]
fn test_external_mount_path() {
    assert_eq!(Coastfile::external_mount_path(0), "/host-external-wt/0");
    assert_eq!(Coastfile::external_mount_path(3), "/host-external-wt/3");
}

// ---------------------------------------------------------------------------
// Glob pattern tests
// ---------------------------------------------------------------------------

#[test]
fn test_is_glob_pattern() {
    assert!(Coastfile::is_glob_pattern("~/.shep/repos/*/wt"));
    assert!(Coastfile::is_glob_pattern("/foo/ba?/baz"));
    assert!(Coastfile::is_glob_pattern("/foo/[abc]/bar"));
    assert!(!Coastfile::is_glob_pattern("~/.codex/worktrees"));
    assert!(!Coastfile::is_glob_pattern(".worktrees"));
    assert!(!Coastfile::is_glob_pattern("/absolute/path"));
}

#[test]
fn test_glob_root_computation() {
    use std::path::Path;

    assert_eq!(
        Coastfile::glob_root("/home/user/.shep/repos/*/wt"),
        Path::new("/home/user/.shep/repos")
    );
    assert_eq!(Coastfile::glob_root("/foo/ba?/baz"), Path::new("/foo"));
    assert_eq!(Coastfile::glob_root("/a/b/[abc]/c"), Path::new("/a/b"));
    assert_eq!(
        Coastfile::glob_root("/no/globs/here"),
        Path::new("/no/globs/here"),
        "path without globs returns the full path"
    );
    assert_eq!(
        Coastfile::glob_root("/*/everything/is/glob"),
        Path::new("/"),
        "glob in first component returns root"
    );
}

#[test]
fn test_resolve_external_worktree_dirs_expanded_no_globs() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = vec![".worktrees".to_string(), "~/.codex/worktrees".to_string()];
    let result = Coastfile::resolve_external_worktree_dirs_expanded(&dirs, dir.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].mount_index, 1);
    assert_eq!(result[0].raw_pattern, "~/.codex/worktrees");
}

#[test]
fn test_resolve_external_worktree_dirs_expanded_glob_returns_root() {
    let dir = tempfile::tempdir().unwrap();
    let ext = dir.path().join("ext");
    std::fs::create_dir_all(ext.join("aaa").join("wt")).unwrap();
    std::fs::create_dir_all(ext.join("bbb").join("wt")).unwrap();
    std::fs::create_dir_all(ext.join("ccc")).unwrap();

    let pattern = format!("{}/*/wt", ext.display());
    let dirs = vec![".worktrees".to_string(), pattern.clone()];
    let result = Coastfile::resolve_external_worktree_dirs_expanded(&dirs, dir.path());

    assert_eq!(
        result.len(),
        1,
        "glob should produce a single entry for the root"
    );
    assert_eq!(result[0].mount_index, 1, "reuses original index");
    assert_eq!(
        result[0].resolved_path, ext,
        "resolved_path should be the glob root"
    );
    assert_eq!(result[0].raw_pattern, pattern);
}

#[test]
fn test_resolve_external_worktree_dirs_expanded_glob_no_matches_still_returns_root() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("nonexistent");
    let pattern = format!("{}/*/wt", base.display());
    let dirs = vec![".worktrees".to_string(), pattern];
    let result = Coastfile::resolve_external_worktree_dirs_expanded(&dirs, dir.path());
    assert_eq!(
        result.len(),
        1,
        "glob root should be returned even when nothing matches yet"
    );
    assert_eq!(result[0].resolved_path, base);
    assert_eq!(result[0].mount_index, 1);
}

#[test]
fn test_resolve_external_worktree_dirs_expanded_preserves_non_glob_index() {
    let dir = tempfile::tempdir().unwrap();
    let ext = dir.path().join("ext");
    std::fs::create_dir_all(ext.join("hash1").join("wt")).unwrap();

    let glob_pattern = format!("{}/*/wt", ext.display());
    let dirs = vec![
        ".worktrees".to_string(),         // index 0 (local)
        "~/.codex/worktrees".to_string(), // index 1 (external, non-glob)
        glob_pattern,                     // index 2 (external, glob)
        "/some/literal/path".to_string(), // index 3 (external, non-glob)
    ];
    let result = Coastfile::resolve_external_worktree_dirs_expanded(&dirs, dir.path());

    assert_eq!(result.len(), 3);
    assert_eq!(result[0].mount_index, 1, "codex keeps index 1");
    assert_eq!(result[1].mount_index, 2, "glob gets index 2");
    assert_eq!(
        result[1].resolved_path, ext,
        "glob entry resolves to the glob root"
    );
    assert_eq!(result[2].mount_index, 3, "literal keeps index 3");
}

#[test]
fn test_resolve_external_worktree_dirs_expanded_glob_root_covers_future_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let ext = dir.path().join("repos");
    std::fs::create_dir_all(ext.join("aaa").join("wt")).unwrap();
    std::fs::create_dir_all(ext.join("bbb").join("wt")).unwrap();

    let pattern = format!("{}/*/wt", ext.display());
    let dirs = vec![pattern];
    let result = Coastfile::resolve_external_worktree_dirs_expanded(&dirs, dir.path());

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resolved_path, ext);

    // A new directory created after resolution is still under the glob root.
    let future = ext.join("ccc").join("wt");
    std::fs::create_dir_all(&future).unwrap();
    assert!(
        future.starts_with(&result[0].resolved_path),
        "future directory should be covered by the glob root mount"
    );
}

#[test]
fn test_worktree_dir_string_or_vec_compat() {
    let dir = tempfile::tempdir().unwrap();

    let single = Coastfile::parse(
        r#"
[coast]
name = "test"
worktree_dir = ".custom"
"#,
        dir.path(),
    )
    .unwrap();
    assert_eq!(single.worktree_dirs, vec![".custom"]);

    let array = Coastfile::parse(
        r#"
[coast]
name = "test"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
"#,
        dir.path(),
    )
    .unwrap();
    assert_eq!(
        array.worktree_dirs,
        vec![".worktrees", "~/.codex/worktrees"]
    );
}

// ---------------------------------------------------------------------------
// Worktree project_root resolution tests
// ---------------------------------------------------------------------------

fn git_in(root: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed to start");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_from_file_in_nested_worktree_resolves_repo_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    git_in(root, &["init", "-b", "main"]);

    let coastfile_content = "[coast]\nname = \"test-app\"\n";
    std::fs::write(root.join("Coastfile"), coastfile_content).unwrap();
    git_in(root, &["add", "Coastfile"]);
    git_in(root, &["commit", "-m", "init"]);
    git_in(root, &["branch", "feat"]);

    let wt_path = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
    git_in(
        root,
        &["worktree", "add", &wt_path.to_string_lossy(), "feat"],
    );

    let wt_coastfile = wt_path.join("Coastfile");
    assert!(
        wt_coastfile.exists(),
        "worktree should contain tracked Coastfile"
    );

    let cf = Coastfile::from_file(&wt_coastfile).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let canonical_project = cf.project_root.canonicalize().unwrap();
    assert_eq!(
        canonical_project, canonical_root,
        "project_root should resolve to the repo root, not the worktree"
    );
}

#[test]
fn test_from_file_in_normal_repo_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    git_in(root, &["init", "-b", "main"]);

    let coastfile_content = "[coast]\nname = \"test-app\"\n";
    std::fs::write(root.join("Coastfile"), coastfile_content).unwrap();

    let cf = Coastfile::from_file(&root.join("Coastfile")).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let canonical_project = cf.project_root.canonicalize().unwrap();
    assert_eq!(
        canonical_project, canonical_root,
        "project_root should be the Coastfile's parent in a normal repo"
    );
}

#[test]
fn test_from_file_no_git_dir_unchanged() {
    let dir = tempfile::tempdir().unwrap();

    let coastfile_content = "[coast]\nname = \"test-app\"\n";
    std::fs::write(dir.path().join("Coastfile"), coastfile_content).unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(
        cf.project_root,
        dir.path(),
        "project_root should fall back to the Coastfile's parent when no .git exists"
    );
}

#[test]
fn test_from_file_in_claude_worktree_resolves_repo_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    git_in(root, &["init", "-b", "main"]);

    let coastfile_content = "[coast]\nname = \"test-app\"\n";
    std::fs::write(root.join("Coastfile"), coastfile_content).unwrap();
    git_in(root, &["add", "Coastfile"]);
    git_in(root, &["commit", "-m", "init"]);
    git_in(root, &["branch", "redo-website"]);

    let wt_path = root.join(".claude").join("worktrees").join("redo-website");
    std::fs::create_dir_all(wt_path.parent().unwrap()).unwrap();
    git_in(
        root,
        &[
            "worktree",
            "add",
            &wt_path.to_string_lossy(),
            "redo-website",
        ],
    );

    let wt_coastfile = wt_path.join("Coastfile");
    assert!(
        wt_coastfile.exists(),
        "worktree should contain tracked Coastfile"
    );

    let cf = Coastfile::from_file(&wt_coastfile).unwrap();
    let canonical_root = root.canonicalize().unwrap();
    let canonical_project = cf.project_root.canonicalize().unwrap();
    assert_eq!(
        canonical_project, canonical_root,
        "project_root should resolve to the repo root from .claude/worktrees/"
    );
}

// --- private_paths tests ---

#[test]
fn test_private_paths_parsed() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = ["frontend/.next", ".cache"]
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(cf.private_paths, vec!["frontend/.next", ".cache"]);
}

#[test]
fn test_private_paths_defaults_to_empty() {
    let toml = r#"
[coast]
name = "my-app"
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(cf.private_paths.is_empty());
}

#[test]
fn test_private_paths_rejects_absolute() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = ["/var/cache"]
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    assert!(
        err.to_string().contains("must be a relative path"),
        "got: {}",
        err
    );
}

#[test]
fn test_private_paths_rejects_dotdot() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = ["../outside"]
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    assert!(
        err.to_string().contains("must not contain '..'"),
        "got: {}",
        err
    );
}

#[test]
fn test_private_paths_rejects_nested() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = ["frontend/.next", "frontend/.next/cache"]
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    assert!(err.to_string().contains("overlap"), "got: {}", err);
}

#[test]
fn test_private_paths_rejects_empty_entry() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = [""]
"#;
    let err = Coastfile::parse(toml, Path::new("/tmp")).unwrap_err();
    assert!(err.to_string().contains("cannot be empty"), "got: {}", err);
}

#[test]
fn test_private_paths_serialized_and_roundtrips() {
    let toml = r#"
[coast]
name = "my-app"
private_paths = ["frontend/.next", ".turbo"]
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    let serialized = cf.to_standalone_toml();
    assert!(serialized.contains("private_paths"));
    assert!(serialized.contains("frontend/.next"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coastfile.toml");
    std::fs::write(&path, &serialized).unwrap();
    let reparsed = Coastfile::from_file(&path).unwrap();
    assert_eq!(reparsed.private_paths, vec!["frontend/.next", ".turbo"]);
}

#[test]
fn test_build_private_paths_mount_commands_empty() {
    let cmd = Coastfile::build_private_paths_mount_commands(&[]);
    assert!(cmd.is_empty());
}

#[test]
fn test_build_private_paths_mount_commands_single() {
    let paths = vec!["frontend/.next".to_string()];
    let cmd = Coastfile::build_private_paths_mount_commands(&paths);
    assert!(cmd.starts_with(" && "));
    assert!(cmd.contains("mkdir -p '/coast-private/frontend/.next' '/workspace/frontend/.next'"));
    assert!(
        cmd.contains("mount --bind '/coast-private/frontend/.next' '/workspace/frontend/.next'")
    );
}

#[test]
fn test_build_private_paths_mount_commands_multiple() {
    let paths = vec!["frontend/.next".to_string(), ".turbo".to_string()];
    let cmd = Coastfile::build_private_paths_mount_commands(&paths);
    assert!(cmd.contains("/coast-private/frontend/.next"));
    assert!(cmd.contains("/coast-private/.turbo"));
    let bind_count = cmd.matches("mount --bind").count();
    assert_eq!(bind_count, 2);
}

#[test]
fn test_build_private_paths_unmount_commands_empty() {
    let cmd = Coastfile::build_private_paths_unmount_commands(&[]);
    assert!(cmd.is_empty());
}

#[test]
fn test_build_private_paths_unmount_commands_single() {
    let paths = vec!["frontend/.next".to_string()];
    let cmd = Coastfile::build_private_paths_unmount_commands(&paths);
    assert!(cmd.contains("umount -l '/workspace/frontend/.next'"));
    assert!(
        !cmd.contains("find"),
        "should not walk files (lazy unmount only)"
    );
    assert!(cmd.ends_with("; "));
}

#[test]
fn test_build_private_paths_unmount_commands_multiple() {
    let paths = vec!["frontend/.next".to_string(), ".turbo".to_string()];
    let cmd = Coastfile::build_private_paths_unmount_commands(&paths);
    assert!(cmd.contains("umount -l '/workspace/frontend/.next'"));
    assert!(cmd.contains("umount -l '/workspace/.turbo'"));
    let umount_count = cmd.matches("umount -l").count();
    assert_eq!(umount_count, 2);
}

#[test]
fn test_unmount_commands_precede_mount_commands() {
    let paths = vec!["data".to_string()];
    let unmount = Coastfile::build_private_paths_unmount_commands(&paths);
    let mount = Coastfile::build_private_paths_mount_commands(&paths);
    let combined =
        format!("{unmount}umount -l /workspace 2>/dev/null; mount --bind /new /workspace{mount}");
    let private_umount_pos = combined.find("umount -l '/workspace/data'").unwrap();
    let workspace_umount_pos = combined.find("umount -l /workspace 2>/dev/null").unwrap();
    let mount_pos = combined.find("mount --bind '/coast-private/data'").unwrap();
    assert!(
        private_umount_pos < workspace_umount_pos,
        "private path unmount should come before workspace unmount"
    );
    assert!(
        workspace_umount_pos < mount_pos,
        "workspace unmount should come before private_paths remount"
    );
}
