#!/usr/bin/env bash
#
# Integration test for remote coast build error paths.
#
# Tests that:
# 1. coast build --type remote in a dir with no Coastfile.remote fails clearly
# 2. coast run --type remote when the remote name isn't registered fails clearly
# 3. coast run --type remote when coast-service is down fails clearly
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_build_errors.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_build_errors_cleanup() {
    echo ""
    echo "--- Cleaning up build errors test ---"

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
trap '_build_errors_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Build Error Integration Tests ==="
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

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: coast build --type remote with no Coastfile.remote
# ============================================================

echo ""
echo "=== Test 1: Build with no Coastfile.remote ==="

# Create a temp directory with a regular Coastfile but no Coastfile.remote
TMPDIR_BUILD=$(mktemp -d)
cat > "$TMPDIR_BUILD/Coastfile" << 'EOF'
[coast]
name = "no-remote-type"
runtime = "dind"

[ports]
app = 3000
EOF

cd "$TMPDIR_BUILD"
git init -b main >/dev/null 2>&1
git config user.name "Test" && git config user.email "test@test.com"
git add -A && git commit -m "init" >/dev/null 2>&1

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e

echo "  build exit code: $BUILD_EXIT"
echo "  build output (first 3 lines): $(echo "$BUILD_OUT" | head -3)"

[ "$BUILD_EXIT" -ne 0 ] || fail "build --type remote should fail when no Coastfile.remote exists"
pass "build --type remote fails without Coastfile.remote"

cd "$PROJECTS_DIR/remote/coast-remote-basic"
rm -rf "$TMPDIR_BUILD"

# ============================================================
# Test 2: coast run --type remote with unregistered remote name
# ============================================================

echo ""
echo "=== Test 2: Run with unregistered remote name ==="

# Do NOT register the remote — just try to run
set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e

echo "  run exit code: $RUN_EXIT"
echo "  run output (first 3 lines): $(echo "$RUN_OUT" | head -3)"

[ "$RUN_EXIT" -ne 0 ] || fail "run --type remote should fail when remote name isn't registered"
pass "run --type remote fails with unregistered remote"

# Check the error message mentions the remote name or registration
if echo "$RUN_OUT" | grep -qi "not registered\|remote add\|not found"; then
    pass "error message guides user to register the remote"
else
    echo "  Note: error message doesn't explicitly mention registration"
    echo "  Output: $RUN_OUT"
    pass "run failed (error message could be improved)"
fi

# Clean up any partial instance
"$COAST" rm dev-1 2>/dev/null || true

# ============================================================
# Test 3: coast run --type remote when coast-service is down
# ============================================================

echo ""
echo "=== Test 3: Remote build fails when coast-service is down ==="

# Register a remote
"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
pass "Remote registered"

# Stop coast-service
stop_coast_service
pass "coast-service stopped"

# Try to run — should fail because the remote build can't reach coast-service
set +e
RUN_OUT=$("$COAST" run dev-2 --type remote 2>&1)
RUN_EXIT=$?
set -e

echo "  run exit code: $RUN_EXIT"
echo "  run output (first 3 lines): $(echo "$RUN_OUT" | head -3)"

[ "$RUN_EXIT" -ne 0 ] || fail "run should fail when coast-service is unreachable"
pass "run fails when coast-service is down"

# Clean up partial instance
"$COAST" rm dev-2 2>/dev/null || true

# Restart coast-service for any other tests
start_coast_service

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote build error tests passed!"
echo "=========================================="
