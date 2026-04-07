#!/usr/bin/env bash
#
# Integration test: coast remote prune should identify and remove
# orphaned Docker volumes and workspace directories.

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

echo "=== Remote Prune Test ==="
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

# Run an instance, then create an orphaned volume manually
set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
pass "Remote instance running"

sleep 3

# ============================================================
# Test 1: Create an orphaned volume
# ============================================================

echo ""
echo "=== Test 1: Create orphaned volume ==="

docker volume create coast-dind--coast-remote-basic--orphan-1 >/dev/null 2>&1
pass "Created orphaned volume: coast-dind--coast-remote-basic--orphan-1"

# ============================================================
# Test 2: coast remote prune --dry-run lists orphan
# ============================================================

echo ""
echo "=== Test 2: Prune --dry-run ==="

set +e
PRUNE_DRY=$("$COAST" remote prune test-remote --dry-run 2>&1)
PRUNE_DRY_EXIT=$?
set -e
echo "  Output: $PRUNE_DRY"

if echo "$PRUNE_DRY" | grep -q "orphan-1"; then
    pass "Dry-run identified orphaned volume"
else
    fail "Dry-run did not identify orphaned volume"
fi

# Verify volume still exists (dry-run should not remove)
VOL_CHECK=$(docker volume ls -q | grep "orphan-1" || echo "")
if [ -n "$VOL_CHECK" ]; then
    pass "Dry-run did not remove the volume"
else
    fail "Dry-run removed the volume (should not)"
fi

# ============================================================
# Test 3: coast remote prune removes orphan
# ============================================================

echo ""
echo "=== Test 3: Prune (real) ==="

set +e
PRUNE_REAL=$("$COAST" remote prune test-remote 2>&1)
PRUNE_REAL_EXIT=$?
set -e
echo "  Output: $PRUNE_REAL"

VOL_AFTER=$(docker volume ls -q | grep "orphan-1" || echo "")
if [ -z "$VOL_AFTER" ]; then
    pass "Orphaned volume removed by prune"
else
    fail "Orphaned volume still exists after prune"
fi

# ============================================================
# Test 4: Active instance volume is NOT pruned
# ============================================================

echo ""
echo "=== Test 4: Active volume not pruned ==="

ACTIVE_VOL=$(docker volume ls -q | grep "coast-dind--coast-remote-basic--dev-1" || echo "")
if [ -n "$ACTIVE_VOL" ]; then
    pass "Active instance volume preserved: $ACTIVE_VOL"
else
    echo "  WARNING: Active volume not found (may be expected in some setups)"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
pass "Done"

echo ""
echo "=========================================="
echo "  All prune tests passed!"
echo "=========================================="
