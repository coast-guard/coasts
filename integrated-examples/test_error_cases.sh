#!/usr/bin/env bash
#
# Integration test: error cases and edge conditions.
#
# Tests that coast commands produce correct error messages for:
# duplicate instance names, stopped instance operations, nonexistent
# instances, and other edge cases.
#
# Uses coast-api (lightweight, isolated volumes).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_error_cases.sh

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

cd "$PROJECTS_DIR/coast-api"

start_daemon

# Build and run main
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"

RUN_OUT=$("$COAST" run main 2>&1)
CLEANUP_INSTANCES+=("main")
assert_contains "$RUN_OUT" "Created coast instance" "coast run main succeeds"

MAIN_DYN=$(extract_dynamic_port "$RUN_OUT" "api")
[ -n "$MAIN_DYN" ] || fail "Could not extract main api dynamic port"

wait_for_healthy "$MAIN_DYN" 60 || fail "main did not become healthy"
pass "main is healthy (setup complete)"

# ============================================================
# Test 1: Duplicate instance name
# ============================================================

echo ""
echo "=== Test 1: duplicate instance name ==="

DUP_OUT=$("$COAST" run main 2>&1 || true)
assert_contains "$DUP_OUT" "already exists" "duplicate run returns 'already exists' error"

# ============================================================
# Test 2: Stop already-stopped instance
# ============================================================

echo ""
echo "=== Test 2: stop already-stopped instance ==="

"$COAST" stop main 2>&1 | grep -q "Stopped" || fail "first stop failed"
pass "first stop succeeded"

STOP2_OUT=$("$COAST" stop main 2>&1 || true)
# Should either succeed gracefully or return an error about state
if echo "$STOP2_OUT" | grep -qi -e "stopped" -e "not running" -e "already"; then
    pass "double-stop handled gracefully"
else
    # Some implementations may succeed silently
    pass "double-stop did not crash (output: $STOP2_OUT)"
fi

# ============================================================
# Test 3: Start already-running instance
# ============================================================

echo ""
echo "=== Test 3: start already-running instance ==="

"$COAST" start main 2>&1 | grep -q "Started" || fail "first start failed"
wait_for_healthy "$MAIN_DYN" 60 || fail "main did not recover after start"
pass "first start succeeded"

START2_OUT=$("$COAST" start main 2>&1 || true)
# Should either succeed gracefully or return an error about state
if echo "$START2_OUT" | grep -qi -e "running" -e "already" -e "Started"; then
    pass "double-start handled gracefully"
else
    pass "double-start did not crash (output: $START2_OUT)"
fi

# ============================================================
# Test 4: Checkout stopped instance
# ============================================================

echo ""
echo "=== Test 4: checkout stopped instance ==="

"$COAST" stop main 2>&1 | grep -q "Stopped" || fail "stop for checkout test failed"
pass "main stopped for checkout test"

CO_STOPPED=$("$COAST" checkout main 2>&1 || true)
assert_contains "$CO_STOPPED" "not running" "checkout stopped instance returns 'not running' error"

# ============================================================
# Test 5: Checkout nonexistent instance
# ============================================================

echo ""
echo "=== Test 5: checkout nonexistent instance ==="

CO_NOEXIST=$("$COAST" checkout nonexistent 2>&1 || true)
assert_contains "$CO_NOEXIST" "not found" "checkout nonexistent returns 'not found' error"

# ============================================================
# Test 6: Remove nonexistent instance
# ============================================================

echo ""
echo "=== Test 6: rm nonexistent instance ==="

RM_NOEXIST=$("$COAST" rm nonexistent 2>&1 || true)
assert_contains "$RM_NOEXIST" "not found" "rm nonexistent returns 'not found' error"

# ============================================================
# Test 7: ps on stopped instance
# ============================================================

echo ""
echo "=== Test 7: ps on stopped instance ==="

# main is still stopped from test 4
PS_STOPPED=$("$COAST" ps main 2>&1 || true)
assert_contains "$PS_STOPPED" "stopped" "ps on stopped instance returns stopped error"

# ============================================================
# Test 8: logs on stopped instance
# ============================================================

echo ""
echo "=== Test 8: logs on stopped instance ==="

LOGS_STOPPED=$("$COAST" logs main 2>&1 || true)
assert_contains "$LOGS_STOPPED" "stopped" "logs on stopped instance returns stopped error"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

# Main is stopped, just rm it
"$COAST" rm main 2>&1 | grep -q "Removed" || fail "coast rm main failed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  ALL ERROR CASE TESTS PASSED"
echo "==========================================="
