#!/usr/bin/env bash
#
# Integration test: `[shared_services.*].inject = "file:<path>"` on an
# SSG consumer coast writes the canonical connection URL to a file
# visible inside the inner compose app container (Phase 13,
# DESIGN.md §14).
#
# Asserts:
#   1. The file exists at the declared path inside the consumer's
#      `app` service (read-only).
#   2. The file body is the canonical connection URL —
#      `postgres://postgres:dev@postgres:5432/{instance}_{project}` —
#      byte-identical to what `inject = "env:DATABASE_URL"` would set.
#   3. `$DATABASE_URL` is NOT set (file inject replaces env inject).
#   4. The URL read from the file actually connects via the SSG
#      routing, and lands in the auto-created per-instance DB.

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

cd "$PROJECTS_DIR/coast-ssg-consumer-inject-file"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("inject-f")
RUN_OUT=$("$COAST" run inject-f 2>&1)
echo "$RUN_OUT" | tail -10
assert_contains "$RUN_OUT" "Created coast instance 'inject-f'" "consumer run succeeds"

sleep 3

echo ""
echo "=== Step 3: file exists at declared path ==="

STAT_OUT=$("$COAST" exec inject-f --service app -- \
    sh -c 'test -f /run/secrets/db_url && echo "present" || echo "missing"' 2>&1 | tr -d '\r\n')
if [ "$STAT_OUT" != "present" ]; then
    fail "/run/secrets/db_url is not a regular file inside the app service (got '$STAT_OUT')"
fi
pass "/run/secrets/db_url exists as a regular file inside the app service"

# Confirm the :ro bind-mount flag is in effect: writes must fail.
WRITE_OUT=$("$COAST" exec inject-f --service app -- \
    sh -c 'echo tampered > /run/secrets/db_url 2>&1; echo "EXIT=$?"' 2>&1)
echo "$WRITE_OUT"
assert_contains "$WRITE_OUT" "EXIT=1" "writes to /run/secrets/db_url fail (bind mount is read-only)"

echo ""
echo "=== Step 4: file body is the canonical connection URL ==="

DB_URL=$("$COAST" exec inject-f --service app -- cat /run/secrets/db_url 2>&1 | tr -d '\r\n')
echo "observed body: $DB_URL"

EXPECTED="postgres://postgres:dev@postgres:5432/inject-f_coast-ssg-consumer-inject-file"
if [ "$DB_URL" != "$EXPECTED" ]; then
    echo "expected: $EXPECTED"
    fail "file body does not match expected canonical URL"
fi
pass "file body is canonical: host='postgres', port=5432 (not $SSG_DYNAMIC), db='inject-f_...'"

# DESIGN.md §14 sanity: the dynamic port MUST NOT leak into the URL.
if echo "$DB_URL" | grep -q ":$SSG_DYNAMIC/"; then
    fail "file body embeds the SSG dynamic port ($SSG_DYNAMIC) — must use canonical 5432 per DESIGN §14"
fi
pass "URL does not leak the SSG dynamic port"

echo ""
echo "=== Step 5: DATABASE_URL env var is NOT set (file inject replaces env inject) ==="

ENV_OUT=$("$COAST" exec inject-f --service app -- sh -c 'printenv DATABASE_URL || true' 2>&1 | tr -d '\r\n')
if [ -n "$ENV_OUT" ]; then
    fail "DATABASE_URL should be unset when only file inject is declared (got '$ENV_OUT')"
fi
pass "DATABASE_URL is unset"

echo ""
echo "=== Step 6: URL read from the file actually connects ==="

PSQL_OUT=$("$COAST" exec inject-f --service app -- \
    sh -c 'psql "$(cat /run/secrets/db_url)" -c "SELECT 42 AS answer;"' 2>&1)
echo "$PSQL_OUT"
assert_contains "$PSQL_OUT" "answer" "psql using file URL returns header"
assert_contains "$PSQL_OUT" "42" "psql using file URL returns 42"

# Verify we're really in the per-instance DB.
CURRENT_DB=$("$COAST" exec inject-f --service app -- \
    sh -c 'psql "$(cat /run/secrets/db_url)" -tAc "SELECT current_database();"' 2>&1 | tr -d '\r')
echo "current_database() = $CURRENT_DB"
[ "$CURRENT_DB" = "inject-f_coast-ssg-consumer-inject-file" ] \
    || fail "current_database() = '$CURRENT_DB' (expected 'inject-f_coast-ssg-consumer-inject-file')"
pass "connection lands in the auto-created per-instance DB"

# Cleanup.
"$COAST" rm inject-f >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG INJECT FILE TESTS PASSED"
echo "==========================================="
