#!/usr/bin/env bash
#
# Multi-project integration test for coast.
#
# Runs two coast projects simultaneously (coast-demo and coast-api) on the
# same machine. Verifies they don't interfere, that `coast ls` shows
# all instances with their project roots.
#
# Test 5 uses `coast assign` to switch an instance to feature-v2 and verifies
# the branch-specific response.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_multi_project.sh

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

start_daemon

# ============================================================
# Test 1: Build both projects
# ============================================================

echo ""
echo "=== Test 1: Build both projects ==="

cd "$PROJECTS_DIR/coast-demo"
DEMO_BUILD=$("$COAST" build 2>&1)
assert_contains "$DEMO_BUILD" "coast-demo" "coast-demo build succeeds"

cd "$PROJECTS_DIR/coast-api"
API_BUILD=$("$COAST" build 2>&1)
assert_contains "$API_BUILD" "coast-api" "coast-api build succeeds"

# ============================================================
# Test 2: Run instances from both projects simultaneously
# ============================================================

echo ""
echo "=== Test 2: Run instances from both projects ==="

# Run coast-demo main (app canonical port = 33000)
cd "$PROJECTS_DIR/coast-demo"
DEMO_RUN=$("$COAST" run main 2>&1)
CLEANUP_INSTANCES+=("main")
assert_contains "$DEMO_RUN" "Created coast instance" "coast-demo main started"
DEMO_PORT=$(extract_dynamic_port "$DEMO_RUN" "app")
[ -n "$DEMO_PORT" ] || fail "Could not extract coast-demo dynamic port"
pass "coast-demo main port: $DEMO_PORT"

# Run coast-api main (api canonical port = 34000)
cd "$PROJECTS_DIR/coast-api"
API_RUN=$("$COAST" run main 2>&1)
CLEANUP_INSTANCES+=("main")
assert_contains "$API_RUN" "Created coast instance" "coast-api main started"
API_PORT=$(extract_dynamic_port "$API_RUN" "api")
[ -n "$API_PORT" ] || fail "Could not extract coast-api dynamic port"
pass "coast-api main port: $API_PORT"

# Wait for both to be healthy
wait_for_healthy "$DEMO_PORT" 60 || fail "coast-demo did not become healthy"
pass "coast-demo is healthy"

wait_for_healthy "$API_PORT" 60 || fail "coast-api did not become healthy"
pass "coast-api is healthy"

# ============================================================
# Test 3: Both projects respond correctly simultaneously
# ============================================================

echo ""
echo "=== Test 3: Both projects respond simultaneously ==="

DEMO_RESP=$(curl -s "http://localhost:${DEMO_PORT}/")
assert_contains "$DEMO_RESP" "Hello from Coast!" "coast-demo returns its greeting"
assert_contains "$DEMO_RESP" '"branch":"main"' "coast-demo is on main branch"

API_RESP=$(curl -s "http://localhost:${API_PORT}/")
assert_contains "$API_RESP" "API Gateway Ready" "coast-api returns its status"
assert_contains "$API_RESP" '"branch":"main"' "coast-api is on main branch"

# ============================================================
# Test 4: coast ls shows both projects with roots
# ============================================================

echo ""
echo "=== Test 4: coast ls shows both projects ==="

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "coast-demo" "ls shows coast-demo project"
assert_contains "$LS_OUT" "coast-api" "ls shows coast-api project"
assert_contains "$LS_OUT" "ROOT" "ls shows ROOT column header"

# Both should show 'running'
DEMO_LINE=$(echo "$LS_OUT" | grep "coast-demo")
assert_contains "$DEMO_LINE" "running" "coast-demo instance is running"
API_LINE=$(echo "$LS_OUT" | grep "coast-api")
assert_contains "$API_LINE" "running" "coast-api instance is running"

echo ""
echo "  --- coast ls output ---"
echo "$LS_OUT"
echo "  -----------------------"

# ============================================================
# Test 5: Run a second coast-api instance and assign to feature-v2
# ============================================================

echo ""
echo "=== Test 5: Run + assign feature-v2 on coast-api ==="

cd "$PROJECTS_DIR/coast-api"
API_V2_RUN=$("$COAST" run feature-v2 2>&1)
CLEANUP_INSTANCES+=("feature-v2")
assert_contains "$API_V2_RUN" "Created coast instance" "coast-api feature-v2 slot started"
API_V2_PORT=$(extract_dynamic_port "$API_V2_RUN" "api")
[ -n "$API_V2_PORT" ] || fail "Could not extract coast-api feature-v2 port"

wait_for_healthy "$API_V2_PORT" 60 || fail "coast-api feature-v2 slot did not become healthy"
pass "coast-api feature-v2 slot is healthy (running main code)"

# Now assign to feature-v2 branch — builds from branch code via git archive
ASSIGN_OUT=$("$COAST" assign feature-v2 --worktree feature-v2 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned worktree" "assign to feature-v2 succeeded"

wait_for_healthy "$API_V2_PORT" 60 || fail "coast-api feature-v2 did not become healthy after assign"
pass "coast-api feature-v2 is healthy after assign"

API_V2_RESP=$(curl -s "http://localhost:${API_V2_PORT}/")
assert_contains "$API_V2_RESP" "API Gateway V2" "feature-v2 returns v2 message after assign"
assert_contains "$API_V2_RESP" '"branch":"feature-v2"' "feature-v2 is on correct branch"

# Original instances still work
DEMO_CHECK=$(curl -s "http://localhost:${DEMO_PORT}/")
assert_contains "$DEMO_CHECK" "Hello from Coast!" "coast-demo still responds"
API_CHECK=$(curl -s "http://localhost:${API_PORT}/")
assert_contains "$API_CHECK" "API Gateway Ready" "coast-api main still responds"

# ============================================================
# Test 6: coast ls shows all 3 instances across 2 projects
# ============================================================

echo ""
echo "=== Test 6: coast ls with 3 instances, 2 projects ==="

LS_OUT2=$("$COAST" ls 2>&1)

# Count instances
INSTANCE_COUNT=$(echo "$LS_OUT2" | grep -c "running")
[ "$INSTANCE_COUNT" -eq 3 ] || fail "Expected 3 running instances, got $INSTANCE_COUNT"
pass "3 running instances across 2 projects"

echo ""
echo "  --- coast ls output ---"
echo "$LS_OUT2"
echo "  -----------------------"

# ============================================================
# Test 7: Cleanup
# ============================================================

echo ""
echo "=== Test 7: Cleanup ==="

cd "$PROJECTS_DIR/coast-demo"
"$COAST" rm main 2>&1 | grep -q "Removed" || fail "coast rm coast-demo/main failed"
pass "coast-demo main removed"

cd "$PROJECTS_DIR/coast-api"
"$COAST" rm main 2>&1 | grep -q "Removed" || fail "coast rm coast-api/main failed"
pass "coast-api main removed"

"$COAST" rm feature-v2 2>&1 | grep -q "Removed" || fail "coast rm coast-api/feature-v2 failed"
pass "coast-api feature-v2 removed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL MULTI-PROJECT TESTS PASSED"
echo "==========================================="
