#!/usr/bin/env bash
#
# Integration test for coastfile-less builds.
#
# Tests building a coast project using only CLI flags (--name, --compose,
# --port, --config) without a Coastfile on disk, verifying that the full
# build/run lifecycle works, and that CLI flag overrides work on projects
# that DO have a Coastfile.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

start_daemon

# ============================================================
# Test 1: Coastfile-less build with --name and bare project
# ============================================================

echo ""
echo "=== Test 1: Coastfile-less build (bare, no compose) ==="

cd "$PROJECTS_DIR/coast-no-coastfile"

# Verify there is NO Coastfile
[ ! -f Coastfile ] && [ ! -f Coastfile.toml ] || fail "Coastfile should not exist"
pass "No Coastfile present"

BUILD_OUT=$("$COAST" build --name test-nocoastfile 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coastfile-less build succeeds"
assert_contains "$BUILD_OUT" "test-nocoastfile" "build output references --name"
pass "Coastfile-less build complete"

# ============================================================
# Test 2: Run the coastfile-less build
# ============================================================

echo ""
echo "=== Test 2: coast run on coastfile-less project ==="

RUN_OUT=$("$COAST" run nocoast-1 2>&1)
CLEANUP_INSTANCES+=("nocoast-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run nocoast-1 succeeds"
pass "Instance nocoast-1 created"

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "nocoast-1" "ls shows nocoast-1"
pass "Instance listed"

# ============================================================
# Test 3: Error case — coastfile-less without --name
# ============================================================

echo ""
echo "=== Test 3: Error without --name ==="

cd "$PROJECTS_DIR/coast-no-coastfile"
ERROR_OUT=$("$COAST" build 2>&1 || true)
assert_contains "$ERROR_OUT" "No Coastfile found" "error mentions missing Coastfile"
pass "Proper error for missing Coastfile"

# ============================================================
# Test 4: Port flag
# ============================================================

echo ""
echo "=== Test 4: --port flag ==="

BUILD_PORT_OUT=$("$COAST" build --name test-ports --port web=3000 --port api=8080 2>&1)
assert_contains "$BUILD_PORT_OUT" "Build complete" "build with --port flags succeeds"
pass "Port flags accepted"

# ============================================================
# Test 5: CLI override on project WITH Coastfile
# ============================================================

echo ""
echo "=== Test 5: CLI flag override on existing Coastfile ==="

cd "$PROJECTS_DIR/coast-simple"

OVERRIDE_OUT=$("$COAST" build --name override-name 2>&1)
assert_contains "$OVERRIDE_OUT" "Build complete" "build with --name override succeeds"
assert_contains "$OVERRIDE_OUT" "override-name" "output uses overridden name"
pass "CLI flag override works"

# ============================================================
# Test 6: --config flag with inline TOML
# ============================================================

echo ""
echo "=== Test 6: --config flag ==="

cd "$PROJECTS_DIR/coast-no-coastfile"

CONFIG_TOML='[coast.setup]
packages = ["curl"]'

CONFIG_OUT=$("$COAST" build --name test-config --config "$CONFIG_TOML" 2>&1)
assert_contains "$CONFIG_OUT" "Build complete" "build with --config succeeds"
pass "--config flag accepted"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm nocoast-1 2>/dev/null || true
CLEANUP_INSTANCES=()

echo ""
echo "==========================================="
echo "  ALL COASTFILE-LESS BUILD TESTS PASSED"
echo "==========================================="
