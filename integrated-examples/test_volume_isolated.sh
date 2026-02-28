#!/usr/bin/env bash
#
# Integration test: isolated volume strategy
#
# Verifies that strategy="isolated" gives each instance its own fresh volume.
# inst-a writes data, inst-b should NOT see it.
#
# Flow:
#   1. Build + run inst-a, write data to DB
#   2. Stop inst-a, run inst-b
#   3. inst-b should have an empty DB (isolated volume = fresh)
#   4. Clean up

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=========================================="
echo " Test: Volume Strategy — isolated"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate
"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.isolated Coastfile
git add -A && git commit -m "use isolated coastfile" --allow-empty >/dev/null 2>&1 || true

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

wait_for_healthy "$PORT_A" 60 || fail "inst-a not healthy"
pass "inst-a healthy"

# Write data
WRITE_RESP=$(curl -sf "http://localhost:${PORT_A}/db-write")
assert_contains "$WRITE_RESP" "written" "inst-a wrote data to DB"

READ_A=$(curl -sf "http://localhost:${PORT_A}/db-read")
COUNT_A=$(echo "$READ_A" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT_A" -ge 1 ] || fail "inst-a should have data, got count=$COUNT_A"
pass "inst-a has data (count=$COUNT_A)"

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

# --- Verify isolated: inst-b has empty DB ---

READ_B=$(curl -sf "http://localhost:${PORT_B}/db-read")
echo "  inst-b db-read: $READ_B"
COUNT_B=$(echo "$READ_B" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT_B" -eq 0 ] || fail "inst-b should have empty DB (isolated volume), got count=$COUNT_B"
pass "Isolated postgres: inst-b has fresh empty DB"

echo ""
echo "=== All isolated volume tests passed ==="
