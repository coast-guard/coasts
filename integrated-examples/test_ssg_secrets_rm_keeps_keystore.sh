#!/usr/bin/env bash
#
# Phase 33 integration test: `coast ssg rm` and `coast ssg rm
# --with-data` MUST NOT touch the keystore. Only `coast ssg
# secrets clear` removes SSG keystore rows.
#
# Asserts:
#   1. Build + run extracts the secret; postgres sees it.
#   2. `coast ssg rm` (no --with-data) preserves the build pointer,
#      so a follow-up `ssg run` works without rebuilding and the
#      injected secret is unchanged.
#   3. `coast ssg rm --with-data` wipes the build artifact + data
#      volumes + container, but the keystore rows persist. The
#      cleanest oracle for "keystore retained" is `coast ssg
#      secrets clear` reporting "Cleared 2" — that's only true if
#      the rows survived the wipe.
#   4. After `secrets clear`, a second `secrets clear` is a no-op
#      ("Cleared 0") — proves the verb is idempotent.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-secrets"
PG_VAL="rm-test-${RANDOM}"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="

clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Phase 33: env extractor reads vars from the daemon process. Export
# BEFORE `start_daemon`.
export SSG_TEST_PG_PASSWORD="$PG_VAL"
export SSG_TEST_JWT_VALUE="any-value"

start_daemon

cd "$PROJECTS_DIR/$SSG_PROJECT"

"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 8

SSG_CONTAINER="${SSG_PROJECT}-ssg"

verify_secret() {
    local expected="$1"
    local label="$2"
    local observed=""
    # After `rm --with-data` postgres has to bootstrap an empty
    # pgdata volume, which can take ~10s. Poll for the service to
    # come up before grabbing the env.
    local i
    for i in 1 2 3 4 5 6 7 8 9 10; do
        observed=$(docker exec "$SSG_CONTAINER" \
            docker compose -p "$SSG_CONTAINER" \
                -f /coast-artifact/compose.yml \
                -f /coast-runtime/compose.override.yml \
                exec -T postgres printenv POSTGRES_PASSWORD 2>/dev/null | tr -d '\r' || true)
        if [ -n "$observed" ]; then
            break
        fi
        sleep 2
    done
    [ "$observed" = "$expected" ] \
        || fail "$label: expected POSTGRES_PASSWORD='$expected', got '$observed'"
    pass "$label: postgres sees the kept secret"
}

verify_secret "$PG_VAL" "round 1 (initial run)"

echo ""
echo "=== Step 2: rm without --with-data, run again ==="

"$COAST" ssg rm >/dev/null 2>&1 || true
sleep 2

# Run again WITHOUT a fresh build. The materialize step should
# decrypt the kept keystore row.
"$COAST" ssg run >/dev/null 2>&1
sleep 8

verify_secret "$PG_VAL" "round 2 (after rm, no --with-data)"

echo ""
echo "=== Step 3: rm --with-data wipes container + build pointer, but keeps keystore ==="

"$COAST" ssg rm --with-data 2>&1 | tail -3 || true
sleep 2

# `rm --with-data` wipes the SSG entirely (build pointer, container,
# data volumes, runtime state) — the next `coast ssg run` reports
# "no SSG build found" until we rebuild. The keystore is the ONLY
# piece of state that survives.
#
# We verify the keystore survived by attempting `coast ssg secrets
# clear`: if it reports "Cleared 2", the rows were still there
# despite the --with-data wipe. This is the cleanest oracle since
# `coast ssg doctor` requires a manifest (also wiped) to report.
CLEAR_OUT=$("$COAST" ssg secrets clear 2>&1)
echo "$CLEAR_OUT"
assert_contains "$CLEAR_OUT" "Cleared 2" \
    "keystore retained 2 rows through coast ssg rm --with-data"
pass "keystore survived coast ssg rm --with-data (only secrets clear removes rows)"

echo ""
echo "=== Step 4: post-clear behavior ==="

# After `secrets clear`, a fresh `ssg secrets clear` is a no-op.
CLEAR2_OUT=$("$COAST" ssg secrets clear 2>&1)
echo "$CLEAR2_OUT"
assert_contains "$CLEAR2_OUT" "Cleared 0" "second clear is idempotent"
pass "secrets clear is idempotent after a previous clear"

echo ""
echo "==========================================="
echo "  SSG SECRETS RM KEEPS KEYSTORE TEST PASSED"
echo "==========================================="

"$COAST" ssg secrets clear >/dev/null 2>&1 || true
unset SSG_TEST_PG_PASSWORD SSG_TEST_JWT_VALUE
