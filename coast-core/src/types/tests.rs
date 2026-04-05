use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;

use super::*;

#[test]
fn test_instance_status_roundtrip() {
    for status in &[
        InstanceStatus::Enqueued,
        InstanceStatus::Provisioning,
        InstanceStatus::Starting,
        InstanceStatus::Stopping,
        InstanceStatus::Running,
        InstanceStatus::Stopped,
        InstanceStatus::CheckedOut,
        InstanceStatus::Idle,
    ] {
        let s = status.as_db_str();
        let parsed = InstanceStatus::from_db_str(s).unwrap();
        assert_eq!(&parsed, status);
    }
}

#[test]
fn test_instance_status_invalid() {
    assert!(InstanceStatus::from_db_str("invalid").is_none());
    assert!(InstanceStatus::from_db_str("").is_none());
}

#[test]
fn test_instance_status_display() {
    assert_eq!(InstanceStatus::Enqueued.to_string(), "enqueued");
    assert_eq!(InstanceStatus::Provisioning.to_string(), "provisioning");
    assert_eq!(InstanceStatus::Running.to_string(), "running");
    assert_eq!(InstanceStatus::Stopped.to_string(), "stopped");
    assert_eq!(InstanceStatus::CheckedOut.to_string(), "checked_out");
    assert_eq!(InstanceStatus::Idle.to_string(), "idle");
}

#[test]
fn test_instance_status_can_assign() {
    assert!(InstanceStatus::Running.can_assign());
    assert!(InstanceStatus::Idle.can_assign());
    assert!(InstanceStatus::CheckedOut.can_assign());
    assert!(!InstanceStatus::Stopped.can_assign());
    assert!(!InstanceStatus::Provisioning.can_assign());
    assert!(!InstanceStatus::Enqueued.can_assign());
}

#[test]
fn test_enqueued_status_serde_roundtrip() {
    let json = serde_json::to_string(&InstanceStatus::Enqueued).unwrap();
    assert_eq!(json, "\"enqueued\"");
    let deserialized: InstanceStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, InstanceStatus::Enqueued);
}

#[test]
fn test_volume_strategy_from_str() {
    assert_eq!(
        VolumeStrategy::from_str_value("isolated"),
        Some(VolumeStrategy::Isolated)
    );
    assert_eq!(
        VolumeStrategy::from_str_value("shared"),
        Some(VolumeStrategy::Shared)
    );
    assert_eq!(VolumeStrategy::from_str_value("snapshot"), None);
    assert_eq!(VolumeStrategy::from_str_value("invalid"), None);
}

#[test]
fn test_inject_type_parse_env() {
    let inject = InjectType::parse("env:MY_VAR").unwrap();
    assert_eq!(inject, InjectType::Env("MY_VAR".to_string()));
}

#[test]
fn test_inject_type_parse_file() {
    let inject = InjectType::parse("file:/run/secrets/key.json").unwrap();
    assert_eq!(
        inject,
        InjectType::File(PathBuf::from("/run/secrets/key.json"))
    );
}

#[test]
fn test_inject_type_parse_empty_env() {
    let result = InjectType::parse("env:");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be empty"));
}

#[test]
fn test_inject_type_parse_empty_file() {
    let result = InjectType::parse("file:");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("cannot be empty"));
}

#[test]
fn test_inject_type_parse_invalid() {
    let result = InjectType::parse("something:else");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid inject format"));
}

#[test]
fn test_inject_type_roundtrip() {
    let env = InjectType::Env("PGPASSWORD".to_string());
    assert_eq!(InjectType::parse(&env.to_inject_string()).unwrap(), env);

    let file = InjectType::File(PathBuf::from("/run/secrets/gcp.json"));
    assert_eq!(InjectType::parse(&file.to_inject_string()).unwrap(), file);
}

#[test]
fn test_runtime_type_roundtrip() {
    for rt in &[RuntimeType::Dind, RuntimeType::Sysbox, RuntimeType::Podman] {
        let s = rt.as_str();
        let parsed = RuntimeType::from_str_value(s).unwrap();
        assert_eq!(&parsed, rt);
    }
}

#[test]
fn test_runtime_type_invalid() {
    assert!(RuntimeType::from_str_value("docker").is_none());
    assert!(RuntimeType::from_str_value("").is_none());
}

