#!/usr/bin/env bash
#
# Integration test: auto-unassign on daemon restart after worktree deletion.
#
# Verifies that when a worktree directory is removed from the host while
# the daemon is NOT running, the daemon detects the orphaned assignment
# on startup and automatically unassigns the instance back to the default
# branch.
#
# Uses coast-bare (bare services, main + feature-v2 branches).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_worktree_daemon_restart.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Custom cleanup: the daemon may be running in daemonized mode (not --foreground)
# after `coast daemon start`, so we kill both ways.
_custom_cleanup() {
    echo ""
    echo "--- Cleaning up ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

    "$COAST" daemon kill 2>/dev/null || true
    pkill -f "coastd" 2>/dev/null || true
    sleep 1

    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

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

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-bare"

start_daemon

# ============================================================
# Test 1: Build and run
# ============================================================

echo ""
echo "=== Test 1: Build and run ==="

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed"; }
pass "coast build succeeded"

RUN_OUTPUT=$($COAST run test-restart 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed"; }
CLEANUP_INSTANCES+=("test-restart")
pass "coast run test-restart succeeded"

sleep 3

# ============================================================
# Test 2: Assign to feature-v2
# ============================================================

echo ""
echo "=== Test 2: Assign to feature-v2 ==="

ASSIGN_OUT=$($COAST assign test-restart --worktree feature-v2 2>&1) || { echo "$ASSIGN_OUT"; fail "coast assign failed"; }
assert_contains "$ASSIGN_OUT" "feature-v2" "assign output references feature-v2"
pass "assigned to feature-v2"

sleep 3

SERVERJS=$($COAST exec test-restart -- cat /workspace/server.js 2>&1)
assert_contains "$SERVERJS" "v2" "workspace has v2 server.js"
pass "workspace verified on feature-v2"

# ============================================================
# Test 3: Kill daemon, delete worktree while daemon is down
# ============================================================

echo ""
echo "=== Test 3: Kill daemon and delete worktree ==="

$COAST daemon kill 2>&1 || true
sleep 2

# Verify daemon is actually dead
if $COAST ls 2>/dev/null; then
    fail "daemon should not be reachable after kill"
fi
pass "daemon killed"

rm -rf .worktrees/feature-v2
pass "deleted .coasts/feature-v2 while daemon was down"

# ============================================================
# Test 4: Restart daemon, wait for startup reconciliation
# ============================================================

echo ""
echo "=== Test 4: Restart daemon and wait for auto-unassign ==="

$COAST daemon start 2>&1 || { fail "coast daemon start failed"; }
pass "daemon restarted"

# The startup reconciliation spawns background tasks that detect the
# orphaned worktree and auto-unassign. This involves: detection (instant)
# + 2s propagation delay + first unassign attempt + possible container
# restart + retry.
MAX_WAIT=45
WAITED=0
UNASSIGNED=false

while [ "$WAITED" -lt "$MAX_WAIT" ]; do
    sleep 1
    WAITED=$((WAITED + 1))
    LS_OUT=$($COAST ls 2>&1) || continue
    if echo "$LS_OUT" | grep -q "test-restart" && echo "$LS_OUT" | grep -q "main"; then
        UNASSIGNED=true
        break
    fi
done

if [ "$UNASSIGNED" = "true" ]; then
    pass "instance auto-unassigned to main after ${WAITED}s"
else
    echo "  coast ls output: $LS_OUT"
    fail "instance was not auto-unassigned within ${MAX_WAIT}s"
fi

# ============================================================
# Test 5: Verify workspace reverted to main
# ============================================================

echo ""
echo "=== Test 5: Verify workspace is back on main ==="

sleep 3

SERVERJS_MAIN=$($COAST exec test-restart -- cat /workspace/server.js 2>&1)
assert_not_contains "$SERVERJS_MAIN" "v2" "workspace no longer has v2 code"
pass "workspace files reverted to main"

# ============================================================
# Test 6: Remove instance
# ============================================================

echo ""
echo "=== Test 6: Remove instance ==="

RM_OUTPUT=$($COAST rm test-restart 2>&1) || { echo "$RM_OUTPUT"; fail "coast rm failed"; }
pass "coast rm succeeded"

LS_FINAL=$($COAST ls 2>&1)
assert_not_contains "$LS_FINAL" "test-restart" "instance removed from listing"
pass "instance removed"

# ============================================================
# Summary
# ============================================================

echo ""
echo "=== All worktree daemon-restart tests passed! ==="
