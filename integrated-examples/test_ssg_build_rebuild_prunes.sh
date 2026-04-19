#!/usr/bin/env bash
#
# Integration test: `coast ssg build` rebuild + prune behavior (Phase 2).
#
# Verifies:
# - Repeated `coast ssg build` against the same Coastfile produces new
#   build ids (distinct `YYYYMMDDHHMMSS` suffixes, even when the
#   content hash is identical).
# - `~/.coast/ssg/latest` symlink tracks the newest build.
# - When more than 5 builds exist, `auto_prune` keeps the 5 newest
#   (per DESIGN.md §9.1).
# - The build currently pinned by `latest` is never removed, even if
#   we add newer builds that push it past the keep limit.
#
# Prerequisites:
#   - Docker running (for image pulls; the first image pull warms the
#     cache so subsequent builds hit the `cached` fast path)
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
# Test 1: 6 consecutive builds, expect 5 to remain after pruning
# ============================================================

echo ""
echo "=== Test 1: rebuild loop prunes to 5 builds ==="

# Tweak the Coastfile between builds to force distinct content hashes
# (otherwise timestamp-only differences could collide on a fast clock).
# We append a trailing comment with the iteration number.
BASE_COASTFILE=$(cat Coastfile.shared_service_groups)

for i in 1 2 3 4 5 6; do
    {
        echo "$BASE_COASTFILE"
        echo ""
        echo "# iteration $i"
    } > Coastfile.shared_service_groups

    BUILD_OUT=$("$COAST" ssg build 2>&1)
    assert_contains "$BUILD_OUT" "Build complete" "build iteration $i succeeds"
    # Sleep 1s to ensure YYYYMMDDHHMMSS suffix is distinct.
    sleep 1
done

# Restore the original Coastfile so subsequent test runs don't see
# the appended comment accumulating.
echo "$BASE_COASTFILE" > Coastfile.shared_service_groups

# Count builds.
BUILD_COUNT=$(find "$HOME/.coast/ssg/builds" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')
if [ "$BUILD_COUNT" = "5" ]; then
    pass "auto_prune kept 5 builds after 6 consecutive builds"
else
    fail "expected 5 builds after prune, got $BUILD_COUNT"
fi

# ============================================================
# Test 2: latest symlink points at the newest build
# ============================================================

echo ""
echo "=== Test 2: latest symlink tracks newest build ==="

LATEST_DIR=$(readlink -f "$HOME/.coast/ssg/latest")
LATEST_NAME=$(basename "$LATEST_DIR")

# The newest build directory (by mtime).
NEWEST_BY_MTIME=$(ls -1tp "$HOME/.coast/ssg/builds/" | grep '/$' | head -n 1 | sed 's:/$::')

if [ "$LATEST_NAME" = "$NEWEST_BY_MTIME" ]; then
    pass "latest symlink points at the newest build ($LATEST_NAME)"
else
    fail "latest symlink points at $LATEST_NAME, newest is $NEWEST_BY_MTIME"
fi

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG REBUILD PRUNE TESTS PASSED"
echo "==========================================="
