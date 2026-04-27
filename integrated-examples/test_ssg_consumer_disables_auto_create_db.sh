#!/usr/bin/env bash
#
# Integration test: consumer `auto_create_db = false` explicitly
# disables per-instance DB creation even when the SSG's postgres has
# `auto_create_db = true` (Phase 9 SETTLED #34, backfilled in Phase 14).
#
# Asserts the three-valued `Option<bool>` override end-to-end:
#   1. The per-instance DB (`{instance}_{project}`) is NOT created
#      inside the SSG postgres.
#   2. `$DATABASE_URL` inside the consumer still embeds that canonical
#      DB name (URL shape is independent of auto_create_db per
#      DESIGN §13).
#   3. Connecting via the URL fails with "does not exist" — direct
#      proof that auto_create_db = false suppressed creation.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) -- SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-disable-auto-db"

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

echo ""
echo "=== Step 1: SSG up (auto_create_db = true on postgres) ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-disable-auto-db"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1

sleep 6

echo ""
echo "=== Step 2: consumer build + run (auto_create_db = false override) ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-disable-auto-db"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("inst-n")
RUN_OUT=$("$COAST" run inst-n 2>&1)
echo "$RUN_OUT" | tail -10
assert_contains "$RUN_OUT" "Created coast instance 'inst-n'" "consumer run succeeds"

sleep 3

EXPECTED_DB="inst-n_coast-ssg-consumer-disable-auto-db"

echo ""
echo "=== Step 3: per-instance DB was NOT created inside SSG postgres ==="

# List databases directly from the SSG postgres via `coast ssg exec`.
# With auto_create_db = false on the consumer, the per-instance DB
# must not exist.
DB_LIST=$("$COAST" ssg exec --service postgres -- \
    psql -U postgres -tAc "SELECT datname FROM pg_database;" 2>&1)
echo "$DB_LIST"
if echo "$DB_LIST" | grep -qx "$EXPECTED_DB"; then
    fail "per-instance DB '$EXPECTED_DB' exists; auto_create_db=false override did not suppress creation"
fi
pass "auto_create_db=false override suppressed per-instance DB creation"

echo ""
echo "=== Step 4: DATABASE_URL still embeds the canonical per-instance DB name ==="

# DESIGN §13: URL shape is independent of auto_create_db. The
# consumer sees the canonical `{instance}_{project}` DB name even
# though that DB was never created.
URL=$("$COAST" exec inst-n --service app -- sh -c 'echo "$DATABASE_URL"' | tr -d '\r\n')
echo "observed: DATABASE_URL=$URL"
assert_contains "$URL" "$EXPECTED_DB" "URL embeds canonical {instance}_{project} DB name"

echo ""
echo "=== Step 5: psql fails because the DB was not auto-created ==="

CONN=$("$COAST" exec inst-n --service app -- \
    sh -c 'psql "$DATABASE_URL" -c "SELECT 1" 2>&1 || true')
echo "$CONN"
assert_contains "$CONN" "does not exist" "psql connect fails: per-instance DB was not auto-created"

# Cleanup.
"$COAST" rm inst-n >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER DISABLE AUTO_CREATE_DB TESTS PASSED"
echo "==========================================="