#[test]
fn test_runtime_type_display() {
    assert_eq!(RuntimeType::Dind.to_string(), "dind");
    assert_eq!(RuntimeType::Sysbox.to_string(), "sysbox");
    assert_eq!(RuntimeType::Podman.to_string(), "podman");
}

#[test]
fn test_coast_instance_serialization() {
    let instance = CoastInstance {
        name: "feature-oauth".to_string(),
        status: InstanceStatus::Running,
        project: "my-app".to_string(),
        branch: Some("feature/oauth".to_string()),
        commit_sha: Some("abc123def456".to_string()),
        container_id: Some("abc123".to_string()),
        runtime: RuntimeType::Dind,
        created_at: Utc::now(),
        worktree_name: None,
        build_id: None,
        coastfile_type: None,
        remote_name: None,
    };

    let json = serde_json::to_string(&instance).unwrap();
    let deserialized: CoastInstance = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "feature-oauth");
    assert_eq!(deserialized.status, InstanceStatus::Running);
    assert_eq!(deserialized.runtime, RuntimeType::Dind);
    assert_eq!(deserialized.commit_sha, Some("abc123def456".to_string()));
}

#[test]
fn test_coast_instance_deserialization_without_commit_sha() {
    // Ensure backward compatibility: JSON without commit_sha should deserialize with None
    let json = r#"{
        "name": "test",
        "status": "running",
        "project": "proj",
        "branch": "main",
        "container_id": "abc",
        "runtime": "dind",
        "created_at": "2026-01-01T00:00:00Z"
    }"#;
    let instance: CoastInstance = serde_json::from_str(json).unwrap();
    assert!(instance.commit_sha.is_none());
}

#[test]
fn test_port_mapping_serialization() {
    let mapping = PortMapping {
        logical_name: "web".to_string(),
        canonical_port: 3000,
        dynamic_port: 52340,
        is_primary: false,
    };

    let json = serde_json::to_string(&mapping).unwrap();
    let deserialized: PortMapping = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.logical_name, "web");
    assert_eq!(deserialized.canonical_port, 3000);
    assert_eq!(deserialized.dynamic_port, 52340);
}

