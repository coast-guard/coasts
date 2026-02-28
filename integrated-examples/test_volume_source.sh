#!/usr/bin/env bash
#
# Integration test: isolated volumes with snapshot_source
#
# Verifies that snapshot_source on an isolated volume copies seed data into
# each new instance on `coast run`, and that instances diverge independently.
#
# Flow:
#   1. Create a seed Docker volume with pre-populated postgres data
#   2. Build + run inst-a — verify it starts with the seed data
#   3. Write additional data to inst-a
#   4. Stop inst-a, run inst-b
#   5. inst-b should have the original seed data (snapshot copied)
#      but NOT inst-a's additional writes (per-instance isolation)
#   6. Clean up seed volume

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SEED_VOL="coast-vol-seed-pg"
SEED_CONTAINER="coast-seed-pg-setup"

register_cleanup

# Extend cleanup to also remove the seed volume and container
_original_cleanup=$(declare -f _do_cleanup)
_do_cleanup() {
    eval "${_original_cleanup#*\{}" 2>/dev/null || true
    echo "  Removing seed volume and container..."
    docker rm -f "$SEED_CONTAINER" 2>/dev/null || true
    docker volume rm "$SEED_VOL" 2>/dev/null || true
}
trap '_do_cleanup' EXIT

echo ""
echo "=========================================="
echo " Test: Volume Strategy — snapshot_source"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate
"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.snapshot_source Coastfile
git add -A && git commit -m "use snapshot_source coastfile" --allow-empty >/dev/null 2>&1 || true

# --- Create seed volume with pre-populated postgres data ---

echo ""
echo "=== Create seed volume ==="

docker volume rm "$SEED_VOL" 2>/dev/null || true
docker rm -f "$SEED_CONTAINER" 2>/dev/null || true

docker run -d \
    --name "$SEED_CONTAINER" \
    -e POSTGRES_USER=coast \
    -e POSTGRES_PASSWORD=coast \
    -e POSTGRES_DB=coast_demo \
    -v "${SEED_VOL}:/var/lib/postgresql/data" \
    postgres:16-alpine >/dev/null

echo "  Waiting for seed postgres to be ready..."
for i in $(seq 1 30); do
    if docker exec "$SEED_CONTAINER" pg_isready -U coast >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
docker exec "$SEED_CONTAINER" pg_isready -U coast >/dev/null 2>&1 || fail "Seed postgres not ready"

docker exec "$SEED_CONTAINER" psql -U coast -d coast_demo -c "
    CREATE TABLE IF NOT EXISTS vol_test (
        id SERIAL PRIMARY KEY,
        label TEXT NOT NULL,
        created_at TIMESTAMPTZ DEFAULT NOW()
    );
    INSERT INTO vol_test (label) VALUES ('seed-row-1'), ('seed-row-2');
" >/dev/null

SEED_COUNT=$(docker exec "$SEED_CONTAINER" psql -U coast -d coast_demo -t -c "SELECT COUNT(*) FROM vol_test;" | tr -d ' ')
[ "$SEED_COUNT" -eq 2 ] || fail "Expected 2 seed rows, got $SEED_COUNT"
pass "Seed volume created with $SEED_COUNT rows"

docker stop "$SEED_CONTAINER" >/dev/null
docker rm "$SEED_CONTAINER" >/dev/null

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

wait_for_healthy "$PORT_A" 90 || fail "inst-a not healthy"
pass "inst-a healthy"

# Verify seed data is present
READ_A=$(curl -sf "http://localhost:${PORT_A}/db-read")
COUNT_A=$(echo "$READ_A" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT_A" -eq 2 ] || fail "inst-a should have 2 seed rows, got count=$COUNT_A"
pass "inst-a has seed data (count=$COUNT_A)"

# Write additional data to inst-a
WRITE_RESP=$(curl -sf "http://localhost:${PORT_A}/db-write")
assert_contains "$WRITE_RESP" "written" "inst-a wrote additional data"

READ_A2=$(curl -sf "http://localhost:${PORT_A}/db-read")
COUNT_A2=$(echo "$READ_A2" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT_A2" -eq 3 ] || fail "inst-a should have 3 rows after write, got count=$COUNT_A2"
pass "inst-a now has $COUNT_A2 rows (2 seed + 1 new)"

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

wait_for_healthy "$PORT_B" 90 || fail "inst-b not healthy"
pass "inst-b healthy"

# --- Verify: inst-b has seed data but NOT inst-a's writes ---

READ_B=$(curl -sf "http://localhost:${PORT_B}/db-read")
echo "  inst-b db-read: $READ_B"
COUNT_B=$(echo "$READ_B" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')

[ "$COUNT_B" -eq 2 ] || fail "inst-b should have exactly 2 seed rows (snapshot_source copy), got count=$COUNT_B"
pass "Snapshot source: inst-b has seed data (count=$COUNT_B)"

[ "$COUNT_B" -lt "$COUNT_A2" ] || fail "inst-b should have fewer rows than inst-a (diverged)"
pass "Per-instance isolation: inst-b does NOT have inst-a's additional writes"

echo ""
echo "=== All snapshot_source volume tests passed ==="
