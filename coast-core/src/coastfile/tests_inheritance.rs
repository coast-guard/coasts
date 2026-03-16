use super::*;
use crate::types::SharedServicePort;

#[test]
fn test_extends_basic() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
web = 3000
postgres = 5432
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[ports]
web = 8080
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.coastfile_type, Some("light".to_string()));
    assert_eq!(cf.ports.get("web"), Some(&8080));
    assert_eq!(cf.ports.get("postgres"), Some(&5432));
    assert_eq!(cf.runtime, RuntimeType::Dind);
}

#[test]
fn test_extends_chain_of_three() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
postgres = 5432
redis = 6379
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
web = 4000
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.final"),
        r#"
[coast]
extends = "Coastfile.mid"

[ports]
redis = 7000
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.final")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.runtime, RuntimeType::Sysbox);
    assert_eq!(cf.ports.get("web"), Some(&4000));
    assert_eq!(cf.ports.get("postgres"), Some(&5432));
    assert_eq!(cf.ports.get("redis"), Some(&7000));
}

#[test]
fn test_extends_child_overrides_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "parent-app"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.custom"),
        r#"
[coast]
extends = "Coastfile"
name = "child-app"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.custom")).unwrap();
    assert_eq!(cf.name, "child-app");
}

#[test]
fn test_extends_inherits_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "parent-app"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.name, "parent-app");
}

#[test]
fn test_extends_merge_secrets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.db_pass]
extractor = "file"
path = "/tmp/db_pass"
inject = "env:DB_PASS"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[secrets.api_key]
extractor = "command"
run = "echo test"
inject = "env:API_KEY"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.secrets.len(), 2);
    let api = cf.secrets.iter().find(|s| s.name == "api_key").unwrap();
    assert_eq!(api.extractor, "command");
    let db = cf.secrets.iter().find(|s| s.name == "db_pass").unwrap();
    assert_eq!(db.extractor, "file");
}

#[test]
fn test_extends_merge_setup_concatenates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[coast.setup]
packages = ["git", "curl"]
run = ["echo base"]
[[coast.setup.files]]
path = "/etc/base.json"
content = "{\"base\":true}"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.dev"),
        r#"
[coast]
extends = "Coastfile"

[coast.setup]
packages = ["curl", "nodejs"]
run = ["echo child"]
[[coast.setup.files]]
path = "/etc/base.json"
content = "{\"child\":true}"
mode = "0600"
[[coast.setup.files]]
path = "/etc/extra.json"
content = "{\"extra\":true}"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.dev")).unwrap();
    assert_eq!(cf.setup.packages, vec!["git", "curl", "nodejs"]);
    assert_eq!(cf.setup.run, vec!["echo base", "echo child"]);
    assert_eq!(cf.setup.files.len(), 2);
    let base = cf
        .setup
        .files
        .iter()
        .find(|f| f.path == "/etc/base.json")
        .unwrap();
    assert_eq!(base.content, "{\"child\":true}");
    assert_eq!(base.mode.as_deref(), Some("0600"));
    let extra = cf
        .setup
        .files
        .iter()
        .find(|f| f.path == "/etc/extra.json")
        .unwrap();
    assert_eq!(extra.content, "{\"extra\":true}");
}

// --- [unset] tests ---

#[test]
fn test_unset_removes_secrets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[secrets.db_pass]
extractor = "env"
var = "DB_PASS"
inject = "env:DB_PASS"

[ports]
web = 3000
redis = 6379
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[unset]
secrets = ["api_key"]
ports = ["redis"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "db_pass");
    assert_eq!(cf.ports.len(), 1);
    assert!(cf.ports.contains_key("web"));
    assert!(!cf.ports.contains_key("redis"));
}

#[test]
fn test_unset_removes_shared_services() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[shared_services.postgres]
image = "postgres:16"
ports = [5432]

[shared_services.redis]
image = "redis:7"
ports = [6379]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[unset]
shared_services = ["redis"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.shared_services.len(), 1);
    assert_eq!(cf.shared_services[0].name, "postgres");
}

// --- includes tests ---

#[test]
fn test_includes_basic() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("secrets.toml"),
        r#"
[coast]

[secrets.extra_key]
extractor = "env"
var = "EXTRA"
inject = "env:EXTRA"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
includes = ["secrets.toml"]

[ports]
web = 3000
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "extra_key");
}

#[test]
fn test_includes_self_overrides_include() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("base-ports.toml"),
        r#"
[coast]

[ports]
web = 8080
api = 9090
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
includes = ["base-ports.toml"]

[ports]
web = 3000
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.ports.get("api"), Some(&9090));
}

#[test]
fn test_includes_with_extends() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[ports]
web = 3000
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("extra-secrets.toml"),
        r#"
[coast]

[secrets.token]
extractor = "env"
var = "TOKEN"
inject = "env:TOKEN"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.full"),
        r#"
