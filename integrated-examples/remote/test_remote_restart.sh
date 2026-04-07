#!/usr/bin/env bash
#
# Integration test: remote coast survives coast-service restart.
#
# Verifies that after stopping and restarting coast-service, the
# previously-running remote instance is still fully operational:
#   - exec returns correct output
#   - logs returns compose output (not a Docker socket error)
#   - ps completes without error
#
# This relies on the reconcile_instances startup hook in coast-service
# that checks Docker container state against the persisted SQLite DB.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_restart.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_restart_cleanup() {
    echo ""
    echo "--- Cleaning up restart test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

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
trap '_restart_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Restart Integration Test ==="
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

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
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

# ============================================================
# Test 1: Baseline — exec, logs, ps all work before restart
# ============================================================

echo ""
echo "=== Test 1: Baseline before restart ==="

EXEC_BEFORE=$("$COAST" exec dev-1 -- echo "before restart" 2>&1)
assert_contains "$EXEC_BEFORE" "before restart" "exec works before restart"

set +e
LOGS_BEFORE=$("$COAST" logs dev-1 2>&1)
LOGS_BEFORE_EXIT=$?
set -e
echo "  logs exit code: $LOGS_BEFORE_EXIT"
[ "$LOGS_BEFORE_EXIT" -eq 0 ] || fail "logs failed before restart (exit $LOGS_BEFORE_EXIT)"
pass "logs works before restart"

set +e
PS_BEFORE=$("$COAST" ps dev-1 2>&1)
PS_BEFORE_EXIT=$?
set -e
echo "  ps exit code: $PS_BEFORE_EXIT"
[ "$PS_BEFORE_EXIT" -eq 0 ] || fail "ps failed before restart (exit $PS_BEFORE_EXIT)"
pass "ps works before restart"

DIND_CONTAINER="coast-remote-basic-coasts-dev-1"

set +e
SSH_EXEC_BEFORE=$(ssh -o StrictHostKeyChecking=no -o BatchMode=yes \
    -i ~/.ssh/coast_test_key root@localhost \
    "docker exec $DIND_CONTAINER echo ssh-baseline" 2>&1)
SSH_EXEC_BEFORE_EXIT=$?
set -e
echo "  ssh exec exit code: $SSH_EXEC_BEFORE_EXIT"
[ "$SSH_EXEC_BEFORE_EXIT" -eq 0 ] || fail "ssh exec failed before restart (exit $SSH_EXEC_BEFORE_EXIT)"
assert_contains "$SSH_EXEC_BEFORE" "ssh-baseline" "ssh exec works before restart"

# ============================================================
# Test 2: Stop coast-service AND the inner container
# This simulates the Docker daemon restarting (e.g. the
# coast-service-dev container is killed and restarted).
# ============================================================

echo ""
echo "=== Test 2: Stop coast-service + inner container ==="

set +e
CONTAINER_BEFORE=$(docker ps -q --filter "name=coast-remote-basic-coasts-dev-1" --filter "status=running" 2>/dev/null | head -1)
set -e
echo "  Container before stop: ${CONTAINER_BEFORE:-none}"
[ -n "$CONTAINER_BEFORE" ] || fail "No running container found before stop"

stop_coast_service
pass "coast-service stopped"

docker stop "$CONTAINER_BEFORE" >/dev/null 2>&1
pass "Inner Docker container stopped (simulating daemon restart)"

# Verify the container exists but is NOT running
set +e
CONTAINER_STOPPED=$(docker ps -q --filter "id=$CONTAINER_BEFORE" --filter "status=exited" 2>/dev/null | head -1)
set -e
[ -n "$CONTAINER_STOPPED" ] || echo "  Note: container not in exited state (may have been removed)"

# ============================================================
# Test 3: Restart coast-service (runs reconcile_instances)
# Reconciliation should detect the stopped container and
# restart it automatically.
# ============================================================

echo ""
echo "=== Test 3: Restart coast-service ==="

start_coast_service
pass "coast-service restarted"

# Wait for reconciliation to complete (it runs in the background and
# waits for the inner Docker daemon, which can take up to 60s).
echo "  Waiting for reconciliation to finish..."
for i in $(seq 1 90); do
    if grep -q "inner daemon healthy\|reconciliation complete\|inner daemon not healthy" /tmp/coast-service-test.log 2>/dev/null; then
        break
    fi
    sleep 1
done

echo "  coast-service log:"
grep -E "reconcil|inner daemon" /tmp/coast-service-test.log 2>/dev/null | tail -5 || true

# Verify reconciliation restarted the container
if command -v sqlite3 >/dev/null 2>&1; then
    INSTANCE_STATUS=$(sqlite3 /root/.coast-service/state.db \
        "SELECT status FROM instances WHERE name='dev-1'" 2>/dev/null || true)
    echo "  Instance status in DB after reconciliation: ${INSTANCE_STATUS:-unknown}"
    assert_eq "$INSTANCE_STATUS" "running" "Reconciliation restored running status"
