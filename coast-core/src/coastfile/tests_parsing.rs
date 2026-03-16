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
