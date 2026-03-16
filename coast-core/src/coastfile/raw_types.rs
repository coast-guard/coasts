/// Raw TOML deserialization structs for the Coastfile schema.
///
/// These map 1:1 to the TOML structure and are converted to the public
/// `Coastfile` type by the validation and parsing logic in the parent module.
use std::collections::HashMap;

use serde::de;
use serde::Deserialize;

/// Raw TOML structure for deserialization.
#[derive(Debug, Deserialize)]
pub(super) struct RawCoastfile {
    pub coast: RawCoastSection,
    #[serde(default)]
    pub ports: HashMap<String, u16>,
    #[serde(default)]
    pub secrets: HashMap<String, RawSecretConfig>,
    #[serde(default)]
    pub inject: Option<RawInjectConfig>,
    #[serde(default)]
    pub volumes: HashMap<String, RawVolumeConfig>,
    #[serde(default)]
    pub shared_services: HashMap<String, RawSharedServiceConfig>,
    #[serde(default)]
    pub assign: Option<RawAssignConfig>,
    #[serde(default)]
    pub egress: HashMap<String, u16>,
    #[serde(default)]
    pub omit: Option<RawOmitConfig>,
    #[serde(default)]
    pub mcp: HashMap<String, RawMcpConfig>,
    #[serde(default)]
    pub mcp_clients: HashMap<String, RawMcpClientConfig>,
    #[serde(default)]
    pub services: HashMap<String, RawBareServiceConfig>,
    #[serde(default)]
    pub healthcheck: HashMap<String, String>,
    #[serde(default)]
    pub agent_shell: Option<RawAgentShellConfig>,
    #[serde(default)]
    pub unset: Option<RawUnsetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawAgentShellConfig {
    pub command: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct RawUnsetConfig {
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub shared_services: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<String>,
    #[serde(default)]
    pub mcp_clients: Vec<String>,
    #[serde(default)]
    pub egress: Vec<String>,
    #[serde(default)]
    pub services: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawAssignConfig {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub services: HashMap<String, String>,
    #[serde(default)]
    pub rebuild_triggers: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawOmitConfig {
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawCoastSection {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub includes: Option<Vec<String>>,
    #[serde(default)]
    pub compose: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub setup: Option<RawSetupConfig>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_or_vec")]
    pub worktree_dir: Option<Vec<String>>,
    #[serde(default)]
    pub default_worktree_dir: Option<String>,
    #[serde(default)]
    pub autostart: Option<bool>,
    #[serde(default)]
    pub primary_port: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawSetupConfig {
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub run: Vec<String>,
    #[serde(default)]
    pub files: Vec<RawSetupFileConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RawSetupFileConfig {
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawSecretConfig {
    pub extractor: String,
    pub inject: String,
    #[serde(default)]
    pub ttl: Option<String>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawInjectConfig {
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawVolumeConfig {
    pub strategy: String,
    pub service: String,
    pub mount: String,
    #[serde(default)]
    pub snapshot_source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawSharedServiceConfig {
    pub image: String,
    #[serde(default)]
    pub ports: Vec<RawSharedServicePort>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub auto_create_db: bool,
    #[serde(default)]
    pub inject: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum RawSharedServicePort {
    Single(u16),
    Mapping(String),
}

#[derive(Debug, Deserialize)]
pub(super) struct RawMcpConfig {
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub install: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawBareServiceConfig {
    pub command: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub restart: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub install: Vec<String>,
    #[serde(default)]
    pub cache: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawMcpClientConfig {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
}

/// Deserialize an optional field that can be either a single string or an array of strings.
///
/// Returns `None` when the field is absent (via `#[serde(default)]`).
/// When present, wraps the result of [`deserialize_string_or_vec`] in `Some`.
pub(super) fn deserialize_optional_string_or_vec<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: de::Deserializer<'de>,
{
    deserialize_string_or_vec(deserializer).map(Some)
}

/// Deserialize a field that can be either a single string or an array of strings.
///
/// Allows Coastfile authors to write `install = "single command"` or
/// `install = ["cmd1", "cmd2"]` interchangeably.
pub(super) fn deserialize_string_or_vec<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct StringOrVec;

    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or array of strings")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Vec<String>, E> {
            Ok(vec![v.to_string()])
        }

        fn visit_seq<A: de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<String>, A::Error> {
            let mut vec = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                vec.push(s);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVec)
}
