#!/usr/bin/env bash
#
# Integration test: production deployment topology.
#
# Verifies that coast-service running inside a Docker container (with the
# host Docker socket mounted) correctly routes shared services through the
# extra Docker network hop. This catches bugs in resolve_docker_gateway_ip
# that only manifest in the production topology.
#
# Topology:
#   DinDinD outer daemon (entrypoint.sh)
#     └── coast-service container (-v /var/run/docker.sock, -v /data:/data)
#           └── DinD coast container (created on host Docker)
#                 └── compose services (connect via extra_hosts)
#
# The critical path:
#   inner compose service -> host.docker.internal (inner DinD gateway)
#   -> DinD container gateway -> host Docker bridge (172.17.0.1/bip)
#   -> reverse SSH tunnel -> local shared services

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up production topology test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true

    docker stop coast-service-container 2>/dev/null || true
    docker rm coast-service-container 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Production Topology Integration Test ==="
echo ""

preflight_checks

clean_slate

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Building coast-service Docker image ---"
docker build -t coast-service-test -f /workspace/Dockerfile.coast-service /workspace 2>&1 | tail -3
pass "coast-service Docker image built"

echo "--- Starting coast-service in container ---"
mkdir -p /data && chmod 777 /data
docker run -d \
    --name coast-service-container \
    --privileged \
    -p 31420:31420 \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v /data:/data \
    coast-service-test

sleep 3

if curl -sf http://localhost:31420/health >/dev/null 2>&1; then
    pass "coast-service container started"
else
    echo "coast-service container logs:"
    docker logs coast-service-container 2>&1 | tail -20
    fail "coast-service container failed to start"
fi

INFO_OUT=$(curl -sf http://localhost:31420/info 2>&1)
assert_contains "$INFO_OUT" '"version"' "coast-service reports version"
assert_contains "$INFO_OUT" '"service_home"' "coast-service reports service_home"
pass "coast-service /info endpoint working"

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-shared-services"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Register remote
# ============================================================

echo ""
echo "=== Test 1: Register remote ==="

ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "remote registered"

TEST_OUT=$("$COAST" remote test test-remote 2>&1)
assert_contains "$TEST_OUT" "reachable" "remote reachable"
pass "Remote registered and reachable"

# ============================================================
# Test 2: Build + Run with shared services
# ============================================================

echo ""
echo "=== Test 2: Build + Run with shared services ==="

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
CLEANUP_INSTANCES+=("dev-1")
assert_contains "$RUN_OUT" "Created coast instance" "remote run succeeds"
pass "Remote instance with shared services created"

# ============================================================
# Test 3: Verify shared service connectivity
# ============================================================

echo ""
echo "=== Test 3: Shared service connectivity ==="

sleep 5

DIND_NAME=$(docker ps --format '{{.Names}}' | grep "coast-remote-shared.*coasts.*dev-1" | head -1)
if [ -z "$DIND_NAME" ]; then
    echo "Available containers:"
    docker ps --format '{{.Names}}'
    fail "DinD container not found"
fi

echo "  DinD container: $DIND_NAME"

HOSTS_OUT=$(docker exec "$DIND_NAME" sh -c "cat /etc/hosts" 2>&1)
echo "  DinD /etc/hosts (relevant entries):"
echo "$HOSTS_OUT" | grep -E "postgres|host.docker" || true

PG_CHECK=$(docker exec "$DIND_NAME" sh -c \
    "echo quit | timeout 5 nc postgres 5432 2>&1 && echo REACHABLE || echo UNREACHABLE" 2>&1)
echo "  postgres connectivity: $PG_CHECK"

if echo "$PG_CHECK" | grep -q "REACHABLE"; then
    pass "postgres reachable from DinD via reverse tunnel"
else
    echo "  Reverse tunnel check on host:"
    ss -tlnp | grep 5432 || echo "  (no listener on 5432)"
    fail "postgres NOT reachable from DinD -- resolve_docker_gateway_ip may be wrong"
fi

# ============================================================
# Test 4: Verify gateway IP is not host-gateway literal
# ============================================================

echo ""
echo "=== Test 4: Gateway IP resolution ==="

COMPOSE_OVERRIDE=$(docker exec "$DIND_NAME" cat /coast-artifact/compose.coast-shared.yml 2>&1)
if echo "$COMPOSE_OVERRIDE" | grep -q "host-gateway"; then
    fail "compose override contains literal 'host-gateway' -- should be a resolved IP"
else
    GATEWAY_IP=$(echo "$COMPOSE_OVERRIDE" | grep "host.docker.internal:" | head -1 | sed 's/.*internal://' | tr -d ' ')
    pass "gateway IP resolved to $GATEWAY_IP (not host-gateway literal)"
fi

# ============================================================
# Test 5: Clean up
# ============================================================

echo ""
echo "=== Test 5: Cleanup ==="

RM_OUT=$("$COAST" rm dev-1 2>&1)
assert_contains "$RM_OUT" "Removed" "instance removed"
CLEANUP_INSTANCES=()
pass "Instance removed"

echo ""
echo "=========================================="
echo "  All production topology tests passed!"
echo "=========================================="
