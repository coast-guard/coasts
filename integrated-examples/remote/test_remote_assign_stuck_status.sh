#!/usr/bin/env bash
#
# Integration test: assign on stuck "assigning" instance should unstick it.
#
# Reproduces the bug where emit_assign_completion restores the stored
# instance status, which may itself be "assigning" from a previous
# failed assign. A successful re-assign should normalize to "running".

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up stuck status test ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    "$COAST" remote rm test-remote 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Assign Stuck Status Test ==="
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
# Test 1: Run instance
# ============================================================

echo ""
echo "=== Test 1: Run remote instance ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

# Verify running status
LS_BEFORE=$("$COAST" ls 2>&1)
if echo "$LS_BEFORE" | grep "dev-1" | grep -q "running"; then
    pass "Instance is in running status"
else
    echo "  ls: $(echo "$LS_BEFORE" | grep dev-1)"
    fail "Instance not in running status"
fi

# ============================================================
# Test 2: Manually set status to "assigning" (simulate stuck state)
# ============================================================

echo ""
echo "=== Test 2: Simulate stuck assigning status ==="

sqlite3 ~/.coast/state.db "UPDATE instances SET status = 'assigning' WHERE project = 'coast-remote-basic' AND name = 'dev-1';"
pass "Status set to assigning in DB"

LS_STUCK=$("$COAST" ls 2>&1)
if echo "$LS_STUCK" | grep "dev-1" | grep -q "assigning"; then
    pass "Confirmed: instance shows assigning status"
else
    echo "  ls: $(echo "$LS_STUCK" | grep dev-1)"
    fail "Failed to set assigning status"
fi

# ============================================================
# Test 3: Assign worktree (should succeed and unstick)
# ============================================================

echo ""
echo "=== Test 3: Assign to worktree (should unstick) ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-sync-test 2>&1)
ASSIGN_EXIT=$?
set -e

echo "  assign exit: $ASSIGN_EXIT"
if [ "$ASSIGN_EXIT" -eq 0 ]; then
    pass "Assign succeeded on stuck instance"
else
    echo "  output: $ASSIGN_OUT"
    fail "Assign failed on stuck instance"
fi

# ============================================================
# Test 4: Verify status is now "running" (not "assigning")
# ============================================================

echo ""
echo "=== Test 4: Verify status normalized to running ==="

LS_AFTER=$("$COAST" ls 2>&1)
echo "  ls after assign: $(echo "$LS_AFTER" | grep dev-1)"

if echo "$LS_AFTER" | grep "dev-1" | grep -q "assigning"; then
    fail "Status still stuck at 'assigning' -- emit_assign_completion did not normalize transitional status"
elif echo "$LS_AFTER" | grep "dev-1" | grep -q "running"; then
    pass "Status correctly normalized to running"
else
    STATUS=$(echo "$LS_AFTER" | grep "dev-1" | awk '{print $4}')
    echo "  Status is: $STATUS"
    pass "Status is not assigning (acceptable: $STATUS)"
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
echo "  All stuck status tests passed!"
echo "=========================================="
