#!/usr/bin/env bash
#
# Integration test: `auto_create_db = true` on an SSG service creates a
# per-instance `{instance}_{project}` database inside the SSG postgres
# when a consumer coast runs (Phase 5, DESIGN.md §13).
#
# Flow verified:
#
#     coast run -> provision.rs ->
#       auto_create_db::run_auto_create_dbs ->
#       (target == "coast-ssg") ->
#       coast_ssg::daemon_integration::create_instance_db_for_consumer ->
#       docker exec coast-ssg docker compose exec -T postgres
#         psql -U postgres -c "... CREATE DATABASE \"auto-a_coast-ssg-consumer-auto-db\" ..."
#
# Fixture: `coast-ssg-auto-db` (SSG with auto_create_db = true) +
# `coast-ssg-consumer-auto-db` (consumer with `from_group = true`).

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
echo "=== Step 1: SSG build + run (auto_create_db = true on postgres) ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$SSG_RUN_OUT" | tail -5
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

# Wait for postgres initdb.
sleep 6

echo ""
echo "=== Step 2: DB does NOT exist yet ==="

# Query the SSG postgres via nested exec to confirm the per-instance DB
# hasn't been created. We don't expect any `auto-a_*` DBs yet.
PRE_LIST=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml -p coast-ssg \
    exec -T postgres psql -U postgres -lqt 2>&1 | awk -F'|' '{print $1}' | tr -d ' ')
echo "$PRE_LIST"
if echo "$PRE_LIST" | grep -q "auto-a_coast-ssg-consumer-auto-db"; then
    fail "per-instance DB exists before the consumer runs (not expected)"
fi
pass "no per-instance DB present before consumer run"

echo ""
echo "=== Step 3: build + run the consumer ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("auto-a")
CONSUMER_RUN_OUT=$("$COAST" run auto-a 2>&1)
echo "$CONSUMER_RUN_OUT" | tail -15
assert_contains "$CONSUMER_RUN_OUT" "Created coast instance 'auto-a'" "consumer run succeeds"

sleep 3

echo ""
echo "=== Step 4: per-instance DB was created inside SSG postgres ==="

POST_LIST=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml -p coast-ssg \
    exec -T postgres psql -U postgres -lqt 2>&1 | awk -F'|' '{print $1}' | tr -d ' ')
echo "$POST_LIST"
echo "$POST_LIST" | grep -qx "auto-a_coast-ssg-consumer-auto-db" \
    || fail "expected DB 'auto-a_coast-ssg-consumer-auto-db' in psql -l output"
pass "per-instance DB 'auto-a_coast-ssg-consumer-auto-db' exists"

echo ""
echo "=== Step 5: creation is idempotent ==="

# Re-running auto_create_db should not error (PostgreSQL's \gexec pattern
# in create_db_command is `IF NOT EXISTS`-equivalent). We exercise this
# by creating a second coast that hits the same SSG.
CLEANUP_INSTANCES+=("auto-b")
SECOND_RUN=$("$COAST" run auto-b 2>&1)
assert_contains "$SECOND_RUN" "Created coast instance 'auto-b'" "second consumer run succeeds"

sleep 2

LIST_AGAIN=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml -p coast-ssg \
    exec -T postgres psql -U postgres -lqt 2>&1 | awk -F'|' '{print $1}' | tr -d ' ')
echo "$LIST_AGAIN" | grep -qx "auto-a_coast-ssg-consumer-auto-db" \
    || fail "DB 'auto-a_...' disappeared after second run"
echo "$LIST_AGAIN" | grep -qx "auto-b_coast-ssg-consumer-auto-db" \
    || fail "DB 'auto-b_...' not created on second run"
pass "both per-instance DBs coexist inside the shared SSG postgres"

# Cleanup.
"$COAST" rm auto-a >/dev/null 2>&1 || true
"$COAST" rm auto-b >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG AUTO_CREATE_DB TESTS PASSED"
echo "==========================================="
