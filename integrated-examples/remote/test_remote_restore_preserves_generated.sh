#!/usr/bin/env bash
#
# Integration test: daemon restart must not delete generated files on
# the remote workspace.
#
# Reproduces the bug where restore_remote_worktrees calls rsync with
# --delete-after, which removes files on the remote that don't exist
# locally (e.g. generated proto clients, .react-router types, etc.).
# These files are gitignored and only exist on the remote after being
# generated inside the DinD container.

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
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Restore Preserves Generated Files Test ==="
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
"$COAST" exec dev-1 -- sh -c "mkdir -p /workspace/app/generated/proto && echo 'export const HEALTH_SERVICE = true;' > /workspace/app/generated/proto/health_service_client.ts" 2>&1
EXEC_EXIT=$?
set -e
[ "$EXEC_EXIT" -eq 0 ] || fail "Failed to create generated file"

VERIFY=$("$COAST" exec dev-1 -- cat /workspace/app/generated/proto/health_service_client.ts 2>&1)
if echo "$VERIFY" | grep -q "HEALTH_SERVICE"; then
    pass "Generated file created on remote workspace"
else
    fail "Generated file not found after creation"
fi

# ============================================================
# Test 2: Kill and restart daemon
# ============================================================

echo ""
echo "=== Test 2: Kill and restart daemon ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
pass "Daemon killed"

start_daemon
sleep 10
pass "Daemon restarted"

# ============================================================
# Test 3: Verify generated file still exists
# ============================================================

echo ""
echo "=== Test 3: Verify generated file preserved after restart ==="

set +e
CHECK=$("$COAST" exec dev-1 -- cat /workspace/app/generated/proto/health_service_client.ts 2>&1)
CHECK_EXIT=$?
set -e

echo "  exec exit: $CHECK_EXIT"
echo "  content: $CHECK"

if echo "$CHECK" | grep -q "HEALTH_SERVICE"; then
    pass "Generated file preserved after daemon restart"
else
    fail "Generated file DELETED by daemon restart rsync -- restore_remote_worktrees used --delete-after"
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
echo "  All restore-preserves-generated tests passed!"
echo "=========================================="
