#!/usr/bin/env bash
#
# Integration test: `coast ssg checkout-build` — pinning anchors a
# consumer to a specific SSG `build_id` (Phase 16 + Phase 31).
#
# Phase 31 reframing: this test originally exercised the pre-§24
# "drift" audit (same-image warn / hard error) under a pin. Phase 29
# deleted that machinery, so the only surviving contract is the
# pin's reproducibility guarantee:
#
#   - `coast ssg checkout-build <id>` writes a pin in
#     `ssg_consumer_pins`.
#   - `coast ssg show-pin` reflects it.
#   - Subsequent SSG rebuilds DO NOT prune the pinned build dir
#     (auto-prune-preserving keeps it alive even after newer builds).
#   - `coast run` against the pinned project resolves the consumer's
#     SSG manifest from the pinned build, not `latest_build_id`.
#   - `coast ssg uncheckout-build` releases the pin and a follow-up
#     run resolves against the project's current `latest_build_id`.
#
# Scenario:
#   1. Build SSG A and run it.
#   2. `coast build` + `coast run pin-a` records `ssg.build_id = A`.
#      Tear the instance down.
#   3. `coast ssg checkout-build <A>`. `show-pin` confirms the pin.
#   4. Mutate the SSG Coastfile + `coast ssg build` → B is now latest.
#   5. Pin keeps build A's directory alive on disk.
#   6. `coast run pin-a` resolves against the PINNED build (A); the
#      consumer's manifest's `ssg.build_id` should equal A.
#   7. `coast ssg uncheckout-build` and re-run; the new instance
#      resolves against latest (B).

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

# Phase 25.5: mutate the consumer's OWN SSG Coastfile + rebuild.
cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
echo "" >> Coastfile.shared_service_groups
echo "# phase16 pin-anchors test — force new build id" >> Coastfile.shared_service_groups
"$COAST" ssg build >/dev/null 2>&1
BUILD_B_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build B id: $BUILD_B_ID"
[ "$BUILD_A_ID" != "$BUILD_B_ID" ] || fail "expected distinct build ids"
pass "SSG rebuilt (A=$BUILD_A_ID, B=$BUILD_B_ID)"

# Phase 16's auto-prune-preserving keeps the pinned build dir on disk
# even after a newer build supersedes `latest`. This is the
# reproducibility anchor — the pinned consumer can replay against the
# exact same artifact tree.
[ -d "$HOME/.coast/ssg/builds/$BUILD_A_ID" ] || fail "pinned build A was pruned"
pass "pinned build A survived the rebuild"

echo ""
echo "=== Step 5: run consumer again — pinned build resolves under the pin ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("pin-a")
RUN2_OUT=$("$COAST" run pin-a 2>&1)
echo "$RUN2_OUT" | tail -40

assert_contains "$RUN2_OUT" "Created coast instance 'pin-a'" "pinned consumer run succeeds"

# Phase 31: the consumer's recorded `ssg.build_id` in the artifact's
# manifest.json must equal the PINNED build (A), not the project's
# current latest_build_id (B). The drift audit that used to verify
# this is gone (Phase 29) so we read the manifest directly.
ARTIFACT_DIR="$HOME/.coast/images/coast-ssg-consumer-auto-db"
LATEST_LINK="$ARTIFACT_DIR/latest"
[ -L "$LATEST_LINK" ] || fail "expected $LATEST_LINK to be a symlink to the consumer's build dir"
MANIFEST="$LATEST_LINK/manifest.json"
[ -f "$MANIFEST" ] || fail "expected consumer manifest at $MANIFEST"

# Match the `"build_id": "<value>"` line inside the `"ssg"` block.
RECORDED_SSG=$(python3 -c '
import json, sys
m = json.load(open(sys.argv[1]))
ssg = m.get("ssg", {}) or {}
print(ssg.get("build_id", ""))
' "$MANIFEST")
echo "consumer manifest recorded ssg.build_id = '$RECORDED_SSG'"

[ "$RECORDED_SSG" = "$BUILD_A_ID" ] || fail \
    "expected pinned consumer to record ssg.build_id=A ($BUILD_A_ID); got '$RECORDED_SSG' (latest is B=$BUILD_B_ID)"
pass "pinned consumer resolved against build A despite B being latest"

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
echo "  ALL SSG PIN-ANCHORS-BUILD TESTS PASSED"
echo "==========================================="
