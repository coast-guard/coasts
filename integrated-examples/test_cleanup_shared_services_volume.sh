#!/usr/bin/env bash
#
# Integration test: shared service volume cleanup on rm
#
# Verifies that:
#   1. Shared services use the raw Docker volume name from the Coastfile
#      (so they share data with regular docker-compose runs)
#   2. `coast shared-services rm` removes the Docker volume along with
#      the container, so stale/polluted data doesn't persist
#   3. After rm + re-run, the database starts clean
#
# This tests the fix for the bug where a polluted volume (from another
# project's docker-compose) couldn't be cleared even after removing and
# rebuilding shared services, because volumes were never cleaned up.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "======================================================="
echo " Test: Shared Services Volume Cleanup on RM"
echo "======================================================="

# --- Setup ---

preflight_checks
clean_slate

# Remove any leftover containers and volumes from this test
docker rm -f vol-cleanup-test-shared-services-postgres 2>/dev/null || true
docker volume rm postgres_data 2>/dev/null || true
docker network rm coast-shared-vol-cleanup-test 2>/dev/null || true

"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/host-shared-services-volume"

start_daemon

# =========================================================
# Phase 1: Shared service uses the raw volume name
# =========================================================

echo ""
echo "=== Phase 1: Shared service uses raw volume name ==="

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

RUN_OUT=$("$COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
echo "$RUN_OUT"
assert_contains "$RUN_OUT" "Created coast instance" "inst-a created"

PORT_A=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$PORT_A" ] || fail "Could not extract inst-a app port"
pass "inst-a port: $PORT_A"

# Verify the Docker volume uses the raw name from the Coastfile
RAW_VOL=$(docker volume ls -q --filter "name=^postgres_data$" 2>/dev/null)
assert_eq "$RAW_VOL" "postgres_data" "shared service uses raw volume name (postgres_data)"

wait_for_healthy "$PORT_A" 60 || fail "inst-a not healthy"
pass "inst-a healthy"

# Verify app connects
DB_INFO=$(curl -sf "http://localhost:${PORT_A}/db-check" 2>&1 || echo '{"error":"connection failed"}')
echo "  db-check: $DB_INFO"
assert_contains "$DB_INFO" "connected" "app connects to postgres"

# Write data
WRITE_OUT=$(curl -sf "http://localhost:${PORT_A}/db-write" 2>&1)
assert_contains "$WRITE_OUT" "written" "wrote data to shared postgres"

# Confirm data is readable
READ_OUT=$(curl -sf "http://localhost:${PORT_A}/db-read" 2>&1)
echo "  db-read: $READ_OUT"
COUNT=$(echo "$READ_OUT" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT" -ge 1 ] || fail "expected at least 1 row, got count=$COUNT"
pass "data confirmed (count=$COUNT)"

# Stop instance
"$COAST" stop inst-a 2>&1
pass "inst-a stopped"

# =========================================================
# Phase 2: coast shared-services rm removes the volume
# =========================================================

echo ""
echo "=== Phase 2: shared-services rm removes volume ==="

RM_OUT=$("$COAST" shared-services rm postgres 2>&1)
echo "  rm output: $RM_OUT"
assert_contains "$RM_OUT" "removed" "shared service removed"

# Verify the Docker volume was cleaned up
sleep 1
VOL_AFTER=$(docker volume ls -q --filter "name=^postgres_data$" 2>/dev/null || true)
if [ -n "$VOL_AFTER" ]; then
    fail "postgres_data volume should have been removed by shared-services rm"
fi
pass "volume cleaned up by shared-services rm"

# =========================================================
# Phase 3: Re-run gets a clean database (no stale data)
# =========================================================

echo ""
echo "=== Phase 3: Re-run gets clean database ==="

# Re-run -- shared services should start fresh with a new volume
RUN_B=$("$COAST" run inst-b 2>&1)
CLEANUP_INSTANCES+=("inst-b")
echo "$RUN_B"
assert_contains "$RUN_B" "Created coast instance" "inst-b created"

PORT_B=$(extract_dynamic_port "$RUN_B" "app")
[ -n "$PORT_B" ] || fail "Could not extract inst-b app port"

wait_for_healthy "$PORT_B" 60 || fail "inst-b not healthy"
pass "inst-b healthy"

# Verify the database is clean -- no stale data from phase 1
DB_READ_B=$(curl -sf "http://localhost:${PORT_B}/db-read" 2>&1)
echo "  db-read: $DB_READ_B"
COUNT_B=$(echo "$DB_READ_B" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
assert_eq "$COUNT_B" "0" "database is clean after rm + re-run (no stale data)"

# =========================================================
# Phase 4: Cleanup
# =========================================================

echo ""
echo "=== Phase 4: Cleanup ==="

"$COAST" shared-services rm postgres 2>/dev/null || true
"$COAST" rm inst-a 2>/dev/null || true
"$COAST" rm inst-b 2>/dev/null || true
CLEANUP_INSTANCES=()

docker volume rm postgres_data 2>/dev/null || true
docker network rm coast-shared-vol-cleanup-test 2>/dev/null || true

pass "cleanup complete"

echo ""
echo "=== All shared services volume cleanup tests passed ==="
