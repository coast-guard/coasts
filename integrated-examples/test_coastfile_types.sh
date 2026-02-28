#!/usr/bin/env bash
#
# Integration test for composable Coastfile types.
#
# Tests the extends/includes/unset inheritance system and the --type flag
# on coast build and coast run. Uses the coast-types example project.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_coastfile_types.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

cd "$PROJECTS_DIR/coast-types"

start_daemon

# ============================================================
# Test 1: Build the default type (Coastfile)
# ============================================================

echo ""
echo "=== Test 1: coast build (default type) ==="

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "coast-types" "build references project name"
pass "Default build complete"

# ============================================================
# Test 2: Build a typed variant (Coastfile.light)
# ============================================================

echo ""
echo "=== Test 2: coast build --type light ==="

BUILD_LIGHT=$("$COAST" build --type light 2>&1)
assert_contains "$BUILD_LIGHT" "coast-types" "light build references project name"
pass "Light build complete"

# ============================================================
# Test 3: Build a typed variant with includes (Coastfile.shared)
# ============================================================

echo ""
echo "=== Test 3: coast build --type shared ==="

BUILD_SHARED=$("$COAST" build --type shared 2>&1)
assert_contains "$BUILD_SHARED" "coast-types" "shared build references project name"
pass "Shared build complete"

# ============================================================
# Test 4: Build a chained variant (Coastfile.chain -> light -> base)
# ============================================================

echo ""
echo "=== Test 4: coast build --type chain ==="

BUILD_CHAIN=$("$COAST" build --type chain 2>&1)
assert_contains "$BUILD_CHAIN" "coast-types" "chain build references project name"
pass "Chain build complete"

# ============================================================
# Test 5: Verify --type default is rejected
# ============================================================

echo ""
echo "=== Test 5: coast build --type default (should fail) ==="

if BUILD_DEFAULT=$("$COAST" build --type default 2>&1); then
    fail "--type default should be rejected"
else
    assert_contains "$BUILD_DEFAULT" "default" "error mentions default"
    pass "--type default correctly rejected"
fi

# ============================================================
# Test 6: coast builds ls shows all builds with types
# ============================================================

echo ""
echo "=== Test 6: coast builds ls ==="

BUILDS_LS=$("$COAST" builds ls 2>&1)
assert_contains "$BUILDS_LS" "coast-types" "builds ls lists the project"
pass "builds ls shows project builds"

# ============================================================
# Test 7: coast builds inspect shows type in metadata
# ============================================================

echo ""
echo "=== Test 7: coast builds inspect ==="

INSPECT=$("$COAST" builds inspect 2>&1)
assert_contains "$INSPECT" "Type:" "inspect shows Type field"
pass "builds inspect includes type metadata"

# ============================================================
# Test 8: Run instance using default build
# ============================================================

echo ""
echo "=== Test 8: coast run (default type) ==="

RUN_DEFAULT=$("$COAST" run types-default 2>&1)
CLEANUP_INSTANCES+=("types-default")
assert_contains "$RUN_DEFAULT" "Created coast instance" "default run creates instance"
pass "Instance types-default created (default type)"

# ============================================================
# Test 9: Run instance using light build
# ============================================================

echo ""
echo "=== Test 9: coast run --type light ==="

RUN_LIGHT=$("$COAST" run types-light --type light 2>&1)
CLEANUP_INSTANCES+=("types-light")
assert_contains "$RUN_LIGHT" "Created coast instance" "light run creates instance"
pass "Instance types-light created (light type)"

# ============================================================
# Test 10: coast ls shows TYPE column when typed instances exist
# ============================================================

echo ""
echo "=== Test 10: coast ls (TYPE column) ==="

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "types-default" "ls shows default instance"
assert_contains "$LS_OUT" "types-light" "ls shows light instance"
assert_contains "$LS_OUT" "TYPE" "ls shows TYPE column header"
pass "ls displays TYPE column"

# ============================================================
# Test 11: Cleanup
# ============================================================

echo ""
echo "=== Test 11: coast rm ==="

"$COAST" rm types-default 2>&1 || true
"$COAST" rm types-light 2>&1 || true
pass "Instances removed"

echo ""
echo "============================================"
echo "  All coastfile types tests passed!"
echo "============================================"
