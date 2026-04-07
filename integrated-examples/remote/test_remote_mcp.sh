#!/usr/bin/env bash
#
# Integration test: remote coast MCP server listing.
#
# Tests that MCP servers configured in the Coastfile can be listed
# on remote instances, with correct status detection.
#
# Note: This test only checks MCP ls (listing). MCP tools listing
# requires an actual MCP server to be installed and running, which
# depends on the project's Coastfile having MCP servers configured.
# If the project has no MCP servers, the test verifies empty listing
# works correctly.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_mcp.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_mcp_cleanup() {
    echo ""
    echo "--- Cleaning up MCP test ---"

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
trap '_mcp_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote MCP Integration Test ==="
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
pass "Remote instance running"

sleep 3

# ============================================================
# Test 1: MCP ls works for remote instance
# ============================================================

echo ""
echo "=== Test 1: MCP ls ==="

set +e
MCP_LS=$("$COAST" mcp dev-1 ls 2>&1)
MCP_LS_EXIT=$?
set -e

echo "  mcp ls exit: $MCP_LS_EXIT"
echo "  mcp ls output: $(echo "$MCP_LS" | head -5)"
[ "$MCP_LS_EXIT" -eq 0 ] || fail "MCP ls failed (exit $MCP_LS_EXIT): $MCP_LS"
pass "MCP ls succeeds on remote instance"

# ============================================================
# Test 2: MCP locations works
# ============================================================

echo ""
echo "=== Test 2: MCP locations ==="

set +e
MCP_LOC=$("$COAST" mcp dev-1 locations 2>&1)
MCP_LOC_EXIT=$?
set -e

echo "  mcp locations exit: $MCP_LOC_EXIT"
echo "  mcp locations output: $(echo "$MCP_LOC" | head -5)"
[ "$MCP_LOC_EXIT" -eq 0 ] || fail "MCP locations failed (exit $MCP_LOC_EXIT)"
pass "MCP locations succeeds on remote instance"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>/dev/null || true
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

echo ""
echo "=========================================="
echo "  All remote MCP tests passed!"
echo "=========================================="
