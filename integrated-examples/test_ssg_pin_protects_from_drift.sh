#!/usr/bin/env bash
#
# Integration test: `coast ssg checkout-build` — pinning protects a
# consumer from SSG drift (Phase 16, DESIGN.md §17-9 SETTLED #41).
#
# Scenario:
#   1. Build SSG A (postgres:16-alpine).
#   2. `coast build` + `coast run pin-a` on the consumer -- records
#      ssg.build_id = A. Tear the instance back down.
#   3. `coast ssg checkout-build <A>` for the consumer project.
#      `show-pin` must confirm the pin.
#   4. Mutate the SSG Coastfile + `coast ssg build` -> build B is
#      now `latest`.
#   5. `coast run pin-a` again -- drift check compares recorded (A)
#      against PINNED (A) and proceeds cleanly; no warn about B.
#   6. `coast ssg uncheckout-build` and re-run -- drift now compares
#      against latest (B). Cleanup.

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
echo "=== Step 1: SSG build A ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
BUILD_A_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build A id: $BUILD_A_ID"

"$COAST" ssg run >/dev/null 2>&1
sleep 5

echo ""
echo "=== Step 2: build consumer + run pin-a — records ssg.build_id = A ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("pin-a")
RUN1_OUT=$("$COAST" run pin-a 2>&1)
assert_contains "$RUN1_OUT" "Created coast instance 'pin-a'" "initial consumer run succeeds"

"$COAST" rm pin-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

echo ""
echo "=== Step 3: pin the consumer to SSG build A ==="

PIN_OUT=$("$COAST" ssg checkout-build "$BUILD_A_ID" 2>&1)
echo "$PIN_OUT"
assert_contains "$PIN_OUT" "Pinned project" "checkout-build reports success"
assert_contains "$PIN_OUT" "$BUILD_A_ID" "checkout-build echoes the pinned build id"

SHOW_OUT=$("$COAST" ssg show-pin 2>&1)
echo "$SHOW_OUT"
assert_contains "$SHOW_OUT" "$BUILD_A_ID" "show-pin reports the pinned build id"
assert_contains "$SHOW_OUT" "is pinned to SSG build" "show-pin confirms the pin"

echo ""
echo "=== Step 4: rebuild SSG -> build B becomes latest ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
echo "" >> Coastfile.shared_service_groups
echo "# phase16 pin-protects test — force new build id" >> Coastfile.shared_service_groups
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
BUILD_B_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build B id: $BUILD_B_ID"
[ "$BUILD_A_ID" != "$BUILD_B_ID" ] || fail "expected distinct build ids"
pass "SSG rebuilt (A=$BUILD_A_ID, B=$BUILD_B_ID)"

# The pin must have kept build A alive (auto-prune pin-aware).
[ -d "$HOME/.coast/ssg/builds/$BUILD_A_ID" ] || fail "pinned build A was pruned"
pass "pinned build A survived the rebuild"

echo ""
echo "=== Step 5: run consumer again — pin protects against drift ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("pin-a")
RUN2_OUT=$("$COAST" run pin-a 2>&1)
echo "$RUN2_OUT" | tail -40

assert_contains "$RUN2_OUT" "Created coast instance 'pin-a'" "pinned consumer run succeeds"
assert_contains "$RUN2_OUT" "Checking SSG drift" "drift check still runs under a pin"

# Drift must NOT warn: recorded (A) matches pinned (A) exactly, so
# the "same-image warn" path never fires and build B's id must not
# appear in any warn detail.
if echo "$RUN2_OUT" | grep -qE "image refs still match"; then
    fail "pinned consumer unexpectedly surfaced the same-image warn path; pin should have skipped it"
fi
pass "no drift warn surfaced under the pin"

"$COAST" rm pin-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

echo ""
echo "=== Step 6: uncheckout-build drops the pin ==="

UNPIN_OUT=$("$COAST" ssg uncheckout-build 2>&1)
echo "$UNPIN_OUT"
assert_contains "$UNPIN_OUT" "Unpinned project" "uncheckout-build reports success"

SHOW2_OUT=$("$COAST" ssg show-pin 2>&1)
assert_contains "$SHOW2_OUT" "No SSG build pin" "show-pin reports no pin after uncheckout"

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG PIN-PROTECTS TESTS PASSED"
echo "==========================================="
