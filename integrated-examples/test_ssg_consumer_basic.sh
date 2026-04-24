#!/usr/bin/env bash
#
# Integration test: consumer coast reaches SSG postgres through the
# synthesized SharedServiceConfig + shared_service_routing path (Phase 4).
#
# Verifies end-to-end connectivity:
#
#     consumer app (inside inner compose)
#       -> postgres:5432 (DNS via compose_rewrite extra_hosts)
#       -> docker0 alias IP (socat listener)
#       -> host.docker.internal:<ssg-dynamic-host-port>
#       -> SSG DinD -> inner postgres
#
# The `app` service in the consumer compose is a long-sleeping
# `postgres:16-alpine` container so the test can run psql from inside
# it to prove real SQL handshake (not just TCP connectivity).
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) -- SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-basic"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

start_daemon

# ============================================================
# Step 1: build + run the SSG (auto-start would also work but
# being explicit lets us inspect the ports before the consumer
# ever runs).
# ============================================================

echo ""
echo "=== Step 1: SSG build + run ==="

# Phase 25.5: build SSG from the consumer's cwd so the SSG is
# owned by the consumer's project (Phase 23 per-project contract).
cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
SSG_BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$SSG_BUILD_OUT" | tail -5
assert_contains "$SSG_BUILD_OUT" "Build complete" "ssg build succeeds"

SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$SSG_RUN_OUT" | tail -5
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
SSG_DYNAMIC=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYNAMIC" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYNAMIC"

# Give postgres a moment to finish initdb inside the SSG.
sleep 5

# ============================================================
# Step 2: build + run the consumer with from_group reference
# ============================================================

echo ""
echo "=== Step 2: consumer build + run ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
CONSUMER_BUILD_OUT=$("$COAST" build 2>&1)
echo "$CONSUMER_BUILD_OUT" | tail -5
assert_contains "$CONSUMER_BUILD_OUT" "Build" "consumer build succeeds"

CLEANUP_INSTANCES+=("inst-a")
CONSUMER_RUN_OUT=$("$COAST" run inst-a 2>&1)
echo "$CONSUMER_RUN_OUT" | tail -15
assert_contains "$CONSUMER_RUN_OUT" "Created coast instance 'inst-a'" "consumer run succeeds"

# Wait a beat for inner compose to be fully up.
sleep 3

# ============================================================
# Step 3: plumbing assertions — /etc/hosts + socat alias
# ============================================================

echo ""
echo "=== Step 3: routing plumbing inside consumer ==="

HOSTS_OUT=$("$COAST" exec inst-a --service app -- cat /etc/hosts 2>&1)
echo "$HOSTS_OUT"
assert_contains "$HOSTS_OUT" "postgres" "/etc/hosts has postgres entry"

# Extract the alias IP the consumer sees for postgres and confirm it's
# on the docker0 bridge (172.17.x.y by default).
ALIAS_IP=$(echo "$HOSTS_OUT" | awk '$2 == "postgres" {print $1}' | head -1)
[ -n "$ALIAS_IP" ] || fail "could not extract postgres alias IP from /etc/hosts"
echo "alias IP for postgres: $ALIAS_IP"
case "$ALIAS_IP" in
    172.*) pass "postgres alias is in docker0 range (172.x)" ;;
    *) fail "unexpected alias IP '$ALIAS_IP' (expected 172.x)" ;;
esac

# ============================================================
# Step 4: real connectivity — psql handshake from consumer app
# to SSG postgres via the synthesized route.
# ============================================================

echo ""
echo "=== Step 4: real SQL through the SSG ==="

PSQL_OUT=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 42 AS answer;'" 2>&1)
echo "$PSQL_OUT"
assert_contains "$PSQL_OUT" "answer" "psql returned the column header"
assert_contains "$PSQL_OUT" "42" "psql returned the answer row"

# Sanity: the same connection string should work round-tripping a
# temp table to prove a writable connection, not just SELECT.
PSQL_WRITE=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'CREATE TEMP TABLE ssg_probe(id int); INSERT INTO ssg_probe VALUES (7); SELECT id FROM ssg_probe;'" 2>&1)
echo "$PSQL_WRITE"
assert_contains "$PSQL_WRITE" "7" "round-trip insert/select succeeded"

pass "consumer app successfully executed SQL against the SSG postgres"

# Cleanup.
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER BASIC TESTS PASSED"
echo "==========================================="
