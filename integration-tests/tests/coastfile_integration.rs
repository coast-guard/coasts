/// Coastfile parsing and artifact building integration tests.
///
/// These tests validate the end-to-end Coastfile parsing pipeline and
/// artifact directory structure creation. They run without Docker.
use std::path::{Path, PathBuf};

use coast_core::artifact;
use coast_core::coastfile::Coastfile;
use coast_core::types::*;
use coast_core::volume;

// ---------------------------------------------------------------------------
// Realistic full Coastfile parsing
// ---------------------------------------------------------------------------

#[test]
fn test_parse_realistic_full_coastfile() {
    let toml = r#"
[coast]
name = "ecommerce-platform"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
web = 3000
api = 4000
postgres = 5432
redis = 6379
elasticsearch = 9200

[secrets.stripe_key]
extractor = "env"
var = "STRIPE_SECRET_KEY"
inject = "env:STRIPE_SECRET_KEY"

[secrets.gcp_credentials]
extractor = "file"
path = "~/.config/gcloud/application_default_credentials.json"
inject = "file:/run/secrets/gcp-credentials.json"

[secrets.aws_session]
extractor = "command"
run = "aws sts get-session-token --output json"
inject = "env:AWS_SESSION_TOKEN"
ttl = "1h"

[inject]
env = ["NODE_ENV", "DEBUG", "LOG_LEVEL"]
files = []

[volumes.postgres_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"

[volumes.elasticsearch_data]
strategy = "isolated"
service = "elasticsearch"
mount = "/usr/share/elasticsearch/data"

[volumes.redis_data]
strategy = "shared"
service = "redis"
mount = "/data"

[volumes.seed_db]
strategy = "isolated"
snapshot_source = "ecommerce_seed_data"
service = "db"
mount = "/var/lib/postgresql/data"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["ecommerce_shared_pg:/var/lib/postgresql/data"]
env = { POSTGRES_PASSWORD = "dev", POSTGRES_USER = "dev" }
auto_create_db = true
inject = "env:DATABASE_URL"

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
"#;

    let root = Path::new("/home/user/dev/ecommerce");
    let coastfile = Coastfile::parse(toml, root).unwrap();

    // Basic validation
    assert_eq!(coastfile.name, "ecommerce-platform");
    assert_eq!(coastfile.runtime, RuntimeType::Dind);

    // Ports
    assert_eq!(coastfile.ports.len(), 5);
    assert_eq!(coastfile.ports.get("web"), Some(&3000));
    assert_eq!(coastfile.ports.get("api"), Some(&4000));
    assert_eq!(coastfile.ports.get("elasticsearch"), Some(&9200));

    // Secrets
    assert_eq!(coastfile.secrets.len(), 3);
    let stripe = coastfile
        .secrets
        .iter()
        .find(|s| s.name == "stripe_key")
        .unwrap();
    assert_eq!(stripe.extractor, "env");
    assert_eq!(
        stripe.inject,
        InjectType::Env("STRIPE_SECRET_KEY".to_string())
    );

    let gcp = coastfile
        .secrets
        .iter()
        .find(|s| s.name == "gcp_credentials")
        .unwrap();
    assert_eq!(gcp.extractor, "file");
    assert_eq!(
        gcp.inject,
        InjectType::File(PathBuf::from("/run/secrets/gcp-credentials.json"))
    );

    let aws = coastfile
        .secrets
        .iter()
        .find(|s| s.name == "aws_session")
        .unwrap();
    assert_eq!(aws.extractor, "command");
    assert_eq!(aws.ttl, Some("1h".to_string()));

    // Inject config
    assert_eq!(coastfile.inject.env, vec!["NODE_ENV", "DEBUG", "LOG_LEVEL"]);
    assert!(coastfile.inject.files.is_empty());

    // Volumes
    assert_eq!(coastfile.volumes.len(), 4);

    let pg_vol = coastfile
        .volumes
        .iter()
        .find(|v| v.name == "postgres_data")
        .unwrap();
    assert_eq!(pg_vol.strategy, VolumeStrategy::Isolated);
    assert_eq!(pg_vol.service, "db");

    let redis_vol = coastfile
        .volumes
        .iter()
        .find(|v| v.name == "redis_data")
        .unwrap();
    assert_eq!(redis_vol.strategy, VolumeStrategy::Shared);

    let seed_vol = coastfile
        .volumes
        .iter()
        .find(|v| v.name == "seed_db")
        .unwrap();
    assert_eq!(seed_vol.strategy, VolumeStrategy::Isolated);
    assert_eq!(
        seed_vol.snapshot_source.as_deref(),
        Some("ecommerce_seed_data")
    );

    // Shared services
    assert_eq!(coastfile.shared_services.len(), 2);
    let pg_svc = coastfile
        .shared_services
        .iter()
        .find(|s| s.name == "postgres")
        .unwrap();
    assert!(pg_svc.auto_create_db);
    assert_eq!(
        pg_svc.inject,
        Some(InjectType::Env("DATABASE_URL".to_string()))
    );

    let redis_svc = coastfile
        .shared_services
        .iter()
        .find(|s| s.name == "redis")
        .unwrap();
    assert!(!redis_svc.auto_create_db);
    assert!(redis_svc.inject.is_none());
}

// ---------------------------------------------------------------------------
// Coastfile error handling
// ---------------------------------------------------------------------------

#[test]
fn test_coastfile_missing_coast_section() {
    let toml = r#"
[ports]
web = 3000
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
}

#[test]
fn test_coastfile_empty_name() {
    let toml = r#"
[coast]
name = ""
compose = "./docker-compose.yml"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("name"));
}

