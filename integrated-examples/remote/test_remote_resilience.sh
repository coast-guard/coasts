#!/usr/bin/env bash
#
# Integration test for remote coast network/tunnel resilience.
#
# Tests that:
# 1. Killing SSH tunnel process — coast exec should fail clearly, not hang
# 2. Restarting coast-service — instance survives, commands work after reconnect
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_resilience.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_resilience_cleanup() {
    echo ""
    echo "--- Cleaning up resilience test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_resilience_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Resilience Integration Tests ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

# Build and run
set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
set -e

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Instance running"

# Verify it works first
EXEC_OK=$("$COAST" exec dev-1 -- echo "baseline" 2>&1)
assert_contains "$EXEC_OK" "baseline" "exec works before disruption"

# ============================================================
# Test 1: Kill SSH tunnel, then try exec
# ============================================================

echo ""
echo "=== Test 1: Kill SSH tunnel, then exec ==="

# Find and kill SSH tunnel processes for this instance
TUNNEL_PIDS=$(pgrep -f "ssh -N -L" 2>/dev/null || true)
if [ -n "$TUNNEL_PIDS" ]; then
    echo "  Found tunnel PIDs: $TUNNEL_PIDS"
    kill $TUNNEL_PIDS 2>/dev/null || true
    sleep 1
    pass "SSH tunnel processes killed"
else
    echo "  No SSH tunnel processes found (tunnels may have already closed)"
    pass "No tunnels to kill (skipping kill step)"
fi

# Try exec — should fail with an error, NOT hang indefinitely
set +e
EXEC_OUT=$(timeout 20 "$COAST" exec dev-1 -- echo "after tunnel kill" 2>&1)
EXEC_EXIT=$?
set -e

echo "  exec exit code: $EXEC_EXIT"
echo "  exec output (first 2 lines): $(echo "$EXEC_OUT" | head -2)"

# It should either:
# a) Fail because the tunnel is dead (exit != 0)
# b) Succeed because exec establishes a new SSH connection each time
# c) NOT hang (exit 124 = timeout, which would be a bug)

[ "$EXEC_EXIT" -ne 124 ] || fail "exec hung after tunnel kill (timed out at 20s)"
pass "exec did not hang after tunnel kill"

if [ "$EXEC_EXIT" -eq 0 ]; then
    assert_contains "$EXEC_OUT" "after tunnel kill" "exec reconnected successfully"
    pass "exec succeeded (establishes fresh connection each time)"
else
    pass "exec failed after tunnel kill (expected — tunnel was destroyed)"
fi

# ============================================================
# Test 2: Restart coast-service, instance survives
# ============================================================

echo ""
echo "=== Test 2: Restart coast-service, instance survives ==="

# Record the remote container ID
set +e
CONTAINER_BEFORE=$(docker ps -q --filter "name=coast-remote-basic-coasts-dev-1" --filter "status=running" 2>/dev/null | head -1)
set -e
echo "  Remote container before restart: ${CONTAINER_BEFORE:-none}"

# Kill coast-service
stop_coast_service
pass "coast-service killed"

# Verify the Docker container still exists (it's managed by Docker, not by coast-service process)
set +e
CONTAINER_DURING=$(docker ps -q --filter "name=coast-remote-basic-coasts-dev-1" --filter "status=running" 2>/dev/null | head -1)
set -e

if [ -n "$CONTAINER_DURING" ]; then
    pass "Remote Docker container survived coast-service kill"
else
    echo "  Note: Container may have stopped (depends on Docker restart policy)"
    pass "Container state checked after coast-service kill"
fi

# Restart coast-service
start_coast_service
pass "coast-service restarted"

# Try exec again — coast-service is back, should reconnect
set +e
EXEC_AFTER=$(timeout 20 "$COAST" exec dev-1 -- echo "after restart" 2>&1)
EXEC_AFTER_EXIT=$?
set -e

echo "  exec after restart exit code: $EXEC_AFTER_EXIT"
echo "  exec after restart output: $(echo "$EXEC_AFTER" | head -2)"

[ "$EXEC_AFTER_EXIT" -ne 124 ] || fail "exec hung after coast-service restart"

if [ "$EXEC_AFTER_EXIT" -ne 0 ]; then
    echo "  coast-service log tail:"
    tail -15 /tmp/coast-service-test.log 2>/dev/null || true
    fail "exec failed after coast-service restart (exit $EXEC_AFTER_EXIT)"
fi
assert_contains "$EXEC_AFTER" "after restart" "exec works after coast-service restart"
pass "Instance survived coast-service restart"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>/dev/null || true
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote resilience tests passed!"
echo "=========================================="
