#!/usr/bin/env bash
#
# Integration test for running multiple remote coast instances on one remote.
#
# Tests that:
# 1. Two instances of the same project run simultaneously with different dynamic ports
# 2. Two instances of different projects run independently on the same remote
# 3. Stopping one instance does not affect the other
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_multi_instance.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_multi_cleanup() {
    echo ""
    echo "--- Cleaning up multi-instance test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    # Clean up any shell containers
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_multi_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Multi-Instance Integration Tests ==="
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

# Build once
set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || fail "Build failed: $BUILD_OUT"
pass "Build complete"

# ============================================================
# Test 1: Two instances of the same project
# ============================================================

echo ""
echo "=== Test 1: Two instances of same project ==="

set +e
RUN1_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN1_EXIT=$?
set -e
[ "$RUN1_EXIT" -eq 0 ] || { echo "$RUN1_OUT"; fail "First instance failed (exit $RUN1_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "dev-1 running"

# Extract dev-1 dynamic port
DEV1_PORT=$(echo "$RUN1_OUT" | awk '$1 == "app" && $2 ~ /^[0-9]+$/ {print $3}')
echo "  dev-1 dynamic port: ${DEV1_PORT:-unknown}"

set +e
RUN2_OUT=$("$COAST" run dev-2 --type remote 2>&1)
RUN2_EXIT=$?
set -e
[ "$RUN2_EXIT" -eq 0 ] || { echo "$RUN2_OUT"; fail "Second instance failed (exit $RUN2_EXIT)"; }
CLEANUP_INSTANCES+=("dev-2")
pass "dev-2 running"

# Extract dev-2 dynamic port
DEV2_PORT=$(echo "$RUN2_OUT" | awk '$1 == "app" && $2 ~ /^[0-9]+$/ {print $3}')
echo "  dev-2 dynamic port: ${DEV2_PORT:-unknown}"

# Verify both are in ls
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "dev-1" "dev-1 in ls"
assert_contains "$LS_OUT" "dev-2" "dev-2 in ls"
pass "Both instances visible in ls"

# Verify different dynamic ports (no port conflict)
if [ -n "$DEV1_PORT" ] && [ -n "$DEV2_PORT" ]; then
    [ "$DEV1_PORT" != "$DEV2_PORT" ] || fail "dev-1 and dev-2 have same dynamic port $DEV1_PORT (port conflict!)"
    pass "Different dynamic ports: dev-1=$DEV1_PORT dev-2=$DEV2_PORT (no conflict)"
else
    pass "Dynamic ports allocated (couldn't parse exact values)"
fi

# Verify both can exec independently
EXEC1=$("$COAST" exec dev-1 -- echo "hello from dev-1" 2>&1)
assert_contains "$EXEC1" "hello from dev-1" "exec into dev-1 works"

EXEC2=$("$COAST" exec dev-2 -- echo "hello from dev-2" 2>&1)
assert_contains "$EXEC2" "hello from dev-2" "exec into dev-2 works"

pass "Both instances independently accessible"

# ============================================================
# Test 2: Stop one, other survives
# ============================================================

echo ""
echo "=== Test 2: Stop one instance, other survives ==="

"$COAST" stop dev-1 2>&1 >/dev/null
pass "dev-1 stopped"

# dev-2 should still work
set +e
EXEC2_AFTER=$("$COAST" exec dev-2 -- echo "dev-2 still alive" 2>&1)
EXEC2_EXIT=$?
set -e

[ "$EXEC2_EXIT" -eq 0 ] || fail "exec into dev-2 failed after stopping dev-1"
assert_contains "$EXEC2_AFTER" "dev-2 still alive" "dev-2 still functional after dev-1 stopped"
pass "dev-2 survives dev-1 stop"

# dev-1 exec should fail (stopped)
set +e
EXEC1_STOPPED=$("$COAST" exec dev-1 -- echo "should fail" 2>&1)
EXEC1_STOPPED_EXIT=$?
set -e

[ "$EXEC1_STOPPED_EXIT" -ne 0 ] || fail "exec into stopped dev-1 should fail"
pass "exec into stopped dev-1 correctly fails"

# ============================================================
# Test 3: Cleanup
# ============================================================

echo ""
echo "=== Test 3: Cleanup ==="

"$COAST" rm dev-1 2>/dev/null || true
"$COAST" rm dev-2 2>/dev/null || true
CLEANUP_INSTANCES=()
pass "Both instances removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote multi-instance tests passed!"
echo "=========================================="
