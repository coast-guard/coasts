#!/usr/bin/env bash
#
# Integration test: coast rm for remote instances must clean up the
# DinD Docker volume and workspace directory, not leave them orphaned.

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

echo "=== Remote RM Cleans Volumes Test ==="
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
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
pass "Remote instance running"

sleep 3

# ============================================================
# Test 1: Verify DinD volume exists
# ============================================================

echo ""
echo "=== Test 1: Verify DinD volume exists ==="

VOL_EXISTS=$(docker volume ls -q | grep -x "coast-dind--coast-remote-basic--dev-1" || echo "")
if [ -n "$VOL_EXISTS" ]; then
    pass "DinD volume exists: $VOL_EXISTS"
else
    echo "  Available volumes:"
    docker volume ls -q | grep coast || echo "  (none)"
    fail "DinD volume not found"
fi

# ============================================================
# Test 2: Remove the instance
# ============================================================

echo ""
echo "=== Test 2: Remove instance ==="

"$COAST" rm dev-1 2>&1 >/dev/null
pass "Instance removed"

sleep 2

# ============================================================
# Test 3: Verify DinD volume is cleaned up
# ============================================================

echo ""
echo "=== Test 3: Verify DinD volume cleaned up ==="

VOL_AFTER=$(docker volume ls -q | grep -x "coast-dind--coast-remote-basic--dev-1" || echo "")
if [ -z "$VOL_AFTER" ]; then
    pass "DinD volume cleaned up (no orphaned volume)"
else
    fail "DinD volume still exists after rm: $VOL_AFTER"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
pass "Done"

echo ""
echo "=========================================="
echo "  All rm-cleans-volumes tests passed!"
echo "=========================================="
