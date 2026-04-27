//! Pure port-allocation primitives.
//!
//! Lifted from `coast-daemon/src/port_manager.rs` so that `coast-ssg` can
//! reuse the same ephemeral-port-range scan without depending on
//! `coast-daemon` (which would be a dependency cycle).
//!
//! `coast-daemon/src/port_manager.rs` retains the higher-level socat
//! orchestration, checkout logic, and DB state — only the raw
//! bind-probing helpers live here. The daemon's public functions are
//! thin delegators to this module (see §17.11 deviation entry in
//! `coast-ssg/DESIGN.md`).
use std::collections::HashSet;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use crate::error::{CoastError, Result};

/// Start of the ephemeral port range (IANA-registered).
pub const PORT_RANGE_START: u16 = 49152;

/// End of the ephemeral port range.
pub const PORT_RANGE_END: u16 = 65535;

/// Maximum number of allocation attempts before giving up.
pub const MAX_ALLOCATION_ATTEMPTS: u32 = 1000;

/// Result of probing a single port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortBindStatus {
    /// Port is free and can be bound.
    Available,
    /// Another process already holds the port.
    InUse,
    /// Binding requires elevated privileges.
    PermissionDenied,
    /// Binding failed for an unexpected reason (details as a string).
    UnexpectedError(String),
}

fn timestamp_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn can_connect_to_port(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok()
}

/// Probe a port to determine whether it is available.
///
/// Distinguishes `InUse` from `PermissionDenied` by falling back to a
/// connect probe on the loopback address.
pub fn inspect_port_binding(port: u16) -> PortBindStatus {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(listener) => {
            drop(listener);
            PortBindStatus::Available
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => PortBindStatus::InUse,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            if can_connect_to_port(port) {
                PortBindStatus::InUse
            } else {
                PortBindStatus::PermissionDenied
            }
        }
        Err(error) => PortBindStatus::UnexpectedError(error.to_string()),
    }
}

/// Check whether a port is available by attempting to bind a TCP listener on it.
pub fn is_port_available(port: u16) -> bool {
    matches!(inspect_port_binding(port), PortBindStatus::Available)
}

/// Allocate a dynamic port by finding an unused port in the ephemeral range.
pub fn allocate_dynamic_port() -> Result<u16> {
    allocate_dynamic_port_excluding(&HashSet::new())
}

/// Allocate a dynamic port while excluding a known set of unusable ports.
///
/// Used by provisioning retries so we don't immediately hand back a
/// host port that Docker has already rejected.
pub fn allocate_dynamic_port_excluding(excluded_ports: &HashSet<u16>) -> Result<u16> {
    let range_size = u32::from(PORT_RANGE_END - PORT_RANGE_START + 1);
    let start_offset = (std::process::id() ^ (timestamp_nanos() as u32)) % range_size;

    let mut inspected_candidates = 0u32;
    for i in 0..range_size {
        let offset = (start_offset + i) % range_size;
        let port = PORT_RANGE_START + offset as u16;

        if excluded_ports.contains(&port) {
            continue;
        }

        inspected_candidates += 1;
        if inspected_candidates > MAX_ALLOCATION_ATTEMPTS {
            break;
        }

        if is_port_available(port) {
            return Ok(port);
        }
    }

    Err(CoastError::port(format!(
        "Could not find an available port after {MAX_ALLOCATION_ATTEMPTS} attempts \
         in range {PORT_RANGE_START}-{PORT_RANGE_END}. Too many ports may be in use. \
         Try stopping some coast instances with `coast stop <name>` to free ports."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_range_constants() {
        assert_eq!(PORT_RANGE_START, 49152);
        assert_eq!(PORT_RANGE_END, 65535);
        assert!(MAX_ALLOCATION_ATTEMPTS > 0);
    }

    #[test]
    fn test_allocate_dynamic_port_returns_port_in_range() {
        let port = allocate_dynamic_port().expect("should allocate a port");
        assert!(port >= PORT_RANGE_START);
        assert!(port <= PORT_RANGE_END);
    }

    #[test]
    fn test_allocate_dynamic_port_excluding_respects_exclusions() {
        let first = allocate_dynamic_port().expect("first");
        let mut excluded = HashSet::new();
        excluded.insert(first);
        let second = allocate_dynamic_port_excluding(&excluded).expect("second");
        assert_ne!(first, second);
        assert!((PORT_RANGE_START..=PORT_RANGE_END).contains(&second));
    }

    #[test]
    fn test_inspect_port_binding_available_for_ephemeral() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let status = inspect_port_binding(port);
        assert!(matches!(
            status,
            PortBindStatus::Available | PortBindStatus::InUse
        ));
    }

    #[test]
    fn test_is_port_available_returns_false_when_held() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral");
        let port = listener.local_addr().expect("addr").port();
        assert!(
            !is_port_available(port),
            "port should not be available while held"
        );
        drop(listener);
    }
}
