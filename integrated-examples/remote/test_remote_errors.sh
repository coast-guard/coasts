#!/usr/bin/env bash
#
# Integration test for remote coast error paths.
#
# Tests that:
# 1. Duplicate coast remote add fails with "already exists"
# 2. coast remote test against unreachable host fails with timeout, doesn't hang
# 3. coast remote rm while an instance is running documents current behavior
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync, mutagen installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_errors.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_errors_cleanup() {
    echo ""
    echo "--- Cleaning up errors test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true
    "$COAST" remote rm unreachable-vm 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_errors_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Error Path Integration Tests ==="
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

# ============================================================
# Test 1: Duplicate remote add
# ============================================================

echo ""
echo "=== Test 1: Duplicate coast remote add ==="

ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "first remote add succeeds"

set +e
DUP_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
DUP_EXIT=$?
set -e

echo "  duplicate exit code: $DUP_EXIT"
echo "  duplicate output: $DUP_OUT"

[ "$DUP_EXIT" -ne 0 ] || fail "duplicate remote add should fail but exited 0"
pass "duplicate remote add returns non-zero exit code"

assert_contains "$DUP_OUT" "already exists" "error message mentions 'already exists'"

# Verify the original remote is still intact
LS_OUT=$("$COAST" remote ls 2>&1)
assert_contains "$LS_OUT" "test-remote" "original remote still in list after duplicate attempt"
pass "duplicate add did not corrupt state"

# Clean up for next test
"$COAST" remote rm test-remote 2>&1 >/dev/null

# ============================================================
# Test 2: Unreachable host test
# ============================================================

echo ""
echo "=== Test 2: coast remote test against unreachable host ==="

ADD_OUT=$("$COAST" remote add unreachable-vm "root@192.0.2.1" 2>&1)
assert_contains "$ADD_OUT" "added" "unreachable remote registered"

set +e
TEST_OUT=$(timeout 30 "$COAST" remote test unreachable-vm 2>&1)
TEST_EXIT=$?
set -e

echo "  test exit code: $TEST_EXIT"
echo "  test output: $TEST_OUT"

[ "$TEST_EXIT" -ne 0 ] || fail "testing unreachable host should fail but exited 0"
pass "unreachable host test returns non-zero exit code"

# Should NOT have timed out (the 30s timeout wrapper should not have fired;
# the SSH ConnectTimeout=10 inside the handler should have caught it first)
[ "$TEST_EXIT" -ne 124 ] || fail "test timed out at 30s — SSH ConnectTimeout is not working"
pass "unreachable host test failed promptly (did not hang)"

# Clean up
"$COAST" remote rm unreachable-vm 2>&1 >/dev/null

# ============================================================
# Test 3: Remote rm while instance is running
# ============================================================

echo ""
echo "=== Test 3: coast remote rm while instance is running ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
pass "Remote re-registered"

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || fail "Build failed: $BUILD_OUT"
pass "Build complete"

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

# Verify instance is actually running
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "dev-1" "instance visible before rm"

# Now try to remove the remote while instance is running
set +e
RM_REMOTE_OUT=$("$COAST" remote rm test-remote 2>&1)
RM_REMOTE_EXIT=$?
set -e

echo "  rm remote exit code: $RM_REMOTE_EXIT"
echo "  rm remote output: $RM_REMOTE_OUT"

# Document current behavior: coast remote rm currently succeeds even with
# running instances (it just deletes the DB row). This is a known limitation.
# Future improvement: refuse or warn when instances are using the remote.
if [ "$RM_REMOTE_EXIT" -eq 0 ]; then
    pass "remote rm succeeded (current behavior: allows rm with running instances)"

    # The instance should still be in ls (shadow record persists)
    LS_AFTER=$("$COAST" ls 2>&1)
    assert_contains "$LS_AFTER" "dev-1" "instance shadow still exists after remote rm"
    pass "instance shadow persists after remote rm"
else
    pass "remote rm refused while instance running (protective behavior)"
fi

# Clean up the instance
"$COAST" rm dev-1 2>/dev/null || true
CLEANUP_INSTANCES=()

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote error tests passed!"
echo "=========================================="
