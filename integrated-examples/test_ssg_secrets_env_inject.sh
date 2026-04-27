#!/usr/bin/env bash
#
# Phase 33 integration test: `[secrets.<name>]` declared in the
# SSG Coastfile with `extractor = "env"` and
# `inject = "env:POSTGRES_PASSWORD"` results in
# `$POSTGRES_PASSWORD` being injected into the postgres inner
# container at `coast ssg run` time via the per-run
# `compose.override.yml`.
#
# Asserts:
#   1. `coast ssg build` emits an `Extracting secrets` step in the
#      progress output and stores the value in the keystore.
#   2. `coast ssg run` writes a `compose.override.yml` to
#      `~/.coast/ssg/runs/coast-ssg-secrets/`.
#   3. Inside the postgres container, `$POSTGRES_PASSWORD` matches
#      the build-time env var ("supersecret").
#   4. The keystore row survives `coast ssg stop`.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-secrets"
SECRET_VALUE="supersecret_${RANDOM}"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="

clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Phase 33: keystore is global; clean any stale rows from prior runs
# of this test.
rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Phase 33: env extractor runs inside the daemon process. Export
# BEFORE `start_daemon` so the spawned `coastd` inherits them.
export SSG_TEST_PG_PASSWORD="$SECRET_VALUE"
export SSG_TEST_JWT_VALUE="ignored-by-this-test"

start_daemon

echo ""
echo "=== Step 1: build with [secrets.*] extracts via env extractor ==="

cd "$PROJECTS_DIR/$SSG_PROJECT"

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"
assert_contains "$BUILD_OUT" "Build complete" "ssg build succeeds"
assert_contains "$BUILD_OUT" "Extracting secrets" "build emits Extracting secrets step"
assert_contains "$BUILD_OUT" "POSTGRES_PASSWORD" "build extracts the env-inject target"
assert_contains "$BUILD_OUT" "2 extracted" "build reports 2 secrets extracted"
pass "ssg build extracted secrets"

LATEST_DIR="$(readlink -f "$HOME/.coast/ssg/latest")"
MANIFEST=$(cat "$LATEST_DIR/manifest.json")
assert_contains "$MANIFEST" "secret_injects" "manifest carries secret_injects array"
assert_contains "$MANIFEST" "POSTGRES_PASSWORD" "manifest records the env inject target"
pass "manifest captures the inject shape (no values)"

echo ""
echo "=== Step 2: run materializes the override + injects POSTGRES_PASSWORD ==="

"$COAST" ssg run >/dev/null 2>&1
sleep 8

OVERRIDE="$HOME/.coast/ssg/runs/$SSG_PROJECT/compose.override.yml"
[ -f "$OVERRIDE" ] || fail "expected compose.override.yml at '$OVERRIDE'"
pass "compose.override.yml written to per-run scratch dir"

OVERRIDE_BODY=$(cat "$OVERRIDE")
echo "$OVERRIDE_BODY"
assert_contains "$OVERRIDE_BODY" "POSTGRES_PASSWORD" "override declares POSTGRES_PASSWORD"
assert_contains "$OVERRIDE_BODY" "$SECRET_VALUE" "override carries the decrypted value (test-only check)"

echo ""
echo "=== Step 3: postgres inner container sees the secret as env var ==="

# Run inside the SSG outer DinD: `docker exec` into the postgres
# inner-compose service to read POSTGRES_PASSWORD. The compose
# project name is `<project>-ssg`.
SSG_CONTAINER="${SSG_PROJECT}-ssg"
PG_PASSWORD_OBSERVED=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres printenv POSTGRES_PASSWORD 2>&1 | tr -d '\r')
echo "observed POSTGRES_PASSWORD inside postgres: $PG_PASSWORD_OBSERVED"
[ "$PG_PASSWORD_OBSERVED" = "$SECRET_VALUE" ] \
    || fail "expected POSTGRES_PASSWORD='$SECRET_VALUE' inside postgres; got '$PG_PASSWORD_OBSERVED'"
pass "POSTGRES_PASSWORD inside the postgres container matches the build-time env extractor"

echo ""
echo "=== Step 4: keystore row survives stop ==="

"$COAST" ssg stop >/dev/null 2>&1 || true
sleep 2

DOCTOR_OUT=$("$COAST" ssg doctor 2>&1)
echo "$DOCTOR_OUT"
# After build (extracted) but before clear, the doctor should NOT
# emit a "missing from the keystore" finding for pg_password.
if echo "$DOCTOR_OUT" | grep -q "pg_password.*missing from the keystore"; then
    fail "doctor reports pg_password missing despite recent build"
fi
pass "doctor does not flag pg_password as missing after build (keystore row present)"

echo ""
echo "==========================================="
echo "  SSG SECRETS ENV INJECT TEST PASSED"
echo "==========================================="

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true
unset SSG_TEST_PG_PASSWORD SSG_TEST_JWT_VALUE
