#!/usr/bin/env bash
#
# Integration test: Phase 1 parse-time conflicts surface cleanly at
# `coast build` (Phase 4 assertion).
#
# Two sub-cases per DESIGN.md §6.1:
#   (a) `[shared_services.postgres] from_group = true, image = "..."`.
#       Phase 1 forbidden-field check. `coast build` fails listing
#       `image` as forbidden.
#   (b) Two `[shared_services.postgres]` blocks in the same file.
#       TOML disallows duplicate keys at the lexer level; `coast
#       build` fails with a TOML parse error mentioning the section.
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

start_daemon

# ============================================================
# Case (a): forbidden-field conflict
# ============================================================

echo ""
echo "=== Case (a): from_group = true + forbidden image field ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-conflict-forbidden"
set +e
FORBIDDEN_OUT=$("$COAST" build 2>&1)
FORBIDDEN_RC=$?
set -e
echo "$FORBIDDEN_OUT"
echo "exit code: $FORBIDDEN_RC"

[ "$FORBIDDEN_RC" -ne 0 ] || fail "coast build with forbidden fields must exit non-zero"
pass "coast build exited non-zero for forbidden-field conflict"

assert_contains "$FORBIDDEN_OUT" "from_group" "error mentions from_group"
assert_contains "$FORBIDDEN_OUT" "image" "error mentions the forbidden 'image' field"
assert_contains "$FORBIDDEN_OUT" "postgres" "error mentions the conflicting service name"

# ============================================================
# Case (b): TOML duplicate-key conflict
# ============================================================

echo ""
echo "=== Case (b): duplicate [shared_services.postgres] sections ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-conflict-duplicate"
set +e
DUPLICATE_OUT=$("$COAST" build 2>&1)
DUPLICATE_RC=$?
set -e
echo "$DUPLICATE_OUT"
echo "exit code: $DUPLICATE_RC"

[ "$DUPLICATE_RC" -ne 0 ] || fail "coast build with duplicate sections must exit non-zero"
pass "coast build exited non-zero for duplicate-section conflict"

# TOML parsers typically surface "duplicate key" or similar wording
# when a table is redefined. Accept either phrasing, but the section
# name must be cited so the user can locate the offense.
if echo "$DUPLICATE_OUT" | grep -qE "(duplicate|redefin|already|defined more than once)"; then
    pass "error mentions the duplicate/redefinition condition"
else
    fail "error does not clearly indicate a duplicate-section problem: $DUPLICATE_OUT"
fi
assert_contains "$DUPLICATE_OUT" "shared_services" "error cites the shared_services table"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER CONFLICT TESTS PASSED"
echo "==========================================="
