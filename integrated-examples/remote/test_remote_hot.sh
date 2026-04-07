#!/usr/bin/env bash
#
# Integration test: hot-reload assign strategy on remote coasts.
#
# Tests that assigning with strategy "hot" swaps the filesystem without
# restarting services. The bare service re-reads data.json on every request,
# so hot swap is sufficient for the change to be visible.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_hot.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_hot_cleanup() {
    echo ""
    echo "--- Cleaning up hot test ---"

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
trap '_hot_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Hot-Reload Assign Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-hot"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance on main
# ============================================================

echo ""
echo "=== Test 1: Run on main ==="

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

sleep 3

# Verify main data
set +e
MAIN_DATA=$("$COAST" exec dev-1 -- cat /workspace/data.json 2>&1)
set -e
assert_contains "$MAIN_DATA" "main-data" "Main data.json confirmed on remote"

# ============================================================
# Test 2: Hot assign to feature branch
# ============================================================

echo ""
echo "=== Test 2: Hot assign to feature-hot-test ==="

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-hot-test 2>&1)
ASSIGN_EXIT=$?
set -e
echo "  assign exit: $ASSIGN_EXIT"
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }
pass "Assigned to feature-hot-test"

sleep 5

# Verify feature data is visible (hot reload — no service restart needed)
set +e
FEATURE_DATA=$("$COAST" exec dev-1 -- cat /workspace/data.json 2>&1)
set -e

if echo "$FEATURE_DATA" | grep -q "feature-data"; then
    pass "Feature data.json visible on remote after hot assign"
else
    echo "  Expected: feature-data"
    echo "  Got: $FEATURE_DATA"
    fail "Feature data not found after hot assign"
fi

# Verify coast ls shows worktree
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "feature-hot-test" "coast ls shows worktree after hot assign"

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
echo "  All remote hot-reload tests passed!"
echo "=========================================="