#[test]
fn test_coastfile_without_compose_succeeds() {
    let toml = r#"
[coast]
name = "my-app"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_ok());
    let cf = result.unwrap();
    assert!(cf.compose.is_none());
}

#[test]
fn test_coastfile_invalid_runtime() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"
runtime = "lxc"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid runtime"));
}

#[test]
fn test_coastfile_zero_port() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[ports]
web = 0
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("port"));
}

#[test]
fn test_coastfile_invalid_inject_syntax() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[secrets.bad]
extractor = "file"
path = "/tmp/secret"
inject = "neither:format"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("invalid inject format"));
}

#[test]
fn test_coastfile_invalid_volume_strategy() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[volumes.data]
strategy = "replicated"
service = "app"
mount = "/data"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid strategy"));
}

#[test]
fn test_coastfile_snapshot_source_on_shared_rejected() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[volumes.data]
strategy = "shared"
snapshot_source = "some_volume"
service = "db"
mount = "/data"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("snapshot_source is only valid with strategy 'isolated'"));
}

#[test]
fn test_coastfile_snapshot_source_on_isolated_accepted() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[volumes.seed_data]
strategy = "isolated"
snapshot_source = "coast_seed_pg_data"
service = "db"
mount = "/var/lib/postgresql/data"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.volumes.len(), 1);
    let vol = &coastfile.volumes[0];
    assert_eq!(vol.strategy, VolumeStrategy::Isolated);
    assert_eq!(vol.snapshot_source.as_deref(), Some("coast_seed_pg_data"));
    assert_eq!(vol.service, "db");
}

#[test]
fn test_coastfile_isolated_without_snapshot_source() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[volumes.pg_data]
strategy = "isolated"
service = "db"
mount = "/var/lib/postgresql/data"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.volumes.len(), 1);
    let vol = &coastfile.volumes[0];
    assert_eq!(vol.strategy, VolumeStrategy::Isolated);
    assert!(vol.snapshot_source.is_none());
}

#[test]
fn test_volume_snapshot_source_deleted_on_rm() {
    let volumes = vec![
        VolumeConfig {
            name: "pg_data".to_string(),
            strategy: VolumeStrategy::Isolated,
            service: "db".to_string(),
            mount: PathBuf::from("/var/lib/postgresql/data"),
            snapshot_source: Some("seed_vol".to_string()),
        },
        VolumeConfig {
            name: "plain".to_string(),
            strategy: VolumeStrategy::Isolated,
            service: "app".to_string(),
            mount: PathBuf::from("/data"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "shared_cache".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "cache".to_string(),
            mount: PathBuf::from("/cache"),
            snapshot_source: None,
        },
    ];

    let to_delete = volume::volumes_to_delete(&volumes, "inst-1");
    assert_eq!(to_delete.len(), 2);
    assert!(to_delete.contains(&"coast--inst-1--pg_data".to_string()));
    assert!(to_delete.contains(&"coast--inst-1--plain".to_string()));
    assert!(!to_delete.iter().any(|v| v.contains("shared_cache")));
}

#[test]
fn test_coastfile_snapshot_strategy_no_longer_valid() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"

[volumes.data]
strategy = "snapshot"
service = "db"
mount = "/data"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("invalid strategy"));
}

