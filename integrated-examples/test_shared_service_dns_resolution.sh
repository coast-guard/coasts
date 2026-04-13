#!/usr/bin/env bash
#
# Integration test: shared service socat routing with subnet collision
#
# Reproduces the Docker Desktop bug where the shared service network subnet
# collides with the inner DinD's docker0 subnet. On Docker Desktop, Docker
# assigns 172.18.0.0/16 to user-created networks AND the inner dockerd picks
# the same range for docker0, so resolved container IPs route to docker0
# instead of the outer network interface.
#
# To simulate this in DinDinD (which uses non-default subnets), the test
# pre-creates the shared network on 172.17.1.0/24 — a subnet within the
# inner DinD's default docker0 range (172.17.0.0/16). This forces the
# same routing collision that Docker Desktop users hit.
#
# This test bypasses the coast-wrapper (uses $REAL_COAST directly) so the
# daemon's socat proxy is the ONLY forwarding path.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=========================================="
echo " Test: Shared Service Subnet Collision"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate

docker rm -f coast-volumes-ss-shared-services-db coast-volumes-ss-shared-services-cache 2>/dev/null || true
docker volume rm coast-vol-test-pg 2>/dev/null || true
docker network rm coast-shared-coast-volumes-ss 2>/dev/null || true

# Pre-create the shared network on a subnet that collides with the inner
# DinD's default docker0 (172.17.0.0/16). The daemon will reuse this
# network instead of creating a new one, so shared service containers
# get IPs like 172.17.1.x — which route to docker0 inside the Coast
# DinD container instead of the outer network interface.
echo ""
echo "=== Force subnet collision ==="
docker network create --subnet 172.17.1.0/24 coast-shared-coast-volumes-ss
pass "pre-created shared network on colliding subnet 172.17.1.0/24"

"$HELPERS_DIR/setup.sh"

cd "$PROJECTS_DIR/coast-volumes"
cp Coastfile.shared_services Coastfile
git add -A && git commit -m "use shared_services coastfile" --allow-empty >/dev/null 2>&1 || true

start_daemon

# --- Build ---

echo ""
echo "=== Build ==="
BUILD_OUT=$("$REAL_COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# --- Run using REAL_COAST (bypasses wrapper socat) ---

echo ""
echo "=== Run inst-a (no wrapper) ==="
RUN_A=$("$REAL_COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
echo "$RUN_A"
assert_contains "$RUN_A" "Created coast instance" "inst-a created"

PORT_A=$(extract_dynamic_port "$RUN_A" "app")
[ -n "$PORT_A" ] || fail "Could not extract inst-a app port"
pass "inst-a port: $PORT_A"

# --- Verify shared services on host ---

echo ""
echo "=== Verify host-side shared services ==="
SS_PS=$(docker ps --filter "name=coast-volumes-ss-shared-services" --format '{{.Names}} {{.Status}}')
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-db" "shared postgres running on host"
assert_contains "$SS_PS" "coast-volumes-ss-shared-services-cache" "shared redis running on host"

# --- Verify socat targets use host.docker.internal ---

echo ""
echo "=== Verify socat upstream targets ==="

CONTAINER_NAME="coast-volumes-ss-coasts-inst-a"
SOCAT_PS=$(docker exec "$CONTAINER_NAME" ps aux 2>/dev/null | grep "socat TCP-LISTEN" | grep -v grep || true)
echo "  socat processes: $SOCAT_PS"

if [ -z "$SOCAT_PS" ]; then
  fail "No socat processes found inside DinD container"
fi

# socat upstream targets must use host.docker.internal, not resolved IPs
# (resolved IPs would collide with inner docker0 on the 172.17.x.x subnet)
HDI_TARGETS=$(echo "$SOCAT_PS" | grep -oE 'TCP:host\.docker\.internal:[0-9]+' || true)
echo "  host.docker.internal targets: $HDI_TARGETS"
[ -n "$HDI_TARGETS" ] || fail "socat should use host.docker.internal as upstream, not IPs"
pass "all socat upstream targets use host.docker.internal"

# --- Verify app health and connectivity ---

echo ""
echo "=== Verify app connectivity ==="

wait_for_healthy "$PORT_A" 60 || fail "inst-a not healthy"
pass "inst-a healthy"

DB_CHECK=$(curl -sf "http://localhost:${PORT_A}/db-check" 2>&1 || echo '{"error":"connection failed"}')
echo "  db-check: $DB_CHECK"
assert_contains "$DB_CHECK" "connected" "app connects to shared postgres via daemon socat"

CACHE_CHECK=$(curl -sf "http://localhost:${PORT_A}/cache-check" 2>&1 || echo '{"error":"connection failed"}')
echo "  cache-check: $CACHE_CHECK"
assert_contains "$CACHE_CHECK" "connected" "app connects to shared redis via daemon socat"

# --- Verify data operations ---

WRITE_RESP=$(curl -sf "http://localhost:${PORT_A}/db-write")
assert_contains "$WRITE_RESP" "written" "db write succeeds"

READ_RESP=$(curl -sf "http://localhost:${PORT_A}/db-read")
echo "  db-read: $READ_RESP"
COUNT=$(echo "$READ_RESP" | grep -o '"count":[0-9]*' | grep -o '[0-9]*')
[ "$COUNT" -ge 1 ] || fail "expected at least 1 row, got count=$COUNT"
pass "data operations work through daemon socat (count=$COUNT)"

echo ""
echo "=== All shared service subnet collision tests passed ==="
