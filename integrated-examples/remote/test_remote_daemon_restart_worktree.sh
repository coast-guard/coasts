#!/usr/bin/env bash
#
# Integration test: daemon restart recovery for remote worktrees.
#
# Verifies that after a daemon restart (simulating sleep/wake or reboot),
# remote instances with assigned worktrees recover correctly:
# 1. Shell container /workspace points to the worktree (not project root)
# 2. Remote workspace has the worktree content
# 3. Mutagen session is restarted for continuous sync
#
# This test must FAIL before the fix (restore_remote_worktrees) and
# PASS after it.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up daemon restart worktree test ---"

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

echo "=== Remote Daemon Restart Worktree Recovery Test ==="
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
# Test 1: Run + Assign to worktree
# ============================================================

echo ""
echo "=== Test 1: Run and assign to feature worktree ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null
pass "Builds complete"

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 3

# Verify main branch content
MAIN_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
if echo "$MAIN_CONTENT" | grep -q "Hello from Remote Coast!"; then
    pass "Main branch content confirmed"
else
    fail "Main branch content not found"
fi

# Assign to feature worktree
mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-sync-test 2>&1)
ASSIGN_EXIT=$?
set -e
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }
pass "Assigned to feature-sync-test"

sleep 5

# Verify feature content is on remote
FEATURE_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
if echo "$FEATURE_CONTENT" | grep -q "Hello from feature branch!"; then
    pass "Feature branch content confirmed on remote"
else
    echo "  Got: $(echo "$FEATURE_CONTENT" | grep -i hello || echo '(none)')"
    fail "Feature branch content not found"
fi

# ============================================================
# Test 2: Simulate sleep/reboot (kill daemon + restart shell container)
# ============================================================

echo ""
echo "=== Test 2: Simulate sleep/reboot ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
pass "Daemon killed"

# Simulate what happens when Docker Desktop restarts after sleep/reboot:
# The mount --bind overlay inside the shell container is lost, reverting
# /workspace to the original Docker bind mount (project root).
SHELL_CONTAINER="coast-remote-basic-coasts-dev-1-shell"
docker exec "$SHELL_CONTAINER" sh -c "umount -l /workspace 2>/dev/null; mount --bind /host-project /workspace" 2>/dev/null || true
sleep 1

# Kill any mutagen sessions (they die on real restart)
docker exec "$SHELL_CONTAINER" sh -c "mutagen sync terminate coast-coast-remote-basic-dev-1 2>/dev/null; mutagen daemon stop 2>/dev/null" || true
sleep 1
pass "Simulated sleep/reboot (mount reset + mutagen killed)"

# Verify the mount was actually reset
PRE_FIX_CONTENT=$(docker exec "$SHELL_CONTAINER" cat /workspace/server.js 2>&1 || true)
if echo "$PRE_FIX_CONTENT" | grep -q "Hello from Remote Coast!"; then
    pass "Confirmed: shell /workspace reverted to project root"
else
    echo "  Warning: shell /workspace still has worktree content"
fi

start_daemon
sleep 5
pass "Daemon restarted"

# ============================================================
# Test 3: Verify worktree content after restart
# ============================================================

echo ""
echo "=== Test 3: Verify worktree content after daemon restart ==="

set +e
POST_RESTART_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
EXEC_EXIT=$?
set -e

echo "  exec exit: $EXEC_EXIT"
if [ "$EXEC_EXIT" -ne 0 ]; then
    echo "  exec output: $POST_RESTART_CONTENT"
    echo "  coastd log (last 10):"
    tail -10 /tmp/coastd-test.log 2>/dev/null || true
    fail "coast exec failed after daemon restart (exit $EXEC_EXIT)"
fi

if echo "$POST_RESTART_CONTENT" | grep -q "Hello from feature branch!"; then
    pass "Worktree content preserved after daemon restart"
else
    echo "  Expected: Hello from feature branch!"
    echo "  Got: $(echo "$POST_RESTART_CONTENT" | grep -i hello || echo '(none)')"
    fail "Worktree content lost after daemon restart -- shows project root instead of worktree"
fi

# ============================================================
# Test 4: Verify mutagen session restored
# ============================================================

echo ""
echo "=== Test 4: Verify mutagen session after restart ==="

SHELL_CONTAINER="coast-remote-basic-coasts-dev-1-shell"
set +e
MUTAGEN_LIST=$(docker exec "$SHELL_CONTAINER" mutagen sync list 2>&1)
MUTAGEN_EXIT=$?
set -e

if [ "$MUTAGEN_EXIT" -eq 0 ] && echo "$MUTAGEN_LIST" | grep -q "coast-coast-remote-basic-dev-1"; then
    pass "Mutagen session restored after daemon restart"
else
    echo "  mutagen output: $(echo "$MUTAGEN_LIST" | head -5)"
    fail "Mutagen session not restored after daemon restart"
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
echo "  All daemon restart worktree tests passed!"
echo "=========================================="
