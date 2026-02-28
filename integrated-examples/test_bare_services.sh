#!/usr/bin/env bash
#
# Integration test for bare process services (no Docker Compose).
#
# Tests the coast-bare example which uses [services] to run
# a Node.js server as a supervised process inside a DinD container.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_bare_services.sh

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
# Test 1: Build (bare services project)
# ============================================================

echo ""
echo "=== Test 1: Build bare services project ==="

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed"; }
assert_contains "$BUILD_OUTPUT" "coast-bare" "build output mentions project name"
pass "coast build succeeded for bare services project"

# ============================================================
# Test 2: Run — services auto-start, status is Running
# ============================================================

echo ""
echo "=== Test 2: Run with auto-started bare services ==="

RUN_OUTPUT=$($COAST run test-bare 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed"; }
pass "coast run test-bare succeeded"

# Give services a moment to start
sleep 3

LS_OUTPUT=$($COAST ls 2>&1)
assert_contains "$LS_OUTPUT" "test-bare" "instance listed"
# The instance should be Running (not Idle) because it has [services]
assert_contains "$LS_OUTPUT" "running" "instance status is running"
pass "instance is running with bare services"

# ============================================================
# Test 3: PS — shows service status
# ============================================================

echo ""
echo "=== Test 3: PS shows bare service status ==="

PS_OUTPUT=$($COAST ps test-bare 2>&1) || { echo "$PS_OUTPUT"; fail "coast ps failed"; }
assert_contains "$PS_OUTPUT" "web" "ps shows web service"
assert_contains "$PS_OUTPUT" "running" "web service is running"
pass "coast ps shows running bare services"

# ============================================================
# Test 4: Logs — shows service output
# ============================================================

echo ""
echo "=== Test 4: Logs from bare services ==="

LOGS_OUTPUT=$($COAST logs test-bare 2>&1) || { echo "$LOGS_OUTPUT"; fail "coast logs failed"; }
assert_contains "$LOGS_OUTPUT" "listening" "logs contain server startup message"
pass "coast logs shows bare service output"

# ============================================================
# Test 5: Ports — dynamic port accessible
# ============================================================

echo ""
echo "=== Test 5: Dynamic port access ==="

PORTS_OUTPUT=$($COAST ports test-bare 2>&1) || { echo "$PORTS_OUTPUT"; fail "coast ports failed"; }
assert_contains "$PORTS_OUTPUT" "web" "ports shows web service"
assert_contains "$PORTS_OUTPUT" "40000" "ports shows canonical port"
DYNAMIC_PORT=$(extract_dynamic_port "$RUN_OUTPUT" "web")

if [ -n "$DYNAMIC_PORT" ]; then
  # Try to reach the server via dynamic port
  sleep 1
  CURL_OUTPUT=$(curl -s --max-time 5 "http://localhost:${DYNAMIC_PORT}" 2>&1) || true
  if echo "$CURL_OUTPUT" | grep -q "Hello from Coast"; then
    pass "server reachable via dynamic port ${DYNAMIC_PORT}"
  else
    echo "curl output: $CURL_OUTPUT"
    fail "server not reachable via dynamic port ${DYNAMIC_PORT}"
  fi
else
  fail "could not extract dynamic port for web service"
fi

# ============================================================
# Test 6: Exec still works
# ============================================================

echo ""
echo "=== Test 6: Exec into bare services instance ==="

EXEC_OUTPUT=$($COAST exec test-bare -- node --version 2>&1) || { echo "$EXEC_OUTPUT"; fail "exec failed"; }
assert_contains "$EXEC_OUTPUT" "v" "node version output"
pass "coast exec works in bare services instance"

# ============================================================
# Test 7: Stop and start
# ============================================================

echo ""
echo "=== Test 7: Stop and start ==="

STOP_OUTPUT=$($COAST stop test-bare 2>&1) || { echo "$STOP_OUTPUT"; fail "coast stop failed"; }
pass "coast stop succeeded"

START_OUTPUT=$($COAST start test-bare 2>&1) || { echo "$START_OUTPUT"; fail "coast start failed"; }
pass "coast start succeeded"

# Give services a moment to restart
sleep 3

LS_OUTPUT2=$($COAST ls 2>&1)
assert_contains "$LS_OUTPUT2" "running" "instance is running after restart"
pass "instance running after stop/start cycle"

# ============================================================
# Test 8: Assign to feature-v2 branch
# ============================================================

echo ""
echo "=== Test 8: Assign to feature-v2 ==="

ASSIGN_OUT=$($COAST assign test-bare --worktree feature-v2 2>&1) || { echo "$ASSIGN_OUT"; fail "coast assign to feature-v2 failed"; }
assert_contains "$ASSIGN_OUT" "feature-v2" "assign output references feature-v2"
pass "coast assign to feature-v2 succeeded"

SERVERJS_CONTENT=$($COAST exec test-bare -- cat /workspace/server.js 2>&1)
assert_contains "$SERVERJS_CONTENT" "v2" "/workspace has feature-v2 server.js"
pass "workspace files switched to feature-v2"

sleep 5

if [ -n "$DYNAMIC_PORT" ]; then
  V2_RESP=$(curl -s --max-time 5 "http://localhost:${DYNAMIC_PORT}" 2>&1) || true
  assert_contains "$V2_RESP" "v2" "response contains v2 after branch switch"
  assert_contains "$V2_RESP" '"version"' "response contains version field"
  pass "server serves v2 response after assign"
else
  fail "no dynamic port available for v2 check"
fi

# ============================================================
# Test 9: Unassign back to project root
# ============================================================

echo ""
echo "=== Test 9: Unassign back to project root ==="

UNASSIGN_OUT=$($COAST unassign test-bare 2>&1) || { echo "$UNASSIGN_OUT"; fail "coast unassign failed"; }
pass "coast unassign succeeded"

SERVERJS_MAIN=$($COAST exec test-bare -- cat /workspace/server.js 2>&1)
assert_not_contains "$SERVERJS_MAIN" "v2" "/workspace has main server.js (no v2)"
pass "workspace files switched back to main"

sleep 5

if [ -n "$DYNAMIC_PORT" ]; then
  MAIN_RESP=$(curl -s --max-time 5 "http://localhost:${DYNAMIC_PORT}" 2>&1) || true
  assert_contains "$MAIN_RESP" "Hello from Coast bare services!" "response is original main version"
  pass "server serves main response after assign back"
else
  fail "no dynamic port available for main check"
fi

# ============================================================
# Test 10: Remove
# ============================================================

echo ""
echo "=== Test 10: Remove instance ==="

RM_OUTPUT=$($COAST rm test-bare 2>&1) || { echo "$RM_OUTPUT"; fail "coast rm failed"; }
pass "coast rm succeeded"

LS_OUTPUT3=$($COAST ls 2>&1)
assert_not_contains "$LS_OUTPUT3" "test-bare" "instance removed from listing"

# Verify no Docker containers remain
REMAINING=$(docker ps -a --filter "label=coast.managed=true" --format '{{.Names}}' 2>/dev/null | grep "coast-bare" || true)
if [ -z "$REMAINING" ]; then
  pass "no managed containers remain"
else
  fail "managed containers still exist: $REMAINING"
fi

# ============================================================
# Summary
# ============================================================

echo ""
echo "=== All bare services tests passed! ==="
