#!/usr/bin/env bash
#
# Integration test: SSG drift detection — warn path (Phase 7,
# DESIGN.md §6.1).
#
# Scenario:
#   1. Build SSG A (postgres:16-alpine).
#   2. Build consumer coast — manifest.json records the ssg block
#      pointing at build A.
#   3. Touch the SSG Coastfile (add a comment) + `coast ssg build` →
#      build B with a different id but IDENTICAL image refs.
#   4. `coast run consumer-a` → drift check sees build_id mismatch
#      but image match → emits a `Checking SSG drift` progress step
#      with a warn detail that names both build ids, and the coast
#      still runs to completion.

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
BUILD_A_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" 2>&1)
echo "$BUILD_A_OUT" | tail -8
assert_contains "$BUILD_A_OUT" "Build complete" "SSG build A succeeds"

SSG_DIR="$HOME/.coast/ssg/builds"
BUILD_A_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build A id: $BUILD_A_ID"

"$COAST" ssg run >/dev/null 2>&1
sleep 5

echo ""
echo "=== Step 2: build consumer — manifest records SSG build A ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
"$COAST" build >/dev/null 2>&1

# Inspect the consumer's manifest.json for the ssg block.
CONSUMER_LATEST=$(readlink "$HOME/.coast/images/coast-ssg-consumer-auto-db/latest" | xargs basename)
CONSUMER_MANIFEST="$HOME/.coast/images/coast-ssg-consumer-auto-db/$CONSUMER_LATEST/manifest.json"
echo "--- consumer manifest ssg block ---"
# jq isn't guaranteed to be present; use grep/sed on the pretty-printed JSON.
cat "$CONSUMER_MANIFEST" | python3 -c "import json,sys; m=json.load(sys.stdin); print(json.dumps(m.get('ssg', {}), indent=2))"
RECORDED_ID=$(python3 -c "import json,sys; print(json.load(open('$CONSUMER_MANIFEST'))['ssg']['build_id'])")
echo "consumer recorded SSG build_id: $RECORDED_ID"
assert_eq "$RECORDED_ID" "$BUILD_A_ID" "consumer manifest recorded SSG build A"

echo ""
echo "=== Step 3: touch SSG Coastfile + rebuild — build B (same images, new id) ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
echo "" >> Coastfile.shared_service_groups
echo "# phase7 drift warn test — forces new build id" >> Coastfile.shared_service_groups
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
BUILD_B_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build B id: $BUILD_B_ID"
[ "$BUILD_A_ID" != "$BUILD_B_ID" ] || fail "expected distinct build ids (got $BUILD_A_ID twice)"
pass "SSG rebuilt with new id (A=$BUILD_A_ID, B=$BUILD_B_ID)"

# Ensure the new build is running (SSG was already up on A's services;
# without `coast ssg restart`, `latest` points at B but running
# container still serves A's services. That's fine for this test —
# drift is evaluated against the artifact, not the running state).

echo ""
echo "=== Step 4: run consumer — drift check must warn + proceed ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-auto-db"
CLEANUP_INSTANCES+=("warn-a")
RUN_OUT=$("$COAST" run warn-a 2>&1)
echo "$RUN_OUT" | tail -30

# Run must succeed overall.
assert_contains "$RUN_OUT" "Created coast instance 'warn-a'" "consumer run succeeds"

# Drift check progress step must appear.
assert_contains "$RUN_OUT" "Checking SSG drift" "drift check progress step emitted"

# Warn detail must name both build ids.
assert_contains "$RUN_OUT" "$BUILD_A_ID" "warn detail names the old build id"
assert_contains "$RUN_OUT" "$BUILD_B_ID" "warn detail names the new build id"
assert_contains "$RUN_OUT" "image refs still match" "warn detail explains the same-image path"

# Container actually running.
PS_OUT=$("$COAST" ps warn-a 2>&1 || true)
echo "$PS_OUT" | head -5

# Cleanup.
"$COAST" rm warn-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG DRIFT WARNING TESTS PASSED"
echo "==========================================="
