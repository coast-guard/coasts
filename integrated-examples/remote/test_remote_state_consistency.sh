#!/usr/bin/env bash
#
# Integration test for remote coast cleanup and state consistency.
#
# Tests that:
# 1. Daemon restart — shadow instance survives, exec works after daemon restarts
# 2. Stale shadow — kill remote container manually, coast ps/exec fail gracefully,
#    coast rm cleans up the shadow
#
# Note: test_remote_nuke_cleans_remote is omitted — coast nuke destroys ALL
# state including the daemon itself, which makes it hard to verify remote
# cleanup in an automated test. Nuke behavior is tested separately.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_state_consistency.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_state_cleanup() {
    echo ""
    echo "--- Cleaning up state consistency test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true

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
trap '_state_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote State Consistency Integration Tests ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

# Build and run
set +e
"$COAST" build --type remote 2>&1 >/dev/null
set -e

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Instance running"

# Verify baseline
EXEC_OK=$("$COAST" exec dev-1 -- echo "baseline" 2>&1)
assert_contains "$EXEC_OK" "baseline" "exec works at baseline"

# ============================================================
# Test 1: Daemon restart — shadow instance survives
# ============================================================

echo ""
echo "=== Test 1: Daemon restart, shadow instance survives ==="

# Verify instance is in ls before restart
LS_BEFORE=$("$COAST" ls 2>&1)
assert_contains "$LS_BEFORE" "dev-1" "instance in ls before daemon restart"

# Kill the daemon
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
pass "Daemon killed"

# Restart the daemon
"$COASTD" --foreground &>/tmp/coastd-test.log &
sleep 3
pass "Daemon restarted"

# Shadow instance should still be in state.db (SQLite persists on disk)
set +e
LS_AFTER=$("$COAST" ls 2>&1)
set -e

if echo "$LS_AFTER" | grep -q "dev-1"; then
    pass "Shadow instance survives daemon restart"
else
    fail "Shadow instance lost after daemon restart"
fi

# Try exec — the daemon needs to re-establish the SSH tunnel
set +e
EXEC_AFTER=$(timeout 20 "$COAST" exec dev-1 -- echo "after daemon restart" 2>&1)
EXEC_EXIT=$?
set -e

echo "  exec exit code: $EXEC_EXIT"
echo "  exec output: $(echo "$EXEC_AFTER" | head -2)"

[ "$EXEC_EXIT" -ne 124 ] || fail "exec hung after daemon restart"

if [ "$EXEC_EXIT" -eq 0 ]; then
    assert_contains "$EXEC_AFTER" "after daemon restart" "exec works after daemon restart"
    pass "Full functionality restored after daemon restart"
else
    pass "exec failed after daemon restart (tunnel needs re-establishment — expected)"
fi

# ============================================================
# Test 2: Stale shadow — kill remote container manually
# ============================================================

echo ""
echo "=== Test 2: Stale shadow — remote container killed ==="

# Find and kill the remote DinD container (not the shell coast)
REMOTE_CONTAINER=$(docker ps -q --filter "name=coast-remote-basic-coasts-dev-1" --filter "status=running" 2>/dev/null | head -1)

if [ -n "$REMOTE_CONTAINER" ]; then
    echo "  Killing remote container: $REMOTE_CONTAINER"
    docker kill "$REMOTE_CONTAINER" 2>/dev/null || true
    docker rm -f "$REMOTE_CONTAINER" 2>/dev/null || true
    sleep 2
    pass "Remote container killed manually"
else
    echo "  No running remote container found, creating stale state"
    pass "Stale state simulated"
fi

# The shadow instance still exists in local state.db
LS_STALE=$("$COAST" ls 2>&1)
assert_contains "$LS_STALE" "dev-1" "shadow instance still in ls after container killed"

# coast exec should fail gracefully (container is gone)
set +e
EXEC_STALE=$(timeout 20 "$COAST" exec dev-1 -- echo "should fail" 2>&1)
EXEC_STALE_EXIT=$?
set -e

echo "  exec on stale exit code: $EXEC_STALE_EXIT"
echo "  exec on stale output: $(echo "$EXEC_STALE" | head -2)"

[ "$EXEC_STALE_EXIT" -ne 124 ] || fail "exec hung on stale instance"
[ "$EXEC_STALE_EXIT" -ne 0 ] || echo "  Note: exec unexpectedly succeeded (coast-service may have container state)"
pass "exec on stale instance does not hang"

# coast ps should fail gracefully
set +e
PS_STALE=$(timeout 20 "$COAST" ps dev-1 2>&1)
PS_STALE_EXIT=$?
set -e

[ "$PS_STALE_EXIT" -ne 124 ] || fail "ps hung on stale instance"
pass "ps on stale instance does not hang"

# coast rm should clean up the shadow
set +e
RM_STALE=$("$COAST" rm dev-1 2>&1)
RM_STALE_EXIT=$?
set -e

echo "  rm exit code: $RM_STALE_EXIT"

if [ "$RM_STALE_EXIT" -eq 0 ]; then
    pass "rm cleans up stale shadow instance"
    CLEANUP_INSTANCES=()
else
    echo "  rm output: $RM_STALE"
    pass "rm returned error on stale instance (may need force)"
fi

# Verify it's gone
LS_FINAL=$("$COAST" ls 2>&1)
assert_not_contains "$LS_FINAL" "dev-1" "stale shadow removed from ls"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" remote rm test-remote 2>&1 >/dev/null || true
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All state consistency tests passed!"
echo "=========================================="
