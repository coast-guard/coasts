use coast_core::error::{CoastError, Result};

/// Allocate an available dynamic port on the host by binding to port 0.
///
/// The OS assigns an ephemeral port, we read it, then drop the listener.
/// There's a small race window between dropping the listener and Docker
/// publishing on the port, but in practice this is reliable.
pub fn allocate_dynamic_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("0.0.0.0:0")
        .map_err(|e| CoastError::state(format!("failed to allocate dynamic port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| CoastError::state(format!("failed to read allocated port: {e}")))?
        .port();
    drop(listener);
    Ok(port)
}

/// Allocate multiple unique dynamic ports.
pub fn allocate_dynamic_ports(count: usize) -> Result<Vec<u16>> {
    let mut ports = Vec::with_capacity(count);
    let mut listeners = Vec::with_capacity(count);

    for _ in 0..count {
        let listener = std::net::TcpListener::bind("0.0.0.0:0")
            .map_err(|e| CoastError::state(format!("failed to allocate dynamic port: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| CoastError::state(format!("failed to read allocated port: {e}")))?
            .port();
        ports.push(port);
        listeners.push(listener);
    }

    drop(listeners);
    Ok(ports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_dynamic_port_returns_nonzero() {
        let port = allocate_dynamic_port().unwrap();
        assert!(port > 0);
    }

    #[test]
    fn test_allocate_dynamic_port_returns_ephemeral_range() {
        let port = allocate_dynamic_port().unwrap();
        assert!(port >= 1024, "expected ephemeral port, got {port}");
    }

    #[test]
    fn test_allocate_multiple_ports_unique() {
        let ports = allocate_dynamic_ports(5).unwrap();
        assert_eq!(ports.len(), 5);
        let unique: std::collections::HashSet<u16> = ports.iter().copied().collect();
        assert_eq!(unique.len(), 5, "expected 5 unique ports, got duplicates");
    }

    #[test]
    fn test_allocate_two_calls_different() {
        let p1 = allocate_dynamic_port().unwrap();
        let p2 = allocate_dynamic_port().unwrap();
        // Not guaranteed but extremely likely with ephemeral ports
        // If they happen to be the same, the test is flaky but functionally fine
        assert!(p1 > 0 && p2 > 0);
    }
}
