#!/usr/bin/env bash
#
# Integration test: remote unassign should be fast (comparable to assign).
#
# Reproduces the bug where unassign does a full rsync + full compose
# restart (docker compose down + up) instead of the hot path that
# assign uses. This makes unassign take 10-30x longer than assign.
#
# The test:
#   1. Runs a remote instance and assigns a worktree (warm-up)
#   2. Times a second assign (baseline)
#   3. Times unassign
#   4. Asserts unassign completes within 3x of assign time
#   5. Verifies workspace is back on project root after unassign
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_unassign_speed.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_unassign_speed_cleanup() {
    echo ""
    echo "--- Cleaning up unassign speed test ---"

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
trap '_unassign_speed_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Unassign Speed Integration Test ==="
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

echo "--- Verifying external worktree ---"
git worktree list
[ -d /tmp/coast-assign-worktrees/feature-assign-test ] || fail "External worktree not found"
pass "External worktree exists"

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
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 3

# ============================================================
# Warm-up: first assign (may be slower due to initial sync)
# ============================================================

echo ""
echo "=== Warm-up: initial assign ==="

set +e
"$COAST" assign dev-1 -w feature-assign-test 2>&1 >/dev/null
set -e
sleep 3
pass "Warm-up assign complete"

# ============================================================
# Test 1: Time unassign
# ============================================================

echo ""
echo "=== Test 1: Time unassign ==="

UNASSIGN_START=$(date +%s)
set +e
UNASSIGN_OUT=$("$COAST" unassign dev-1 2>&1)
UNASSIGN_EXIT=$?
set -e
UNASSIGN_END=$(date +%s)
UNASSIGN_SECS=$((UNASSIGN_END - UNASSIGN_START))

echo "  unassign exit: $UNASSIGN_EXIT"
echo "  unassign time: ${UNASSIGN_SECS}s"
[ "$UNASSIGN_EXIT" -eq 0 ] || { echo "$UNASSIGN_OUT"; fail "Unassign failed (exit $UNASSIGN_EXIT)"; }

sleep 3

# ============================================================
# Test 2: Time assign (baseline for comparison)
# ============================================================

echo ""
echo "=== Test 2: Time assign (baseline) ==="

ASSIGN_START=$(date +%s)
set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-assign-test 2>&1)
ASSIGN_EXIT=$?
set -e
ASSIGN_END=$(date +%s)
ASSIGN_SECS=$((ASSIGN_END - ASSIGN_START))

echo "  assign exit: $ASSIGN_EXIT"
echo "  assign time: ${ASSIGN_SECS}s"
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }

sleep 3

# ============================================================
# Test 3: Assert unassign is not unreasonably slow
# ============================================================

echo ""
echo "=== Test 3: Speed comparison ==="
echo "  Assign:   ${ASSIGN_SECS}s"
echo "  Unassign: ${UNASSIGN_SECS}s"

MAX_UNASSIGN=$((ASSIGN_SECS * 3 + 5))
if [ "$UNASSIGN_SECS" -gt "$MAX_UNASSIGN" ]; then
    fail "Unassign too slow: ${UNASSIGN_SECS}s vs assign ${ASSIGN_SECS}s (max allowed: ${MAX_UNASSIGN}s)"
fi
pass "Unassign speed acceptable (${UNASSIGN_SECS}s <= ${MAX_UNASSIGN}s)"

# ============================================================
# Test 4: Verify workspace is back on project root
# ============================================================

echo ""
echo "=== Test 4: Workspace back on project root ==="

set +e
MAIN_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/MAIN_ONLY_MARKER.txt 2>&1)
set -e
assert_contains "$MAIN_CONTENT" "this file only exists on main" "Workspace back on project root"

# ============================================================
# Test 5: Second unassign (from assign above) for consistency
# ============================================================

echo ""
echo "=== Test 5: Second unassign timing ==="

UNASSIGN2_START=$(date +%s)
set +e
"$COAST" unassign dev-1 2>&1 >/dev/null
set -e
UNASSIGN2_END=$(date +%s)
UNASSIGN2_SECS=$((UNASSIGN2_END - UNASSIGN2_START))

echo "  Second unassign time: ${UNASSIGN2_SECS}s"
if [ "$UNASSIGN2_SECS" -gt "$MAX_UNASSIGN" ]; then
    fail "Second unassign too slow: ${UNASSIGN2_SECS}s (max: ${MAX_UNASSIGN}s)"
fi
pass "Second unassign also fast (${UNASSIGN2_SECS}s)"

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
echo "  All unassign speed tests passed!"
echo "  Assign: ${ASSIGN_SECS}s | Unassign: ${UNASSIGN_SECS}s"
echo "=========================================="
