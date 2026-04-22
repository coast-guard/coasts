#!/usr/bin/env bash
#
# Integration test: global `coast --working-dir <dir> ssg build`
# reaches the subcommand (Phase 9 SETTLED #31, backfilled in Phase 14).
#
# Verifies two call shapes:
#   1. `coast --working-dir <dir> ssg build` from an unrelated cwd
#      succeeds (global flag flows through the top-level dispatch).
#   2. When both `coast --working-dir` and `ssg build --working-dir`
#      are set, the subcommand flag wins.

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

start_daemon

echo ""
echo "=== Test 1: coast --working-dir <dir> ssg build (from unrelated cwd) ==="

# Run from $HOME so the Coastfile.shared_service_groups lookup cannot
# succeed via cwd resolution — only the global flag can point the
# parser at the right dir.
cd "$HOME"
OUT=$("$COAST" --working-dir "$PROJECTS_DIR/coast-ssg-minimal" ssg build 2>&1)
echo "$OUT" | tail -10
assert_contains "$OUT" "Build complete" "global --working-dir reaches ssg build"

# Sanity: the build artifact landed under ~/.coast/ssg/.
[ -L "$HOME/.coast/ssg/latest" ] \
    || fail "expected ~/.coast/ssg/latest to exist after global --working-dir build"
pass "latest symlink present after global --working-dir build"

echo ""
echo "=== Test 2: subcommand --working-dir wins when both are set ==="

# Point the global flag at a bogus dir but the subcommand flag at the
# real fixture — build must succeed, proving the subcommand flag took
# precedence over the global one.
rm -rf "$HOME/.coast/ssg"
cd "$HOME"
OUT2=$("$COAST" --working-dir "/does/not/exist/definitely-not-here" ssg build \
    --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
echo "$OUT2" | tail -10
assert_contains "$OUT2" "Build complete" "subcommand --working-dir wins over global"

[ -L "$HOME/.coast/ssg/latest" ] \
    || fail "expected ~/.coast/ssg/latest to exist after subcommand-wins build"
pass "latest symlink present after subcommand-wins build"

echo ""
echo "==========================================="
echo "  ALL SSG BUILD GLOBAL --working-dir TESTS PASSED"
echo "==========================================="
