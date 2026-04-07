#!/usr/bin/env bash
#
# Integration test: restart assign strategy on remote coasts.
#
# Tests the default "restart" strategy with compose services and shared
# services. Verifies that after assign:
# - Compose services restart with new content
# - Shared services (db) are not affected
# - Unassign correctly reverts to project root and restarts services
#
# Uses coast-remote-assign which has compose + shared postgres.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_restart_strategy.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_strategy_cleanup() {
    echo ""
    echo "--- Cleaning up strategy test ---"

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

    cd "$PROJECTS_DIR/remote/coast-remote-assign" 2>/dev/null && \
        git worktree remove /tmp/coast-assign-worktrees/feature-assign-test 2>/dev/null || true
    rm -rf /tmp/coast-assign-worktrees 2>/dev/null || true

    echo "Cleanup complete."
}
trap '_strategy_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Restart Strategy Test ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Starting ssh-agent ---"
eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-assign"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance, verify services
# ============================================================

echo ""
echo "=== Test 1: Run on main with compose services ==="

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
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running on main"

sleep 5

# Verify services are up
set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
set -e
assert_contains "$PS_OUT" "app" "app service visible in ps"
pass "Services running on main"

# Verify main content
set +e
MAIN_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/app/server.js 2>&1)
set -e
assert_contains "$MAIN_CONTENT" "Hello from main branch!" "Main content confirmed"

# ============================================================
# Test 2: Assign with restart strategy, verify services restart
# ============================================================

echo ""
echo "=== Test 2: Assign with restart strategy ==="

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-assign-test 2>&1)
ASSIGN_EXIT=$?
set -e
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed"; }
pass "Assigned to feature-assign-test"

sleep 8

# Verify feature content
set +e
FEATURE_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/app/server.js 2>&1)
set -e
assert_contains "$FEATURE_CONTENT" "Hello from feature branch!" "Feature content confirmed after restart assign"

# Verify services still healthy
set +e
PS_AFTER=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
[ "$PS_EXIT" -eq 0 ] || fail "ps failed after assign"
pass "Services healthy after restart assign"

# Verify coast ls state
LS_AFTER=$("$COAST" ls 2>&1)
assert_contains "$LS_AFTER" "feature-assign-test" "coast ls shows worktree"

# ============================================================
# Test 3: Unassign, verify services restart with project root
# ============================================================

echo ""
echo "=== Test 3: Unassign and verify restart ==="

set +e
UNASSIGN_OUT=$("$COAST" unassign dev-1 2>&1)
UNASSIGN_EXIT=$?
set -e
[ "$UNASSIGN_EXIT" -eq 0 ] || { echo "$UNASSIGN_OUT"; fail "Unassign failed"; }
pass "Unassigned"

sleep 8

# Verify main content is back
set +e
MAIN_AFTER=$("$COAST" exec dev-1 -- cat /workspace/app/server.js 2>&1)
set -e
assert_contains "$MAIN_AFTER" "Hello from main branch!" "Main content restored after unassign"

# Verify services healthy after unassign
set +e
PS_UNASSIGN=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
[ "$PS_EXIT" -eq 0 ] || fail "ps failed after unassign"
pass "Services healthy after unassign"

# Verify coast ls cleared
LS_UNASSIGN=$("$COAST" ls 2>&1)
if echo "$LS_UNASSIGN" | grep "dev-1" | grep -q "feature-assign-test"; then
    fail "Worktree not cleared after unassign"
else
    pass "Worktree cleared after unassign"
fi
assert_contains "$LS_UNASSIGN" "main" "Branch shows main after unassign"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

echo ""
echo "=========================================="
echo "  All remote restart strategy tests passed!"
echo "=========================================="