[coast]
extends = "Coastfile"
includes = ["extra-secrets.toml"]

[ports]
api = 8080
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.full")).unwrap();
    assert_eq!(cf.name, "my-app");
    assert_eq!(cf.ports.get("web"), Some(&3000));
    assert_eq!(cf.ports.get("api"), Some(&8080));
    assert_eq!(cf.secrets.len(), 1);
    assert_eq!(cf.secrets[0].name, "token");
}

#[test]
fn test_include_cannot_have_extends() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("nested.toml"),
        r#"
[coast]
extends = "Coastfile"
name = "nested"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.bad"),
        r#"
[coast]
name = "bad"
includes = ["nested.toml"]
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.bad"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("cannot use extends"));
}

// --- cycle detection tests ---

#[test]
fn test_extends_cycle_detection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
extends = "Coastfile.other"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.other"),
        r#"
[coast]
name = "other"
extends = "Coastfile"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("circular"));
}

#[test]
fn test_extends_self_cycle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
extends = "Coastfile"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("circular"));
}

// --- parse() rejects extends/includes ---

#[test]
fn test_parse_rejects_extends() {
    let toml = r#"
[coast]
name = "my-app"
extends = "Coastfile"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("extends"));
}

#[test]
fn test_parse_rejects_includes() {
    let toml = r#"
[coast]
name = "my-app"
includes = ["other.toml"]
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("includes"));
}

// --- omit concatenation test ---

#[test]
fn test_extends_omit_concatenated() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[omit]
services = ["keycloak"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[omit]
services = ["redash"]
volumes = ["keycloak-db-data"]
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.omit.services, vec!["keycloak", "redash"]);
    assert_eq!(cf.omit.volumes, vec!["keycloak-db-data"]);
}

#[test]
fn test_extends_merge_egress() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[egress]
host-api = 48080
postgres = 5432
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[egress]
postgres = 15432
redis = 6379
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.egress.get("host-api"), Some(&48080));
    assert_eq!(cf.egress.get("postgres"), Some(&15432));
    assert_eq!(cf.egress.get("redis"), Some(&6379));
}

#[test]
fn test_extends_egress_zero_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[egress]
bad = 0
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.light"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("egress"));
    assert!(err.contains("bad"));
}

#[test]
fn test_extends_merge_mcp_servers_by_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[mcp.filesystem]
command = "npx"
args = ["@mcp/server-filesystem", "/workspace"]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[mcp.filesystem]
proxy = "host"

[mcp.host-db]
proxy = "host"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.mcp_servers.len(), 2);
    let filesystem = cf
        .mcp_servers
        .iter()
        .find(|server| server.name == "filesystem")
        .unwrap();
    assert!(filesystem.is_host_proxied());
    assert!(filesystem.command.is_none());
    assert!(cf.mcp_servers.iter().any(|server| server.name == "host-db"));
}

#[test]
fn test_extends_merge_mcp_clients_by_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[mcp_clients.cursor]
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[mcp_clients.cursor]
config_path = "/workspace/.cursor/merged.json"

[mcp_clients.exotic]
run = "custom-mcp-sync"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.mcp_clients.len(), 2);
    let cursor = cf
        .mcp_clients
        .iter()
        .find(|client| client.name == "cursor")
        .unwrap();
    assert_eq!(
        cursor.config_path.as_deref(),
        Some("/workspace/.cursor/merged.json")
    );
    let exotic = cf
        .mcp_clients
        .iter()
        .find(|client| client.name == "exotic")
        .unwrap();
    assert_eq!(exotic.run.as_deref(), Some("custom-mcp-sync"));
}

#[test]
fn test_extends_healthcheck_is_not_inherited() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[ports]
web = 3000

[healthcheck]
web = "/"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert!(cf.healthcheck.is_empty());
}

#[test]
fn test_extends_ports_zero_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[ports]
web = 3000
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[ports]
bad = 0
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.light"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("port"));
    assert!(err.contains("bad"));
}

#[test]
fn test_extends_merge_volumes_by_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[volumes.cache]
strategy = "isolated"
service = "web"
mount = "/cache"

[volumes.data]
strategy = "shared"
service = "db"
mount = "/data"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"

[volumes.cache]
strategy = "shared"
service = "api"
mount = "/cache-override"

[volumes.logs]
strategy = "isolated"
service = "web"
mount = "/logs"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.volumes.len(), 3);

    let cache = cf
        .volumes
        .iter()
        .find(|volume| volume.name == "cache")
        .unwrap();
    assert_eq!(cache.service, "api");
    assert_eq!(cache.mount, PathBuf::from("/cache-override"));

    let data = cf
        .volumes
        .iter()
        .find(|volume| volume.name == "data")
        .unwrap();
    assert_eq!(data.service, "db");

    let logs = cf
        .volumes
        .iter()
        .find(|volume| volume.name == "logs")
        .unwrap();
    assert_eq!(logs.mount, PathBuf::from("/logs"));
}

