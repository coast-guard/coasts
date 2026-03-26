#!/usr/bin/env bash
#
# Integration test: worktree-based branch switching and file sync.
#
# Verifies that `coast assign` creates git worktrees, that /workspace inside
# the DinD container reflects the correct branch, and that edits inside the
# container are visible on the host worktree (host-bound via bind mount).
#
# Uses coast-benchmark (zero npm deps, single server.js, unique JSON per branch).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_sync.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

# Only need coast-benchmark (2 branches)
COAST_BENCHMARK_COUNT=2 "$HELPERS_DIR/setup.sh" 2>&1 | tail -3
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-benchmark"

start_daemon

# Build
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# ============================================================
# Test A: Run instance — defaults to main, /workspace shows host files
# ============================================================

echo ""
echo "=== Test A: coast run dev-slot (defaults to main) ==="

RUN_OUT=$("$COAST" run dev-slot 2>&1)
CLEANUP_INSTANCES+=("dev-slot")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dev-slot succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
wait_for_healthy "$DYN_PORT" 60 || fail "dev-slot did not become healthy"
pass "dev-slot healthy on port $DYN_PORT"

# Verify coast ls shows main branch
LS_AFTER_RUN=$("$COAST" ls 2>&1)
assert_contains "$LS_AFTER_RUN" "main" "coast ls shows main branch after run"

# Verify /workspace inside DinD has the host project files
INNER_FILES=$("$COAST" exec dev-slot -- ls /workspace/server.js 2>&1)
assert_contains "$INNER_FILES" "server.js" "/workspace contains server.js"

INNER_BRANCH=$("$COAST" exec dev-slot -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$INNER_BRANCH" "main" "/workspace is on main branch"

# Verify the HTTP response matches main
RESP=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"main"' "dev-slot serves main branch response"

# ============================================================
# Test B: Assign to feature-01 — worktree created, /workspace switches
# ============================================================

echo ""
echo "=== Test B: coast assign dev-slot --worktree feature-01 ==="

ASSIGN_OUT=$("$COAST" assign dev-slot --worktree feature-01 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned worktree" "assign to feature-01 succeeds"
wait_for_healthy "$DYN_PORT" 60 || fail "dev-slot not healthy after assign"
pass "dev-slot healthy after assign to feature-01"

# Verify worktree directory exists on host
[ -d ".worktrees/feature-01" ] || fail ".worktrees/feature-01 worktree directory not created"
pass "worktree .worktrees/feature-01/ exists on host"

# Verify /workspace inside DinD shows feature-01
INNER_BRANCH=$("$COAST" exec dev-slot -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$INNER_BRANCH" "feature-01" "/workspace shows feature-01 branch"

# Verify coast ls shows feature-01 and WORKTREE column
LS_AFTER_ASSIGN=$("$COAST" ls 2>&1)
assert_contains "$LS_AFTER_ASSIGN" "feature-01" "coast ls shows feature-01 branch"

# Verify HTTP response matches feature-01
RESP=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"feature-01"' "dev-slot serves feature-01 response"

# ============================================================
# Test C: HMR — write inside DinD /workspace, verify host worktree sees it
# ============================================================

echo ""
echo "=== Test C: HMR — edits inside DinD visible on host worktree ==="

"$COAST" exec dev-slot -- sh -c "echo 'hmr-test-marker' > /workspace/hmr-test-file.txt" 2>&1

# The worktree on the host should have the file (host-bound)
[ -f ".worktrees/feature-01/hmr-test-file.txt" ] || fail "hmr-test-file.txt should exist in host worktree"
HOST_CONTENT=$(cat .worktrees/feature-01/hmr-test-file.txt)
assert_eq "$HOST_CONTENT" "hmr-test-marker" "DinD write visible on host worktree"
pass "HMR sync confirmed: DinD → host worktree"

# Also verify the reverse: write on host worktree, visible in DinD
echo "host-written" > .worktrees/feature-01/host-marker.txt
INNER_CONTENT=$("$COAST" exec dev-slot -- cat /workspace/host-marker.txt 2>&1)
assert_eq "$INNER_CONTENT" "host-written" "host write visible inside DinD"
pass "Bidirectional sync confirmed: host worktree ↔ DinD"

# Clean up test files
rm -f .worktrees/feature-01/hmr-test-file.txt .worktrees/feature-01/host-marker.txt

# ============================================================
# Test D: Assign back to main — /workspace switches back
# ============================================================

echo ""
echo "=== Test D: coast assign dev-slot --worktree main ==="

ASSIGN_BACK=$("$COAST" assign dev-slot --worktree main 2>&1)
assert_contains "$ASSIGN_BACK" "Assigned worktree" "assign back to main succeeds"
wait_for_healthy "$DYN_PORT" 60 || fail "dev-slot not healthy after assign back to main"
pass "dev-slot healthy after assign back to main"

INNER_BRANCH=$("$COAST" exec dev-slot -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$INNER_BRANCH" "main" "/workspace shows main branch after reassign"

RESP=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"main"' "dev-slot serves main response after reassign"

# ============================================================
# Test E: Stop + start — /workspace still correct
# ============================================================

echo ""
echo "=== Test E: stop + start preserves workspace ==="

STOP_OUT=$("$COAST" stop dev-slot 2>&1)
assert_contains "$STOP_OUT" "Stopped" "coast stop succeeds"

START_OUT=$("$COAST" start dev-slot 2>&1)
[ $? -eq 0 ] || fail "coast start failed"

wait_for_healthy "$DYN_PORT" 60 || fail "dev-slot not healthy after start"
pass "dev-slot healthy after stop+start"

INNER_BRANCH=$("$COAST" exec dev-slot -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$INNER_BRANCH" "main" "/workspace still on main after stop+start"

RESP=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"feature":"main"' "dev-slot still serves main after stop+start"

# ============================================================
# Test F: Remove — cleanup worktrees and container
# ============================================================

echo ""
echo "=== Test F: coast rm cleans up ==="

RM_OUT=$("$COAST" rm dev-slot 2>&1)
assert_contains "$RM_OUT" "Removed" "coast rm dev-slot succeeds"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "no instances remaining"

# Verify the coast container is gone
CONTAINERS=$(docker ps -aq --filter "label=coast.managed=true" 2>/dev/null || true)
[ -z "$CONTAINERS" ] || fail "coast container still running after rm"
pass "container cleaned up"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

git checkout main 2>/dev/null || true
git checkout -- . 2>/dev/null || true
git clean -fd 2>/dev/null || true

echo ""
echo "==========================================="
echo "  ALL SYNC TESTS PASSED"
echo "==========================================="
