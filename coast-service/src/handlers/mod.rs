/// Handler modules for the coast-service remote control plane.
///
/// Each handler implements a remote operation that would normally be
/// performed locally by coast-daemon. The coast-service receives these
/// requests over HTTP from the local daemon via an SSH tunnel.
pub mod assign;
pub mod build;
pub mod container_stats;
pub mod exec;
pub mod logs;
pub mod mcp;
pub mod prune;
pub mod ps;
pub mod restart_services;
pub mod rm;
pub mod run;
pub mod secret;
pub mod service_control;
pub mod start;
pub mod stop;