#[test]
fn test_extends_child_paths_resolve_from_child_dir() {
    let dir = tempfile::tempdir().unwrap();
    let base_dir = dir.path().join("base");
    let child_dir = dir.path().join("variants");
    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::create_dir_all(&child_dir).unwrap();

    std::fs::write(
        base_dir.join("Coastfile"),
        r#"
[coast]
name = "my-app"
compose = "./docker-compose.base.yml"
"#,
    )
    .unwrap();

    std::fs::write(
        child_dir.join("Coastfile.light"),
        r#"
[coast]
extends = "../base/Coastfile"
compose = "./docker-compose.child.yml"
root = "../workspace"
"#,
    )
    .unwrap();

    let child_path = child_dir.join("Coastfile.light");
    let cf = Coastfile::from_file(&child_path).unwrap();
    assert_eq!(
        cf.compose,
        Some(child_dir.join("./docker-compose.child.yml"))
    );
    assert_eq!(cf.project_root, child_dir.join("../workspace"));
}

#[test]
fn test_extends_primary_port_can_reference_inherited_port() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[ports]
web = 3000
postgres = 5432
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"
primary_port = "web"
"#,
    )
    .unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert_eq!(cf.primary_port.as_deref(), Some("web"));
}

#[test]
fn test_extends_primary_port_invalid_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"

[ports]
web = 3000
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"
primary_port = "admin"
"#,
    )
    .unwrap();

    let result = Coastfile::from_file(&dir.path().join("Coastfile.light"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("primary_port"));
    assert!(err.contains("admin"));
}

#[test]
fn test_extends_worktree_dir_inheritance_and_override() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
worktree_dir = ".coasts-worktrees"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.inherit"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.override"),
        r#"
[coast]
extends = "Coastfile"
worktree_dir = ".custom-worktrees"
"#,
    )
    .unwrap();

    let inherited = Coastfile::from_file(&dir.path().join("Coastfile.inherit")).unwrap();
    assert_eq!(inherited.worktree_dirs, vec![".coasts-worktrees"]);

    let overridden = Coastfile::from_file(&dir.path().join("Coastfile.override")).unwrap();
    assert_eq!(overridden.worktree_dirs, vec![".custom-worktrees"]);
}

#[test]
fn test_extends_autostart_inheritance_and_override() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
autostart = false
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.inherit"),
        r#"
[coast]
extends = "Coastfile"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.override"),
        r#"
[coast]
extends = "Coastfile"
autostart = true
"#,
    )
    .unwrap();

    let inherited = Coastfile::from_file(&dir.path().join("Coastfile.inherit")).unwrap();
    assert!(!inherited.autostart);

    let overridden = Coastfile::from_file(&dir.path().join("Coastfile.override")).unwrap();
    assert!(overridden.autostart);
}

