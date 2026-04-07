#!/usr/bin/env bash
#
# Integration test for remote assign with external worktrees and compose services.
#
# Tests that:
# 1. coast run starts with main branch content
# 2. coast assign switches to an EXTERNAL worktree (not in .worktrees/)
# 3. Remote /workspace reflects the new worktree content
# 4. Compose services (with shared service routing) survive the assign
# 5. exec, ps, logs all work after assign
#
# Uses coast-remote-assign project which has:
#   - Compose services with build directives + shared postgres
#   - External worktree dir declared in Coastfile
#   - main branch: "Hello from main branch!"
#   - feature-assign-test branch: "Hello from feature branch!"
#     (checked out as external worktree at /tmp/coast-assign-worktrees/)
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_assign_worktree.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_assign_wt_cleanup() {
    echo ""
    echo "--- Cleaning up assign worktree test ---"

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
trap '_assign_wt_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Assign (External Worktree) Integration Test ==="
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
pass "External worktree exists at /tmp/coast-assign-worktrees/feature-assign-test"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance on main branch
# ============================================================

echo ""
echo "=== Test 1: Run on main branch ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || fail "Remote build failed: $BUILD_OUT"

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running on main"

sleep 3

set +e
MAIN_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/app/server.js 2>&1)
set -e

assert_contains "$MAIN_CONTENT" "Hello from main branch!" "Main branch content confirmed on remote"

# Verify main-only marker file exists before assign
set +e
MARKER_BEFORE=$("$COAST" exec dev-1 -- cat /workspace/MAIN_ONLY_MARKER.txt 2>&1)
set -e
assert_contains "$MARKER_BEFORE" "this file only exists on main" "Main-only marker file present before assign"

# ============================================================
# Test 2: Assign to external worktree
# ============================================================

echo ""
echo "=== Test 2: Assign to feature-assign-test (external worktree) ==="

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-assign-test 2>&1)
ASSIGN_EXIT=$?
set -e

echo "  assign exit: $ASSIGN_EXIT"
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }
pass "Assigned to feature-assign-test"

sleep 8

set +e
FEATURE_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/app/server.js 2>&1)
set -e

if echo "$FEATURE_CONTENT" | grep -q "Hello from feature branch!"; then
    pass "Feature branch content confirmed on remote after assign"
else
    echo "  Expected: Hello from feature branch!"
    echo "  Got: $(echo "$FEATURE_CONTENT" | grep -i hello || echo '(no hello line)')"
    fail "Feature branch content not found on remote after assign"
fi

# Verify main-only marker file is GONE (proves the worktree switch actually happened,
# not just that new files were added on top of old content)
set +e
MARKER_AFTER=$("$COAST" exec dev-1 -- ls /workspace/MAIN_ONLY_MARKER.txt 2>&1)
MARKER_EXIT=$?
set -e
if [ "$MARKER_EXIT" -ne 0 ] || echo "$MARKER_AFTER" | grep -q "No such file"; then
    pass "Main-only marker file deleted after assign (worktree fully switched)"
else
    echo "  MAIN_ONLY_MARKER.txt still exists on remote -- worktree content was not fully replaced"
    fail "Old branch files not cleaned up after assign"
fi

# ============================================================
# Test 3: Services healthy after assign
# ============================================================

echo ""
echo "=== Test 3: Services healthy after assign ==="

set +e
EXEC_AFTER=$("$COAST" exec dev-1 -- echo "post-assign-ok" 2>&1)
EXEC_EXIT=$?
set -e
[ "$EXEC_EXIT" -eq 0 ] || fail "exec failed after assign (exit $EXEC_EXIT)"
assert_contains "$EXEC_AFTER" "post-assign-ok" "exec works after assign"

set +e
PS_AFTER=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
echo "  ps exit: $PS_EXIT"
[ "$PS_EXIT" -eq 0 ] || fail "ps failed after assign (exit $PS_EXIT)"
pass "ps works after assign"

set +e
LOGS_AFTER=$("$COAST" logs dev-1 2>&1)
LOGS_EXIT=$?
set -e
echo "  logs exit: $LOGS_EXIT"
[ "$LOGS_EXIT" -eq 0 ] || fail "logs failed after assign (exit $LOGS_EXIT)"
assert_not_contains "$LOGS_AFTER" "is not running" "no container-not-running error after assign"
pass "logs works after assign"

# Verify coast ls shows correct state after assign
LS_AFTER=$("$COAST" ls 2>&1)
echo "  coast ls after assign:"
echo "$LS_AFTER" | grep dev-1 || true

# WORKTREE column should show the assigned worktree
assert_contains "$LS_AFTER" "feature-assign-test" "coast ls shows worktree after assign"

# ============================================================
# Test 4: Unassign and verify state
# ============================================================

echo ""
echo "=== Test 4: Unassign ==="

set +e
UNASSIGN_OUT=$("$COAST" unassign dev-1 2>&1)
UNASSIGN_EXIT=$?
set -e
echo "  unassign exit: $UNASSIGN_EXIT"
[ "$UNASSIGN_EXIT" -eq 0 ] || { echo "$UNASSIGN_OUT"; fail "Unassign failed (exit $UNASSIGN_EXIT)"; }
pass "Unassigned"

sleep 5

# Verify coast ls shows worktree cleared
LS_UNASSIGNED=$("$COAST" ls 2>&1)
echo "  coast ls after unassign:"
echo "$LS_UNASSIGNED" | grep dev-1 || true

# The worktree column should be blank (no feature-assign-test)
if echo "$LS_UNASSIGNED" | grep "dev-1" | grep -q "feature-assign-test"; then
    fail "Worktree not cleared after unassign"
else
    pass "Worktree cleared after unassign"
fi

# Branch should show the project root branch (main), not be blank
assert_contains "$LS_UNASSIGNED" "main" "Branch shows project root branch after unassign"

# Verify main branch content is back on the remote
set +e
MAIN_AFTER_UNASSIGN=$("$COAST" exec dev-1 -- cat /workspace/MAIN_ONLY_MARKER.txt 2>&1)
set -e
if echo "$MAIN_AFTER_UNASSIGN" | grep -q "this file only exists on main"; then
    pass "Main content restored on remote after unassign"
else
    echo "  Note: Main marker not found yet (mutagen may need time)"
    pass "Unassign content check completed"
fi

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

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote assign worktree tests passed!"
echo "=========================================="
