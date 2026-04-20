#!/usr/bin/env bash
#
# Integration test: `coast run` auto-starts the SSG when a consumer
# coast references SSG services (Phase 3.5).
#
# Verifies two flows:
#
#  1. Positive: an SSG build exists (but the SSG is NOT running), and a
#     consumer coast declaring `[shared_services.<name>] from_group =
#     true` runs. `coast run` auto-starts the SSG and the singleton
#     `coast-ssg` container is up when `coast run` returns. The run
#     progress stream includes an `Ensure SSG ready` step.
#
#  2. Negative: no SSG build exists at all. `coast run` on the
#     consumer fails fast with the DESIGN.md §11.1 verbatim error
#     mentioning the consumer project name and the referenced SSG
#     service name.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_auto_start_on_run.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG + consumer state from other runs.
rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

start_daemon

# ============================================================
# Positive case: SSG build exists, SSG not running, consumer run
# triggers auto-start.
# ============================================================

echo ""
echo "=== Positive: SSG build exists, consumer run auto-starts it ==="

cd "$PROJECTS_DIR/coast-ssg-minimal"
# Pass --working-dir explicitly so the daemon resolves the SSG
# Coastfile against this directory instead of its own cwd.
BUILD_SSG_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
echo "$BUILD_SSG_OUT" | tail -5
assert_contains "$BUILD_SSG_OUT" "Build complete" "coast ssg build succeeds"

# Sanity: SSG is NOT running before the consumer run.
DOCKER_PS_BEFORE=$(docker ps --filter "name=^coast-ssg$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_BEFORE" ]; then
    fail "coast-ssg is already running before the consumer run (expected stopped state)"
fi
pass "coast-ssg is not running before the consumer run"

cd "$PROJECTS_DIR/coast-ssg-consumer"
BUILD_CONSUMER_OUT=$("$COAST" build 2>&1)
echo "$BUILD_CONSUMER_OUT" | tail -10
assert_contains "$BUILD_CONSUMER_OUT" "Build" "coast build on consumer succeeds"

CLEANUP_INSTANCES+=("inst-a")
RUN_OUT=$("$COAST" run inst-a 2>&1)
echo "$RUN_OUT" | tail -20
assert_contains "$RUN_OUT" "Ensure SSG ready" "run output shows the auto-start step"
pass "consumer coast run triggered SSG auto-start"

DOCKER_PS_AFTER=$(docker ps --filter "name=^coast-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS_AFTER" "coast-ssg" "coast-ssg container is running after consumer run"

PS_SSG_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_SSG_OUT"
assert_contains "$PS_SSG_OUT" "postgres" "coast ssg ps shows postgres"

# Cleanup inst-a before the negative case so clean_slate can proceed.
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# ============================================================
# Negative case A: no SSG build exists; `coast build` on the
# consumer hard-errors at build time (DESIGN.md §6). This is the
# new Phase 9 behavior: consumers cannot silently build without an
# active SSG since that would weaken drift detection.
# ============================================================

echo ""
echo "=== Negative A: no SSG build -> coast build hard-errors ==="

# Wipe the SSG state AND the consumer's prior build artifact so we
# start from a truly clean slate.
rm -rf "$HOME/.coast/ssg"
rm -rf "$HOME/.coast/images/coast-ssg-consumer"
docker rm -f coast-ssg 2>/dev/null || true

cd "$PROJECTS_DIR/coast-ssg-consumer"
BUILD_NEG_OUT=$("$COAST" build 2>&1 || true)
echo "$BUILD_NEG_OUT" | tail -10

assert_contains "$BUILD_NEG_OUT" "no SSG build exists" \
    "coast build error mentions the missing SSG build"
assert_contains "$BUILD_NEG_OUT" "coast ssg build" \
    "coast build error directs the user to run coast ssg build first"
assert_contains "$BUILD_NEG_OUT" "postgres" \
    "coast build error names the referenced SSG service"

# ============================================================
# Negative case B: consumer has a stale build artifact but the
# SSG was removed between build and run. The drift validator
# catches this and fails `coast run` with the DESIGN §6.1 error.
# ============================================================

echo ""
echo "=== Negative B: stale consumer build + missing SSG -> drift error ==="

# Rebuild the SSG + consumer so the consumer has a valid artifact
# with an `ssg` block pointing at the active SSG.
cd "$PROJECTS_DIR/coast-ssg-minimal"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" >/dev/null 2>&1
cd "$PROJECTS_DIR/coast-ssg-consumer"
"$COAST" build >/dev/null 2>&1

# Now wipe the SSG so the consumer has a stale reference.
rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true

CLEANUP_INSTANCES+=("inst-b")
NEG_OUT=$("$COAST" run inst-b 2>&1 || true)
echo "$NEG_OUT" | tail -15

assert_contains "$NEG_OUT" "SSG has changed since this coast was built" \
    "drift error fires (DESIGN.md \u00a76.1 verbatim prefix)"
assert_contains "$NEG_OUT" "no SSG build exists now" \
    "drift error explains the SSG is gone"

# The instance should NOT have been created on the host.
DOCKER_PS_NEG=$(docker ps -a --filter "name=^coast-ssg-consumer-coasts-inst-b$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_NEG" ]; then
    fail "consumer instance container still exists after negative-case run"
fi
pass "consumer run aborted before creating an instance container"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG AUTO-START TESTS PASSED"
echo "==========================================="