#[test]
fn test_mcp_clients_reject_custom_format_without_path() {
    let toml = r#"
[coast]
name = "my-app"

[mcp_clients.my-tool]
format = "claude-code"
"#;
    let result = Coastfile::parse(toml, Path::new("/tmp"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("mcp_clients 'my-tool'"));
    assert!(err.contains("config_path"));
}

#[test]
fn test_parse_mcp_servers_and_clients_together() {
    let toml = r#"
[coast]
name = "my-app"

[mcp.filesystem]
command = "npx"
args = ["@mcp/server-filesystem", "/workspace"]

[mcp.host-db]
proxy = "host"

[mcp_clients.claude-code]

[mcp_clients.cursor]
"#;
    let coastfile = Coastfile::parse(toml, Path::new("/tmp")).unwrap();
    assert_eq!(coastfile.mcp_servers.len(), 2);
    assert_eq!(coastfile.mcp_clients.len(), 2);
}

#[test]
fn test_standalone_toml_roundtrip() {
    let toml_input = r#"
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
runtime = "dind"
worktree_dir = ".worktrees"

[coast.setup]
packages = ["git", "curl"]
run = ["echo hello"]
[[coast.setup.files]]
path = "/etc/tool/config.json"
content = "{\"enabled\":true}"
mode = "0644"

[ports]
web = 3000
api = 8080

[shared_services.postgres]
image = "postgres:16"
ports = [5432]
volumes = ["pg_data:/var/lib/postgresql/data"]
env = { POSTGRES_USER = "app", POSTGRES_PASSWORD = "pass" }

[volumes.cache]
strategy = "shared"
service = "backend"
mount = "/cache"

[secrets.api_key]
extractor = "env"
var = "API_KEY"
inject = "env:API_KEY"

[omit]
services = ["debug", "test-only"]
volumes = ["test-vol"]

[assign]
default = "none"
[assign.services]
backend = "rebuild"
web = "restart"

[mcp.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]

[mcp_clients.claude-code]
"#;
    let dir = tempfile::tempdir().unwrap();
    let original = Coastfile::parse(toml_input, dir.path()).unwrap();
    let standalone = original.to_standalone_toml();
    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();

    assert_eq!(reparsed.name, original.name);
    assert_eq!(reparsed.runtime, original.runtime);
    assert_eq!(reparsed.worktree_dirs, original.worktree_dirs);
    assert_eq!(reparsed.autostart, original.autostart);
    assert_eq!(reparsed.ports, original.ports);
    assert_eq!(reparsed.setup.packages, original.setup.packages);
    assert_eq!(reparsed.setup.run, original.setup.run);
    assert_eq!(reparsed.setup.files, original.setup.files);
    assert_eq!(
        reparsed.shared_services.len(),
        original.shared_services.len()
    );
    assert_eq!(reparsed.volumes.len(), original.volumes.len());
    assert_eq!(reparsed.secrets.len(), original.secrets.len());
    assert_eq!(reparsed.omit.services, original.omit.services);
    assert_eq!(reparsed.omit.volumes, original.omit.volumes);
    assert_eq!(reparsed.assign.default, original.assign.default);
    assert_eq!(
        reparsed.assign.services.len(),
        original.assign.services.len()
    );
    assert_eq!(reparsed.mcp_servers.len(), original.mcp_servers.len());
    assert_eq!(reparsed.mcp_clients.len(), original.mcp_clients.len());
}

#[test]
fn test_standalone_toml_omits_empty_optional_sections() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile = Coastfile::parse(
        r#"
[coast]
name = "minimal"
"#,
        dir.path(),
    )
    .unwrap();

    let standalone = coastfile.to_standalone_toml();

    assert!(standalone.contains("[coast]"));
    assert!(standalone.contains("runtime = \"dind\""));
    assert!(standalone.contains("worktree_dir = \".worktrees\""));
    assert!(standalone.contains("\n[assign]\ndefault = \"restart\"\n"));
    assert!(!standalone.contains("compose = "));
    assert!(!standalone.contains("primary_port = "));
    assert!(!standalone.contains("autostart = false"));
    assert!(!standalone.contains("[coast.setup]"));
    assert!(!standalone.contains("[ports]"));
    assert!(!standalone.contains("[healthcheck]"));
    assert!(!standalone.contains("[shared_services."));
    assert!(!standalone.contains("[volumes."));
    assert!(!standalone.contains("[secrets."));
    assert!(!standalone.contains("[egress]"));
    assert!(!standalone.contains("[omit]"));
    assert!(!standalone.contains("[mcp."));
    assert!(!standalone.contains("[services."));
    assert!(!standalone.contains("[agent_shell]"));
    assert!(!standalone.contains("[mcp_clients."));
}

#[test]
fn test_standalone_toml_sorts_ports_healthcheck_and_egress() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile = Coastfile::parse(
        r#"
[coast]
name = "sorted"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080
postgres = 5432

[healthcheck]
web = "/"
api = "/healthz"

[egress]
postgres = 5432
host-api = 48080
"#,
        dir.path(),
    )
    .unwrap();

    let standalone = coastfile.to_standalone_toml();

    assert!(standalone.contains("\n[ports]\napi = 8080\npostgres = 5432\nweb = 3000\n"));
    assert!(standalone.contains("\n[healthcheck]\napi = \"/healthz\"\nweb = \"/\"\n"));
    assert!(standalone.contains("\n[egress]\nhost-api = 48080\npostgres = 5432\n"));
}

#[test]
fn test_standalone_toml_preserves_coast_fields_and_relative_compose() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile = Coastfile::parse(
        r#"
[coast]
name = "my-app"
compose = "./infra/docker-compose.yml"
worktree_dir = ".custom-worktrees"
autostart = false
primary_port = "web"

[ports]
web = 3000
"#,
        dir.path(),
    )
    .unwrap();

    let standalone = coastfile.to_standalone_toml();

    assert!(standalone.contains("compose = \"./infra/docker-compose.yml\""));
    assert!(standalone.contains("worktree_dir = \".custom-worktrees\""));
    assert!(standalone.contains("autostart = false"));
    assert!(standalone.contains("primary_port = \"web\""));

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert_eq!(
        reparsed.compose,
        Some(dir.path().join("./infra/docker-compose.yml"))
    );
    assert_eq!(reparsed.worktree_dirs, vec![".custom-worktrees"]);
    assert!(!reparsed.autostart);
    assert_eq!(reparsed.primary_port.as_deref(), Some("web"));
}