#[test]
fn test_coastfile_invalid_toml() {
    let result = Coastfile::parse("{{not valid toml", Path::new("/tmp"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Coastfile defaults and resolution
// ---------------------------------------------------------------------------

#[test]
fn test_coastfile_default_runtime_is_dind() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./dc.yml"
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(cf.runtime, RuntimeType::Dind);
}

#[test]
fn test_coastfile_all_runtimes() {
    for rt in &["dind", "sysbox", "podman"] {
        let toml = format!(
            r#"
[coast]
name = "app"
compose = "./dc.yml"
runtime = "{rt}"
"#
        );
        let cf = Coastfile::parse(&toml, Path::new("/tmp")).unwrap();
        assert_eq!(cf.runtime.as_str(), *rt);
    }
}

#[test]
fn test_coastfile_compose_relative_path_resolution() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let root = Path::new("/home/user/dev/project");
    let cf = Coastfile::parse(toml, root).unwrap();
    assert_eq!(
        cf.compose,
        Some(PathBuf::from("/home/user/dev/project/docker-compose.yml"))
    );
}

#[test]
fn test_coastfile_compose_absolute_path_preserved() {
    let toml = r#"
[coast]
name = "my-app"
compose = "/absolute/path/docker-compose.yml"
"#;
    let cf = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(
        cf.compose,
        Some(PathBuf::from("/absolute/path/docker-compose.yml"))
    );
}

// ---------------------------------------------------------------------------
// Artifact building integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_artifact_build_with_inject_files() {
    let dir = tempfile::tempdir().unwrap();
    let artifact_dir = dir.path().join("artifact");
    std::fs::create_dir_all(artifact_dir.join("inject")).unwrap();

    // Create source files to inject
    let src_dir = dir.path().join("source");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("config.json"), r#"{"key": "value"}"#).unwrap();
    std::fs::write(src_dir.join("cert.pem"), "-----BEGIN CERTIFICATE-----\n...").unwrap();

    let files = vec![
        src_dir.join("config.json").to_string_lossy().to_string(),
        src_dir.join("cert.pem").to_string_lossy().to_string(),
    ];

    let copied = artifact::copy_inject_files(&files, &artifact_dir).unwrap();
    assert_eq!(copied.len(), 2);
    assert!(copied.contains(&"config.json".to_string()));
    assert!(copied.contains(&"cert.pem".to_string()));

    // Verify files exist in inject directory
    assert!(artifact_dir.join("inject").join("config.json").exists());
    assert!(artifact_dir.join("inject").join("cert.pem").exists());

    // Verify content
    let content = std::fs::read_to_string(artifact_dir.join("inject").join("config.json")).unwrap();
    assert_eq!(content, r#"{"key": "value"}"#);
}

#[test]
fn test_artifact_inject_missing_file_error() {
    let dir = tempfile::tempdir().unwrap();
    let artifact_dir = dir.path().join("artifact");
    std::fs::create_dir_all(artifact_dir.join("inject")).unwrap();

    let files = vec!["/nonexistent/coast-test-file-xyz.txt".to_string()];
    let result = artifact::copy_inject_files(&files, &artifact_dir);
    assert!(result.is_err());
}

#[test]
fn test_artifact_inject_empty_files() {
    let dir = tempfile::tempdir().unwrap();
    let artifact_dir = dir.path().join("artifact");
    std::fs::create_dir_all(&artifact_dir).unwrap();

    let copied = artifact::copy_inject_files(&[], &artifact_dir).unwrap();
    assert!(copied.is_empty());
}

// ---------------------------------------------------------------------------
// Volume warning generation for shared + database combos
// ---------------------------------------------------------------------------

#[test]
fn test_volume_warnings_comprehensive() {
    let volumes = vec![
        VolumeConfig {
            name: "pg_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "postgres".to_string(),
            mount: PathBuf::from("/var/lib/postgresql/data"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "mysql_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "mysql-primary".to_string(),
            mount: PathBuf::from("/var/lib/mysql"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "mongo_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "mongodb".to_string(),
            mount: PathBuf::from("/data/db"),
            snapshot_source: None,
        },
        VolumeConfig {
            name: "app_data".to_string(),
            strategy: VolumeStrategy::Shared,
            service: "app".to_string(),
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
        // Isolated volumes should NOT generate warnings even for databases
        VolumeConfig {
            name: "isolated_pg".to_string(),
            strategy: VolumeStrategy::Isolated,
            service: "postgres".to_string(),
            mount: PathBuf::from("/var/lib/postgresql/data"),
            snapshot_source: None,
        },
    ];

    let warnings = volume::generate_volume_warnings(&volumes);

    // postgres, mysql, mongodb, redis are all database-like
    // "app" is NOT database-like
    // isolated postgres should NOT trigger a warning
    assert_eq!(
        warnings.len(),
        4,
        "expected warnings for pg, mysql, mongo, redis but got: {:?}",
        warnings
    );

    // Verify each database service gets its own warning
    assert!(warnings.iter().any(|w| w.contains("pg_data")));
    assert!(warnings.iter().any(|w| w.contains("mysql_data")));
    assert!(warnings.iter().any(|w| w.contains("mongo_data")));
    assert!(warnings.iter().any(|w| w.contains("redis_data")));

    // app_data should NOT be in warnings
    assert!(!warnings.iter().any(|w| w.contains("app_data")));

    // isolated_pg should NOT be in warnings
    assert!(!warnings.iter().any(|w| w.contains("isolated_pg")));
}

// ---------------------------------------------------------------------------
// Coastfile from_file integration
// ---------------------------------------------------------------------------

#[test]
fn test_coastfile_from_file_integration() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile_path = dir.path().join("Coastfile");

    let toml = r#"
[coast]
name = "file-test"
compose = "./docker-compose.yml"
runtime = "sysbox"

[ports]
web = 8080
api = 9090

[volumes.data]
strategy = "isolated"
service = "app"
mount = "/app/data"
"#;

    std::fs::write(&coastfile_path, toml).unwrap();

    let cf = Coastfile::from_file(&coastfile_path).unwrap();
    assert_eq!(cf.name, "file-test");
    assert_eq!(cf.runtime, RuntimeType::Sysbox);
    assert_eq!(cf.project_root, dir.path());
    assert_eq!(cf.ports.len(), 2);
    assert_eq!(cf.volumes.len(), 1);
}

#[test]
fn test_coastfile_from_nonexistent_file() {
    let result = Coastfile::from_file(Path::new("/tmp/nonexistent-coast-test-xyz/Coastfile"));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// MCP servers + clients integration
// ---------------------------------------------------------------------------

#[test]
fn test_parse_coastfile_with_mcp_servers_and_clients() {
    let toml = r#"
[coast]
name = "mcp-project"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
app = 49500

[mcp.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]

[mcp.echo]
source = "./mcp-echo"
install = ["npm install"]
command = "node"
args = ["server.js"]
env = { DEBUG = "true" }

[mcp.host-db]
proxy = "host"
command = "npx"
args = ["-y", "@mcp/server-postgres"]
env = { POSTGRES_URL = "postgresql://localhost:5432/dev" }

[mcp.host-lookup]
proxy = "host"

[mcp_clients.claude-code]

[mcp_clients.cursor]
config_path = "/workspace/.cursor/mcp.json"

[mcp_clients.my-fork]
format = "claude-code"
config_path = "/home/user/.my-fork/mcp.json"

[mcp_clients.exotic]
run = "my-connector-script --write"
"#;

    let root = Path::new("/home/user/dev/mcp-project");
    let coastfile = Coastfile::parse(toml, root).unwrap();

    // MCP servers
    assert_eq!(coastfile.mcp_servers.len(), 4, "expected 4 MCP servers");

    let internal_servers: Vec<_> = coastfile
        .mcp_servers
        .iter()
        .filter(|m| !m.is_host_proxied())
        .collect();
    assert_eq!(internal_servers.len(), 2, "expected 2 internal MCP servers");

    let host_servers: Vec<_> = coastfile
        .mcp_servers
        .iter()
        .filter(|m| m.is_host_proxied())
        .collect();
    assert_eq!(host_servers.len(), 2, "expected 2 host-proxied MCP servers");

    let echo = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == "echo")
        .unwrap();
    assert_eq!(echo.source, Some("./mcp-echo".to_string()));
    assert_eq!(echo.install, vec!["npm install"]);
    assert_eq!(echo.command, Some("node".to_string()));
    assert_eq!(echo.env.get("DEBUG").unwrap(), "true");

    let host_lookup = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == "host-lookup")
        .unwrap();
    assert!(host_lookup.is_host_proxied());
    assert!(host_lookup.command.is_none());

    // MCP clients
    assert_eq!(coastfile.mcp_clients.len(), 4, "expected 4 MCP clients");

    let claude = coastfile
        .mcp_clients
        .iter()
        .find(|c| c.name == "claude-code")
        .unwrap();
    assert_eq!(claude.format, Some(McpClientFormat::ClaudeCode));
    assert_eq!(
        claude.resolved_config_path(),
        Some("/root/.claude/mcp_servers.json")
    );

    let cursor = coastfile
        .mcp_clients
        .iter()
        .find(|c| c.name == "cursor")
        .unwrap();
    assert_eq!(cursor.format, Some(McpClientFormat::Cursor));
    assert_eq!(
        cursor.resolved_config_path(),
        Some("/workspace/.cursor/mcp.json")
    );

    let fork = coastfile
        .mcp_clients
        .iter()
        .find(|c| c.name == "my-fork")
        .unwrap();
    assert_eq!(fork.format, Some(McpClientFormat::ClaudeCode));
    assert_eq!(
        fork.resolved_config_path(),
        Some("/home/user/.my-fork/mcp.json")
    );

    let exotic = coastfile
        .mcp_clients
        .iter()
        .find(|c| c.name == "exotic")
        .unwrap();
    assert!(exotic.is_command_based());
    assert_eq!(exotic.run, Some("my-connector-script --write".to_string()));
    assert!(exotic.resolved_config_path().is_none());
}

#[test]
fn test_mcp_client_connector_serialization_roundtrip() {
    let config = McpClientConnectorConfig {
        name: "claude-code".to_string(),
        format: Some(McpClientFormat::ClaudeCode),
        config_path: Some("/custom/path.json".to_string()),
        run: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpClientConnectorConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "claude-code");
    assert_eq!(deserialized.format, Some(McpClientFormat::ClaudeCode));
    assert_eq!(
        deserialized.config_path,
        Some("/custom/path.json".to_string())
    );
    assert!(deserialized.run.is_none());
}

#[test]
fn test_mcp_server_config_serialization_roundtrip() {
    let config = McpServerConfig {
        name: "filesystem".to_string(),
        proxy: None,
        command: Some("npx".to_string()),
        args: vec![
            "-y".to_string(),
            "@mcp/server-filesystem".to_string(),
            "/workspace".to_string(),
        ],
        env: std::collections::HashMap::new(),
        install: vec!["npm install @mcp/server-filesystem".to_string()],
        source: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "filesystem");
    assert!(!deserialized.is_host_proxied());
    assert_eq!(deserialized.command, Some("npx".to_string()));
    assert_eq!(deserialized.args.len(), 3);
    assert_eq!(deserialized.install.len(), 1);
}

#[test]
fn test_parse_coastfile_mcp_internal_with_source() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[mcp.custom]
source = "./tools/my-mcp"
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
env = { KEY = "val" }
"#;
    let root = Path::new("/home/user/dev/my-app");
    let coastfile = Coastfile::parse(toml, root).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 1);
    let mcp = coastfile
        .mcp_servers
        .iter()
        .find(|m| m.name == "custom")
        .unwrap();
    assert_eq!(mcp.source, Some("./tools/my-mcp".to_string()));
    assert_eq!(mcp.install, vec!["npm install", "npm run build"]);
    assert_eq!(mcp.command, Some("node".to_string()));
    assert_eq!(mcp.args, vec!["dist/index.js"]);
    assert_eq!(mcp.env.get("KEY").unwrap(), "val");
    assert!(!mcp.is_host_proxied());
}