#[test]
fn test_volume_config_serialization() {
    let config = VolumeConfig {
        name: "postgres_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "db".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: VolumeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.strategy, VolumeStrategy::Isolated);
    assert!(deserialized.snapshot_source.is_none());
}

#[test]
fn test_volume_config_isolated_with_snapshot_source() {
    let config = VolumeConfig {
        name: "seed_data".to_string(),
        strategy: VolumeStrategy::Isolated,
        service: "db".to_string(),
        mount: PathBuf::from("/var/lib/postgresql/data"),
        snapshot_source: Some("coast_seed_pg_data".to_string()),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: VolumeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.strategy, VolumeStrategy::Isolated);
    assert_eq!(
        deserialized.snapshot_source.as_deref(),
        Some("coast_seed_pg_data")
    );
}

#[test]
fn test_shared_service_config_serialization() {
    let mut env = HashMap::new();
    env.insert("POSTGRES_PASSWORD".to_string(), "dev".to_string());

    let config = SharedServiceConfig {
        name: "postgres".to_string(),
        image: "postgres:16".to_string(),
        ports: vec![SharedServicePort::same(5432)],
        volumes: vec!["coast_shared_pg:/var/lib/postgresql/data".to_string()],
        env,
        auto_create_db: true,
        inject: Some(InjectType::Env("DATABASE_URL".to_string())),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: SharedServiceConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.auto_create_db, true);
    assert_eq!(
        deserialized.inject,
        Some(InjectType::Env("DATABASE_URL".to_string()))
    );
    assert_eq!(deserialized.ports, vec![SharedServicePort::same(5432)]);
}

#[test]
fn test_secret_config_serialization() {
    let mut params = HashMap::new();
    params.insert("item".to_string(), "claude-code-api-key".to_string());

    let config = SecretConfig {
        name: "claude_api_key".to_string(),
        extractor: "macos-keychain".to_string(),
        params,
        inject: InjectType::Env("CLAUDE_API_KEY".to_string()),
        ttl: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: SecretConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.extractor, "macos-keychain");
}

#[test]
fn test_setup_config_default_is_empty() {
    let setup = SetupConfig::default();
    assert!(setup.is_empty());
    assert!(setup.packages.is_empty());
    assert!(setup.run.is_empty());
    assert!(setup.files.is_empty());
}

#[test]
fn test_setup_config_with_packages_not_empty() {
    let setup = SetupConfig {
        packages: vec!["nodejs".to_string(), "npm".to_string()],
        run: vec![],
        files: vec![],
    };
    assert!(!setup.is_empty());
}

#[test]
fn test_setup_config_with_run_not_empty() {
    let setup = SetupConfig {
        packages: vec![],
        run: vec!["npm install -g something".to_string()],
        files: vec![],
    };
    assert!(!setup.is_empty());
}

#[test]
fn test_setup_config_with_files_not_empty() {
    let setup = SetupConfig {
        packages: vec![],
        run: vec![],
        files: vec![SetupFileConfig {
            path: "/root/.tool/config.json".to_string(),
            content: "{\"ok\":true}".to_string(),
            mode: Some("0600".to_string()),
        }],
    };
    assert!(!setup.is_empty());
}

#[test]
fn test_setup_config_serialization() {
    let setup = SetupConfig {
        packages: vec!["git".to_string(), "curl".to_string()],
        run: vec!["echo hello".to_string()],
        files: vec![SetupFileConfig {
            path: "/root/.tool/config.json".to_string(),
            content: "{\"enabled\":true}".to_string(),
            mode: Some("0600".to_string()),
        }],
    };
    let json = serde_json::to_string(&setup).unwrap();
    let deserialized: SetupConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.packages, vec!["git", "curl"]);
    assert_eq!(deserialized.run, vec!["echo hello"]);
    assert_eq!(deserialized.files.len(), 1);
    assert_eq!(deserialized.files[0].path, "/root/.tool/config.json");
    assert_eq!(deserialized.files[0].mode.as_deref(), Some("0600"));
}

#[test]
fn test_host_inject_config() {
    let config = HostInjectConfig {
        env: vec!["NODE_ENV".to_string(), "DEBUG".to_string()],
        files: vec!["~/.ssh/id_ed25519".to_string(), "~/.gitconfig".to_string()],
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: HostInjectConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.env.len(), 2);
    assert_eq!(deserialized.files.len(), 2);
}

// --- AssignAction tests ---

#[test]
fn test_assign_action_from_str_value() {
    assert_eq!(
        AssignAction::from_str_value("none"),
        Some(AssignAction::None)
    );
    assert_eq!(
        AssignAction::from_str_value("restart"),
        Some(AssignAction::Restart)
    );
    assert_eq!(
        AssignAction::from_str_value("rebuild"),
        Some(AssignAction::Rebuild)
    );
    assert_eq!(AssignAction::from_str_value("invalid"), Option::None);
    assert_eq!(AssignAction::from_str_value(""), Option::None);
    assert_eq!(AssignAction::from_str_value("RESTART"), Option::None);
}

#[test]
fn test_assign_action_display() {
    assert_eq!(AssignAction::None.to_string(), "none");
    assert_eq!(AssignAction::Restart.to_string(), "restart");
    assert_eq!(AssignAction::Rebuild.to_string(), "rebuild");
}

#[test]
fn test_assign_action_default() {
    assert_eq!(AssignAction::default(), AssignAction::Restart);
}

#[test]
fn test_assign_action_serialization() {
    let action = AssignAction::Rebuild;
    let json = serde_json::to_string(&action).unwrap();
    assert_eq!(json, "\"rebuild\"");
    let deserialized: AssignAction = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, AssignAction::Rebuild);
}

// --- AssignConfig tests ---

#[test]
fn test_assign_config_default() {
    let config = AssignConfig::default();
    assert_eq!(config.default, AssignAction::Restart);
    assert!(config.services.is_empty());
    assert!(config.rebuild_triggers.is_empty());
}

#[test]
fn test_assign_config_action_for_service_default() {
    let config = AssignConfig::default();
    assert_eq!(config.action_for_service("api"), AssignAction::Restart);
    assert_eq!(config.action_for_service("worker"), AssignAction::Restart);
}

