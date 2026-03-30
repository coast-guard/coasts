#!/usr/bin/env bash
#
# Integration test for Coastfile .toml extension support.
#
# Verifies that Coastfile.toml and Coastfile.{type}.toml work end-to-end
# through the real daemon: build, extends resolution, run, ls, and the
# tie-break rule (prefer .toml when both exist).
#
# Reuses the coast-types example project by renaming files in-place.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_coastfile_toml.sh

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

# Save originals so we can restore in cleanup
cp Coastfile Coastfile.orig
cp Coastfile.light Coastfile.light.orig

start_daemon

# ============================================================
# Test 1: Build default type from Coastfile.toml
# ============================================================

echo ""
echo "=== Test 1: coast build with Coastfile.toml ==="

mv Coastfile Coastfile.toml

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "coast-types" "toml build references project name"
assert_contains "$BUILD_OUT" "Build complete" "toml build succeeds"
pass "Default .toml build complete"

# ============================================================
# Test 2: Build typed variant from Coastfile.light.toml
#          (also tests extends = "Coastfile" discovering Coastfile.toml)
# ============================================================

echo ""
echo "=== Test 2: coast build --type light with Coastfile.light.toml ==="

mv Coastfile.light Coastfile.light.toml

BUILD_LIGHT=$("$COAST" build --type light 2>&1)
assert_contains "$BUILD_LIGHT" "coast-types" "light .toml build references project"
assert_contains "$BUILD_LIGHT" "Build complete" "light .toml build succeeds"
pass "Typed .toml build complete (extends resolved Coastfile.toml)"

# ============================================================
# Test 3: --type toml is rejected (reserved name)
# ============================================================

echo ""
echo "=== Test 3: coast build --type toml (should fail) ==="

if BUILD_TOML=$("$COAST" build --type toml 2>&1); then
    fail "--type toml should be rejected"
else
    assert_contains "$BUILD_TOML" "reserved" "error mentions reserved"
    pass "--type toml correctly rejected"
fi

# ============================================================
# Test 4: Run instance from .toml build, verify with ls
# ============================================================

echo ""
echo "=== Test 4: coast run + ls with .toml build ==="

RUN_OUT=$("$COAST" run toml-test-1 2>&1)
CLEANUP_INSTANCES+=("toml-test-1")
assert_contains "$RUN_OUT" "Created coast instance" "run creates instance from .toml build"

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "toml-test-1" "ls shows .toml-built instance"
pass "Run + ls with .toml build"

# ============================================================
# Test 5: Tie-break — .toml preferred over plain Coastfile
# ============================================================

echo ""
echo "=== Test 5: Tie-break (.toml wins over plain Coastfile) ==="

# Coastfile.toml already exists with name = "coast-types".
# Create a plain Coastfile with a different name to detect which one wins.
cat > Coastfile << 'EOF'
[coast]
name = "plain-should-lose"
runtime = "dind"
EOF

BUILD_TIE=$("$COAST" build 2>&1)
assert_contains "$BUILD_TIE" "coast-types" ".toml variant was used (tie-break)"
assert_not_contains "$BUILD_TIE" "plain-should-lose" "plain Coastfile was not used"
pass "Tie-break: .toml preferred over plain Coastfile"

# ============================================================
# Test 6: Cleanup
# ============================================================

echo ""
echo "=== Test 6: Cleanup ==="

"$COAST" rm toml-test-1 2>&1 || true
CLEANUP_INSTANCES=()

# Restore original files
rm -f Coastfile Coastfile.toml Coastfile.light.toml
cp Coastfile.orig Coastfile
cp Coastfile.light.orig Coastfile.light
rm -f Coastfile.orig Coastfile.light.orig

pass "Files restored"

echo ""
echo "============================================"
echo "  All Coastfile .toml extension tests passed!"
echo "============================================"