#[test]
fn test_parse_coastfile_mcp_host_by_name() {
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
    assert_eq!(mcp.proxy, Some(McpProxyMode::Host));
    assert!(mcp.command.is_none());
    assert!(mcp.args.is_empty());
}

#[test]
fn test_parse_coastfile_mcp_install_string_vs_array() {
    let single = r#"
[coast]
name = "a"

[mcp.tool]
install = "npm install something"
command = "npx"
args = ["something"]
"#;
    let array = r#"
[coast]
name = "b"

[mcp.tool]
install = ["npm install", "npm run build"]
command = "node"
args = ["dist/index.js"]
"#;

    let cf1 = Coastfile::parse(single, Path::new("/tmp")).unwrap();
    assert_eq!(cf1.mcp_servers[0].install, vec!["npm install something"]);

    let cf2 = Coastfile::parse(array, Path::new("/tmp")).unwrap();
    assert_eq!(
        cf2.mcp_servers[0].install,
        vec!["npm install", "npm run build"]
    );
}

#[test]
fn test_parse_coastfile_mcp_empty_section() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert!(
        coastfile.mcp_servers.is_empty(),
        "no [mcp] section should produce empty mcp_servers"
    );
    assert!(
        coastfile.mcp_clients.is_empty(),
        "no [mcp_clients] section should produce empty mcp_clients"
    );
}

