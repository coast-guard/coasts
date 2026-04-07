#!/usr/bin/env bash
#
# Integration test: remote assign must preserve gitignored generated files.
#
# Reproduces the bug where `coast assign` for remote instances uses
# rsync --delete-after, which deletes generated files (proto clients,
# .react-router types, etc.) that only exist on the remote workspace
# because they're in .gitignore and not in the local worktree.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
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

echo "=== Remote Assign Preserves Generated Files Test ==="
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

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 5

# ============================================================
# Test 1: Create generated files on the remote workspace
# ============================================================

echo ""
echo "=== Test 1: Create generated files inside remote DinD ==="

set +e
"$COAST" exec dev-1 -- sh -c "mkdir -p /workspace/generated/proto && echo 'export const HEALTH = true;' > /workspace/generated/proto/health_client.ts" 2>&1
set -e

VERIFY=$("$COAST" exec dev-1 -- cat /workspace/generated/proto/health_client.ts 2>&1)
if echo "$VERIFY" | grep -q "HEALTH"; then
    pass "Generated file created on remote"
else
    fail "Failed to create generated file"
fi

# ============================================================
# Test 2: Assign to worktree (triggers rsync --delete-after)
# ============================================================

echo ""
echo "=== Test 2: Assign to feature-sync-test worktree ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

"$COAST" assign dev-1 -w feature-sync-test 2>&1 >/dev/null
pass "Assigned to feature-sync-test"

sleep 3

# Verify the worktree content changed (feature branch has different greeting)
GREETING=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
if echo "$GREETING" | grep -q "feature branch"; then
    pass "Worktree content updated correctly"
else
    echo "  server.js content: $(echo "$GREETING" | head -3)"
    echo "  WARNING: worktree content may not have updated"
fi

# ============================================================
# Test 3: Verify generated file survived the assign
# ============================================================

echo ""
echo "=== Test 3: Verify generated file preserved after assign ==="

set +e
CHECK=$("$COAST" exec dev-1 -- cat /workspace/generated/proto/health_client.ts 2>&1)
CHECK_EXIT=$?
set -e

echo "  exec exit: $CHECK_EXIT"
echo "  content: $CHECK"

if echo "$CHECK" | grep -q "HEALTH"; then
    pass "Generated file preserved after assign"
else
    fail "Generated file DELETED by assign rsync -- --delete-after removed gitignored files"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All assign-preserves-generated tests passed!"
echo "=========================================="
