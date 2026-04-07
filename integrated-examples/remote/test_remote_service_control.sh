#!/usr/bin/env bash
#
# Integration test for per-service stop/start/restart on remote instances.
#
# Verifies that the daemon correctly forwards service control commands
# to coast-service for remote instances, matching local behavior.
#
# Tests:
#   1. coast ps shows the app service running
#   2. Stop the app service via daemon HTTP API
#   3. coast ps shows the app service stopped/exited
#   4. Start the app service
#   5. coast ps shows the app service running again
#   6. Restart the app service
#   7. coast ps still shows the app service running
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_service_control.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

DAEMON_API="http://localhost:31415"

_svc_cleanup() {
    echo ""
    echo "--- Cleaning up service control test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_svc_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Service Control Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-compose-build"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
"$COAST" build --type remote 2>&1 >/dev/null
set -e

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "$RUN_OUT"
    fail "Run failed (exit $RUN_EXIT)"
fi
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 5

# ============================================================
# Test 1: Baseline — app service is running
# ============================================================

echo ""
echo "=== Test 1: Baseline — app service running ==="

PS_OUT=$("$COAST" ps dev-1 2>&1)
echo "$PS_OUT" | head -10
if echo "$PS_OUT" | grep -q "app.*running"; then
    pass "app service is running"
else
    fail "app service not running in baseline ps output"
fi

# ============================================================
# Test 2: Stop app service via daemon API
# ============================================================

echo ""
echo "=== Test 2: Stop app service ==="

STOP_RESP=$(curl -sS -X POST "${DAEMON_API}/api/v1/service/stop" \
    -H "content-type: application/json" \
    -d '{"project":"coast-remote-compose","name":"dev-1","service":"app"}' 2>&1)
echo "  stop response: $STOP_RESP"
assert_contains "$STOP_RESP" "success" "stop API returned success"
pass "app service stopped"

sleep 2

# ============================================================
# Test 3: Verify app service is stopped
# ============================================================

echo ""
echo "=== Test 3: Verify app service stopped ==="

PS_AFTER_STOP=$("$COAST" ps dev-1 2>&1)
echo "$PS_AFTER_STOP" | head -10
if echo "$PS_AFTER_STOP" | grep -q "app.*running"; then
    fail "app service still shows running after stop"
fi
pass "app service no longer running"

# ============================================================
# Test 4: Start app service
# ============================================================

echo ""
echo "=== Test 4: Start app service ==="

START_RESP=$(curl -sS -X POST "${DAEMON_API}/api/v1/service/start" \
    -H "content-type: application/json" \
    -d '{"project":"coast-remote-compose","name":"dev-1","service":"app"}' 2>&1)
echo "  start response: $START_RESP"
assert_contains "$START_RESP" "success" "start API returned success"
pass "app service started"

sleep 3

# ============================================================
# Test 5: Verify app service is running again
# ============================================================

echo ""
echo "=== Test 5: Verify app service running again ==="

PS_AFTER_START=$("$COAST" ps dev-1 2>&1)
echo "$PS_AFTER_START" | head -10
if echo "$PS_AFTER_START" | grep -q "app.*running"; then
    pass "app service is running after start"
else
    fail "app service not running after start"
fi

# ============================================================
# Test 6: Restart app service
# ============================================================

echo ""
echo "=== Test 6: Restart app service ==="

RESTART_RESP=$(curl -sS -X POST "${DAEMON_API}/api/v1/service/restart" \
    -H "content-type: application/json" \
    -d '{"project":"coast-remote-compose","name":"dev-1","service":"app"}' 2>&1)
echo "  restart response: $RESTART_RESP"
assert_contains "$RESTART_RESP" "success" "restart API returned success"
pass "app service restarted"

sleep 3

# ============================================================
# Test 7: Verify app service still running after restart
# ============================================================

echo ""
echo "=== Test 7: Verify app service running after restart ==="

PS_AFTER_RESTART=$("$COAST" ps dev-1 2>&1)
echo "$PS_AFTER_RESTART" | head -10
if echo "$PS_AFTER_RESTART" | grep -q "app.*running"; then
    pass "app service running after restart"
else
    fail "app service not running after restart"
fi

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote service control tests passed!"
echo "=========================================="
