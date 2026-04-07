#!/usr/bin/env bash
#
# Integration test: rebuild assign strategy on remote coasts.
#
# Tests that assigning with strategy "rebuild" rebuilds the Docker image
# and restarts the service with the new image. The feature branch changes
# version.txt (a rebuild trigger), so the image is rebuilt on assign.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_rebuild.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_rebuild_cleanup() {
    echo ""
    echo "--- Cleaning up rebuild test ---"

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
trap '_rebuild_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Rebuild Assign Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-rebuild"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Run instance on main
# ============================================================

echo ""
echo "=== Test 1: Run on main ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
set -e

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running on main"

sleep 5

# Verify main version
set +e
MAIN_VER=$("$COAST" exec dev-1 -- cat /workspace/app/version.txt 2>&1)
set -e
assert_contains "$MAIN_VER" "v1-main" "Main version confirmed on remote"

# ============================================================
# Test 2: Rebuild assign to feature branch
# ============================================================

echo ""
echo "=== Test 2: Rebuild assign to feature-rebuild-test ==="

set +e
ASSIGN_OUT=$("$COAST" assign dev-1 -w feature-rebuild-test 2>&1)
ASSIGN_EXIT=$?
set -e
echo "  assign exit: $ASSIGN_EXIT"
[ "$ASSIGN_EXIT" -eq 0 ] || { echo "$ASSIGN_OUT"; fail "Assign failed (exit $ASSIGN_EXIT)"; }
pass "Assigned to feature-rebuild-test"

sleep 8

# Verify version.txt changed on remote
set +e
FEATURE_VER=$("$COAST" exec dev-1 -- cat /workspace/app/version.txt 2>&1)
set -e
assert_contains "$FEATURE_VER" "v2-feature" "Feature version confirmed on remote after rebuild assign"

# Verify coast ls shows worktree
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "feature-rebuild-test" "coast ls shows worktree after rebuild assign"

# Verify services are healthy
set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
[ "$PS_EXIT" -eq 0 ] || fail "ps failed after rebuild assign"
pass "Services healthy after rebuild assign"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

echo ""
echo "=========================================="
echo "  All remote rebuild tests passed!"
echo "=========================================="
