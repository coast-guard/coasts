#!/usr/bin/env bash
#
# Integration test: SSG drift detection — hard error on missing
# service (Phase 7, DESIGN.md §6.1).
#
# Scenario:
#   1. `coast ssg build` with postgres + redis → SSG build A.
#   2. Build consumer coast referencing BOTH services — manifest
#      records them both.
#   3. Rebuild SSG dropping redis → SSG build B.
#   4. `coast run consumer-a` → drift check sees redis missing →
#      hard-errors with the DESIGN §6.1 verbatim sentence + a
#      missing-service suffix naming redis.
#   5. No coast container was created (fail-fast).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) -- SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-multi"

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
echo "=== Step 1: SSG build A with postgres + redis ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-multi"
"$COAST" ssg build >/dev/null 2>&1
BUILD_A_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build A id: $BUILD_A_ID"

"$COAST" ssg run >/dev/null 2>&1
sleep 5

# Confirm both services are in the active manifest.
PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT" | head -10
assert_contains "$PS_OUT" "postgres" "SSG build A has postgres"
assert_contains "$PS_OUT" "redis" "SSG build A has redis"

echo ""
echo "=== Step 2: build consumer referencing both services ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-multi"
"$COAST" build >/dev/null 2>&1

CONSUMER_LATEST=$(readlink "$HOME/.coast/images/coast-ssg-consumer-multi/latest" | xargs basename)
CONSUMER_MANIFEST="$HOME/.coast/images/coast-ssg-consumer-multi/$CONSUMER_LATEST/manifest.json"
RECORDED_SERVICES=$(python3 -c "import json,sys; print(','.join(json.load(open('$CONSUMER_MANIFEST'))['ssg']['services']))")
echo "consumer recorded services: $RECORDED_SERVICES"
assert_contains "$RECORDED_SERVICES" "postgres" "consumer manifest records postgres"
assert_contains "$RECORDED_SERVICES" "redis" "consumer manifest records redis"

echo ""
echo "=== Step 3: rewrite SSG Coastfile WITHOUT redis + rebuild ==="

# Phase 25.5: rewrite the consumer's OWN SSG Coastfile (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-multi"
cat > Coastfile.shared_service_groups << 'SSG_POSTGRES_ONLY_EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_PASSWORD = "coast" }
SSG_POSTGRES_ONLY_EOF

"$COAST" ssg build >/dev/null 2>&1
BUILD_B_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build B id: $BUILD_B_ID"
[ "$BUILD_A_ID" != "$BUILD_B_ID" ] || fail "expected distinct build ids"

echo ""
echo "=== Step 4: coast run must hard-error with DESIGN §6.1 wording ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-multi"
# We expect this to fail; don't let `set -e` abort the test.
set +e
RUN_OUT=$("$COAST" run drift-a 2>&1)
RUN_RC=$?
set -e
echo "$RUN_OUT"
echo "exit code: $RUN_RC"

[ "$RUN_RC" -ne 0 ] || fail "coast run must exit non-zero when drift is a hard error"
assert_contains "$RUN_OUT" "SSG has changed since this coast was built" \
    "error contains DESIGN \u00a76.1 verbatim sentence"
assert_contains "$RUN_OUT" "redis" "error mentions the missing service"
# The suffix enumerates available services; postgres is still present.
assert_contains "$RUN_OUT" "postgres" "error shows available services in the suffix"

echo ""
echo "=== Step 5: no coast container was created ==="

DRIFT_CONTAINER="coast-ssg-consumer-multi-coasts-drift-a"
RUNNING=$(docker ps -a --filter "name=^${DRIFT_CONTAINER}$" --format "{{.Names}}")
if [ -n "$RUNNING" ]; then
    fail "expected no coast container for drift-a; found: $RUNNING"
fi
pass "drift hard-error aborted before container creation"

# Cleanup.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG DRIFT MISSING-SERVICE TESTS PASSED"
echo "==========================================="
