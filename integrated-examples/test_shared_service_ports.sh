#!/usr/bin/env bash
#
# Integration test: shared service ports excluded from dynamic allocation
#
# Verifies that ports served by [shared_services.*] do NOT appear in the
# dynamic port table, even when also declared in [ports]. This prevents:
#   1. Confusing dynamic port rows for fixed shared services
#   2. Socat bind failures during checkout (port already in use)
#
# Uses coast-volumes with a Coastfile that has overlapping entries:
#   [ports]                  -> app=33100, db=5432, cache=6379
#   [shared_services.db]     -> ports=[5432]
#   [shared_services.cache]  -> ports=[6379]
#
# Expected: only "app" gets a dynamic port; db and cache do not.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=============================================="
echo " Test: Shared service ports — no dynamic alloc"
echo "=============================================="

# --- Setup ---

preflight_checks
clean_slate

docker rm -f coast-volumes-ss-shared-services-db coast-volumes-ss-shared-services-cache 2>/dev/null || true
docker volume rm coast-vol-test-pg 2>/dev/null || true
docker network rm coast-shared-coast-volumes-ss 2>/dev/null || true

"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.shared_services_with_ports Coastfile
git add -A && git commit -m "use shared_services_with_ports coastfile" --allow-empty >/dev/null 2>&1 || true

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

# --- Verify run output port table ---

echo ""
echo "=== Verify run output port table ==="
APP_PORT=$(extract_dynamic_port "$RUN_A" "app")
[ -n "$APP_PORT" ] || fail "Could not extract app dynamic port from run output"
pass "app dynamic port from run output: $APP_PORT"

DB_PORT=$(extract_dynamic_port "$RUN_A" "db")
CACHE_PORT=$(extract_dynamic_port "$RUN_A" "cache")

if [ -n "$DB_PORT" ]; then
    fail "db should NOT have a dynamic port in run output, got: $DB_PORT"
fi
pass "db has no dynamic port in run output"

if [ -n "$CACHE_PORT" ]; then
    fail "cache should NOT have a dynamic port in run output, got: $CACHE_PORT"
fi
pass "cache has no dynamic port in run output"

# --- Verify coast ports command ---

echo ""
echo "=== Verify coast ports ==="
PORTS_OUT=$("$COAST" ports inst-a 2>&1)
echo "$PORTS_OUT"

assert_contains "$PORTS_OUT" "Port allocations" "ports output has header"
assert_contains "$PORTS_OUT" "app" "ports output lists app service"
assert_contains "$PORTS_OUT" "33100" "ports output shows app canonical port"

assert_not_contains "$PORTS_OUT" "5432" "ports output must NOT contain shared db port 5432"
assert_not_contains "$PORTS_OUT" "6379" "ports output must NOT contain shared cache port 6379"

# --- Count port rows (only app should appear) ---
# Port data rows have at least two multi-digit numbers (canonical + dynamic).

PORT_ROW_COUNT=$(echo "$PORTS_OUT" | grep -cE '[0-9]{2,}[[:space:]]+[0-9]{2,}' || true)
if [ "$PORT_ROW_COUNT" -ne 1 ]; then
    fail "Expected exactly 1 port row (app), got $PORT_ROW_COUNT"
fi
pass "Exactly 1 port row in coast ports output"

# --- Verify shared services still run on host ---

echo ""
echo "=== Verify shared services on host ==="
SS_PS=$(docker ps --filter "name=coast-volumes-ss-shared-services" --format '{{.Names}}')
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-db" "shared postgres running on host"
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-cache" "shared redis running on host"

# --- Done ---

echo ""
echo "=============================================="
echo " ALL SHARED SERVICE PORT TESTS PASSED"
echo "=============================================="