#[test]
fn test_parse_coastfile_mcp_reject_internal_no_command() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.broken]
install = ["npm install something"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("command"),
        "error should mention missing command: {err}"
    );
}

#[test]
fn test_parse_coastfile_mcp_reject_host_with_install() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.bad]
proxy = "host"
install = ["npm install foo"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("install"),
        "error should mention install: {err}"
    );
    assert!(
        err.contains("proxy") || err.contains("host"),
        "error should mention proxy/host: {err}"
    );
}

#[test]
fn test_parse_coastfile_mcp_reject_host_with_source() {
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
    assert!(err.contains("source"), "error should mention source: {err}");
    assert!(
        err.contains("proxy") || err.contains("host"),
        "error should mention proxy/host: {err}"
    );
}

#[test]
fn test_parse_coastfile_mcp_reject_invalid_proxy() {
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
    assert!(
        err.contains("cloud"),
        "error should mention the invalid value: {err}"
    );
}

// ===========================================================================
// Coastfile composable types — extends, includes, unset
// ===========================================================================

#[test]
fn test_coastfile_extends_inherits_all_fields() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
runtime = "dind"

[coast.setup]
packages = ["curl", "jq"]
run = ["echo base"]

[ports]
web = 3000
api = 4000
postgres = 5432

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.db_pass]
extractor = "env"
var = "DB_PASS"
inject = "env:DB_PASS"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
env = { POSTGRES_PASSWORD = "dev" }

[shared_services.redis]
image = "redis:7"
ports = [6379]

[omit]
services = ["debug"]

[inject]
env = ["NODE_ENV"]
files = ["~/.gitconfig"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[coast.setup]
packages = ["nodejs"]
run = ["echo light"]

[ports]
api = 5000

[unset]
secrets = ["db_pass"]
shared_services = ["postgres"]
ports = ["postgres"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();

    // Name inherited from parent
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
    assert_eq!(cf.runtime, RuntimeType::Dind);

    // Setup: packages deduped, run concatenated
    assert!(cf.setup.packages.contains(&"curl".to_string()));
    assert!(cf.setup.packages.contains(&"jq".to_string()));
    assert!(cf.setup.packages.contains(&"nodejs".to_string()));
    assert_eq!(cf.setup.run, vec!["echo base", "echo light"]);

    // Ports: child overrides api, inherits web, postgres unset
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.ports.get("api"), Some(&5000));
    assert!(
        !cf.ports.contains_key("postgres"),
        "postgres port should be unset"
    );

    // Secrets: db_pass unset, api_key inherited
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "api_key");

    // Shared services: postgres unset, redis inherited
    assert_eq!(cf.shared_services.len(), 1);
    assert_eq!(cf.shared_services[0].name, "redis");

    // Omit: inherited from parent
    assert!(cf.omit.services.contains(&"debug".to_string()));

    // Inject: inherited
    assert_eq!(cf.inject.env, vec!["NODE_ENV"]);
    assert_eq!(cf.inject.files, vec!["~/.gitconfig"]);
}

#[test]
fn test_coastfile_extends_three_level_chain() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "chain-app"

[ports]
web = 3000
api = 4000
db = 5432
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.mid"),
        r#"
[coast]
extends = "Coastfile"
runtime = "sysbox"