#[test]
fn test_assign_config_action_for_service_override() {
    let mut services = HashMap::new();
    services.insert("api".to_string(), AssignAction::Restart);
    services.insert("worker".to_string(), AssignAction::Rebuild);
    services.insert("postgres".to_string(), AssignAction::None);

    let config = AssignConfig {
        default: AssignAction::None,
        services,
        rebuild_triggers: HashMap::new(),
        exclude_paths: vec![],
    };

    assert_eq!(config.action_for_service("api"), AssignAction::Restart);
    assert_eq!(config.action_for_service("worker"), AssignAction::Rebuild);
    assert_eq!(config.action_for_service("postgres"), AssignAction::None);
    // Unlisted service falls back to default
    assert_eq!(config.action_for_service("redis"), AssignAction::None);
}

#[test]
fn test_assign_config_serialization() {
    let mut services = HashMap::new();
    services.insert("api".to_string(), AssignAction::Restart);

    let mut triggers = HashMap::new();
    triggers.insert(
        "worker".to_string(),
        vec!["Dockerfile".to_string(), "package.json".to_string()],
    );

    let config = AssignConfig {
        default: AssignAction::None,
        services,
        rebuild_triggers: triggers,
        exclude_paths: vec![],
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: AssignConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.default, AssignAction::None);
    assert_eq!(
        deserialized.action_for_service("api"),
        AssignAction::Restart
    );
    assert_eq!(
        deserialized.rebuild_triggers.get("worker").unwrap(),
        &vec!["Dockerfile".to_string(), "package.json".to_string()]
    );
}

// --- OmitConfig tests ---

#[test]
fn test_omit_config_default_is_empty() {
    let omit = OmitConfig::default();
    assert!(omit.is_empty());
    assert!(omit.services.is_empty());
    assert!(omit.volumes.is_empty());
}

#[test]
fn test_omit_config_with_services() {
    let omit = OmitConfig {
        services: vec!["langfuse".to_string(), "redash".to_string()],
        volumes: vec![],
    };
    assert!(!omit.is_empty());
}

#[test]
fn test_omit_config_with_volumes() {
    let omit = OmitConfig {
        services: vec![],
        volumes: vec!["keycloak-db-data".to_string()],
    };
    assert!(!omit.is_empty());
}

#[test]
fn test_omit_config_serialization() {
    let omit = OmitConfig {
        services: vec!["nginx-proxy".to_string(), "keycloak".to_string()],
        volumes: vec!["keycloak-db-data".to_string()],
    };
    let json = serde_json::to_string(&omit).unwrap();
    let deserialized: OmitConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.services, vec!["nginx-proxy", "keycloak"]);
    assert_eq!(deserialized.volumes, vec!["keycloak-db-data"]);
}

// --- McpProxyMode tests ---

#[test]
fn test_mcp_proxy_mode_from_str() {
    assert_eq!(
        McpProxyMode::from_str_value("host"),
        Some(McpProxyMode::Host)
    );
    assert_eq!(McpProxyMode::from_str_value("cloud"), None);
    assert_eq!(McpProxyMode::from_str_value(""), None);
    assert_eq!(McpProxyMode::from_str_value("HOST"), None);
}

#[test]
fn test_mcp_proxy_mode_display() {
    assert_eq!(McpProxyMode::Host.to_string(), "host");
}

#[test]
fn test_mcp_proxy_mode_serialization() {
    let mode = McpProxyMode::Host;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, "\"host\"");
    let deserialized: McpProxyMode = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, McpProxyMode::Host);
}

// --- McpServerConfig tests ---

#[test]
fn test_mcp_server_config_internal_serialization() {
    let mut env = HashMap::new();
    env.insert("API_KEY".to_string(), "secret".to_string());

    let config = McpServerConfig {
        name: "custom-tool".to_string(),
        proxy: None,
        command: Some("node".to_string()),
        args: vec!["dist/index.js".to_string()],
        env,
        install: vec!["npm install".to_string(), "npm run build".to_string()],
        source: Some("./tools/my-mcp".to_string()),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "custom-tool");
    assert!(deserialized.proxy.is_none());
    assert!(!deserialized.is_host_proxied());
    assert_eq!(deserialized.command, Some("node".to_string()));
    assert_eq!(deserialized.args, vec!["dist/index.js"]);
    assert_eq!(deserialized.env.get("API_KEY").unwrap(), "secret");
    assert_eq!(deserialized.install.len(), 2);
    assert_eq!(deserialized.source, Some("./tools/my-mcp".to_string()));
}

