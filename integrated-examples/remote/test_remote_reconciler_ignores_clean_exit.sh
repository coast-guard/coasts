#!/usr/bin/env bash
#
# Integration test: the coast-service reconciler must NOT loop-restart
# services that exit cleanly (exit code 0).
#
# Reproduces the bug where a compose service like backend-test (with
# entrypoint: ["sh"]) exits immediately with code 0, causing the
# reconciler to detect "missing" services and run docker compose up -d
# every 15 seconds in an infinite loop. This disrupts other services.
#
# The exit-zero service exits with code 0. The reconciler must ignore it.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
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
trap '_cleanup' EXIT

echo "=== Remote Reconciler Ignores Clean Exit Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate

eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-compose"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 10

# ============================================================
# Test 1: Verify running services and exited service
# ============================================================

echo ""
echo "=== Test 1: Verify service states ==="

set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
set -e
echo "$PS_OUT" | head -8

if echo "$PS_OUT" | grep -q "app.*running"; then
    pass "app is running"
else
    fail "app not running"
fi

if echo "$PS_OUT" | grep -q "fragile-cache.*running"; then
    pass "fragile-cache is running"
else
    fail "fragile-cache not running"
fi

# ============================================================
# Test 2: Wait and verify reconciler does NOT loop
# ============================================================

echo ""
echo "=== Test 2: Verify reconciler does not loop ==="

# Snapshot coast-service log
CS_LOG_BEFORE=$(wc -l < /tmp/coast-service-test.log | tr -d ' ')
echo "  coast-service log lines before wait: $CS_LOG_BEFORE"

# Wait for more than one reconciler cycle (15s)
sleep 20
pass "Waited 20 seconds"

# Count new "compose services restarted" or "healing" entries
HEAL_COUNT=$(tail -n +$((CS_LOG_BEFORE + 1)) /tmp/coast-service-test.log | grep -c "compose services restarted\|healing" || true)
HEAL_COUNT=$(echo "$HEAL_COUNT" | tr -d '[:space:]')
echo "  New healing/restart log entries: $HEAL_COUNT"

if [ "$HEAL_COUNT" -gt 0 ]; then
    echo "  coast-service recent logs:"
    tail -n +$((CS_LOG_BEFORE + 1)) /tmp/coast-service-test.log | grep "compose\|heal" | head -5
    fail "Reconciler is looping: $HEAL_COUNT restart entries in 20 seconds (exit-zero service with code 0 should be ignored)"
else
    pass "Reconciler did not loop (0 restart entries)"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All reconciler-ignores-clean-exit tests passed!"
echo "=========================================="