[ports]
api = 5000
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.final"),
        r#"
[coast]
extends = "Coastfile.mid"

[ports]
debug = 9999
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.final")).unwrap();
    assert_eq!(cf.name, "chain-app");
    assert_eq!(cf.coastfile_type, Some("final".to_string()));
    assert_eq!(cf.runtime, RuntimeType::Sysbox);
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.ports.get("api"), Some(&5000));
    assert_eq!(cf.ports.get("db"), Some(&5432));
    assert_eq!(cf.ports.get("debug"), Some(&9999));
}

#[test]
fn test_coastfile_includes_merges_fragments() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("extra-secrets.toml"),
        r#"
[coast]

[secrets.extra_token]
extractor = "env"
var = "TOKEN"
inject = "env:TOKEN"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("extra-ports.toml"),
        r#"
[coast]

[ports]
monitoring = 9090
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "includes-app"
includes = ["extra-secrets.toml", "extra-ports.toml"]

[ports]
web = 3000
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.name, "includes-app");
    assert_eq!(cf.coastfile_type, None);

    // Self's ports take precedence, include's ports are merged in
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.ports.get("monitoring"), Some(&9090));

    // Include's secrets are merged
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "extra_token");
}

#[test]
fn test_coastfile_extends_with_includes() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "combo-app"

[ports]
web = 3000

[secrets.base_key]
extractor = "env"
var = "BASE_KEY"
inject = "env:BASE_KEY"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("ci-secrets.toml"),
        r#"
[coast]

[secrets.ci_token]
extractor = "env"
var = "CI_TOKEN"
inject = "env:CI_TOKEN"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.ci"),
        r#"
[coast]
extends = "Coastfile"
includes = ["ci-secrets.toml"]

[ports]
web = 8080

[unset]
secrets = ["base_key"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.ci")).unwrap();
    assert_eq!(cf.name, "combo-app");
    assert_eq!(cf.coastfile_type, Some("ci".to_string()));

    // Self overrides parent's web port
    assert_eq!(cf.ports.get("web"), Some(&8080));

    // base_key is unset, ci_token from include remains
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "ci_token");
}

#[test]
fn test_coastfile_unset_all_categories() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "unset-app"

[ports]
web = 3000
api = 4000

[egress]
host-api = 48080

[secrets.key_a]
extractor = "env"
var = "A"
inject = "env:A"

[secrets.key_b]
extractor = "env"
var = "B"
inject = "env:B"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]

[volumes.pg_data]
strategy = "isolated"
service = "db"
mount = "/data"

[mcp.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.minimal"),
        r#"
[coast]
extends = "Coastfile"

[unset]
ports = ["api"]
egress = ["host-api"]
secrets = ["key_b"]
shared_services = ["postgres"]
volumes = ["pg_data"]
mcp = ["context7"]
mcp_clients = ["claude-code"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.minimal")).unwrap();

    assert_eq!(cf.ports.len(), 1);
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert!(cf.egress.is_empty());
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "key_a");
    assert!(cf.shared_services.is_empty());
    assert!(cf.volumes.is_empty());
    assert!(cf.mcp_servers.is_empty());
    assert!(cf.mcp_clients.is_empty());
}

#[test]
fn test_coastfile_cycle_detection_direct() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "cycle"
extends = "Coastfile.other"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.other"),
        r#"
[coast]
name = "cycle"
extends = "Coastfile"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("circular"),
        "should detect circular dependency: {err}"
    );
}

#[test]
fn test_coastfile_cycle_detection_self() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "self-cycle"
extends = "Coastfile"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("circular"),
        "should detect self-referential extends: {err}"
    );
}

#[test]
fn test_coastfile_type_from_filename() {
    assert_eq!(
        Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile")).unwrap(),
        None
    );
    assert_eq!(
        Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.light")).unwrap(),
        Some("light".to_string())
    );
    assert_eq!(
        Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.ci.minimal")).unwrap(),
        Some("ci.minimal".to_string())
    );
}

#[test]
fn test_coastfile_type_default_is_illegal() {
    let result = Coastfile::coastfile_type_from_path(Path::new("/proj/Coastfile.default"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Coastfile.default"),
        "error should mention Coastfile.default: {err}"
    );
}

#[test]
fn test_coastfile_include_cannot_have_extends() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "base"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("bad-include.toml"),
        r#"
[coast]
extends = "Coastfile"
name = "sneaky"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.bad"),
        r#"
[coast]
name = "main"
includes = ["bad-include.toml"]
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.bad"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("cannot use extends"),
        "should reject includes with extends: {err}"
    );
}

#[test]
fn test_coastfile_extends_missing_parent_errors() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile.orphan"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.orphan"));
    assert!(result.is_err(), "should fail when parent doesn't exist");
}

#[test]
fn test_coastfile_omit_concatenation_across_extends() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "omit-app"

