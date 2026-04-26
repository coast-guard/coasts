#!/usr/bin/env bash
#
# Phase 33 integration test: changing the source value an SSG
# extractor reads (here: the env var the `env` extractor consults)
# and rebuilding causes the new value to be visible inside the
# postgres container at run time.
#
# Asserts:
#   1. First build extracts $SSG_TEST_PG_PASSWORD = "v1".
#   2. Run injects v1 into postgres as $POSTGRES_PASSWORD.
#   3. Stop, rebuild with $SSG_TEST_PG_PASSWORD = "v2".
#   4. Run again — postgres now sees v2.
#
# Proves the keystore is correctly re-keyed on rebuild (per the
# `delete_secrets_for_image` call at the top of `extract_ssg_secrets`).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-secrets"
PG_V1="value_one_${RANDOM}"
PG_V2="value_two_${RANDOM}"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="

clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Phase 33: env extractor inherits the daemon process's env. Round 1
# vars must exist before `start_daemon` so the FIRST build sees v1.
# Round 2 patches the env via `kill -HUP` would be cleaner, but
# `start_daemon` doesn't accept env patching mid-run; we restart the
# daemon between rounds to pick up the new value.
export SSG_TEST_JWT_VALUE="constant-jwt"
export SSG_TEST_PG_PASSWORD="$PG_V1"

start_daemon

echo ""
echo "=== Round 1: build with v1, run, observe ==="

cd "$PROJECTS_DIR/$SSG_PROJECT"

"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 8

SSG_CONTAINER="${SSG_PROJECT}-ssg"
OBSERVED_V1=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres printenv POSTGRES_PASSWORD 2>&1 | tr -d '\r')
[ "$OBSERVED_V1" = "$PG_V1" ] \
    || fail "round 1: expected POSTGRES_PASSWORD='$PG_V1', got '$OBSERVED_V1'"
pass "round 1: postgres sees v1='$PG_V1'"

echo ""
echo "=== Stop + rebuild with v2 ==="

"$COAST" ssg stop >/dev/null 2>&1 || true
sleep 2
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true
sleep 2

# Restart the daemon so the new env value is visible to the env
# extractor. Without this, `coast ssg build` would re-extract the
# stale value because the daemon process still has v1 in its env.
export SSG_TEST_PG_PASSWORD="$PG_V2"
pkill -f "coastd" 2>/dev/null || true
sleep 1
start_daemon

cd "$PROJECTS_DIR/$SSG_PROJECT"
"$COAST" ssg build >/dev/null 2>&1
pass "rebuild succeeded with new env var"

echo ""
echo "=== Round 2: run again and verify v2 ==="

"$COAST" ssg run >/dev/null 2>&1
sleep 8

OBSERVED_V2=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres printenv POSTGRES_PASSWORD 2>&1 | tr -d '\r')
[ "$OBSERVED_V2" = "$PG_V2" ] \
    || fail "round 2: expected POSTGRES_PASSWORD='$PG_V2', got '$OBSERVED_V2'"
pass "round 2: postgres sees v2='$PG_V2' (rebuild re-extracted)"

# v1 must NOT leak through: a busted re-extract path that left v1
# in the keystore would surface here.
[ "$OBSERVED_V2" != "$PG_V1" ] || fail "stale v1 leaked through rebuild"
pass "v1 is gone from the keystore after rebuild"

echo ""
echo "==========================================="
echo "  SSG SECRETS REBUILD REEXTRACTS TEST PASSED"
echo "==========================================="

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true
unset SSG_TEST_PG_PASSWORD SSG_TEST_JWT_VALUE
