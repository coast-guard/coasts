//! Remote VM management and SSH tunnel infrastructure.
//!
//! This module provides:
//! - SSH tunnel lifecycle management
//! - Remote coastd setup/installation
//! - Connection health monitoring
//! - Mutagen file synchronization
//! - Remote daemon client for request forwarding

pub mod client;
pub mod mutagen;
pub mod setup;
pub mod tunnel;

#[allow(unused_imports)]
pub use client::RemoteDaemonClient;
pub use client::RemoteRoute;
pub use mutagen::MutagenManager;
pub use setup::RemoteSetup;
pub use tunnel::TunnelManager;
