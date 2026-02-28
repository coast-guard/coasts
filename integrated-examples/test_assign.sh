#!/usr/bin/env bash
#
# Integration test: coast assign — seamless branch switching on a single slot.
#
# Demonstrates the core value proposition of `coast assign`: one running
# container, swap branches in ~5s without tearing down and rebuilding.
# Assign creates a git worktree at .coasts/<branch>/ and remounts /workspace.
#
# Per spec: checked-out instances cannot be assigned. The developer controls
# the branch directly via the host worktree. Run `coast checkout --none` first.
#
# Uses coast-benchmark (zero npm deps, single server.js, unique JSON per branch).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_assign.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

# Initialize examples — only 2 feature branches needed
COAST_BENCHMARK_COUNT=2 "$HELPERS_DIR/setup.sh"
pass "Examples initialized (feature-01, feature-02)"

cd "$PROJECTS_DIR/coast-benchmark"

# Start daemon
start_daemon

# Build
echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"

# ============================================================
# Test 1: Run slot-1 (defaults to current HEAD = main)
# ============================================================

echo ""
echo "=== Test 1: coast run slot-1 (auto-branch = main) ==="

RUN_OUT=$("$COAST" run slot-1 2>&1)
CLEANUP_INSTANCES+=("slot-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run slot-1 succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port for slot-1"
pass "Dynamic port: $DYN_PORT"

wait_for_healthy "$DYN_PORT" 60 || fail "slot-1 did not become healthy"
pass "slot-1 is healthy"

RESP=$(curl -s "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"main"' "slot-1 serves main branch initially"

# ============================================================
# Test 2: Checkout slot-1 (canonical port)
# ============================================================

echo ""
echo "=== Test 2: checkout slot-1 ==="

CO_OUT=$("$COAST" checkout slot-1 2>&1)
assert_contains "$CO_OUT" "Checked out coast instance" "checkout slot-1 succeeds"

sleep 1

CANON_RESP=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_RESP" '"feature":"main"' "canonical port 39000 returns main"

# ============================================================
# Test 3: Assign rejected while checked out (per spec)
# ============================================================

echo ""
echo "=== Test 3: assign rejected while checked out ==="

ASSIGN_REJECT=$("$COAST" assign slot-1 --worktree feature-01 2>&1 || true)
assert_contains "$ASSIGN_REJECT" "checked out" "assign error mentions 'checked out'"
pass "coast assign correctly rejected for checked-out instance"

# Verify instance is still serving main on canonical port
CANON_STILL=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_STILL" '"feature":"main"' "canonical port still serves main after rejected assign"

# ============================================================
# Test 4: Release checkout, then assign to feature-01
# ============================================================

echo ""
echo "=== Test 4: release checkout + assign to feature-01 ==="

CO_NONE=$("$COAST" checkout --none 2>&1)
assert_contains "$CO_NONE" "Unbound all canonical ports" "checkout --none succeeds"

ASSIGN_OUT=$("$COAST" assign slot-1 --worktree feature-01 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned branch" "coast assign to feature-01 succeeds"
assert_contains "$ASSIGN_OUT" "feature-01" "assign output references feature-01"

wait_for_healthy "$DYN_PORT" 60 || fail "slot-1 did not become healthy after assign to feature-01"
pass "slot-1 healthy after assign to feature-01"

RESP=$(curl -s "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"feature-01"' "slot-1 now returns feature-01 response"

# Verify /workspace reflects the assigned branch via worktree
WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "feature-01" "/workspace shows assigned branch feature-01"

# Verify worktree exists on host
[ -d ".coasts/feature-01" ] || fail ".coasts/feature-01 worktree not created"
pass "worktree .coasts/feature-01/ exists on host"

# ============================================================
# Test 5: Assign to feature-02 (second swap, still not checked out)
# ============================================================

echo ""
echo "=== Test 5: assign slot-1 to feature-02 (second swap) ==="

ASSIGN_OUT=$("$COAST" assign slot-1 --worktree feature-02 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned branch" "second assign succeeds"

wait_for_healthy "$DYN_PORT" 60 || fail "slot-1 did not become healthy after assign to feature-02"
pass "slot-1 healthy after assign to feature-02"

RESP=$(curl -s "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"feature-02"' "slot-1 now returns feature-02 response"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "feature-02" "/workspace shows assigned branch feature-02"

# ============================================================
# Test 6: Assign back to feature-01 (bidirectional)
# ============================================================

echo ""
echo "=== Test 6: assign back to feature-01 (bidirectional) ==="

ASSIGN_OUT=$("$COAST" assign slot-1 --worktree feature-01 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned branch" "third assign succeeds"

wait_for_healthy "$DYN_PORT" 60 || fail "slot-1 did not become healthy after assign back to feature-01"
pass "slot-1 healthy after assign back to feature-01"

RESP=$(curl -s "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"feature-01"' "slot-1 returns feature-01 again"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "feature-01" "/workspace shows feature-01 after bidirectional assign"

# ============================================================
# Test 7: Checkout after assign — canonical port reflects assigned branch
# ============================================================

echo ""
echo "=== Test 7: checkout after assign ==="

CO_AFTER=$("$COAST" checkout slot-1 2>&1)
assert_contains "$CO_AFTER" "Checked out coast instance" "checkout after assign succeeds"

sleep 1

CANON_AFTER=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_AFTER" '"feature":"feature-01"' "canonical port reflects assigned branch feature-01"

# Release checkout for next test
"$COAST" checkout --none 2>&1 >/dev/null || true

# ============================================================
# Test 8: coast ls shows correct branch and worktree
# ============================================================

echo ""
echo "=== Test 8: coast ls shows feature-01 ==="

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "slot-1" "coast ls shows slot-1"
assert_contains "$LS_OUT" "feature-01" "coast ls shows feature-01 as current branch"

# ============================================================
# Test 9: Dynamic port persists across all assigns
# ============================================================

echo ""
echo "=== Test 9: dynamic port unchanged ==="

PORTS_OUT=$("$COAST" ports slot-1 2>&1)
assert_contains "$PORTS_OUT" "$DYN_PORT" "dynamic port $DYN_PORT is still allocated after all assigns"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

cd "$PROJECTS_DIR/coast-benchmark"
git checkout main 2>/dev/null || true

"$COAST" rm slot-1 2>&1 | grep -q "Removed" || fail "coast rm slot-1 failed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  ALL ASSIGN TESTS PASSED"
echo "==========================================="
