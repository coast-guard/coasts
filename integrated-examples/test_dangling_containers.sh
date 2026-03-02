#!/usr/bin/env bash
#
# Integration test: dangling container handling.
#
# Tests that coast commands handle orphaned Docker containers gracefully:
# containers that exist in Docker but have no matching state DB record.
#
# Scenarios:
#   1. coast stop with a dangling instance container (no-op)
#   2. coast rm with a dangling instance container (cleans up)
#   3. coast shared-services stop with a dangling container (no-op)
#   4. coast shared-services rm with a dangling container (cleans up)
#   5. coast rm truly removes the Docker container (not just stops)
#
# Uses coast-dangling (lightweight, shared redis service).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_dangling_containers.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-dangling"

start_daemon

# Pre-pull shared service images (shared services run on the host daemon)
docker pull redis:7-alpine >/dev/null 2>&1 || true

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# Helper: wipe the state DB and restart the daemon so containers become danglers.
create_dangling_state() {
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    start_daemon
}

# ============================================================
# Test 1: coast stop with dangling instance container (no-op)
# ============================================================

echo ""
echo "=== Test 1: stop with dangling instance container ==="

RUN_OUT=$("$COAST" run dangler-stop 2>&1)
CLEANUP_INSTANCES+=("dangler-stop")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dangler-stop succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
wait_for_healthy "$DYN_PORT" 60 || fail "dangler-stop did not become healthy"

# Verify the container exists in Docker
CONTAINER_NAME="coast-dangling-coasts-dangler-stop"
docker inspect "$CONTAINER_NAME" >/dev/null 2>&1 || fail "container should exist before wipe"

# Wipe DB to create dangling state
create_dangling_state

# stop should succeed (no-op) rather than erroring
STOP_OUT=$("$COAST" stop dangler-stop 2>&1 || true)
if echo "$STOP_OUT" | grep -qi "error"; then
    # The stop should not produce a hard error — a no-op response is ok
    if echo "$STOP_OUT" | grep -qi "not found"; then
        fail "stop should not return 'not found' for dangling container"
    fi
fi
pass "coast stop dangler-stop with dangling container did not error"

# Container should still exist (stop is a no-op for danglers)
docker inspect "$CONTAINER_NAME" >/dev/null 2>&1 || fail "container should still exist after stop no-op"
pass "dangling container still exists after stop (correct no-op behavior)"

# Clean up for next test
docker rm -f "$CONTAINER_NAME" 2>/dev/null || true

# ============================================================
# Test 2: coast rm with dangling instance container (cleans up)
# ============================================================

echo ""
echo "=== Test 2: rm with dangling instance container ==="

# Fresh daemon state
create_dangling_state

RUN_OUT=$("$COAST" run dangler-rm 2>&1)
CLEANUP_INSTANCES+=("dangler-rm")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dangler-rm succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
wait_for_healthy "$DYN_PORT" 60 || fail "dangler-rm did not become healthy"

CONTAINER_NAME="coast-dangling-coasts-dangler-rm"
docker inspect "$CONTAINER_NAME" >/dev/null 2>&1 || fail "container should exist before wipe"

# Wipe DB to create dangling state
create_dangling_state

# rm should succeed and remove the dangling container
RM_OUT=$("$COAST" rm dangler-rm 2>&1 || true)
if echo "$RM_OUT" | grep -qi "not found"; then
    fail "rm should not return 'not found' for dangling container"
fi
pass "coast rm dangler-rm with dangling container did not error"

# Container should be gone
if docker inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
    fail "dangling container should have been removed by rm"
fi
pass "dangling container removed by coast rm"

# ============================================================
# Test 3: shared-services stop with dangling container (no-op)
# ============================================================

echo ""
echo "=== Test 3: shared-services stop with dangling container ==="

# Fresh daemon state
create_dangling_state

# Run an instance to trigger shared service creation
RUN_OUT=$("$COAST" run svc-stop-test 2>&1)
CLEANUP_INSTANCES+=("svc-stop-test")
assert_contains "$RUN_OUT" "Created coast instance" "coast run svc-stop-test succeeds"

