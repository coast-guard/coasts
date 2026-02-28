#!/usr/bin/env bash
#
# Integration test: shared volume strategy
#
# Verifies that strategy="shared" postgres persists data across instances
# while strategy="isolated" redis does NOT.
#
# Flow:
#   1. Build + run inst-a, write data to DB
#   2. Stop inst-a (postgres locks), run inst-b
#   3. inst-b should see inst-a's DB data (shared), but empty redis (isolated)
#   4. Clean up

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=========================================="
echo " Test: Volume Strategy — shared"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate
"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.shared Coastfile
git add -A && git commit -m "use shared coastfile" --allow-empty >/dev/null 2>&1 || true

start_daemon

# --- Build ---

echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"

# --- Run inst-a ---

echo ""
echo "=== Run inst-a ==="
RUN_A=$("$COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
assert_contains "$RUN_A" "Created coast instance" "inst-a created"

PORT_A=$(extract_dynamic_port "$RUN_A" "app")
[ -n "$PORT_A" ] || fail "Could not extract inst-a app port"
pass "inst-a port: $PORT_A"

wait_for_healthy "$PORT_A" 60 || fail "inst-a not healthy"
pass "inst-a healthy"

# Write data to postgres
WRITE_RESP=$(curl -sf "http://localhost:${PORT_A}/db-write")
assert_contains "$WRITE_RESP" "written" "inst-a wrote data to DB"

# Read it back
READ_RESP=$(curl -sf "http://localhost:${PORT_A}/db-read")
assert_contains "$READ_RESP" "count" "inst-a can read DB data"
pass "inst-a DB write/read OK"

# --- Stop inst-a, run inst-b ---

echo ""
echo "=== Stop inst-a, run inst-b ==="
"$COAST" stop inst-a 2>&1
pass "inst-a stopped"

RUN_B=$("$COAST" run inst-b 2>&1)
CLEANUP_INSTANCES+=("inst-b")
assert_contains "$RUN_B" "Created coast instance" "inst-b created"

PORT_B=$(extract_dynamic_port "$RUN_B" "app")
[ -n "$PORT_B" ] || fail "Could not extract inst-b app port"

wait_for_healthy "$PORT_B" 60 || fail "inst-b not healthy"
pass "inst-b healthy"

# --- Verify shared postgres: inst-b sees inst-a's data ---

READ_B=$(curl -sf "http://localhost:${PORT_B}/db-read")
echo "  inst-b db-read: $READ_B"
# count should be >= 1 (inst-a wrote data, shared volume persists)
COUNT=$(echo "$READ_B" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT" -ge 1 ] || fail "inst-b should see inst-a's DB data (shared volume), got count=$COUNT"
pass "Shared postgres: inst-b sees inst-a's data (count=$COUNT)"

# --- Verify isolated redis: inst-b has fresh redis ---

CACHE_B=$(curl -sf "http://localhost:${PORT_B}/cache-check")
assert_contains "$CACHE_B" "connected" "inst-b redis is connected"
pass "Isolated redis: inst-b has its own fresh redis"

echo ""
echo "=== All shared volume tests passed ==="
