#!/usr/bin/env bash
#
# Integration test: coast daemon restart behavior.
#
# Verifies that `coast daemon restart` works correctly in both cases:
#   1. Daemon is NOT running -> restart starts it
#   2. Daemon IS running -> restart kills and restarts it (new PID)
#
# Prerequisites:
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_daemon_restart.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

_custom_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    "$COAST" daemon kill 2>/dev/null || true
    pkill -f "coastd" 2>/dev/null || true
    sleep 1
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    echo "Cleanup complete."
}
trap '_custom_cleanup' EXIT

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

pkill -f "coastd" 2>/dev/null || true
sleep 1
rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

STATUS_OUT=$("$COAST" daemon status 2>&1 || true)
assert_contains "$STATUS_OUT" "not running" "daemon is initially stopped"

# ============================================================
# Test 1: restart when daemon is NOT running -> should start it
# ============================================================

echo ""
echo "=== Test 1: restart from stopped state ==="

"$COAST" daemon restart 2>&1 || fail "coast daemon restart failed when daemon was stopped"
sleep 2

STATUS_OUT=$("$COAST" daemon status 2>&1 || true)
assert_contains "$STATUS_OUT" "is running" "daemon is running after restart from stopped state"

PID_BEFORE=$(cat ~/.coast/coastd.pid 2>/dev/null | tr -d '[:space:]')
[ -n "$PID_BEFORE" ] || fail "PID file should exist after restart"
pass "daemon started with PID $PID_BEFORE"

# ============================================================
# Test 2: restart when daemon IS running -> should get new PID
# ============================================================

echo ""
echo "=== Test 2: restart from running state ==="

"$COAST" daemon restart 2>&1 || fail "coast daemon restart failed when daemon was running"
sleep 2

STATUS_OUT=$("$COAST" daemon status 2>&1 || true)
assert_contains "$STATUS_OUT" "is running" "daemon is running after restart from running state"

PID_AFTER=$(cat ~/.coast/coastd.pid 2>/dev/null | tr -d '[:space:]')
[ -n "$PID_AFTER" ] || fail "PID file should exist after second restart"

if [ "$PID_BEFORE" = "$PID_AFTER" ]; then
    fail "PID should change after restart (before=$PID_BEFORE, after=$PID_AFTER)"
fi
pass "daemon restarted with new PID $PID_AFTER (was $PID_BEFORE)"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL DAEMON RESTART TESTS PASSED"
echo "==========================================="
