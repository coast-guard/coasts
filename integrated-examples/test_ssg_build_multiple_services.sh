#!/usr/bin/env bash
#
# Integration test: `coast ssg build` with multiple services (Phase 2).
#
# Verifies `coast ssg build` against the coast-ssg-multi-service project
# (postgres + redis, both `*-alpine`):
#
# - Streaming build succeeds.
# - Manifest lists both services, alphabetically sorted.
# - Compose file defines both services.
# - `coast ssg ps` output shows both services.
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

cd "$PROJECTS_DIR/coast-ssg-multi-service"

start_daemon

# ============================================================
# Test 1: coast ssg build with two services
# ============================================================

echo ""
echo "=== Test 1: coast ssg build (postgres + redis) ==="

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"

assert_contains "$BUILD_OUT" "Build complete" "coast ssg build succeeds"
assert_contains "$BUILD_OUT" "postgres" "build output mentions postgres"
assert_contains "$BUILD_OUT" "redis" "build output mentions redis"
pass "multi-service SSG build complete"

# ============================================================
# Test 2: manifest contains both services, sorted
# ============================================================

echo ""
echo "=== Test 2: manifest shape ==="

LATEST_DIR="$(readlink -f "$HOME/.coast/ssg/latest")"
MANIFEST=$(cat "$LATEST_DIR/manifest.json")

assert_contains "$MANIFEST" "postgres:16-alpine" "manifest has postgres image"
assert_contains "$MANIFEST" "redis:7-alpine" "manifest has redis image"

# postgres should appear before redis (alphabetical sort).
POSTGRES_POS=$(echo "$MANIFEST" | grep -b -o '"name": "postgres"' | head -1 | cut -d: -f1)
REDIS_POS=$(echo "$MANIFEST" | grep -b -o '"name": "redis"' | head -1 | cut -d: -f1)
if [ -n "$POSTGRES_POS" ] && [ -n "$REDIS_POS" ] && [ "$POSTGRES_POS" -lt "$REDIS_POS" ]; then
    pass "manifest services are sorted alphabetically (postgres before redis)"
else
    fail "manifest service ordering is wrong (postgres=$POSTGRES_POS, redis=$REDIS_POS)"
fi

# ============================================================
# Test 3: synthesized compose.yml has both services
# ============================================================

echo ""
echo "=== Test 3: synthesized compose.yml ==="

COMPOSE=$(cat "$LATEST_DIR/compose.yml")
assert_contains "$COMPOSE" "postgres:" "compose has postgres service"
assert_contains "$COMPOSE" "redis:" "compose has redis service"
assert_contains "$COMPOSE" "postgres:16-alpine" "postgres image correct"
assert_contains "$COMPOSE" "redis:7-alpine" "redis image correct"
pass "synthesized compose has both services"

# ============================================================
# Test 4: coast ssg ps shows both services
# ============================================================

echo ""
echo "=== Test 4: coast ssg ps output ==="

PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT"
assert_contains "$PS_OUT" "postgres" "ssg ps shows postgres"
assert_contains "$PS_OUT" "redis" "ssg ps shows redis"
pass "coast ssg ps lists both services"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG MULTI-SERVICE TESTS PASSED"
echo "==========================================="
