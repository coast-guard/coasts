#!/usr/bin/env bash
#
# Integration test: pinned build directory has been pruned/deleted ->
# `coast run` hard-errors with the DESIGN §17-9 SETTLED #41 wording
# (Phase 16).
#
# Scenario:
#   1. Build SSG A.
#   2. `coast build` + pin consumer to A.
#   3. Simulate an out-of-band prune by `rm -rf ~/.coast/ssg/builds/A`.
#   4. `coast run` -> must hard-error with "no longer exists" /
#      "uncheckout-build" / "coast ssg build" guidance.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) -- SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-auto-db"

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
echo "=== Step 1: SSG build A ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" ssg build >/dev/null 2>&1
BUILD_A_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build A id: $BUILD_A_ID"

"$COAST" ssg run >/dev/null 2>&1
sleep 5

echo ""
echo "=== Step 2: build consumer + pin to A ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

PIN_OUT=$("$COAST" ssg checkout-build "$BUILD_A_ID" 2>&1)
assert_contains "$PIN_OUT" "Pinned project" "pin succeeds"

echo ""
echo "=== Step 3: stop SSG + delete build A from disk ==="

# Stopping first so the running SSG doesn't interfere with drift
# evaluation: we want the pin-pruned error, not a drift-vs-running
# error. We also need to be able to rm the build dir safely.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# Nuke the pinned build.
rm -rf "$HOME/.coast/ssg/builds/$BUILD_A_ID"
[ ! -d "$HOME/.coast/ssg/builds/$BUILD_A_ID" ] || fail "pinned build dir still exists after rm -rf"
pass "pinned build dir was removed"

# Ensure there's no latest symlink either, so the fallback doesn't
# paper over the pin-pruned error.
rm -f "$HOME/.coast/ssg/latest"

echo ""
echo "=== Step 4: run consumer -> hard-error citing uncheckout-build ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
set +e
RUN_OUT=$("$COAST" run pin-missing 2>&1)
EC=$?
set -e

echo "$RUN_OUT" | tail -30
[ "$EC" -ne 0 ] || fail "expected coast run to fail when pinned build is missing"

assert_contains "$RUN_OUT" "$BUILD_A_ID" "error names the missing pinned build id"
assert_contains "$RUN_OUT" "no longer exists" "error explains the build is gone"
assert_contains "$RUN_OUT" "uncheckout-build" "error points at uncheckout-build remedy"

# Cleanup.
"$COAST" ssg uncheckout-build >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG PIN-MISSING TESTS PASSED"
echo "==========================================="
