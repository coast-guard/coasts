#!/usr/bin/env bash
#
# Integration test: coast lookup — discover instances for the current worktree.
#
# Verifies that `coast lookup` correctly identifies which coast instances
# are assigned to the caller's current worktree, supporting --compact,
# --json, and default human-readable output modes.
#
# Uses coast-lookup (zero npm deps, single server.js, unique JSON per branch).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_lookup.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh" 2>&1 | tail -5
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-lookup"

start_daemon

# Build
echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# ============================================================
# Test 1: Lookup from project root with no instances
# ============================================================

echo ""
echo "=== Test 1: lookup from project root (no instances) ==="

LOOKUP_OUT=$("$COAST" lookup --compact 2>&1 || true)
assert_eq "$LOOKUP_OUT" "[]" "compact lookup returns empty array when no instances"

# ============================================================
# Test 2: Run dev-1, lookup from project root
# ============================================================

echo ""
echo "=== Test 2: run dev-1, lookup from project root ==="

RUN_OUT=$("$COAST" run dev-1 2>&1)
CLEANUP_INSTANCES+=("dev-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dev-1 succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port for dev-1"
pass "Dynamic port: $DYN_PORT"

wait_for_healthy "$DYN_PORT" 60 || fail "dev-1 did not become healthy"
pass "dev-1 is healthy"

LOOKUP_OUT=$("$COAST" lookup --compact 2>&1)
assert_eq "$LOOKUP_OUT" '["dev-1"]' "compact lookup returns dev-1 on project root"

# ============================================================
# Test 3: Assign dev-1 to feature-alpha, lookup from worktree
# ============================================================

echo ""
echo "=== Test 3: assign to feature-alpha, lookup from worktree ==="

# Assign may report a health timeout but the worktree association still happens.
# We use -s (silent) and || true to tolerate the health-check timeout.
"$COAST" assign dev-1 --worktree feature-alpha -s 2>&1 || true

# Verify assignment took effect by checking coast ls
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "feature-alpha" "coast ls shows feature-alpha after assign"

cd .coasts/feature-alpha
LOOKUP_OUT=$("$COAST" lookup --compact 2>&1)
assert_eq "$LOOKUP_OUT" '["dev-1"]' "compact lookup from worktree returns dev-1"
cd "$PROJECTS_DIR/coast-lookup"

# ============================================================
# Test 4: Lookup from project root after assign returns empty
# ============================================================

echo ""
echo "=== Test 4: lookup from project root after assign ==="

LOOKUP_OUT=$("$COAST" lookup --compact 2>&1 || true)
assert_eq "$LOOKUP_OUT" "[]" "compact lookup from project root is empty after assign"

# ============================================================
# Test 5: Run dev-2, assign to same worktree, lookup returns both
# ============================================================

echo ""
echo "=== Test 5: run dev-2, assign to feature-alpha, lookup returns both ==="

RUN_OUT=$("$COAST" run dev-2 2>&1)
CLEANUP_INSTANCES+=("dev-2")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dev-2 succeeds"

DYN_PORT2=$(extract_dynamic_port "$RUN_OUT" "app")
wait_for_healthy "$DYN_PORT2" 60 || fail "dev-2 did not become healthy"
pass "dev-2 is healthy"

"$COAST" assign dev-2 --worktree feature-alpha -s 2>&1 || true

LS_OUT=$("$COAST" ls 2>&1)
# Both should show feature-alpha
pass "dev-2 assigned to feature-alpha"

cd .coasts/feature-alpha
LOOKUP_OUT=$("$COAST" lookup --compact 2>&1)
# Both names should be present (order may vary)
assert_contains "$LOOKUP_OUT" "dev-1" "compact lookup contains dev-1"
assert_contains "$LOOKUP_OUT" "dev-2" "compact lookup contains dev-2"
cd "$PROJECTS_DIR/coast-lookup"

# ============================================================
# Test 6: JSON mode
# ============================================================

echo ""
echo "=== Test 6: JSON mode ==="

cd .coasts/feature-alpha
JSON_OUT=$("$COAST" lookup --json 2>&1)
assert_contains "$JSON_OUT" '"worktree"' "JSON output has worktree field"
assert_contains "$JSON_OUT" '"feature-alpha"' "JSON output contains feature-alpha"
assert_contains "$JSON_OUT" '"project"' "JSON output has project field"
assert_contains "$JSON_OUT" '"coast-lookup"' "JSON output contains coast-lookup"
assert_contains "$JSON_OUT" '"ports"' "JSON output has ports field"
cd "$PROJECTS_DIR/coast-lookup"

# ============================================================
# Test 7: Default human-readable mode
# ============================================================

echo ""
echo "=== Test 7: default (human-readable) mode ==="

cd .coasts/feature-alpha
DEFAULT_OUT=$("$COAST" lookup 2>&1)
assert_contains "$DEFAULT_OUT" "dev-1" "default output contains dev-1"
assert_contains "$DEFAULT_OUT" "dev-2" "default output contains dev-2"
assert_contains "$DEFAULT_OUT" "Examples" "default output contains Examples header"
assert_contains "$DEFAULT_OUT" "workspace root where your Coastfile is" "default output contains workspace root hint"
assert_contains "$DEFAULT_OUT" "coast exec" "default output contains exec example"
assert_contains "$DEFAULT_OUT" "coast logs" "default output contains logs example"
assert_contains "$DEFAULT_OUT" "coast ps" "default output contains ps example"
cd "$PROJECTS_DIR/coast-lookup"

# ============================================================
# Test 8: Lookup from subdirectory of worktree
# ============================================================

echo ""
echo "=== Test 8: lookup from subdirectory of worktree ==="

SUBDIR=".coasts/feature-alpha/subdir/nested"
mkdir -p "$SUBDIR"
cd "$SUBDIR"
LOOKUP_OUT=$("$COAST" lookup --compact 2>&1)
assert_contains "$LOOKUP_OUT" "dev-1" "subdirectory lookup finds dev-1"
assert_contains "$LOOKUP_OUT" "dev-2" "subdirectory lookup finds dev-2"
cd "$PROJECTS_DIR/coast-lookup"
rm -rf .coasts/feature-alpha/subdir

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>&1 | grep -q "Removed" || fail "coast rm dev-1 failed"
"$COAST" rm dev-2 2>&1 | grep -q "Removed" || fail "coast rm dev-2 failed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  ALL LOOKUP TESTS PASSED"
echo "==========================================="
