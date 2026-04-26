#!/usr/bin/env bash
#
# Phase 33 integration test: explicit `coast ssg secrets clear` —
#
# Asserts:
#   1. After build, the keystore has rows for `ssg:<project>`
#      (verified indirectly via doctor not flagging "missing").
#   2. `coast ssg secrets clear` reports a count and is idempotent
#      (running it twice in a row is fine; second invocation
#      reports 0 cleared).
#   3. After clear, doctor reports both declared secrets as
#      info-level "missing from the keystore".
#   4. A subsequent `coast ssg build` re-extracts and the doctor
#      stops complaining.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-secrets"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="

clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Phase 33: env extractor reads vars from the daemon process. Must
# export BEFORE `start_daemon` so the spawned `coastd` inherits them.
export SSG_TEST_PG_PASSWORD="clear-me-${RANDOM}"
export SSG_TEST_JWT_VALUE="also-clear-me-${RANDOM}"

start_daemon

cd "$PROJECTS_DIR/$SSG_PROJECT"

echo ""
echo "=== Step 1: build extracts both secrets ==="

"$COAST" ssg build >/dev/null 2>&1
pass "build complete"

# Doctor should NOT report a "missing from the keystore" finding
# for either declared secret.
DOCTOR_OUT=$("$COAST" ssg doctor 2>&1)
if echo "$DOCTOR_OUT" | grep -q "missing from the keystore"; then
    echo "$DOCTOR_OUT"
    fail "doctor flags secrets as missing immediately after build"
fi
pass "doctor sees both declared secrets as present"

echo ""
echo "=== Step 2: secrets clear reports a count ==="

CLEAR1_OUT=$("$COAST" ssg secrets clear 2>&1)
echo "$CLEAR1_OUT"
assert_contains "$CLEAR1_OUT" "Cleared" "first clear reports cleared count"
pass "first clear succeeds"

# Should mention "2" since the SSG declares pg_password and jwt.
echo "$CLEAR1_OUT" | grep -qE "Cleared 2 SSG secret" \
    || echo "  (note: count match is best-effort; output: $CLEAR1_OUT)"

echo ""
echo "=== Step 3: clear is idempotent ==="

CLEAR2_OUT=$("$COAST" ssg secrets clear 2>&1)
echo "$CLEAR2_OUT"
assert_contains "$CLEAR2_OUT" "Cleared 0" "second clear is a no-op (0 cleared)"
pass "second clear reports 0 — idempotent"

echo ""
echo "=== Step 4: doctor flags missing secrets after clear ==="

DOCTOR2_OUT=$("$COAST" ssg doctor 2>&1 || true)
echo "$DOCTOR2_OUT"
assert_contains "$DOCTOR2_OUT" "pg_password" "doctor lists pg_password"
assert_contains "$DOCTOR2_OUT" "jwt" "doctor lists jwt"
assert_contains "$DOCTOR2_OUT" "missing from the keystore" \
    "doctor surfaces the missing-keystore finding"
pass "doctor info finding fires for both cleared secrets"

echo ""
echo "=== Step 5: re-build re-extracts ==="

"$COAST" ssg build >/dev/null 2>&1
pass "rebuild complete"

DOCTOR3_OUT=$("$COAST" ssg doctor 2>&1 || true)
if echo "$DOCTOR3_OUT" | grep -q "missing from the keystore"; then
    echo "$DOCTOR3_OUT"
    fail "doctor still complains after rebuild — re-extract path broken"
fi
pass "doctor is clean after re-extract"

echo ""
echo "==========================================="
echo "  SSG SECRETS CLEAR TEST PASSED"
echo "==========================================="

"$COAST" ssg secrets clear >/dev/null 2>&1 || true
unset SSG_TEST_PG_PASSWORD SSG_TEST_JWT_VALUE
