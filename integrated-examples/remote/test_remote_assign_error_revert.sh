#!/usr/bin/env bash
#
# Integration test: remote assign error path reverts instance status.
#
# Verifies that when a remote assign fails (e.g., coast-service is
# unreachable), the instance status reverts from "assigning" back to
# its previous status (e.g., "running") instead of staying stuck.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up assign error revert test ---"

    # Restart coast-service if killed
    start_coast_service 2>/dev/null || true

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true

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

echo "=== Remote Assign Error Revert Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance and verify it's running
# ============================================================

echo ""
echo "=== Test 1: Run remote instance ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed"; }
CLEANUP_INSTANCES+=("dev-1")

LS_OUT=$("$COAST" ls 2>&1)
if echo "$LS_OUT" | grep "dev-1" | grep -q "running"; then
    pass "Instance is running"
else
    echo "$LS_OUT"
    fail "Instance not in running status"
fi

# ============================================================
# Test 2: Kill coast-service, trigger assign (should fail)
# ============================================================

echo ""
echo "=== Test 2: Assign with dead coast-service ==="

stop_coast_service
sleep 1
pass "coast-service killed"

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-sync-test 2>&1)
ASSIGN_EXIT=$?
set -e

echo "  assign exit: $ASSIGN_EXIT"
if [ "$ASSIGN_EXIT" -ne 0 ]; then
    pass "Assign correctly failed (coast-service unreachable)"
else
    echo "  Warning: assign succeeded despite dead coast-service"
fi

# ============================================================
# Test 3: Verify status reverted (not stuck in "assigning")
# ============================================================

echo ""
echo "=== Test 3: Verify status is NOT stuck in assigning ==="

sleep 1
LS_AFTER=$("$COAST" ls 2>&1)
echo "  ls output: $(echo "$LS_AFTER" | grep dev-1)"

if echo "$LS_AFTER" | grep "dev-1" | grep -q "assigning"; then
    fail "Instance stuck in 'assigning' -- status was not reverted on error"
else
    pass "Status correctly reverted (not stuck in assigning)"
fi

# ============================================================
# Test 4: Restart coast-service, verify instance is usable
# ============================================================

echo ""
echo "=== Test 4: Verify instance usable after recovery ==="

start_coast_service

set +e
EXEC_OUT=$("$COAST" exec dev-1 -- echo "still-alive" 2>&1)
EXEC_EXIT=$?
set -e

if [ "$EXEC_EXIT" -eq 0 ] && echo "$EXEC_OUT" | grep -q "still-alive"; then
    pass "Instance usable after failed assign + coast-service restart"
else
    echo "  exec exit: $EXEC_EXIT, output: $EXEC_OUT"
    fail "Instance not usable after recovery"
fi

# ============================================================
# Test 5: Clean up
# ============================================================

echo ""
echo "=== Test 5: Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All assign error revert tests passed!"
echo "=========================================="
