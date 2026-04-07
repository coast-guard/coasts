#!/usr/bin/env bash
#
# Integration test: rsync with a non-root SSH user that requires sudo.
#
# The dindind base image creates `testuser` with passwordless sudo.
# This test registers a remote as testuser@localhost and verifies the
# full lifecycle works: build, run, assign. The daemon should detect
# that testuser has sudo and use `--rsync-path=sudo rsync`.
#
# Contrast with test_remote_basic.sh which uses root@localhost (no sudo).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up sudo rsync test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-sudo 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Rsync Sudo Integration Test ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Setting up SSH ---"
setup_localhost_ssh

echo "--- Setting up SSH for testuser ---"
ssh-keygen -t ed25519 -f /tmp/testuser_key -N "" -q 2>/dev/null || true
mkdir -p /home/testuser/.ssh
chmod 700 /home/testuser/.ssh
cp /tmp/testuser_key.pub /home/testuser/.ssh/authorized_keys
chmod 600 /home/testuser/.ssh/authorized_keys
chown -R testuser:testuser /home/testuser/.ssh

ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes \
    -i /tmp/testuser_key testuser@localhost echo "ssh ok" 2>/dev/null
pass "testuser SSH configured"

SUDO_CHECK=$(ssh -o BatchMode=yes -i /tmp/testuser_key testuser@localhost "sudo -n true && echo OK" 2>&1)
if echo "$SUDO_CHECK" | grep -q "OK"; then
    pass "testuser has passwordless sudo"
else
    fail "testuser does not have passwordless sudo: $SUDO_CHECK"
fi

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Register remote as testuser (non-root)
# ============================================================

echo ""
echo "=== Test 1: Register remote as testuser ==="

ADD_OUT=$("$COAST" remote add test-sudo "testuser@localhost" --key /tmp/testuser_key 2>&1)
assert_contains "$ADD_OUT" "added" "remote added as testuser"

TEST_OUT=$("$COAST" remote test test-sudo 2>&1)
assert_contains "$TEST_OUT" "reachable" "testuser remote reachable"
pass "Non-root remote registered"

# ============================================================
# Test 2: Build (requires sudo rsync for workspace sync)
# ============================================================

echo ""
echo "=== Test 2: Build with sudo rsync ==="

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
BUILD_OUT=$("$COAST" build --type remote --remote test-sudo 2>&1)
BUILD_EXIT=$?
set -e
echo "  build exit: $BUILD_EXIT"
if [ "$BUILD_EXIT" -ne 0 ]; then
    echo "  build output: $BUILD_OUT"
    tail -20 /tmp/coastd-test.log 2>/dev/null || true
    fail "Remote build failed (exit $BUILD_EXIT)"
fi
assert_contains "$BUILD_OUT" "Build complete" "remote build with sudo rsync succeeds"
pass "Remote build complete (used sudo rsync)"

# ============================================================
# Test 3: Run (creates workspace owned by root, needs sudo)
# ============================================================

echo ""
echo "=== Test 3: Run with sudo rsync ==="

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote --remote test-sudo 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance created with sudo rsync"

# ============================================================
# Test 4: Exec to verify workspace content
# ============================================================

echo ""
echo "=== Test 4: Verify workspace content ==="

sleep 3

set +e
EXEC_OUT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
set -e

if echo "$EXEC_OUT" | grep -q "Hello from Remote Coast!"; then
    pass "Workspace content correct via sudo rsync"
else
    echo "  Got: $(echo "$EXEC_OUT" | head -5)"
    fail "Workspace content not found"
fi

# ============================================================
# Test 5: Assign (rsync over root-owned files with sudo)
# ============================================================

echo ""
echo "=== Test 5: Assign with sudo rsync ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-sync-test 2>&1)
ASSIGN_EXIT=$?
set -e

[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }
pass "Assign with sudo rsync succeeded"

sleep 5

set +e
FEATURE_CONTENT=$("$COAST" exec dev-1 -- cat /workspace/server.js 2>&1)
set -e

if echo "$FEATURE_CONTENT" | grep -q "Hello from feature branch!"; then
    pass "Feature branch content synced via sudo rsync"
else
    echo "  Got: $(echo "$FEATURE_CONTENT" | grep -i hello || echo '(none)')"
    fail "Feature branch content not found after assign"
fi

# ============================================================
# Test 6: Clean up
# ============================================================

echo ""
echo "=== Test 6: Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All sudo rsync tests passed!"
echo "=========================================="
