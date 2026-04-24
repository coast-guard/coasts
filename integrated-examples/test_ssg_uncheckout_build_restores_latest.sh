#!/usr/bin/env bash
#
# Integration test: `coast ssg uncheckout-build` — after dropping a
# pin, drift evaluates against `latest` again (Phase 16).
#
# Scenario:
#   1. Build SSG A (auto-db consumer's image refs).
#   2. `coast build` the consumer at A.
#   3. Pin to A, then rebuild SSG with a DIFFERENT image ref
#      (postgres:16 -> postgres:17) -> build B is latest.
#   4. With the pin still in place, `coast run` must succeed
#      (pin points at A which still matches the consumer's recorded
#      manifest).
#   5. `uncheckout-build` drops the pin.
#   6. `coast run` now evaluates against latest (B) and drift MUST
#      hard-error with the DESIGN §6.1 sentence.

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
echo "=== Step 1: SSG build A (postgres:16-alpine) ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" ssg build >/dev/null 2>&1
BUILD_A_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build A id: $BUILD_A_ID"

"$COAST" ssg run >/dev/null 2>&1
sleep 5

echo ""
echo "=== Step 2: consumer build records A ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

echo ""
echo "=== Step 3: pin consumer to A + rebuild SSG to B with postgres:17 ==="

PIN_OUT=$("$COAST" ssg checkout-build "$BUILD_A_ID" 2>&1)
assert_contains "$PIN_OUT" "Pinned project" "pin succeeds"

# Phase 25.5: mutate the consumer's OWN SSG Coastfile (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
# Swap the postgres image ref to a DIFFERENT tag so build B has
# materially changed image refs (hard-error territory for drift).
sed -i.orig 's|postgres:16-alpine|postgres:17-alpine|g' Coastfile.shared_service_groups
"$COAST" ssg build >/dev/null 2>&1 || {
    # Restore on error.
    mv Coastfile.shared_service_groups.orig Coastfile.shared_service_groups
    fail "SSG build B failed"
}
BUILD_B_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build B id: $BUILD_B_ID"
[ "$BUILD_A_ID" != "$BUILD_B_ID" ] || fail "expected distinct build ids"
pass "SSG rebuilt (A=$BUILD_A_ID, B=$BUILD_B_ID)"

echo ""
echo "=== Step 4: run under pin -> succeeds (pin protects) ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("unpin-a")
RUN_PINNED_OUT=$("$COAST" run unpin-a 2>&1)
echo "$RUN_PINNED_OUT" | tail -20
assert_contains "$RUN_PINNED_OUT" "Created coast instance 'unpin-a'" "pinned run succeeds under A"
"$COAST" rm unpin-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

echo ""
echo "=== Step 5: uncheckout-build drops the pin ==="

UNPIN_OUT=$("$COAST" ssg uncheckout-build 2>&1)
assert_contains "$UNPIN_OUT" "Unpinned project" "uncheckout succeeds"

echo ""
echo "=== Step 6: run without pin -> drift hard-error against latest (B) ==="

set +e
RUN_UNPINNED_OUT=$("$COAST" run unpin-a 2>&1)
EC=$?
set -e

echo "$RUN_UNPINNED_OUT" | tail -30
[ "$EC" -ne 0 ] || fail "expected coast run to fail after uncheckout (drift vs B)"
assert_contains "$RUN_UNPINNED_OUT" "SSG has changed since this coast was built" \
    "drift hard-error surfaces after uncheckout"

# Restore Coastfile to avoid polluting other tests.
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
mv Coastfile.shared_service_groups.orig Coastfile.shared_service_groups 2>/dev/null || true

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG UNCHECKOUT-BUILD TESTS PASSED"
echo "==========================================="