#[test]
fn test_standalone_toml_roundtrip_with_advanced_mcp_fields() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile = Coastfile::parse(
        r#"
[coast]
name = "my-app"

[mcp.host-db]
proxy = "host"
command = "npx"
args = ["-y", "@mcp/server-postgres"]

[mcp.custom-tool]
source = "./tools/my-mcp"
install = "npm install"
command = "node"
args = ["dist/index.js"]

[mcp_clients.my-fork]
format = "claude-code"
config_path = "/workspace/.my-fork/mcp.json"

[mcp_clients.exotic]
run = "custom-mcp-sync"
"#,
        dir.path(),
    )
    .unwrap();

    let standalone = coastfile.to_standalone_toml();
    assert!(standalone.contains("[mcp.host-db]"));
    assert!(standalone.contains("proxy = \"host\""));
    assert!(standalone.contains("source = \"./tools/my-mcp\""));
    assert!(standalone.contains("install = \"npm install\""));
    assert!(standalone.contains("[mcp_clients.my-fork]"));
    assert!(standalone.contains("format = \"claude-code\""));
    assert!(standalone.contains("config_path = \"/workspace/.my-fork/mcp.json\""));
    assert!(standalone.contains("[mcp_clients.exotic]"));
    assert!(standalone.contains("run = \"custom-mcp-sync\""));

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert_eq!(reparsed.mcp_servers.len(), 2);
    assert_eq!(reparsed.mcp_clients.len(), 2);

    let host_db = reparsed
        .mcp_servers
        .iter()
        .find(|server| server.name == "host-db")
        .unwrap();
    assert!(host_db.is_host_proxied());
    assert_eq!(host_db.command.as_deref(), Some("npx"));

    let custom_tool = reparsed
        .mcp_servers
        .iter()
        .find(|server| server.name == "custom-tool")
        .unwrap();
    assert_eq!(custom_tool.source.as_deref(), Some("./tools/my-mcp"));
    assert_eq!(custom_tool.install, vec!["npm install"]);

    let my_fork = reparsed
        .mcp_clients
        .iter()
        .find(|client| client.name == "my-fork")
        .unwrap();
    assert_eq!(
        my_fork.format,
        Some(crate::types::McpClientFormat::ClaudeCode)
    );
    assert_eq!(
        my_fork.config_path.as_deref(),
        Some("/workspace/.my-fork/mcp.json")
    );

    let exotic = reparsed
        .mcp_clients
        .iter()
        .find(|client| client.name == "exotic")
        .unwrap();
    assert_eq!(exotic.run.as_deref(), Some("custom-mcp-sync"));
}

#[test]
fn test_standalone_roundtrip_preserves_mapped_shared_service_ports() {
    let dir = tempfile::tempdir().unwrap();
    let coastfile = Coastfile::parse(
        r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[shared_services.postgis-db]
image = "ghcr.io/baosystems/postgis:12-3.3"
ports = ["5433:5432", 6432]
"#,
        dir.path(),
    )
    .unwrap();

    let standalone = coastfile.to_standalone_toml();
    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();

    assert_eq!(
        reparsed.shared_services[0].ports,
        vec![
            SharedServicePort::new(5433, 5432),
            SharedServicePort::same(6432),
        ]
    );
}

#[test]
fn test_standalone_toml_roundtrip_assign_exclude_paths_and_triggers() {
    let toml_input = r#"
[coast]
name = "test-app"
compose = "./docker-compose.yml"

[assign]
default = "none"
exclude_paths = ["docs", ".yarn", "apps/mobile"]

[assign.services]
web = "hot"
api = "restart"
worker = "rebuild"

[assign.rebuild_triggers]
api = ["Dockerfile", "package.json"]
worker = ["Dockerfile", "Gemfile", "Gemfile.lock"]
"#;
    let dir = tempfile::tempdir().unwrap();
    let original = Coastfile::parse(toml_input, dir.path()).unwrap();

    assert_eq!(
        original.assign.exclude_paths,
        vec!["docs", ".yarn", "apps/mobile"]
    );
    assert_eq!(original.assign.rebuild_triggers.len(), 2);

    let standalone = original.to_standalone_toml();

    assert!(
        standalone.contains("exclude_paths"),
        "standalone toml must contain exclude_paths"
    );
    assert!(
        standalone.contains("rebuild_triggers"),
        "standalone toml must contain rebuild_triggers section"
    );
    assert!(
        standalone.contains("docs"),
        "exclude_paths must contain 'docs'"
    );
    assert!(
        standalone.contains("apps/mobile"),
        "exclude_paths must contain 'apps/mobile'"
    );
    assert!(
        standalone.contains("Gemfile.lock"),
        "rebuild_triggers must contain 'Gemfile.lock'"
    );

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert_eq!(reparsed.assign.exclude_paths, original.assign.exclude_paths);
    assert_eq!(
        reparsed.assign.rebuild_triggers,
        original.assign.rebuild_triggers
    );
    assert_eq!(reparsed.assign.services, original.assign.services);
    assert_eq!(reparsed.assign.default, original.assign.default);
}

#[test]
fn test_standalone_toml_from_extended_coastfile() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Coastfile"),
        r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
redis = 6379

[shared_services.postgres]
image = "postgres:16"
ports = [5432]