#[test]
fn test_mcp_server_config_host_proxied_serialization() {
    let config = McpServerConfig {
        name: "postgres".to_string(),
        proxy: Some(McpProxyMode::Host),
        command: Some("npx".to_string()),
        args: vec!["-y".to_string(), "@mcp/server-postgres".to_string()],
        env: HashMap::new(),
        install: vec![],
        source: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.proxy, Some(McpProxyMode::Host));
    assert!(deserialized.is_host_proxied());
    assert_eq!(deserialized.command, Some("npx".to_string()));
}

#[test]
fn test_mcp_server_config_host_by_name() {
    let config = McpServerConfig {
        name: "host-lookup".to_string(),
        proxy: Some(McpProxyMode::Host),
        command: None,
        args: vec![],
        env: HashMap::new(),
        install: vec![],
        source: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_host_proxied());
    assert!(deserialized.command.is_none());
}

// --- McpClientFormat tests ---

#[test]
fn test_mcp_client_format_from_str() {
    assert_eq!(
        McpClientFormat::from_str_value("claude-code"),
        Some(McpClientFormat::ClaudeCode)
    );
    assert_eq!(
        McpClientFormat::from_str_value("cursor"),
        Some(McpClientFormat::Cursor)
    );
    assert_eq!(McpClientFormat::from_str_value("vscode"), None);
    assert_eq!(McpClientFormat::from_str_value(""), None);
}

#[test]
fn test_mcp_client_format_display() {
    assert_eq!(McpClientFormat::ClaudeCode.to_string(), "claude-code");
    assert_eq!(McpClientFormat::Cursor.to_string(), "cursor");
}

#[test]
fn test_mcp_client_format_default_paths() {
    assert_eq!(
        McpClientFormat::ClaudeCode.default_config_path(),
        "/root/.claude/mcp_servers.json"
    );
    assert_eq!(
        McpClientFormat::Cursor.default_config_path(),
        "/workspace/.cursor/mcp.json"
    );
}

#[test]
fn test_mcp_client_format_serialization() {
    let fmt = McpClientFormat::ClaudeCode;
    let json = serde_json::to_string(&fmt).unwrap();
    assert_eq!(json, "\"claude-code\"");
    let deserialized: McpClientFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, McpClientFormat::ClaudeCode);

    let fmt = McpClientFormat::Cursor;
    let json = serde_json::to_string(&fmt).unwrap();
    assert_eq!(json, "\"cursor\"");
    let deserialized: McpClientFormat = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, McpClientFormat::Cursor);
}

// --- McpClientConnectorConfig tests ---

#[test]
fn test_mcp_client_connector_builtin_serialization() {
    let config = McpClientConnectorConfig {
        name: "claude-code".to_string(),
        format: Some(McpClientFormat::ClaudeCode),
        config_path: None,
        run: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: McpClientConnectorConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "claude-code");
    assert_eq!(deserialized.format, Some(McpClientFormat::ClaudeCode));
    assert!(!deserialized.is_command_based());
    assert_eq!(
        deserialized.resolved_config_path(),
        Some("/root/.claude/mcp_servers.json")
    );
}

#[test]
fn test_mcp_client_connector_builtin_with_path_override() {
    let config = McpClientConnectorConfig {
        name: "claude-code".to_string(),
        format: Some(McpClientFormat::ClaudeCode),
        config_path: Some("/custom/path/mcp.json".to_string()),
        run: None,
    };

    assert_eq!(config.resolved_config_path(), Some("/custom/path/mcp.json"));
}

#[test]
fn test_mcp_client_connector_custom_with_run() {
    let config = McpClientConnectorConfig {
        name: "exotic-tool".to_string(),
        format: None,
        config_path: None,
        run: Some("my-connector-script --output /etc/exotic/mcp.conf".to_string()),
    };

    assert!(config.is_command_based());
    assert!(config.resolved_config_path().is_none());
}

