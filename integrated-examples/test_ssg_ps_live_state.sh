#!/usr/bin/env bash
#
# Integration test: `coast ssg ps` merges live state from the state DB
# (Phase 9 SETTLED #36, backfilled in Phase 14).
#
# Walks the SSG through build → run → stop and asserts that `ps`
# reflects each state:
#   1. After `ssg build` (no run): status shows `built`, no live
#      port row.
#   2. After `ssg run`: status shows `running` and the ports table
#      shows the real dynamic host port (same value `ssg ports`
#      reports).
#   3. After `ssg stop`: status shows `stopped`.

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

echo ""
echo "=== Step 1: built but not run — ps shows status=built ==="

"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" >/dev/null 2>&1

PS_BUILT=$("$COAST" ssg ps 2>&1)
echo "$PS_BUILT"
assert_contains "$PS_BUILT" "built" "pre-run ps reports status=built"
# Pre-run there should be no live port row, so no DYNAMIC column.
if echo "$PS_BUILT" | grep -q "DYNAMIC"; then
    fail "pre-run ps must not include the ports table (no live rows yet)"
fi
pass "pre-run ps has no live port table"

echo ""
echo "=== Step 2: coast ssg run — ps shows status=running + live dynamic port ==="

"$COAST" ssg run >/dev/null 2>&1
sleep 5

PS_RUNNING=$("$COAST" ssg ps 2>&1)
echo "$PS_RUNNING"
assert_contains "$PS_RUNNING" "running" "post-run ps reports status=running"

# Cross-reference ssg ports for the live dynamic host port value.
PORTS_DYN=$("$COAST" ssg ports 2>&1 | awk '/^  postgres/ {print $3}')
[ -n "$PORTS_DYN" ] || fail "could not read live dynamic port from ssg ports"
pass "live dynamic host port (from ssg ports): $PORTS_DYN"

# That exact value must appear somewhere in the ps output.
assert_contains "$PS_RUNNING" "$PORTS_DYN" "post-run ps includes the live dynamic host port"

echo ""
echo "=== Step 3: coast ssg stop — ps shows status=stopped ==="

"$COAST" ssg stop >/dev/null 2>&1
sleep 2

PS_STOPPED=$("$COAST" ssg ps 2>&1)
echo "$PS_STOPPED"
assert_contains "$PS_STOPPED" "stopped" "post-stop ps reports status=stopped"

# Cleanup.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG PS LIVE STATE TESTS PASSED"
echo "==========================================="
