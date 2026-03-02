#!/usr/bin/env bash
#
# Integration test for `coast restart-services`.
#
# Tests that restart-services tears down and brings back compose services
# (coast-demo) and bare services (coast-bare), and that autostart=false
# results in a teardown-only operation (coast-noautostart).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_restart_services.sh

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
# Test 1: restart-services on a compose project (coast-demo)
# ============================================================

echo ""
echo "=== Test 1: restart-services with compose (coast-demo) ==="

cd "$PROJECTS_DIR/coast-demo"

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed for coast-demo"; }
pass "coast-demo build succeeded"

RUN_OUTPUT=$($COAST run rs-compose 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed for coast-demo"; }
CLEANUP_INSTANCES+=("rs-compose")
pass "coast run rs-compose succeeded"

DYN_PORT=$(extract_dynamic_port "$RUN_OUTPUT" "app" | tail -1)
[ -n "$DYN_PORT" ] || fail "Could not extract app dynamic port"

wait_for_healthy "$DYN_PORT" 90 || fail "coast-demo did not become healthy"
pass "coast-demo is healthy before restart"

RESTART_OUTPUT=$($COAST restart-services rs-compose 2>&1) || { echo "$RESTART_OUTPUT"; fail "coast restart-services failed for coast-demo"; }
assert_contains "$RESTART_OUTPUT" "ok" "restart-services returned ok"
pass "coast restart-services rs-compose succeeded"

# Re-extract dynamic port after restart (port allocations persist)
PORTS_OUTPUT=$($COAST ports rs-compose 2>&1)
DYN_PORT_AFTER=$(echo "$PORTS_OUTPUT" | awk '$1 == "app" {print $3}')
[ -n "$DYN_PORT_AFTER" ] || { DYN_PORT_AFTER=$DYN_PORT; }

wait_for_healthy "$DYN_PORT_AFTER" 90 || fail "coast-demo did not become healthy after restart-services"
pass "coast-demo is healthy after restart-services"

# ============================================================
# Test 2: restart-services on a bare services project (coast-bare)
# ============================================================

echo ""
echo "=== Test 2: restart-services with bare services (coast-bare) ==="

cd "$PROJECTS_DIR/coast-bare"

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed for coast-bare"; }
pass "coast-bare build succeeded"

RUN_OUTPUT=$($COAST run rs-bare 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed for coast-bare"; }
CLEANUP_INSTANCES+=("rs-bare")
pass "coast run rs-bare succeeded"

BARE_DYN_PORT=$(extract_dynamic_port "$RUN_OUTPUT" "web" | tail -1)
[ -n "$BARE_DYN_PORT" ] || fail "Could not extract web dynamic port"

sleep 5

RESTART_OUTPUT=$($COAST restart-services rs-bare 2>&1) || { echo "$RESTART_OUTPUT"; fail "coast restart-services failed for coast-bare"; }
assert_contains "$RESTART_OUTPUT" "ok" "restart-services returned ok for bare"
pass "coast restart-services rs-bare succeeded"

# ============================================================
# Test 3: restart-services on a stopped instance should fail
# ============================================================

echo ""
echo "=== Test 3: restart-services on stopped instance fails ==="

cd "$PROJECTS_DIR/coast-demo"
$COAST stop rs-compose 2>&1 || true
sleep 2

RESTART_FAIL_OUTPUT=$($COAST restart-services rs-compose 2>&1) && {
    fail "restart-services should have failed on stopped instance"
} || true
assert_contains "$RESTART_FAIL_OUTPUT" "cannot have services restarted" "correct error for stopped instance"
pass "restart-services correctly rejected stopped instance"

# Restart the instance for cleanup
$COAST start rs-compose 2>&1 || true
sleep 5

# ============================================================
# Test 4: restart-services on nonexistent instance should fail
# ============================================================

echo ""
echo "=== Test 4: restart-services on nonexistent instance fails ==="

RESTART_NF_OUTPUT=$($COAST restart-services nonexistent-xyz 2>&1) && {
    fail "restart-services should have failed on nonexistent instance"
} || true
assert_contains "$RESTART_NF_OUTPUT" "not found" "correct error for missing instance"
pass "restart-services correctly rejected nonexistent instance"

# ============================================================
# Test 5: restart-services with autostart=false (coast-noautostart)
# ============================================================

echo ""
echo "=== Test 5: restart-services with autostart=false ==="

cd "$PROJECTS_DIR/coast-noautostart"

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed for coast-noautostart"; }
pass "coast-noautostart build succeeded"

# Since autostart=false, services won't start on coast run — instance will be Idle.
RUN_OUTPUT=$($COAST run rs-noauto 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed for coast-noautostart"; }
CLEANUP_INSTANCES+=("rs-noauto")
pass "coast run rs-noauto succeeded"

# With autostart=false the instance is Idle (no services running).
# restart-services should reject Idle instances since there's nothing to restart.
RESTART_OUTPUT=$($COAST restart-services rs-noauto 2>&1) && {
    fail "restart-services should have failed on idle instance"
} || true
assert_contains "$RESTART_OUTPUT" "cannot have services restarted" "correct error for idle (autostart=false) instance"
pass "restart-services correctly rejected idle (autostart=false) instance"

echo ""
echo "=== All restart-services tests passed ==="
