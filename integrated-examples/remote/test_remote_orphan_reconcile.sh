#!/usr/bin/env bash
#
# Integration test: orphan reconciler must NOT unassign remote worktrees.
#
# Reproduces the bug where reconcile_orphaned_worktrees detects remote
# instance worktrees as "orphaned" on daemon restart and auto-unassigns
# them. The reconciler scans local disk for worktree directories, but
# remote worktrees may not exist locally (or may be at external paths).
# It should skip instances with remote_host.is_some().
#
# Reproduction strategy: assign to worktree, then remove the local
# worktree directory before daemon restart. The reconciler should
# preserve the remote assignment but doesn't -- it treats all instances
# the same and considers the missing local dir an orphan.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up orphan reconcile test ---"
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
    cd "$PROJECTS_DIR/remote/coast-remote-basic" 2>/dev/null || true
    git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Orphan Reconcile Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-basic"
start_daemon

# ============================================================
# Test 1: Run + assign to worktree
# ============================================================

echo ""
echo "=== Test 1: Run and assign to worktree ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed"; }
CLEANUP_INSTANCES+=("dev-1")

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

"$COAST" assign dev-1 -w feature-sync-test 2>&1 >/dev/null
pass "Assigned to feature-sync-test"

sleep 3

# Verify worktree is set
LS_BEFORE=$("$COAST" ls 2>&1)
if echo "$LS_BEFORE" | grep "dev-1" | grep -q "feature-sync-test"; then
    pass "Worktree assignment confirmed before restart"
else
    echo "  ls: $(echo "$LS_BEFORE" | grep dev-1)"
    fail "Worktree not assigned"
fi

# ============================================================
# Test 2: Remove local worktree directory, then restart daemon
# ============================================================

echo ""
echo "=== Test 2: Remove local worktree dir + restart daemon ==="

# Remove the local worktree directory so the reconciler can't find it.
# For remote instances this should be irrelevant -- the worktree lives
# on the remote host, not locally. But the reconciler doesn't know that.
rm -rf .worktrees/feature-sync-test
git worktree prune 2>/dev/null || true
pass "Local worktree directory removed"

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
pass "Daemon killed"

start_daemon
sleep 8
pass "Daemon restarted"

# ============================================================
# Test 3: Verify worktree preserved after restart
# ============================================================

echo ""
echo "=== Test 3: Verify worktree preserved (should FAIL) ==="

LS_AFTER=$("$COAST" ls 2>&1)
echo "  ls after restart: $(echo "$LS_AFTER" | grep dev-1)"

if echo "$LS_AFTER" | grep "dev-1" | grep -q "feature-sync-test"; then
    pass "Worktree assignment preserved after daemon restart"
else
    fail "Worktree assignment LOST after daemon restart -- reconcile_orphaned_worktrees incorrectly unassigned remote instance"
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
echo "  All orphan reconcile tests passed!"
echo "=========================================="