[omit]
services = ["debug"]

[assign]
default = "none"
[assign.services]
backend = "rebuild"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.path().join("Coastfile.light"),
        r#"
[coast]
extends = "Coastfile"
autostart = false

[unset]
ports = ["web", "redis"]
shared_services = ["postgres"]

[omit]
services = ["backend", "web"]
"#,
    )
    .unwrap();

    let light = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert!(!light.autostart);
    assert!(light.ports.is_empty());
    assert!(light.shared_services.is_empty());
    assert!(light.omit.services.contains(&"debug".to_string()));
    assert!(light.omit.services.contains(&"backend".to_string()));

    let standalone = light.to_standalone_toml();
    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();

    assert_eq!(reparsed.name, "my-app");
    assert!(!reparsed.autostart);
    assert!(reparsed.ports.is_empty());
    assert!(reparsed.shared_services.is_empty());
    assert!(reparsed.omit.services.contains(&"debug".to_string()));
    assert!(reparsed.omit.services.contains(&"backend".to_string()));
    assert!(reparsed.omit.services.contains(&"web".to_string()));
}

#[test]
fn test_agent_shell_parsed() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"

[agent_shell]
command = "claude --dangerously-skip-permissions"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert!(cf.agent_shell.is_some());
    assert_eq!(
        cf.agent_shell.unwrap().command,
        "claude --dangerously-skip-permissions"
    );
}

#[test]
fn test_agent_shell_missing() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert!(cf.agent_shell.is_none());
}

#[test]
fn test_agent_shell_standalone_roundtrip() {
    let toml = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"

[agent_shell]
command = "claude --dangerously-skip-permissions"
"#;
    let dir = tempfile::tempdir().unwrap();
    let original = Coastfile::parse(toml, dir.path()).unwrap();
    let standalone = original.to_standalone_toml();
    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert!(reparsed.agent_shell.is_some());
    assert_eq!(
        reparsed.agent_shell.unwrap().command,
        "claude --dangerously-skip-permissions"
    );
}

#[test]
fn test_agent_shell_extends() {
    let dir = tempfile::tempdir().unwrap();

    let base = r#"
[coast]
name = "my-app"
compose = "./docker-compose.yml"
runtime = "dind"

[agent_shell]
command = "claude --base-mode"
"#;
    std::fs::write(dir.path().join("Coastfile"), base).unwrap();

    let child = r#"
[coast]
extends = "Coastfile"

[agent_shell]
command = "claude --child-override"
"#;
    std::fs::write(dir.path().join("Coastfile.light"), child).unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile.light")).unwrap();
    assert!(cf.agent_shell.is_some());
    assert_eq!(cf.agent_shell.unwrap().command, "claude --child-override");

    // Also test inheritance (child without agent_shell inherits parent's)
    let child_no_agent = r#"
[coast]
extends = "Coastfile"
autostart = false
"#;
    std::fs::write(dir.path().join("Coastfile.minimal"), child_no_agent).unwrap();
    let cf2 = Coastfile::from_file(&dir.path().join("Coastfile.minimal")).unwrap();
    assert!(cf2.agent_shell.is_some());
    assert_eq!(cf2.agent_shell.unwrap().command, "claude --base-mode");
}

// --- Bare services tests ---

#[test]
fn test_parse_coastfile_with_services() {
    let toml = r#"
[coast]
name = "bare-app"

[services.web]
command = "npm run dev"
port = 3000
restart = "on-failure"

[services.worker]
command = "npm run worker"
restart = "always"

[ports]
web = 3000
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert_eq!(cf.name, "bare-app");
    assert_eq!(cf.services.len(), 2);
    assert!(cf.compose.is_none());

    let web = cf.services.iter().find(|s| s.name == "web").unwrap();
    assert_eq!(web.command, "npm run dev");
    assert_eq!(web.port, Some(3000));
    assert_eq!(web.restart, crate::types::RestartPolicy::OnFailure);

    let worker = cf.services.iter().find(|s| s.name == "worker").unwrap();
    assert_eq!(worker.command, "npm run worker");
    assert_eq!(worker.port, None);
    assert_eq!(worker.restart, crate::types::RestartPolicy::Always);
}

#[test]
fn test_mixed_compose_and_bare_services() {
    let toml = r#"
[coast]
name = "mixed"
compose = "docker-compose.yml"

[services.vite]
command = "npm run dev"
port = 3040
restart = "on-failure"

[services.worker]
command = "node worker.js"
restart = "always"

[ports]
app = 3000
vite = 3040
"#;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert_eq!(cf.name, "mixed");
    assert!(cf.compose.is_some());
    assert_eq!(cf.services.len(), 2);

    let vite = cf.services.iter().find(|s| s.name == "vite").unwrap();
    assert_eq!(vite.command, "npm run dev");
    assert_eq!(vite.port, Some(3040));
    assert_eq!(vite.restart, crate::types::RestartPolicy::OnFailure);

    let worker = cf.services.iter().find(|s| s.name == "worker").unwrap();
    assert_eq!(worker.command, "node worker.js");
    assert_eq!(worker.restart, crate::types::RestartPolicy::Always);
}

