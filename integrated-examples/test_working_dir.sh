#!/usr/bin/env bash
#
# Integration test for --working-dir flag.
#
# Tests that --working-dir decouples the build's registered project_root
# from the Coastfile's location, verifying manifest content, coast lookup
# behavior, and combined use with coastfile-less builds.
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
# Test 1: Build with --working-dir sets project_root
# ============================================================

echo ""
echo "=== Test 1: Build with --working-dir ==="

cd "$PROJECTS_DIR/coast-working-dir"

WD_TARGET="/tmp/coast-wd-test-target"
mkdir -p "$WD_TARGET"

BUILD_OUT=$("$COAST" --working-dir "$WD_TARGET" build 2>&1) || {
    echo "  Build failed with output:"
    echo "$BUILD_OUT"
    fail "build with --working-dir failed"
}
assert_contains "$BUILD_OUT" "Build complete" "build with --working-dir succeeds"
pass "Build with --working-dir complete"

# ============================================================
# Test 2: Verify manifest stores correct project_root
# ============================================================

echo ""
echo "=== Test 2: Verify manifest project_root ==="

INSPECT_OUT=$("$COAST" builds inspect coast-working-dir 2>&1)
assert_contains "$INSPECT_OUT" "$WD_TARGET" "inspect shows --working-dir as project_root"
pass "Manifest project_root is --working-dir target"

# ============================================================
# Test 3: Build with --working-dir and coastfile-less flags
# ============================================================

echo ""
echo "=== Test 3: --working-dir + coastfile-less ==="

cd /tmp

WD_COMBINED="/tmp/coast-wd-combined-target"
mkdir -p "$WD_COMBINED"

BUILD_COMBINED=$("$COAST" --working-dir "$WD_COMBINED" build --name wd-nocoast 2>&1) || {
    echo "  Build failed with output:"
    echo "$BUILD_COMBINED"
    fail "coastfile-less + --working-dir build failed"
}
assert_contains "$BUILD_COMBINED" "Build complete" "coastfile-less + --working-dir build succeeds"
pass "Combined coastfile-less + --working-dir build complete"

# ============================================================
# Test 4: Run instance and verify --working-dir project
# ============================================================

echo ""
echo "=== Test 4: Run instance from --working-dir build ==="

cd "$PROJECTS_DIR/coast-working-dir"
RUN_OUT=$("$COAST" run wd-1 2>&1)
CLEANUP_INSTANCES+=("wd-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run wd-1 succeeds"
pass "Instance created from --working-dir build"

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "wd-1" "ls shows wd-1"

# ============================================================
# Test 5: Relative path for --working-dir
# ============================================================

echo ""
echo "=== Test 5: Relative --working-dir path ==="

cd "$PROJECTS_DIR/coast-working-dir"

REL_TARGET="$PROJECTS_DIR/coast-working-dir/rel-wd-target"
mkdir -p "$REL_TARGET"

BUILD_REL=$("$COAST" --working-dir ./rel-wd-target build 2>&1) || {
    echo "  Build failed with output:"
    echo "$BUILD_REL"
    fail "build with relative --working-dir failed"
}
assert_contains "$BUILD_REL" "Build complete" "build with relative --working-dir succeeds"
pass "Relative --working-dir path resolved"

rmdir "$REL_TARGET" 2>/dev/null || true

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm wd-1 2>/dev/null || true
CLEANUP_INSTANCES=()
rm -rf "$WD_TARGET" "$WD_COMBINED"

echo ""
echo "==========================================="
echo "  ALL WORKING-DIR TESTS PASSED"
echo "==========================================="
