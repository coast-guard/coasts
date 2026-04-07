/// Shared types used across all Coast crates.
///
/// These types represent the core domain model: instances, ports, volumes,
/// secrets, shared services, runtime configuration, and remote execution.
///
/// Submodules:
/// - [`instance`]: CoastInstance, InstanceStatus, PortMapping
/// - [`volume`]: VolumeStrategy, VolumeConfig, SharedServiceConfig, SecretConfig, InjectType
/// - [`service`]: RestartPolicy, BareServiceConfig
/// - [`runtime`]: RuntimeType, SetupConfig, SetupFileConfig, HostInjectConfig
/// - [`assign`]: AssignAction, AssignConfig, OmitConfig
/// - [`mcp`]: McpProxyMode, McpServerConfig, McpClientFormat, McpClientConnectorConfig
/// - [`remote`]: RemoteConfig, SyncStrategy
pub mod assign;
pub mod instance;
pub mod mcp;
pub mod remote;
pub mod runtime;
pub mod service;
pub mod volume;

#[cfg(test)]
mod tests;

pub use assign::*;
pub use instance::*;
pub use mcp::*;
pub use remote::*;
pub use runtime::*;
pub use service::*;
pub use volume::*;