fi

# ============================================================
# Test 4: Exec works after restart (HTTP API path)
# ============================================================

echo ""
echo "=== Test 4: Exec after restart (API) ==="

set +e
EXEC_AFTER=$(timeout 20 "$COAST" exec dev-1 -- echo "after restart" 2>&1)
EXEC_AFTER_EXIT=$?
set -e

echo "  exec exit code: $EXEC_AFTER_EXIT"
echo "  exec output: $(echo "$EXEC_AFTER" | head -3)"

[ "$EXEC_AFTER_EXIT" -ne 124 ] || fail "exec hung after restart (timed out)"

if [ "$EXEC_AFTER_EXIT" -ne 0 ]; then
    echo "  coast-service log tail:"
    tail -20 /tmp/coast-service-test.log 2>/dev/null || true
    fail "exec failed after coast-service restart (exit $EXEC_AFTER_EXIT)"
fi
assert_contains "$EXEC_AFTER" "after restart" "exec returns correct output after restart"
pass "Exec works after restart (API)"

# ============================================================
# Test 4b: SSH docker exec works after restart (shell path)
# This is the path the UI shell uses: daemon SSH's to the
# remote host and runs docker exec inside the DinD container.
# ============================================================

echo ""
echo "=== Test 4b: SSH exec after restart (shell path) ==="

DIND_CONTAINER="coast-remote-basic-coasts-dev-1"

set +e
SSH_EXEC_AFTER=$(timeout 20 ssh -o StrictHostKeyChecking=no -o BatchMode=yes \
    -i ~/.ssh/coast_test_key root@localhost \
    "docker exec $DIND_CONTAINER echo ssh-shell-test" 2>&1)
SSH_EXEC_EXIT=$?
set -e

echo "  ssh exec exit code: $SSH_EXEC_EXIT"
echo "  ssh exec output: $(echo "$SSH_EXEC_AFTER" | head -3)"

[ "$SSH_EXEC_EXIT" -ne 124 ] || fail "ssh exec hung after restart (timed out)"

if [ "$SSH_EXEC_EXIT" -ne 0 ]; then
    echo "  docker ps -a for DinD container:"
    docker ps -a --filter "name=$DIND_CONTAINER" --format '{{.ID}} {{.Status}} {{.Names}}' 2>/dev/null || true
    echo "  coast-service log tail:"
    tail -20 /tmp/coast-service-test.log 2>/dev/null || true
    fail "ssh exec failed after restart (exit $SSH_EXEC_EXIT) — this is the shell path"
fi
assert_contains "$SSH_EXEC_AFTER" "ssh-shell-test" "ssh exec returns correct output after restart"
pass "SSH exec works after restart (shell path)"

# ============================================================
# Test 5: Logs works after restart
# ============================================================

echo ""
echo "=== Test 5: Logs after restart ==="

set +e
LOGS_AFTER=$(timeout 20 "$COAST" logs dev-1 2>&1)
LOGS_AFTER_EXIT=$?
set -e

echo "  logs exit code: $LOGS_AFTER_EXIT"
echo "  logs output (first 3 lines): $(echo "$LOGS_AFTER" | head -3)"

[ "$LOGS_AFTER_EXIT" -ne 124 ] || fail "logs hung after restart (timed out)"

if [ "$LOGS_AFTER_EXIT" -ne 0 ]; then
    echo "  coast-service log tail:"
    tail -20 /tmp/coast-service-test.log 2>/dev/null || true
    fail "logs failed after coast-service restart (exit $LOGS_AFTER_EXIT)"
fi

assert_not_contains "$LOGS_AFTER" "docker.sock" "logs does not show Docker socket error"
assert_not_contains "$LOGS_AFTER" "docker not available" "logs does not say docker unavailable"
assert_not_contains "$LOGS_AFTER" "is not running" "logs does not show container-not-running error"
pass "Logs works after restart"

# ============================================================
# Test 6: PS works after restart
# ============================================================

echo ""
echo "=== Test 6: PS after restart ==="

set +e
PS_AFTER=$(timeout 20 "$COAST" ps dev-1 2>&1)
PS_AFTER_EXIT=$?
set -e

echo "  ps exit code: $PS_AFTER_EXIT"
echo "  ps output (first 3 lines): $(echo "$PS_AFTER" | head -3)"

[ "$PS_AFTER_EXIT" -ne 124 ] || fail "ps hung after restart (timed out)"

if [ "$PS_AFTER_EXIT" -ne 0 ]; then
    echo "  coast-service log tail:"
    tail -20 /tmp/coast-service-test.log 2>/dev/null || true
    fail "ps failed after coast-service restart (exit $PS_AFTER_EXIT)"
fi
pass "PS works after restart"

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
echo "  All remote restart tests passed!"
echo "=========================================="
