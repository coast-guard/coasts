#!/usr/bin/env bash
#
# Integration test: `[shared_services.*].inject = "env:NAME"` on an SSG
# consumer coast populates `$NAME` inside the coast container with the
# canonical connection URL (Phase 5, DESIGN.md §14).
#
# Asserts:
#   1. `DATABASE_URL` is set in the consumer app container env.
#   2. The URL is `postgres://postgres:dev@postgres:5432/{instance}_{project}` —
#      canonical host (service name `postgres`), canonical port (5432,
#      NOT the SSG's dynamic host port), DB name matches the one
#      auto_create_db produced.
#   3. `psql "$DATABASE_URL"` actually connects and returns SQL.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

start_daemon

echo ""
echo "=== Step 1: SSG up ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1

PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
SSG_DYNAMIC=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYNAMIC" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYNAMIC"

sleep 6

echo ""
echo "=== Step 2: consumer build + run ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("inject-a")
RUN_OUT=$("$COAST" run inject-a 2>&1)
echo "$RUN_OUT" | tail -10
assert_contains "$RUN_OUT" "Created coast instance 'inject-a'" "consumer run succeeds"

sleep 3

echo ""
echo "=== Step 3: DATABASE_URL env var is set in the app container ==="

# `coast exec --service app -- env` — grab the full env and grep.
ENV_OUT=$("$COAST" exec inject-a --service app -- env 2>&1)
DB_URL=$(echo "$ENV_OUT" | grep '^DATABASE_URL=' | head -1 | sed 's/^DATABASE_URL=//')
echo "observed: DATABASE_URL=$DB_URL"

EXPECTED="postgres://postgres:dev@postgres:5432/inject-a_coast-ssg-consumer-auto-db"
if [ "$DB_URL" != "$EXPECTED" ]; then
    echo "expected: DATABASE_URL=$EXPECTED"
    fail "DATABASE_URL does not match expected canonical-port URL"
fi
pass "DATABASE_URL is canonical: host='postgres', port=5432 (not $SSG_DYNAMIC), db='inject-a_...'"

# DESIGN.md §14 sanity: the dynamic port MUST NOT leak into the URL.
if echo "$DB_URL" | grep -q ":$SSG_DYNAMIC/"; then
    fail "URL embeds the SSG dynamic port ($SSG_DYNAMIC) — must use canonical 5432 per DESIGN §14"
fi
pass "URL does not leak the SSG dynamic port"

echo ""
echo "=== Step 4: DATABASE_URL actually connects through the SSG routing ==="

# Use the URL verbatim — psql should accept the connection via the
# canonical name and the socat/reverse-tunnel routing does the rest.
PSQL_OUT=$("$COAST" exec inject-a --service app -- \
    psql "$DB_URL" -c 'SELECT 42 AS answer;' 2>&1)
echo "$PSQL_OUT"
assert_contains "$PSQL_OUT" "answer" "psql using DATABASE_URL returns header"
assert_contains "$PSQL_OUT" "42" "psql using DATABASE_URL returns 42"

# Verify we're really in the per-instance DB, not the default one.
CURRENT_DB=$("$COAST" exec inject-a --service app -- \
    psql "$DB_URL" -tAc 'SELECT current_database();' 2>&1 | tr -d '\r')
echo "current_database() = $CURRENT_DB"
[ "$CURRENT_DB" = "inject-a_coast-ssg-consumer-auto-db" ] \
    || fail "current_database() = '$CURRENT_DB' (expected 'inject-a_coast-ssg-consumer-auto-db')"
pass "connection lands in the auto-created per-instance DB"

# Cleanup.
"$COAST" rm inject-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG INJECT ENV TESTS PASSED"
echo "==========================================="