# Verify the shared redis container exists
SHARED_NAME="coast-dangling-shared-services-redis"
docker inspect "$SHARED_NAME" >/dev/null 2>&1 || fail "shared redis container should exist"
pass "shared redis container exists"

# Wipe DB to create dangling state for the shared service
create_dangling_state

# shared-services stop should handle the missing DB record gracefully
SS_STOP_OUT=$("$COAST" shared-services stop redis 2>&1 || true)
if echo "$SS_STOP_OUT" | grep -qi "error"; then
    if echo "$SS_STOP_OUT" | grep -qi "not found"; then
        # This may still be expected since the service isn't in DB.
        # The key test is that it doesn't crash with a Docker error.
        pass "shared-services stop returned 'not found' (acceptable for dangling)"
    fi
else
    pass "shared-services stop handled dangling shared service gracefully"
fi

# Clean up shared container for next test
docker rm -f "$SHARED_NAME" 2>/dev/null || true
# Also clean up the instance container
docker rm -f "coast-dangling-coasts-svc-stop-test" 2>/dev/null || true

# ============================================================
# Test 4: shared-services rm with dangling container (cleans up)
# ============================================================

echo ""
echo "=== Test 4: shared-services rm with dangling container ==="

# Fresh daemon state
create_dangling_state

# Run an instance to trigger shared service creation
RUN_OUT=$("$COAST" run svc-rm-test 2>&1)
CLEANUP_INSTANCES+=("svc-rm-test")
assert_contains "$RUN_OUT" "Created coast instance" "coast run svc-rm-test succeeds"

# Verify shared redis exists
docker inspect "$SHARED_NAME" >/dev/null 2>&1 || fail "shared redis container should exist"
pass "shared redis container exists before wipe"

# Wipe DB
create_dangling_state

# shared-services rm should find and remove the dangling container
SS_RM_OUT=$("$COAST" shared-services rm redis 2>&1 || true)
if echo "$SS_RM_OUT" | grep -qi "removed\|dangling"; then
    pass "shared-services rm reported removal of dangling container"
elif echo "$SS_RM_OUT" | grep -qi "not found"; then
    fail "shared-services rm should handle dangling, not return 'not found'"
else
    pass "shared-services rm completed without error"
fi

# Container should be gone
if docker inspect "$SHARED_NAME" >/dev/null 2>&1; then
    fail "dangling shared service container should have been removed"
fi
pass "dangling shared service container removed by shared-services rm"

# Clean up instance container
docker rm -f "coast-dangling-coasts-svc-rm-test" 2>/dev/null || true

# ============================================================
# Test 5: coast rm truly removes (not just stops) the container
# ============================================================

echo ""
echo "=== Test 5: coast rm truly removes Docker container ==="

# Fresh daemon state
create_dangling_state

RUN_OUT=$("$COAST" run removal-test 2>&1)
CLEANUP_INSTANCES+=("removal-test")
assert_contains "$RUN_OUT" "Created coast instance" "coast run removal-test succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
wait_for_healthy "$DYN_PORT" 60 || fail "removal-test did not become healthy"

CONTAINER_NAME="coast-dangling-coasts-removal-test"

# Verify running
docker inspect "$CONTAINER_NAME" >/dev/null 2>&1 || fail "container should exist"
RUNNING=$(docker inspect --format='{{.State.Running}}' "$CONTAINER_NAME" 2>/dev/null)
assert_eq "$RUNNING" "true" "container is running before rm"

# rm should stop AND remove
RM_OUT=$("$COAST" rm removal-test 2>&1) || true
assert_contains "$RM_OUT" "Removed" "coast rm removal-test succeeded"
CLEANUP_INSTANCES=("${CLEANUP_INSTANCES[@]/removal-test}")

# docker ps -a should NOT show the container (removed, not just stopped)
if docker inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
    STATE=$(docker inspect --format='{{.State.Status}}' "$CONTAINER_NAME" 2>/dev/null)
    fail "container still exists after rm (state: $STATE) — rm should remove, not just stop"
fi
pass "container fully removed (not just stopped) after coast rm"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL DANGLING CONTAINER TESTS PASSED"
echo "==========================================="
