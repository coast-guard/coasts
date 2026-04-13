#!/usr/bin/env bash
#
# Integration test: shared service socat DNS resolution
#
# Verifies that the daemon resolves shared service container IPs on the host
# Docker daemon and passes them (not container names) to socat inside DinD.
#
# This test bypasses the coast-wrapper (uses $REAL_COAST directly) so the
# daemon's socat proxy is the ONLY forwarding path. Before the fix, socat
# used container names that don't resolve inside DinD (the inner dockerd's
# DNS shadows the host's), causing "unexpected EOF" errors.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

echo ""
echo "=========================================="
echo " Test: Shared Service DNS Resolution"
echo "=========================================="

# --- Setup ---

preflight_checks
clean_slate

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

# --- Key assertion: socat targets are IPs, not hostnames ---

echo ""
echo "=== Verify socat upstream targets are IPs ==="

CONTAINER_NAME="coast-volumes-ss-coasts-inst-a"
SOCAT_PS=$(docker exec "$CONTAINER_NAME" ps aux 2>/dev/null | grep "socat TCP-LISTEN" | grep -v grep || true)
echo "  socat processes: $SOCAT_PS"

if [ -z "$SOCAT_PS" ]; then
  fail "No socat processes found inside DinD container"
fi

# Every socat upstream target (TCP:...:port) should be an IP, not a hostname
HOSTNAME_TARGETS=$(echo "$SOCAT_PS" | grep -oE 'TCP:[a-zA-Z][a-zA-Z0-9_-]*:' || true)
if [ -n "$HOSTNAME_TARGETS" ]; then
  echo "  Found hostname-based socat targets: $HOSTNAME_TARGETS"
  fail "socat upstream targets should be IPs, not container names"
fi

IP_TARGETS=$(echo "$SOCAT_PS" | grep -oE 'TCP:[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+' || true)
echo "  IP-based socat targets: $IP_TARGETS"
[ -n "$IP_TARGETS" ] || fail "no IP-based socat targets found"
pass "all socat upstream targets are IPs"

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
echo "=== All shared service DNS resolution tests passed ==="