// --- RestartPolicy tests ---

#[test]
fn test_restart_policy_from_str() {
    assert_eq!(RestartPolicy::from_str_value("no"), Some(RestartPolicy::No));
    assert_eq!(
        RestartPolicy::from_str_value("on-failure"),
        Some(RestartPolicy::OnFailure)
    );
    assert_eq!(
        RestartPolicy::from_str_value("on_failure"),
        Some(RestartPolicy::OnFailure)
    );
    assert_eq!(
        RestartPolicy::from_str_value("always"),
        Some(RestartPolicy::Always)
    );
    assert_eq!(RestartPolicy::from_str_value("invalid"), None);
}

#[test]
fn test_restart_policy_default() {
    assert_eq!(RestartPolicy::default(), RestartPolicy::No);
}

#[test]
fn test_restart_policy_display() {
    assert_eq!(RestartPolicy::No.to_string(), "no");
    assert_eq!(RestartPolicy::OnFailure.to_string(), "on-failure");
    assert_eq!(RestartPolicy::Always.to_string(), "always");
}

#[test]
fn test_restart_policy_serialization() {
    let policy = RestartPolicy::OnFailure;
    let json = serde_json::to_string(&policy).unwrap();
    assert_eq!(json, "\"on_failure\"");
    let deserialized: RestartPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, RestartPolicy::OnFailure);
}

// --- BareServiceConfig tests ---

#[test]
fn test_bare_service_config_serialization() {
    let config = BareServiceConfig {
        name: "web".to_string(),
        command: "npm run dev".to_string(),
        port: Some(3000),
        restart: RestartPolicy::OnFailure,
        install: vec!["npm install".to_string()],
        cache: vec!["node_modules".to_string()],
    };
    let json = serde_json::to_string(&config).unwrap();
    let deserialized: BareServiceConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, "web");
    assert_eq!(deserialized.command, "npm run dev");
    assert_eq!(deserialized.port, Some(3000));
    assert_eq!(deserialized.restart, RestartPolicy::OnFailure);
    assert_eq!(deserialized.install, vec!["npm install"]);
    assert_eq!(deserialized.cache, vec!["node_modules"]);
}

#[test]
fn test_bare_service_config_defaults() {
    let config = BareServiceConfig {
        name: "worker".to_string(),
        command: "npm run worker".to_string(),
        port: None,
        restart: RestartPolicy::default(),
        install: vec![],
        cache: vec![],
    };
    assert_eq!(config.port, None);
    assert!(config.install.is_empty());
    assert_eq!(config.restart, RestartPolicy::No);
}

#[test]
fn test_mcp_client_connector_custom_format_at_path() {
    let config = McpClientConnectorConfig {
        name: "my-fork".to_string(),
        format: Some(McpClientFormat::ClaudeCode),
        config_path: Some("/home/user/.my-fork/mcp.json".to_string()),
        run: None,
    };

    assert!(!config.is_command_based());
    assert_eq!(
        config.resolved_config_path(),
        Some("/home/user/.my-fork/mcp.json")
    );
    assert_eq!(config.format, Some(McpClientFormat::ClaudeCode));
}

#[test]
fn test_port_health_status_serialization() {
    let status = PortHealthStatus {
        logical_name: "web".to_string(),
        canonical_port: 3000,
        dynamic_port: 63100,
        is_primary: true,
        healthy: true,
    };
    let json = serde_json::to_string(&status).unwrap();
    let deserialized: PortHealthStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.logical_name, "web");
    assert_eq!(deserialized.canonical_port, 3000);
    assert_eq!(deserialized.dynamic_port, 63100);
    assert!(deserialized.is_primary);
    assert!(deserialized.healthy);
}

#[test]
fn test_port_health_status_unhealthy() {
    let status = PortHealthStatus {
        logical_name: "api".to_string(),
        canonical_port: 8080,
        dynamic_port: 63200,
        is_primary: false,
        healthy: false,
    };
    let json = serde_json::to_string(&status).unwrap();
    let deserialized: PortHealthStatus = serde_json::from_str(&json).unwrap();
    assert!(!deserialized.healthy);
    assert!(!deserialized.is_primary);
}
