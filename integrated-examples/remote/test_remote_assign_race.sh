#!/usr/bin/env bash
#
# Integration test: rsync race condition during remote assign.
#
# Reproduces the race where a file-watching service inside the remote
# DinD container regenerates directories while the daemon's rsync
# (running from the host) is writing files during coast assign.
#
# The watcher service simulates React Router's dev server behavior:
# it watches /workspace for .js file changes and deletes/recreates
# a .generated-types/app/+types/ directory in response.
#
# Verifies that:
# 1. coast assign succeeds despite rsync exit code 23 (partial transfer)
# 2. The source files (server.js) sync correctly to the remote
# 3. The feature branch content is present after assign
#
# This test validates the exit code 23 handling in sync.rs.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up assign race test ---"

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

# ============================================================
# Setup
# ============================================================

echo "=== Remote Assign Race Condition Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-file-watcher"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance (watcher service starts)
# ============================================================

echo ""
echo "=== Test 1: Run with file watcher service ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || fail "Remote build failed: $BUILD_OUT"
pass "Remote build complete"

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running with watcher service"

# Wait for watcher to initialize and create +types
sleep 5

# Verify watcher created the generated directory
set +e
WATCHER_CHECK=$("$COAST" exec dev-1 -- ls /workspace/.generated-types/app/+types/root.ts 2>&1)
set -e
if echo "$WATCHER_CHECK" | grep -q "root.ts"; then
    pass "Watcher created +types directory"
else
    echo "  watcher output: $WATCHER_CHECK"
    fail "Watcher did not create +types directory"
fi

# Verify main branch content
set +e
MAIN_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
set -e
if echo "$MAIN_CONTENT" | grep -q "Hello from main branch!"; then
    pass "Main branch content confirmed on remote"
else
    echo "  Got: $(echo "$MAIN_CONTENT" | grep -i hello || echo '(no hello line)')"
    fail "Main branch content not found"
fi

# ============================================================
# Test 2: Assign to feature worktree (triggers the race)
# ============================================================

echo ""
echo "=== Test 2: Assign with active file watcher (race condition) ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-watcher-test feature-watcher-test 2>/dev/null || true
pass "Worktree created"

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-watcher-test 2>&1)
ASSIGN_EXIT=$?
set -e

echo "  assign exit: $ASSIGN_EXIT"
if [ "$ASSIGN_EXIT" -eq 0 ]; then
    pass "Assign succeeded despite file watcher race"
else
    echo "  assign output: $ASSIGN_OUT"
    fail "Assign failed (exit $ASSIGN_EXIT) -- exit code 23 handling may not be working"
fi

# ============================================================
# Test 3: Verify feature branch content synced correctly
# ============================================================

echo ""
echo "=== Test 3: Verify source files synced ==="

sleep 5

set +e
FEATURE_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
set -e

if echo "$FEATURE_CONTENT" | grep -q "Hello from feature branch!"; then
    pass "Feature branch content confirmed on remote after assign"
else
    echo "  Expected: Hello from feature branch!"
    echo "  Got: $(echo "$FEATURE_CONTENT" | grep -i hello || echo '(no hello line)')"
    fail "Feature branch content not found -- source files may not have synced"
fi

# ============================================================
# Test 4: Clean up
# ============================================================

echo ""
echo "=== Test 4: Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Instance removed"

git worktree remove .worktrees/feature-watcher-test 2>/dev/null || true

echo ""
echo "=========================================="
echo "  All assign race condition tests passed!"
echo "=========================================="
