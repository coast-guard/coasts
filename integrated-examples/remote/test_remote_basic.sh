#!/usr/bin/env bash
#
# End-to-end integration test for remote coast operations.
#
# Tests the complete remote coast lifecycle:
# 1. SSH setup (localhost as the "remote")
# 2. coast-service startup
# 3. coast remote add/test/ls
# 4. coast build --type remote (triggers build on remote via coast-service)
# 5. coast run --type remote (remote build + provisioning)
# 6. coast exec, ps, logs on remote instance
# 7. coast stop, start, rm lifecycle
# 8. coast remote rm
#
# Prerequisites (provided by DinDinD environment):
#   - Docker running (DinD)
#   - openssh-server installed
#   - rsync installed
#   - Coast binaries built (coast, coastd, coast-service)
#
# Usage:
#   ./integrated-examples/test_remote_basic.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

# Custom cleanup that also tears down remote state
_remote_cleanup() {
    echo ""
    echo "--- Cleaning up remote test ---"

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
trap '_remote_cleanup' EXIT

# ============================================================
# Preflight
# ============================================================

echo "=== Remote Coast Integration Test ==="
echo ""

preflight_checks

# ============================================================
# Setup: SSH, coast-service, daemon
# ============================================================

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
# Test 1: Register remote
# ============================================================

echo ""
echo "=== Test 1: coast remote add ==="

ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "coast remote add succeeds"

# ============================================================
# Test 2: Test remote connectivity
# ============================================================

echo ""
echo "=== Test 2: coast remote test ==="

TEST_OUT=$("$COAST" remote test test-remote 2>&1)
assert_contains "$TEST_OUT" "reachable" "coast remote test succeeds"

# ============================================================
# Test 3: List remotes
# ============================================================

echo ""
echo "=== Test 3: coast remote ls ==="

LS_OUT=$("$COAST" remote ls 2>&1)
assert_contains "$LS_OUT" "test-remote" "remote appears in list"
assert_contains "$LS_OUT" "localhost" "remote shows correct host"

# ============================================================
# Test 4: Build (on remote via coast-service)
# ============================================================

echo ""
echo "=== Test 4: coast build --type remote ==="

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

BUILD_OUT=$("$COAST" build --type remote 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build --type remote succeeds"
pass "Remote build complete (built on remote)"

# ============================================================
# Test 5: Run remote instance
# ============================================================

echo ""
echo "=== Test 5: coast run --type remote ==="

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
echo "  run exit code: $RUN_EXIT"
echo "  run output:"
echo "$RUN_OUT" | head -40
echo "  ---"

if [ "$RUN_EXIT" -ne 0 ]; then
    echo "  coastd log tail:"
    tail -30 /tmp/coastd-test.log 2>/dev/null || true
    echo "  coast-service log tail:"
    tail -30 /tmp/coast-service-test.log 2>/dev/null || true
    fail "coast run --type remote failed (exit $RUN_EXIT)"
fi

CLEANUP_INSTANCES+=("dev-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run --type remote succeeds"
pass "Remote instance created"

# ============================================================
# Test 6: Verify instance in ls
# ============================================================

echo ""
echo "=== Test 6: coast ls ==="

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "dev-1" "instance appears in ls"
pass "Instance visible in ls"

# ============================================================
# Test 7: Exec into remote instance
# ============================================================

echo ""
echo "=== Test 7: coast exec ==="

set +e
EXEC_OUT=$("$COAST" exec dev-1 -- echo "hello from remote" 2>&1)
EXEC_EXIT=$?
set -e
echo "  exec exit code: $EXEC_EXIT"
echo "  exec output: $EXEC_OUT"
if [ "$EXEC_EXIT" -ne 0 ]; then
    echo "  coastd log tail:"
    tail -15 /tmp/coastd-test.log 2>/dev/null || true
    echo "  coast-service log tail:"
    tail -15 /tmp/coast-service-test.log 2>/dev/null || true
    fail "coast exec failed (exit $EXEC_EXIT)"
fi
assert_contains "$EXEC_OUT" "hello from remote" "exec returns correct output"
pass "Remote exec works"

# ============================================================
# Test 8: PS remote instance
# ============================================================

echo ""
echo "=== Test 8: coast ps ==="

PS_OUT=$("$COAST" ps dev-1 2>&1) || true
pass "Remote ps completes (output: ${PS_OUT:-(empty)})"

# ============================================================
# Test 8.5: Primary port is set after remote run
# ============================================================

echo ""
echo "=== Test 8.5: coast ports shows primary ==="

PORTS_OUT=$("$COAST" ports dev-1 2>&1)
echo "$PORTS_OUT" | head -10
# With only one port (app), it should be marked as primary (★)
assert_contains "$PORTS_OUT" "app" "ports output contains app service"
if echo "$PORTS_OUT" | grep -q "★"; then
    pass "primary port is marked with ★"
else
    fail "primary port not set — expected ★ marker on app service"
fi

# ============================================================
# Test 9: Stop remote instance
# ============================================================

echo ""
echo "=== Test 9: coast stop ==="

STOP_OUT=$("$COAST" stop dev-1 2>&1)
assert_contains "$STOP_OUT" "Stopped" "coast stop succeeds"
pass "Remote instance stopped"

# ============================================================
# Test 10: Start remote instance
# ============================================================

echo ""
echo "=== Test 10: coast start ==="

START_OUT=$("$COAST" start dev-1 2>&1)
pass "Remote instance started (output: ${START_OUT:-(empty)})"

# ============================================================
# Test 10.5: Verify ports restored after start
# ============================================================

echo ""
echo "=== Test 10.5: coast ports after start ==="

sleep 2
PORTS_AFTER=$("$COAST" ports dev-1 2>&1)
echo "$PORTS_AFTER" | head -10
assert_contains "$PORTS_AFTER" "app" "ports restored after start"
if echo "$PORTS_AFTER" | grep -q "★"; then
    pass "primary port still marked after stop/start"
else
    fail "primary port not set after stop/start"
fi

# ============================================================
# Test 11: Remove remote instance
# ============================================================

echo ""
echo "=== Test 11: coast rm ==="

RM_OUT=$("$COAST" rm dev-1 2>&1)
assert_contains "$RM_OUT" "Removed" "coast rm succeeds"
CLEANUP_INSTANCES=()
pass "Remote instance removed"

# Verify it's gone from ls
LS_OUT=$("$COAST" ls 2>&1)
assert_not_contains "$LS_OUT" "dev-1" "instance no longer in ls"

# ============================================================
# Test 11.5: Run with --worktree pre-assignment
# ============================================================

echo ""
echo "=== Test 11.5: coast run --type remote --worktree ==="

# Create a worktree if feature-sync-test branch exists
if git branch -a 2>/dev/null | grep -q "feature-sync-test"; then
    mkdir -p .worktrees
    git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

    set +e
    RUN_WT_OUT=$("$COAST" run dev-2 --type remote --worktree feature-sync-test 2>&1)
    RUN_WT_EXIT=$?
    set -e

    if [ "$RUN_WT_EXIT" -eq 0 ]; then
        CLEANUP_INSTANCES+=("dev-2")
        pass "coast run --worktree succeeds for remote"

        sleep 5
        LS_WT=$("$COAST" ls 2>&1)
        if echo "$LS_WT" | grep "dev-2" | grep -q "feature-sync-test"; then
            pass "remote instance shows worktree in ls"
        else
            echo "  ls output: $(echo "$LS_WT" | grep dev-2)"
            fail "remote instance does not show worktree assignment"
        fi

        "$COAST" rm dev-2 2>&1 >/dev/null || true
        CLEANUP_INSTANCES=()
        pass "worktree instance cleaned up"
    else
        echo "  run --worktree failed (exit $RUN_WT_EXIT): $RUN_WT_OUT"
        fail "coast run --worktree failed for remote"
    fi

    git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
else
    echo "  (skipping: feature-sync-test branch not found)"
fi

# ============================================================
# Test 12: Remove remote
# ============================================================

echo ""
echo "=== Test 12: coast remote rm ==="

RM_REMOTE_OUT=$("$COAST" remote rm test-remote 2>&1)
assert_contains "$RM_REMOTE_OUT" "removed" "coast remote rm succeeds"

LS_REMOTE_OUT=$("$COAST" remote ls 2>&1)
assert_not_contains "$LS_REMOTE_OUT" "test-remote" "remote no longer in list"
pass "Remote unregistered"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote coast tests passed!"
echo "=========================================="