#[test]
fn test_mixed_inherits_compose_adds_services() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap();

    let base_toml = r#"
[coast]
name = "base-mixed"
compose = "docker-compose.yml"

[ports]
app = 3000
"#;
    std::fs::write(dir.path().join("Coastfile"), base_toml).unwrap();

    let child_toml = r#"
[coast]
extends = "Coastfile"

[services.vite]
command = "npm run dev"
port = 3040

[ports]
vite = 3040
"#;
    let child_path = dir.path().join("Coastfile.dev");
    std::fs::write(&child_path, child_toml).unwrap();
    let cf = Coastfile::from_file(&child_path).unwrap();
    assert_eq!(cf.name, "base-mixed");
    assert!(cf.compose.is_some());
    assert_eq!(cf.services.len(), 1);
    assert_eq!(cf.services[0].name, "vite");
    assert_eq!(cf.ports.len(), 2);
}

#[test]
fn test_mixed_services_validation_still_applies() {
    let toml = r#"
[coast]
name = "bad-mixed"
compose = "docker-compose.yml"

[services.web]
command = "   "
"#;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker-compose.yml"), "version: '3'").unwrap();
    let result = Coastfile::parse(toml, dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("command"));
    assert!(err.contains("empty"));
}

#[test]
fn test_services_empty_command_rejected() {
    let toml = r#"
[coast]
name = "bad"

[services.web]
command = "   "
"#;
    let dir = tempfile::tempdir().unwrap();
    let result = Coastfile::parse(toml, dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("command"));
    assert!(err.contains("empty"));
}

#[test]
fn test_services_invalid_restart_rejected() {
    let toml = r#"
[coast]
name = "bad"

[services.web]
command = "npm start"
restart = "turbo"
"#;
    let dir = tempfile::tempdir().unwrap();
    let result = Coastfile::parse(toml, dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("restart"));
    assert!(err.contains("turbo"));
}

#[test]
fn test_services_port_zero_rejected() {
    let toml = r#"
[coast]
name = "bad"

[services.web]
command = "npm start"
port = 0
"#;
    let dir = tempfile::tempdir().unwrap();
    let result = Coastfile::parse(toml, dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("port"));
    assert!(err.contains("0"));
}

#[test]
fn test_extends_merge_services() {
    let dir = tempfile::tempdir().unwrap();
    let base = r#"
[coast]
name = "base-svc"

[services.web]
command = "npm run dev"
port = 3000
"#;
    let child = r#"
[coast]
extends = "Coastfile.base"

[services.worker]
command = "npm run worker"
restart = "always"
"#;
    std::fs::write(dir.path().join("Coastfile.base"), base).unwrap();
    std::fs::write(dir.path().join("Coastfile"), child).unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.services.len(), 2);
    assert!(cf.services.iter().any(|s| s.name == "web"));
    assert!(cf.services.iter().any(|s| s.name == "worker"));
}

#[test]
fn test_extends_override_service() {
    let dir = tempfile::tempdir().unwrap();
    let base = r#"
[coast]
name = "base-svc"

[services.web]
command = "npm run dev"
port = 3000
"#;
    let child = r#"
[coast]
extends = "Coastfile.base"

[services.web]
command = "npm run start"
port = 8080
"#;
    std::fs::write(dir.path().join("Coastfile.base"), base).unwrap();
    std::fs::write(dir.path().join("Coastfile"), child).unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.services.len(), 1);
    let web = &cf.services[0];
    assert_eq!(web.command, "npm run start");
    assert_eq!(web.port, Some(8080));
}

#[test]
fn test_unset_removes_services() {
    let dir = tempfile::tempdir().unwrap();
    let base = r#"
[coast]
name = "base-svc"

[services.web]
command = "npm run dev"

[services.worker]
command = "npm run worker"
"#;
    let child = r#"
[coast]
extends = "Coastfile.base"

[unset]
services = ["worker"]
"#;
    std::fs::write(dir.path().join("Coastfile.base"), base).unwrap();
    std::fs::write(dir.path().join("Coastfile"), child).unwrap();

    let cf = Coastfile::from_file(&dir.path().join("Coastfile")).unwrap();
    assert_eq!(cf.services.len(), 1);
    assert_eq!(cf.services[0].name, "web");
}

