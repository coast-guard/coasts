#!/usr/bin/env bash
#
# Integration test for the agent shell feature.
#
# Tests that:
#   1. A Coastfile with [agent_shell] builds successfully
#   2. `coast run` auto-spawns the agent shell PTY session
#   3. The agent shell appears in /exec/sessions with agent metadata
#   4. The agent shell session stays alive (not removed on child exit)
#   5. The agent shell DB record tracks the correct state
#   6. `coast stop` cleans up agent shell sessions and DB records
#   7. `coast rm` cleans up everything
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_agent_shell.sh

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

cd "$PROJECTS_DIR/coast-agent-shell"

start_daemon

# ============================================================
# Test 1: Build
# ============================================================

echo ""
echo "=== Test 1: coast build (with agent_shell) ==="

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"
assert_contains "$BUILD_OUT" "coast-agent-shell" "build output references project"
pass "Build complete"

# Verify manifest contains agent_shell
MANIFEST=$(cat ~/.coast/images/coast-agent-shell/*/manifest.json 2>/dev/null | head -1)
assert_contains "$MANIFEST" "agent_shell" "manifest includes agent_shell config"
assert_contains "$MANIFEST" "AGENT SHELL STARTED" "manifest has correct command"
pass "Manifest contains agent_shell"

# ============================================================
# Test 2: Run — agent shell should auto-spawn
# ============================================================

echo ""
echo "=== Test 2: coast run (agent shell auto-spawn) ==="

RUN_OUT=$("$COAST" run test-agent 2>&1)
CLEANUP_INSTANCES+=("test-agent")
assert_contains "$RUN_OUT" "Created coast instance" "coast run succeeds"
pass "Instance test-agent created"

# Give the agent shell a moment to spawn
sleep 3

# ============================================================
# Test 3: Verify agent shell in DB
# ============================================================

echo ""
echo "=== Test 3: agent shell in database ==="

AGENT_ROWS=$(sqlite3 ~/.coast/state.db "SELECT id, is_active, status, session_id FROM agent_shells WHERE project='coast-agent-shell' AND instance_name='test-agent';")
[ -n "$AGENT_ROWS" ] || fail "No agent_shells rows found in database"
pass "Agent shell row exists in DB"

AGENT_ID=$(echo "$AGENT_ROWS" | cut -d'|' -f1)
IS_ACTIVE=$(echo "$AGENT_ROWS" | cut -d'|' -f2)
STATUS=$(echo "$AGENT_ROWS" | cut -d'|' -f3)
SESSION_ID=$(echo "$AGENT_ROWS" | cut -d'|' -f4)

assert_eq "$IS_ACTIVE" "1" "Agent shell is marked as active"
assert_eq "$STATUS" "running" "Agent shell status is running"
[ -n "$SESSION_ID" ] || fail "Agent shell has no session_id"
pass "Agent shell session_id is set: $SESSION_ID"

# ============================================================
# Test 4: Verify agent shell in exec/sessions API
# ============================================================

echo ""
echo "=== Test 4: agent shell in /exec/sessions API ==="

# The daemon serves the API on port 31415
API_RESP=$(curl -sS "http://localhost:31415/api/v1/exec/sessions?project=coast-agent-shell&name=test-agent" 2>&1)
assert_contains "$API_RESP" "agent_shell_id" "API response contains agent_shell_id field"
assert_contains "$API_RESP" "$AGENT_ID" "API response has correct agent shell ID"
assert_contains "$API_RESP" "is_active_agent" "API response contains is_active_agent field"
pass "Agent shell visible in exec/sessions API"

# ============================================================
# Test 5: Verify the session persists (not cleaned up on exit)
# ============================================================

echo ""
echo "=== Test 5: agent session persistence ==="

# Count sessions — should have at least the agent shell
SESSION_COUNT=$(echo "$API_RESP" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$SESSION_COUNT" -ge 1 ] || fail "Expected at least 1 session, got $SESSION_COUNT"
pass "At least 1 exec session exists (agent shell persisted)"

# ============================================================
# Test 6: Spawn additional agent shell via API (non-active)
# ============================================================

echo ""
echo "=== Test 6: spawn additional agent shell via API ==="

SPAWN_RESP=$(curl -sS -X POST "http://localhost:31415/api/v1/exec/agent-shell/spawn" \
  -H "content-type: application/json" \
  -d '{"project":"coast-agent-shell","name":"test-agent"}' 2>&1)

assert_contains "$SPAWN_RESP" "session_id" "spawn API returns session_id"
assert_contains "$SPAWN_RESP" "agent_shell_id" "spawn API returns agent_shell_id"

NEW_AGENT_ID=$(echo "$SPAWN_RESP" | python3 -c 'import sys,json; print(json.load(sys.stdin)["agent_shell_id"])')
NEW_IS_ACTIVE=$(echo "$SPAWN_RESP" | python3 -c 'import sys,json; print("1" if json.load(sys.stdin)["is_active_agent"] else "0")')

assert_eq "$NEW_IS_ACTIVE" "0" "newly spawned agent shell is not active"

DB_NEW_ACTIVE=$(sqlite3 ~/.coast/state.db "SELECT is_active FROM agent_shells WHERE project='coast-agent-shell' AND instance_name='test-agent' AND shell_id=$NEW_AGENT_ID;")
assert_eq "$DB_NEW_ACTIVE" "0" "new agent shell row is non-active in DB"

DB_ACTIVE_COUNT=$(sqlite3 ~/.coast/state.db "SELECT COUNT(*) FROM agent_shells WHERE project='coast-agent-shell' AND instance_name='test-agent' AND is_active=1;")
assert_eq "$DB_ACTIVE_COUNT" "1" "only one active agent shell remains"

API_AFTER_SPAWN=$(curl -sS "http://localhost:31415/api/v1/exec/sessions?project=coast-agent-shell&name=test-agent" 2>&1)
API_SESSION_COUNT=$(echo "$API_AFTER_SPAWN" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
[ "$API_SESSION_COUNT" -ge 2 ] || fail "Expected at least 2 sessions after spawning second agent shell, got $API_SESSION_COUNT"
assert_contains "$API_AFTER_SPAWN" "$NEW_AGENT_ID" "new agent shell id appears in exec sessions API"
pass "Additional non-active agent shell spawned successfully"

# ============================================================
# Test 7: coast stop — cleans up agent shells
# ============================================================

echo ""
echo "=== Test 7: coast stop (agent shell cleanup) ==="

"$COAST" stop test-agent 2>&1 | grep -q "Stopped" || fail "coast stop failed"
pass "coast stop succeeded"

# Agent shells should be gone from DB
AGENT_AFTER_STOP=$(sqlite3 ~/.coast/state.db "SELECT COUNT(*) FROM agent_shells WHERE project='coast-agent-shell' AND instance_name='test-agent';")
assert_eq "$AGENT_AFTER_STOP" "0" "Agent shells cleaned from DB after stop"

# ============================================================
# Test 8: coast rm — full cleanup
# ============================================================

echo ""
echo "=== Test 8: coast rm ==="

"$COAST" rm test-agent 2>&1 | grep -q "Removed" || fail "coast rm failed"
pass "coast rm succeeded"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

REMAINING=$(docker ps -q --filter "label=coast.managed=true" 2>/dev/null)
assert_eq "${REMAINING:-}" "" "no coast containers remain"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL AGENT SHELL TESTS PASSED"
echo "==========================================="
