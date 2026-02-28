/// Coast Docker runtime management.
///
/// Provides the `Runtime` trait and implementations for different container
/// runtimes (DinD, Sysbox, Podman), along with container lifecycle management,
/// Docker Compose interaction, network management, and image caching.
pub mod compose;
pub mod compose_build;
pub mod container;
pub mod dind;
pub mod image_cache;
pub mod network;
pub mod podman;
pub mod runtime;
pub mod sysbox;