[omit]
services = ["keycloak"]
volumes = ["keycloak-data"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.more"),
        r#"
[coast]
extends = "Coastfile"

[omit]
services = ["redash", "langfuse"]
volumes = ["redash-data"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.more")).unwrap();
    assert_eq!(cf.omit.services, vec!["keycloak", "redash", "langfuse"]);
    assert_eq!(cf.omit.volumes, vec!["keycloak-data", "redash-data"]);
}

#[test]
fn test_coastfile_inject_concatenation_across_extends() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "inject-app"

[inject]
env = ["NODE_ENV"]
files = ["~/.gitconfig"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.dev"),
        r#"
[coast]
extends = "Coastfile"

[inject]
env = ["DEBUG"]
files = ["~/.ssh/id_ed25519"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.dev")).unwrap();
    assert_eq!(cf.inject.env, vec!["NODE_ENV", "DEBUG"]);
    assert_eq!(cf.inject.files, vec!["~/.gitconfig", "~/.ssh/id_ed25519"]);
}

#[test]
fn test_coastfile_child_overrides_name() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "parent-name"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.renamed"),
        r#"
[coast]
extends = "Coastfile"
name = "child-name"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.renamed")).unwrap();
    assert_eq!(cf.name, "child-name");
}

#[test]
fn test_coastfile_child_overrides_runtime() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "rt-app"
runtime = "dind"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.sysbox"),
        r#"
[coast]
extends = "Coastfile"
runtime = "sysbox"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.sysbox")).unwrap();
    assert_eq!(cf.runtime, RuntimeType::Sysbox);
}

#[test]
fn test_coastfile_child_inherits_runtime_when_not_specified() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "rt-app"
runtime = "sysbox"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.child"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.child")).unwrap();
    assert_eq!(cf.runtime, RuntimeType::Sysbox);
}

#[test]
fn test_coastfile_assign_child_overrides_parent() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "assign-app"

[assign]
default = "none"

[assign.services]
backend = "rebuild"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.fast"),
        r#"
[coast]
extends = "Coastfile"

[assign]
default = "restart"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.fast")).unwrap();
    assert_eq!(cf.assign.default, AssignAction::Restart);
    // Child fully replaces [assign], so parent services are gone
    assert!(cf.assign.services.is_empty());
}

#[test]
fn test_coastfile_secret_override_by_name() {
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "secret-app"

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.ci"),
        r#"
[coast]
extends = "Coastfile"

[secrets.api_key]
extractor = "command"
run = "echo ci-key"
inject = "env:API_KEY"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.ci")).unwrap();
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].extractor, "command");
}

#[test]
fn test_coastfile_parse_rejects_extends_in_string_mode() {
    let toml = r#"
[coast]
name = "my-app"
extends = "Coastfile"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("extends"),
        "should tell user to use from_file: {err}"
    );
}

#[test]
fn test_coastfile_parse_rejects_includes_in_string_mode() {
    let toml = r#"
[coast]
name = "my-app"
includes = ["other.toml"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("includes"),
        "should tell user to use from_file: {err}"
    );
}

#[test]
fn test_coastfile_integrated_examples_base() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("integrated-examples")
        .join("coast-types");

    if !examples.join("Coastfile").exists() {
        eprintln!(
            "Skipping: coast-types examples not found at {}",
            examples.display()
        );
        return;
    }

    let cf = Coastfile::from_file(&examples.join("Coastfile")).unwrap();
    assert_eq!(cf.name, "coast-types");
    assert_eq!(cf.coastfile_type, None);
    assert_eq!(cf.ports.len(), 4);
    assert_eq!(cf.secrets.len(), 2);
    assert_eq!(cf.shared_services.len(), 2);
}

#[test]
fn test_coastfile_integrated_examples_light() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("integrated-examples")
        .join("coast-types");

    if !examples.join("Coastfile.light").exists() {
        eprintln!("Skipping: coast-types examples not found");
        return;
    }

    let cf = Coastfile::from_file(&examples.join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "coast-types");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));

    // Unset: db_password secret removed, postgres/redis shared services removed
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "api_key");
    assert!(cf.shared_services.is_empty());

    // Ports: postgres and redis unset, api overridden
    assert!(!cf.ports.contains_key("postgres"));
    assert!(!cf.ports.contains_key("redis"));
    assert_eq!(cf.ports.get("api"), Some(&39080));
    assert_eq!(cf.ports.get("web"), Some(&38000));

    // Setup: packages concatenated, run concatenated
    assert!(cf.setup.packages.contains(&"curl".to_string()));
    assert!(cf.setup.packages.contains(&"nodejs".to_string()));
    assert_eq!(cf.setup.run.len(), 2);
}

#[test]
fn test_coastfile_integrated_examples_shared() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("integrated-examples")
        .join("coast-types");

    if !examples.join("Coastfile.shared").exists() {
        eprintln!("Skipping: coast-types examples not found");
        return;
    }

    let cf = Coastfile::from_file(&examples.join("Coastfile.shared")).unwrap();
    assert_eq!(cf.name, "coast-types");
    assert_eq!(cf.coastfile_type, Some("shared".to_string()));

    // All parent shared services + mongodb from child
    assert_eq!(cf.shared_services.len(), 3);
    assert!(cf.shared_services.iter().any(|s| s.name == "postgres"));
    assert!(cf.shared_services.iter().any(|s| s.name == "redis"));
    assert!(cf.shared_services.iter().any(|s| s.name == "mongodb"));

    // mongo_uri secret from the included extra-secrets.toml
    assert!(cf.secrets.iter().any(|s| s.name == "mongo_uri"));

    // Ports: all parent ports + mongodb
    assert_eq!(cf.ports.get("mongodb"), Some(&37017));
    assert_eq!(cf.ports.get("web"), Some(&38000));

    // Omit: concatenated from parent + child
    assert!(cf.omit.services.contains(&"monitoring".to_string()));
    assert!(cf.omit.services.contains(&"debug-tools".to_string()));
}

