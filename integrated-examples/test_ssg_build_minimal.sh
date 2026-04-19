#!/usr/bin/env bash
#
# Integration test: `coast ssg build` on a minimal SSG (Phase 2).
#
# Verifies `coast ssg build` against the coast-ssg-minimal project
# (single postgres service using the `postgres:16-alpine` image):
#
# - Streaming build completes without error.
# - Build artifact is written to ~/.coast/ssg/builds/{build_id}/.
# - `~/.coast/ssg/latest` symlink points at the new build.
# - `manifest.json`, `ssg-coastfile.toml`, `compose.yml` are all present.
# - `coast ssg ps` reports the postgres service from the manifest.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_build_minimal.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG state from other runs.
rm -rf "$HOME/.coast/ssg"

cd "$PROJECTS_DIR/coast-ssg-minimal"

start_daemon

# ============================================================
# Test 1: coast ssg build
# ============================================================

echo ""
echo "=== Test 1: coast ssg build (single service, alpine) ==="

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"

assert_contains "$BUILD_OUT" "Build complete" "coast ssg build succeeds"
assert_contains "$BUILD_OUT" "postgres" "build output mentions postgres service"
pass "SSG build complete"

# ============================================================
# Test 2: artifact layout on disk
# ============================================================

echo ""
echo "=== Test 2: build artifact exists on disk ==="

[ -d "$HOME/.coast/ssg/builds" ] || fail "~/.coast/ssg/builds/ directory missing"
pass "~/.coast/ssg/builds/ exists"

[ -L "$HOME/.coast/ssg/latest" ] || fail "~/.coast/ssg/latest symlink missing"
pass "~/.coast/ssg/latest symlink exists"

LATEST_DIR="$(readlink -f "$HOME/.coast/ssg/latest")"
[ -d "$LATEST_DIR" ] || fail "latest symlink target '$LATEST_DIR' is not a directory"

[ -f "$LATEST_DIR/manifest.json" ] || fail "manifest.json missing in $LATEST_DIR"
[ -f "$LATEST_DIR/ssg-coastfile.toml" ] || fail "ssg-coastfile.toml missing in $LATEST_DIR"
[ -f "$LATEST_DIR/compose.yml" ] || fail "compose.yml missing in $LATEST_DIR"
pass "artifact directory contains manifest.json, ssg-coastfile.toml, compose.yml"

MANIFEST=$(cat "$LATEST_DIR/manifest.json")
assert_contains "$MANIFEST" "postgres" "manifest lists postgres service"
assert_contains "$MANIFEST" "postgres:16-alpine" "manifest records the postgres image"
assert_contains "$MANIFEST" "build_id" "manifest has build_id field"
pass "manifest content is valid"

COMPOSE=$(cat "$LATEST_DIR/compose.yml")
assert_contains "$COMPOSE" "postgres" "compose.yml has postgres service"
assert_contains "$COMPOSE" "image: postgres:16-alpine" "compose.yml uses alpine image"
pass "synthesized compose.yml references the service"

# ============================================================
# Test 3: coast ssg ps reads manifest
# ============================================================

echo ""
echo "=== Test 3: coast ssg ps (reads manifest) ==="

PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT"
assert_contains "$PS_OUT" "postgres" "ssg ps output shows postgres"
assert_contains "$PS_OUT" "postgres:16-alpine" "ssg ps output shows image"
pass "coast ssg ps lists the built service"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG BUILD MINIMAL TESTS PASSED"
echo "==========================================="
