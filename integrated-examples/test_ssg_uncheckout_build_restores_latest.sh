#!/usr/bin/env bash
#
# Integration test: `coast ssg uncheckout-build` — after dropping a
# pin, the consumer's `coast run` resolves against the project's
# current `latest_build_id` (Phase 16 + Phase 31).
#
# Phase 31 reframing: this test originally asserted that an unpinned
# consumer drift-hard-errors against build B. Phase 29 deleted the
# runtime drift audit, so the surviving contract is the pin's
# release semantics:
#
#   - With a pin → runs resolve against the pinned build.
#   - `coast ssg uncheckout-build` drops the row from
#     `ssg_consumer_pins`; `show-pin` reports "no pin".
#   - The next `coast run` resolves against the project's
#     `ssg.latest_build_id` (set by `coast ssg build`), so the
#     consumer's manifest now records the LATEST build_id, not the
#     previously-pinned one.
#
# Scenario:
#   1. Build SSG A (auto-db consumer's image refs).
#   2. `coast build` the consumer at A.
#   3. Pin to A, then rebuild SSG with a DIFFERENT image ref
#      (postgres:16 -> postgres:17) -> build B is latest.
#   4. With the pin still in place, `coast run` succeeds and the
#      consumer's manifest records ssg.build_id = A.
#   5. `uncheckout-build` drops the pin.
#   6. `coast run` again — the consumer's manifest now records
#      ssg.build_id = B (the project's current latest).

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
echo "=== Step 4: run under pin -> consumer manifest records build A ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("unpin-a")
RUN_PINNED_OUT=$("$COAST" run unpin-a 2>&1)
echo "$RUN_PINNED_OUT" | tail -20
assert_contains "$RUN_PINNED_OUT" "Created coast instance 'unpin-a'" "pinned run succeeds under A"

# Phase 31: the consumer's recorded ssg.build_id must equal the
# pinned build (A), not the project's latest (B). Replaces the
# pre-Phase-29 drift "image refs still match" warn assertion.
read_consumer_ssg_build_id() {
    local artifact_latest="$HOME/.coast/images/coast-ssg-consumer-auto-db/latest"
    [ -L "$artifact_latest" ] || { echo ""; return 1; }
    local manifest="$artifact_latest/manifest.json"
    [ -f "$manifest" ] || { echo ""; return 1; }
    python3 -c '
import json, sys
m = json.load(open(sys.argv[1]))
ssg = m.get("ssg", {}) or {}
print(ssg.get("build_id", ""))
' "$manifest"
}
RECORDED_PINNED=$(read_consumer_ssg_build_id)
echo "consumer manifest under pin: ssg.build_id = '$RECORDED_PINNED'"
[ "$RECORDED_PINNED" = "$BUILD_A_ID" ] || fail \
    "expected pinned consumer to record ssg.build_id=A ($BUILD_A_ID); got '$RECORDED_PINNED'"
pass "pinned consumer recorded the pinned build A"

"$COAST" rm unpin-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

echo ""
echo "=== Step 5: uncheckout-build drops the pin ==="

UNPIN_OUT=$("$COAST" ssg uncheckout-build 2>&1)
assert_contains "$UNPIN_OUT" "Unpinned project" "uncheckout succeeds"

echo ""
echo "=== Step 6: run without pin -> consumer resolves against latest (B) ==="

# Phase 31 (post-drift removal): an unpinned consumer's `coast run`
# resolves against the project's current `ssg.latest_build_id`. The
# consumer's manifest now records build B's id, not A's. There is
# NO drift hard-error any more (Phase 29 deleted that path).
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("unpin-a")
set +e
RUN_UNPINNED_OUT=$("$COAST" run unpin-a 2>&1)
EC=$?
set -e
echo "$RUN_UNPINNED_OUT" | tail -20

[ "$EC" -eq 0 ] || fail "unpinned coast run should succeed against latest (B); exit=$EC"
assert_contains "$RUN_UNPINNED_OUT" "Created coast instance 'unpin-a'" "unpinned run succeeds against latest"

RECORDED_LATEST=$(read_consumer_ssg_build_id)
echo "consumer manifest after uncheckout: ssg.build_id = '$RECORDED_LATEST'"
[ "$RECORDED_LATEST" = "$BUILD_B_ID" ] || fail \
    "expected unpinned consumer to record ssg.build_id=B ($BUILD_B_ID); got '$RECORDED_LATEST'"
pass "unpinned consumer recorded the project's latest build (B)"

"$COAST" rm unpin-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

# Restore Coastfile to avoid polluting other tests.
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
mv Coastfile.shared_service_groups.orig Coastfile.shared_service_groups 2>/dev/null || true

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG UNCHECKOUT-BUILD TESTS PASSED"
echo "==========================================="
