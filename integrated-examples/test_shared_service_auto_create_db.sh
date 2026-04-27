#!/usr/bin/env bash
#
# Integration test: `auto_create_db` + `inject` work end-to-end for
# INLINE `[shared_services.*]` (no SSG). DESIGN.md §13 claimed this
# was already implemented prior to Phase 5 — it wasn't. Phase 5's
# orchestrator dispatches inline services through direct
# `docker exec <host-container>` on the host daemon instead of the
# nested SSG path.
#
# Fixture: `coast-shared-service-auto-db` declares an inline postgres
# with `auto_create_db = true, inject = "env:DATABASE_URL"`, and a
# `postgres:16-alpine` app service sleeping forever.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

start_daemon

echo ""
echo "=== Step 1: consumer build + run ==="

cd "$PROJECTS_DIR/coast-shared-service-auto-db"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("inline-a")
RUN_OUT=$("$COAST" run inline-a 2>&1)
echo "$RUN_OUT" | tail -15
assert_contains "$RUN_OUT" "Created coast instance 'inline-a'" "consumer run succeeds"

# Inline shared services start on the host daemon (not inside the
# coast). Give postgres a moment to finish initdb.
sleep 6

echo ""
echo "=== Step 2: per-instance DB created inside inline postgres ==="

# The inline host container is named per the existing convention:
#     {project}-shared-services-{service}
INLINE_CONTAINER="coast-shared-service-auto-db-shared-services-postgres"
DOCKER_PS=$(docker ps --filter "name=^${INLINE_CONTAINER}$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "$INLINE_CONTAINER" "inline shared postgres container is up"

DB_LIST=$(docker exec "$INLINE_CONTAINER" psql -U postgres -lqt 2>&1 | awk -F'|' '{print $1}' | tr -d ' ')
echo "$DB_LIST"
echo "$DB_LIST" | grep -qx "inline-a_coast-shared-service-auto-db" \
    || fail "expected DB 'inline-a_coast-shared-service-auto-db' in psql -l output"
pass "per-instance DB 'inline-a_coast-shared-service-auto-db' exists"

echo ""
echo "=== Step 3: DATABASE_URL set in consumer app container ==="

set +e
ENV_OUT=$("$COAST" exec inline-a --service app -- env 2>&1)
DB_URL=$(echo "$ENV_OUT" | grep '^DATABASE_URL=' | head -1 | sed 's/^DATABASE_URL=//')
set -e
echo "observed: DATABASE_URL=$DB_URL"

EXPECTED="postgres://postgres:dev@postgres:5432/inline-a_coast-shared-service-auto-db"
if [ "$DB_URL" != "$EXPECTED" ]; then
    echo "expected: DATABASE_URL=$EXPECTED"
    echo "full env dump:"
    echo "$ENV_OUT"
    fail "DATABASE_URL does not match canonical inline URL"
fi
pass "DATABASE_URL canonical shape: host=postgres, port=5432, db=inline-a_..."

echo ""
echo "=== Step 4: DATABASE_URL actually connects ==="

PSQL_OUT=$("$COAST" exec inline-a --service app -- \
    psql "$DB_URL" -c 'SELECT 42 AS answer;' 2>&1)
echo "$PSQL_OUT"
assert_contains "$PSQL_OUT" "answer" "psql via DATABASE_URL returns header"
assert_contains "$PSQL_OUT" "42" "psql via DATABASE_URL returns 42"

CURRENT_DB=$("$COAST" exec inline-a --service app -- \
    psql "$DB_URL" -tAc 'SELECT current_database();' 2>&1 | tr -d '\r')
[ "$CURRENT_DB" = "inline-a_coast-shared-service-auto-db" ] \
    || fail "current_database() = '$CURRENT_DB' (expected 'inline-a_coast-shared-service-auto-db')"
pass "connection lands in the auto-created per-instance DB"

# Cleanup.
"$COAST" rm inline-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
docker rm -f "$INLINE_CONTAINER" 2>/dev/null || true

echo ""
echo "==========================================="
echo "  ALL INLINE SHARED-SERVICE AUTO_CREATE_DB TESTS PASSED"
echo "==========================================="