#[test]
fn test_standalone_toml_roundtrip_with_services() {
    let toml = r#"
[coast]
name = "svc-roundtrip"

[services.web]
command = "npm run dev"
port = 3000
restart = "on-failure"

[services.worker]
command = "npm run worker"
restart = "always"

[ports]
web = 3000
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    let standalone = cf.to_standalone_toml();

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert_eq!(reparsed.services.len(), 2);

    let web = reparsed.services.iter().find(|s| s.name == "web").unwrap();
    assert_eq!(web.command, "npm run dev");
    assert_eq!(web.port, Some(3000));
    assert_eq!(web.restart, crate::types::RestartPolicy::OnFailure);

    let worker = reparsed
        .services
        .iter()
        .find(|s| s.name == "worker")
        .unwrap();
    assert_eq!(worker.command, "npm run worker");
    assert_eq!(worker.restart, crate::types::RestartPolicy::Always);
}

#[test]
fn test_services_no_compose_field_is_valid() {
    let toml = r#"
[coast]
name = "bare-only"

[services.api]
command = "go run ."
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert!(cf.compose.is_none());
    assert_eq!(cf.services.len(), 1);
    assert_eq!(cf.services[0].restart, crate::types::RestartPolicy::No);
    assert!(cf.services[0].install.is_empty());
}

#[test]
fn test_services_install_single_string() {
    let toml = r#"
[coast]
name = "install-test"

[services.web]
command = "npm run dev"
install = "npm install"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert_eq!(cf.services[0].install, vec!["npm install"]);
}

#[test]
fn test_services_install_array() {
    let toml = r#"
[coast]
name = "install-test"

[services.web]
command = "npm run dev"
install = ["apk add imagemagick", "npm install"]
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    assert_eq!(
        cf.services[0].install,
        vec!["apk add imagemagick", "npm install"]
    );
}

#[test]
fn test_services_install_roundtrip() {
    let toml = r#"
[coast]
name = "install-rt"

[services.web]
command = "npm run dev"
install = ["apk add imagemagick", "npm install"]

[services.worker]
command = "npm run worker"
install = "pip install -r requirements.txt"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml, dir.path()).unwrap();
    let standalone = cf.to_standalone_toml();
    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();

    let web = reparsed.services.iter().find(|s| s.name == "web").unwrap();
    assert_eq!(web.install, vec!["apk add imagemagick", "npm install"]);

    let worker = reparsed
        .services
        .iter()
        .find(|s| s.name == "worker")
        .unwrap();
    assert_eq!(worker.install, vec!["pip install -r requirements.txt"]);
}

#[test]
fn test_healthcheck_parsing_and_roundtrip() {
    let toml_input = r#"
[coast]
name = "test-app"
compose = "./docker-compose.yml"

[ports]
web = 3000
api = 8080

[healthcheck]
web = "/"
api = "/healthz"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml_input, dir.path()).unwrap();

    assert_eq!(cf.healthcheck.len(), 2);
    assert_eq!(cf.healthcheck.get("web").unwrap(), "/");
    assert_eq!(cf.healthcheck.get("api").unwrap(), "/healthz");

    let standalone = cf.to_standalone_toml();
    assert!(
        standalone.contains("[healthcheck]"),
        "standalone toml must contain [healthcheck] section"
    );
    assert!(standalone.contains("web = \"/\""), "must contain web path");
    assert!(
        standalone.contains("api = \"/healthz\""),
        "must contain api path"
    );

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    assert_eq!(reparsed.healthcheck, cf.healthcheck);
}

#[test]
fn test_healthcheck_empty_by_default() {
    let toml_input = r#"
[coast]
name = "test-app"
compose = "./docker-compose.yml"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml_input, dir.path()).unwrap();
    assert!(cf.healthcheck.is_empty());
}

#[test]
fn test_bare_service_cache_field_parsing() {
    let toml_input = r#"
[coast]
name = "test-app"

[services.vite-web]
command = "npm run dev"
port = 3040
install = "cd /workspace && npm install"
cache = ["node_modules"]

[services.worker]
command = "npm run worker"
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml_input, dir.path()).unwrap();

    let vite = cf.services.iter().find(|s| s.name == "vite-web").unwrap();
    assert_eq!(vite.cache, vec!["node_modules"]);

    let worker = cf.services.iter().find(|s| s.name == "worker").unwrap();
    assert!(worker.cache.is_empty(), "cache should default to empty");
}

#[test]
fn test_bare_service_cache_roundtrip() {
    let toml_input = r#"
[coast]
name = "test-app"

[services.vite-web]
command = "npm run dev"
port = 3040
cache = ["node_modules", ".next"]
"#;
    let dir = tempfile::tempdir().unwrap();
    let cf = Coastfile::parse(toml_input, dir.path()).unwrap();

    let standalone = cf.to_standalone_toml();
    assert!(standalone.contains("node_modules"), "must serialize cache");

    let reparsed = Coastfile::parse(&standalone, dir.path()).unwrap();
    let vite = reparsed
        .services
        .iter()
        .find(|s| s.name == "vite-web")
        .unwrap();
    assert_eq!(vite.cache, vec!["node_modules", ".next"]);
}
