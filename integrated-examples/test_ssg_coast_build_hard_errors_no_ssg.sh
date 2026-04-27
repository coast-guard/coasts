#!/usr/bin/env bash
#
# Integration test: `coast build` on a consumer with `from_group = true`
# hard-errors when no SSG build exists (Phase 9 SETTLED #33, backfilled
# in Phase 14).
#
# DESIGN.md §6.1 requires `coast build` to refuse to produce a
# consumer artifact whose `manifest.json` lacks an `ssg` drift block
# when refs exist. Without this hard-error, consumers could sneak a
# build past the run-time drift check.
#
# Verifies two shapes:
#   1. Clean slate (no SSG build): `coast build` on the consumer
#      fails with the verbatim §6.1 error naming the referenced
#      service and pointing at `coast ssg build`.
#   2. After `coast ssg build` lands a build, the same consumer
#      build succeeds.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25.5: per-project SSG (§23). SSG is owned by the consumer's
# project; step 2 builds it from the consumer's own cwd.
SSG_PROJECT="coast-ssg-consumer-basic"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Clean-slate SSG state: no build of any kind present on disk.
rm -rf "$HOME/.coast/ssg"

start_daemon

echo ""
echo "=== Step 1: coast build hard-errors when no SSG build exists ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
BUILD_OUT=$("$COAST" build 2>&1 || true)
echo "$BUILD_OUT" | tail -20

assert_contains "$BUILD_OUT" "no SSG build exists" "error names the missing-SSG-build condition"
assert_contains "$BUILD_OUT" "coast ssg build" "error points user at the remedy command"
assert_contains "$BUILD_OUT" "postgres" "error names the referenced SSG service"

# Also assert the build returned a non-zero exit code (captured
# above via `|| true`). `$COAST build` should always fail here —
# if it succeeded silently, the consumer would produce an artifact
# missing the `ssg` drift block and sneak past `coast run`'s §6.1
# check.
RC_OUT=$("$COAST" build 2>&1 >/dev/null; echo "EXIT=$?")
assert_contains "$RC_OUT" "EXIT=" "captured exit code from failing build"
if echo "$RC_OUT" | grep -q "EXIT=0"; then
    fail "coast build should have exited non-zero when no SSG build exists"
fi
pass "coast build exits non-zero when no SSG build exists"

echo ""
echo "=== Step 2: after coast ssg build, consumer build succeeds ==="

# Phase 25.5: build SSG from the consumer's own cwd so the SSG is
# owned by the consumer's project (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" ssg build >/dev/null 2>&1
BUILD_OUT2=$("$COAST" build 2>&1)
echo "$BUILD_OUT2" | tail -10
assert_contains "$BUILD_OUT2" "Build" "consumer build succeeds once SSG build exists"

echo ""
echo "==========================================="
echo "  ALL SSG COAST BUILD HARD-ERROR TESTS PASSED"
echo "==========================================="