#[test]
fn test_coastfile_integrated_examples_chain() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("integrated-examples")
        .join("coast-types");

    if !examples.join("Coastfile.chain").exists() {
        eprintln!("Skipping: coast-types examples not found");
        return;
    }

    let cf = Coastfile::from_file(&examples.join("Coastfile.chain")).unwrap();
    assert_eq!(cf.name, "coast-types");
    assert_eq!(cf.coastfile_type, Some("chain".to_string()));

    // Chain inherits light's unsets (no postgres/redis shared services)
    assert!(cf.shared_services.is_empty());

    // Has debug port from chain + web from base + api from light
    assert_eq!(cf.ports.get("debug"), Some(&39999));
    assert_eq!(cf.ports.get("web"), Some(&38000));
    assert_eq!(cf.ports.get("api"), Some(&39080));

    // Setup: base + light + chain run commands
    assert_eq!(cf.setup.run.len(), 3);
    assert!(cf.setup.run[0].contains("base"));
    assert!(cf.setup.run[1].contains("light"));
    assert!(cf.setup.run[2].contains("chain"));
}

// ===========================================================================
// Real-world: filemap Coastfile.light
// ===========================================================================

#[test]
fn test_filemap_coastfile_light() {
    // Path to the real filemap project's Coastfile.light
    let filemap_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("dev")
        .join("filemap");

    let light_path = filemap_dir.join("Coastfile.light");
    if !light_path.exists() {
        eprintln!(
            "Skipping: filemap Coastfile.light not found at {}",
            light_path.display()
        );
        return;
    }

    let cf = Coastfile::from_file(&light_path).unwrap();

    // Basics
    assert_eq!(cf.name, "filemap");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
    assert_eq!(cf.runtime, RuntimeType::Dind);

    // No ports — all unset
    assert!(
        cf.ports.is_empty(),
        "light should have no ports, got: {:?}",
        cf.ports
    );

    // No shared services — all unset
    assert!(
        cf.shared_services.is_empty(),
        "light should have no shared services, got: {:?}",
        cf.shared_services
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );

    // No Claude secrets, no MCP
    assert!(
        cf.secrets.is_empty(),
        "light should have no secrets, got: {:?}",
        cf.secrets.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    assert!(
        cf.mcp_servers.is_empty(),
        "light should have no MCP servers"
    );
    assert!(
        cf.mcp_clients.is_empty(),
        "light should have no MCP clients"
    );

    // Omit: parent's list + light's additions
    let omitted = &cf.omit.services;
    // From parent
    assert!(omitted.contains(&"nginx-proxy".to_string()));
    assert!(omitted.contains(&"backend-debug".to_string()));
    // From light
    assert!(omitted.contains(&"redis".to_string()));
    assert!(omitted.contains(&"backend".to_string()));
    assert!(omitted.contains(&"mailhog".to_string()));
    assert!(omitted.contains(&"web".to_string()));
    assert!(omitted.contains(&"reach-web".to_string()));

    // Isolated DB volumes
    let pg_vol = cf.volumes.iter().find(|v| v.name == "postgres_data");
    assert!(pg_vol.is_some(), "should have postgres_data volume");
    assert_eq!(pg_vol.unwrap().strategy, VolumeStrategy::Isolated);

    let mongo_vol = cf.volumes.iter().find(|v| v.name == "mongodb_data");
    assert!(mongo_vol.is_some(), "should have mongodb_data volume");
    assert_eq!(mongo_vol.unwrap().strategy, VolumeStrategy::Isolated);

    let redis_vol = cf.volumes.iter().find(|v| v.name == "redis_data");
    assert!(redis_vol.is_some(), "should have redis_data volume");
    assert_eq!(redis_vol.unwrap().strategy, VolumeStrategy::Isolated);

    // go_modules_cache inherited from parent, still shared
    let go_vol = cf.volumes.iter().find(|v| v.name == "go_modules_cache");
    assert!(go_vol.is_some(), "should have go_modules_cache volume");
    assert_eq!(go_vol.unwrap().strategy, VolumeStrategy::Shared);

    // Assign: child overrides parent entirely
    assert_eq!(cf.assign.default, AssignAction::None);
    assert!(cf.assign.services.contains_key("backend-test"));
    assert_eq!(
        cf.assign.action_for_service("backend-test"),
        AssignAction::Rebuild
    );

    // Setup inherited from parent (Go, Node, etc.)
    assert!(!cf.setup.packages.is_empty());
    assert!(
        cf.setup.packages.contains(&"golang".to_string())
            || cf.setup.run.iter().any(|r| r.contains("go"))
    );
}
