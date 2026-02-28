#!/usr/bin/env bash
#
# Integration test: shared_services volume strategy
#
# Verifies that [shared_services] correctly:
#   1. Starts postgres + redis on the HOST Docker daemon
#   2. Disables them inside the inner DinD compose stack
#   3. Inner app services can connect to host-side db/cache
#   4. Data persists across instances (host-side)
#
# This test exercises the code path in handlers/run.rs that creates
# shared service containers, generates compose overrides with stub
# services, and connects the DinD container to the shared bridge network.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=========================================="
echo " Test: Volume Strategy — shared_services"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate

# Also clean up shared service containers from previous runs
docker rm -f coast-volumes-ss-shared-services-db coast-volumes-ss-shared-services-cache 2>/dev/null || true
docker volume rm coast-vol-test-pg 2>/dev/null || true
docker network rm coast-shared-coast-volumes-ss 2>/dev/null || true

"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.shared_services Coastfile
git add -A && git commit -m "use shared_services coastfile" --allow-empty >/dev/null 2>&1 || true

start_daemon

# --- Build ---

echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# --- Run inst-a ---

echo ""
echo "=== Run inst-a ==="
RUN_A=$("$COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
echo "$RUN_A"
assert_contains "$RUN_A" "Created coast instance" "inst-a created"

PORT_A=$(extract_dynamic_port "$RUN_A" "app")
[ -n "$PORT_A" ] || fail "Could not extract inst-a app port"
pass "inst-a port: $PORT_A"

# --- Verify shared service containers on host ---

echo ""
echo "=== Verify host-side shared services ==="

SS_PS=$(docker ps --filter "name=coast-volumes-ss-shared-services" --format '{{.Names}} {{.Status}}')
echo "  Shared services: $SS_PS"
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-db" "shared postgres running on host"
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-cache" "shared redis running on host"

# --- Verify bridge network ---

NET=$(docker network ls --filter "name=coast-shared-coast-volumes-ss" --format '{{.Name}}')
assert_eq "$NET" "coast-shared-coast-volumes-ss" "shared bridge network exists"

# --- Verify app is healthy ---

wait_for_healthy "$PORT_A" 60 || fail "inst-a not healthy"
pass "inst-a healthy"

# --- Verify app connects to shared db ---

DB_CHECK=$(curl -sf "http://localhost:${PORT_A}/db-check" 2>&1 || echo '{"error":"connection failed"}')
echo "  db-check: $DB_CHECK"
assert_contains "$DB_CHECK" "connected" "app connects to shared postgres"

# --- Verify app connects to shared cache ---

CACHE_CHECK=$(curl -sf "http://localhost:${PORT_A}/cache-check" 2>&1 || echo '{"error":"connection failed"}')
echo "  cache-check: $CACHE_CHECK"
assert_contains "$CACHE_CHECK" "connected" "app connects to shared redis"

# --- Write data ---

WRITE_RESP=$(curl -sf "http://localhost:${PORT_A}/db-write")
assert_contains "$WRITE_RESP" "written" "inst-a wrote data to shared DB"
pass "inst-a data operations OK"

# --- Stop inst-a, run inst-b ---

echo ""
echo "=== Stop inst-a, run inst-b ==="
"$COAST" stop inst-a 2>&1
pass "inst-a stopped"

RUN_B=$("$COAST" run inst-b 2>&1)
CLEANUP_INSTANCES+=("inst-b")
echo "$RUN_B"
assert_contains "$RUN_B" "Created coast instance" "inst-b created"

PORT_B=$(extract_dynamic_port "$RUN_B" "app")
[ -n "$PORT_B" ] || fail "Could not extract inst-b app port"

wait_for_healthy "$PORT_B" 60 || fail "inst-b not healthy"
pass "inst-b healthy"

# --- Verify data persists on shared host services ---

READ_B=$(curl -sf "http://localhost:${PORT_B}/db-read")
echo "  inst-b db-read: $READ_B"
COUNT_B=$(echo "$READ_B" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT_B" -ge 1 ] || fail "inst-b should see inst-a's data (shared host postgres), got count=$COUNT_B"
pass "Shared services: inst-b sees inst-a's data (count=$COUNT_B)"

echo ""
echo "=== All shared_services tests passed ==="
