#!/usr/bin/env bash
#
# Integration test: consumer references an SSG service that does not
# exist in the active SSG build (Phase 4, DESIGN.md §6.1 bullet 2).
#
# `coast build` succeeds because build-time cross-checking against the
# SSG is Phase 7 (drift detection) and not in scope here. `coast run`
# must fail fast with the DESIGN.md-shaped error message mentioning
# both the nonexistent service name and the actually-available names.
#
# Prerequisites:
#   - Docker running
#   - Coast binaries built

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

# ============================================================
# Step 1: SSG build + run (so the synth function can actually
# load the manifest to list available services).
# ============================================================

echo ""
echo "=== Step 1: SSG build + run (minimal = postgres only) ==="

cd "$PROJECTS_DIR/coast-ssg-minimal"
SSG_BUILD_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
echo "$SSG_BUILD_OUT" | tail -5
assert_contains "$SSG_BUILD_OUT" "Build complete" "ssg build succeeds"

SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$SSG_RUN_OUT" | tail -5
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

# ============================================================
# Step 2: consumer build (this should succeed — the consumer
# Coastfile is syntactically valid; validation against the SSG
# manifest happens at run time).
# ============================================================

echo ""
echo "=== Step 2: consumer build ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-missing"
CONSUMER_BUILD_OUT=$("$COAST" build 2>&1)
echo "$CONSUMER_BUILD_OUT" | tail -5
assert_contains "$CONSUMER_BUILD_OUT" "Build" "consumer build succeeds (validation is at run time)"

# ============================================================
# Step 3: consumer run — must fail with DESIGN-shaped error.
# ============================================================

echo ""
echo "=== Step 3: coast run (expected to fail) ==="

CLEANUP_INSTANCES+=("inst-a")
set +e
RUN_OUT=$("$COAST" run inst-a 2>&1)
RUN_RC=$?
set -e
echo "$RUN_OUT"
echo "exit code: $RUN_RC"

[ "$RUN_RC" -ne 0 ] || fail "coast run with missing SSG service must exit non-zero"
pass "coast run exited non-zero"

assert_contains "$RUN_OUT" "nonexistent_svc" \
    "error mentions the nonexistent service name"
assert_contains "$RUN_OUT" "does not exist in the active SSG build" \
    "error states that the service is missing from the active SSG build"
assert_contains "$RUN_OUT" "Available services" \
    "error lists the actually-available SSG services"
assert_contains "$RUN_OUT" "postgres" \
    "available-list mentions the actually-published postgres service"

# Sanity: the instance must NOT have been created on the host.
DOCKER_PS=$(docker ps -a --filter "name=^coast-ssg-consumer-missing-coasts-inst-a$" --format "{{.Names}}")
if [ -n "$DOCKER_PS" ]; then
    fail "consumer instance container still exists after missing-service run"
fi
pass "consumer run aborted before creating an instance container"

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER MISSING SERVICE TESTS PASSED"
echo "==========================================="
