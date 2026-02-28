#!/usr/bin/env bash
#
# Integration test for the [egress] directive.
#
# Verifies that a coast instance can reach a service running on the host
# machine through the `host.docker.internal` hostname, both from:
#   1. `coast exec` (the outer DinD container)
#   2. An inner compose service (the app container)
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - node installed on the host (runs host-service)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_egress.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

HOST_SERVICE_PID=""

_egress_cleanup() {
    echo ""
    echo "--- Egress cleanup ---"

    # Kill host service if running
    if [ -n "$HOST_SERVICE_PID" ] && kill -0 "$HOST_SERVICE_PID" 2>/dev/null; then
        kill "$HOST_SERVICE_PID" 2>/dev/null || true
        wait "$HOST_SERVICE_PID" 2>/dev/null || true
        echo "  Killed host-service (PID $HOST_SERVICE_PID)"
    fi

    # Standard cleanup handles coast instances, daemon, socat
    _do_cleanup
}

trap '_egress_cleanup' EXIT

# --- Preflight ---

preflight_checks
command -v node >/dev/null || { echo "node not installed on host (required for host-service)"; exit 1; }
pass "node available on host"

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

start_daemon

# ============================================================
# Phase 0: Start host-service on the host machine
# ============================================================

echo ""
echo "=== Phase 0: Start host-service ==="

cd "$HELPERS_DIR/host-service"
node server.js &
HOST_SERVICE_PID=$!
sleep 1

# Verify it's running
if curl -sf "http://localhost:48080/health" | grep -q "host-service"; then
    pass "host-service running on port 48080"
else
    fail "host-service failed to start"
fi

# ============================================================
# Phase 1: Build and run coast-egress
# ============================================================

echo ""
echo "=== Phase 1: Build and run coast-egress ==="

cd "$PROJECTS_DIR/coast-egress"

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "coast-egress" "coast build references project name"
pass "coast build succeeded"

RUN_OUT=$("$COAST" run egress-1 2>&1)
CLEANUP_INSTANCES+=("egress-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run egress-1 succeeds"
pass "Instance egress-1 created"

# ============================================================
# Phase 2: Host service reachable from DinD exec
# ============================================================

echo ""
echo "=== Phase 2: Host reachable from DinD exec ==="

# The outer DinD container should have host.docker.internal via extra_hosts
EXEC_WGET=$("$COAST" exec egress-1 -- wget -qO- "http://host.docker.internal:48080/health" 2>&1 || true)
assert_contains "$EXEC_WGET" "host-service" "DinD exec can reach host-service via host.docker.internal"

# ============================================================
# Phase 3: Host service reachable from inner compose service
# ============================================================

echo ""
echo "=== Phase 3: Host reachable from inner compose service ==="

# Extract dynamic port for the app service
DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
if [ -z "$DYN_PORT" ]; then
    fail "Could not extract dynamic port for app service"
fi
pass "Dynamic port for app: $DYN_PORT"

# Wait for the inner app service to be healthy
if wait_for_healthy "$DYN_PORT" 60; then
    pass "Inner app service is healthy"
else
    fail "Inner app service did not become healthy within 60s"
fi

# The /egress endpoint proxies to host.docker.internal:48080/health
EGRESS_RESP=$(curl -sf "http://localhost:${DYN_PORT}/egress" 2>&1 || true)
assert_contains "$EGRESS_RESP" "host-service" "Inner compose service reaches host-service via egress"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm egress-1 2>&1 || true
CLEANUP_INSTANCES=()

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL EGRESS TESTS PASSED"
echo "==========================================="
