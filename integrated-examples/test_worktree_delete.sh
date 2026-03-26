#!/usr/bin/env bash
#
# Integration test: auto-unassign when a worktree is deleted.
#
# Verifies that when a worktree directory is removed from the host
# while an instance is assigned to it, the daemon's git watcher
# automatically unassigns the instance back to the default branch.
#
# Uses coast-bare (bare services, main + feature-v2 branches).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_worktree_delete.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-bare"

start_daemon

# ============================================================
# Test 1: Build and run
# ============================================================

echo ""
echo "=== Test 1: Build and run ==="

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed"; }
pass "coast build succeeded"

RUN_OUTPUT=$($COAST run test-del 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed"; }
CLEANUP_INSTANCES+=("test-del")
pass "coast run test-del succeeded"

sleep 3

# ============================================================
# Test 2: Assign to feature-v2
# ============================================================

echo ""
echo "=== Test 2: Assign to feature-v2 ==="

ASSIGN_OUT=$($COAST assign test-del --worktree feature-v2 2>&1) || { echo "$ASSIGN_OUT"; fail "coast assign failed"; }
assert_contains "$ASSIGN_OUT" "feature-v2" "assign output references feature-v2"
pass "assigned to feature-v2"

sleep 3

SERVERJS=$($COAST exec test-del -- cat /workspace/server.js 2>&1)
assert_contains "$SERVERJS" "v2" "workspace has v2 server.js"
pass "workspace verified on feature-v2"

# ============================================================
# Test 3: Delete worktree directory, wait for auto-unassign
# ============================================================

echo ""
echo "=== Test 3: Delete worktree and wait for auto-unassign ==="

rm -rf .worktrees/feature-v2
pass "deleted .coasts/feature-v2 from host"

# The git watcher polls every 2 seconds. On macOS, deleting the worktree
# directory can make the inner daemon transiently unhealthy, so the watcher
# may need to restart the DinD container before retrying the unassign.
# Allow enough time for: detection (2s) + initial delay (2s) + first attempt
# + container restart + inner daemon recovery + second attempt.
MAX_WAIT=45
WAITED=0
UNASSIGNED=false

while [ "$WAITED" -lt "$MAX_WAIT" ]; do
  sleep 1
  WAITED=$((WAITED + 1))
  LS_OUT=$($COAST ls 2>&1)
  if echo "$LS_OUT" | grep -q "test-del" && echo "$LS_OUT" | grep -q "main"; then
    UNASSIGNED=true
    break
  fi
done

if [ "$UNASSIGNED" = "true" ]; then
  pass "instance auto-unassigned to main after ${WAITED}s"
else
  echo "  coast ls output: $LS_OUT"
  fail "instance was not auto-unassigned within ${MAX_WAIT}s"
fi

# ============================================================
# Test 4: Verify workspace reverted to main
# ============================================================

echo ""
echo "=== Test 4: Verify workspace is back on main ==="

sleep 3

SERVERJS_MAIN=$($COAST exec test-del -- cat /workspace/server.js 2>&1)
assert_not_contains "$SERVERJS_MAIN" "v2" "workspace no longer has v2 code"
pass "workspace files reverted to main"

# ============================================================
# Test 5: Remove
# ============================================================

echo ""
echo "=== Test 5: Remove instance ==="

RM_OUTPUT=$($COAST rm test-del 2>&1) || { echo "$RM_OUTPUT"; fail "coast rm failed"; }
pass "coast rm succeeded"

LS_FINAL=$($COAST ls 2>&1)
assert_not_contains "$LS_FINAL" "test-del" "instance removed from listing"
pass "instance removed"

# ============================================================
# Summary
# ============================================================

echo ""
echo "=== All worktree delete tests passed! ==="
