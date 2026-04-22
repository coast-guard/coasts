#!/usr/bin/env bash
#
# Integration test: `coast ssg checkout <SERVICE>` (positional) and
# `coast ssg checkout --service <SERVICE>` (flag form) both work and
# produce equivalent state (Phase 9 SETTLED #32, backfilled in
# Phase 14).
#
# Also asserts the rejection cases:
#   - Conflicting positional + flag values (e.g. `checkout postgres --service redis`)
#   - `--all` combined with a specific service

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
echo "=== Step 1: SSG up ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1

# Wait for postgres initdb inside the SSG.
sleep 6

echo ""
echo "=== Step 2: positional form — coast ssg checkout postgres ==="

POS_OUT=$("$COAST" ssg checkout postgres 2>&1)
echo "$POS_OUT"
assert_contains "$POS_OUT" "checkout complete" "positional form reports checkout complete"

POS_PORTS=$("$COAST" ssg ports 2>&1)
echo "$POS_PORTS"
assert_contains "$POS_PORTS" "checked out" "positional checkout reflected in ssg ports"

# Back off so the flag-form test starts clean.
"$COAST" ssg uncheckout postgres >/dev/null 2>&1

echo ""
echo "=== Step 3: flag form — coast ssg checkout --service postgres ==="

FLAG_OUT=$("$COAST" ssg checkout --service postgres 2>&1)
echo "$FLAG_OUT"
assert_contains "$FLAG_OUT" "checkout complete" "flag form reports checkout complete"

FLAG_PORTS=$("$COAST" ssg ports 2>&1)
echo "$FLAG_PORTS"
assert_contains "$FLAG_PORTS" "checked out" "flag checkout reflected in ssg ports"

# Clean up for the error-case tests below.
"$COAST" ssg uncheckout postgres >/dev/null 2>&1

echo ""
echo "=== Step 4: conflicting positional + flag values are rejected ==="

CONFLICT=$("$COAST" ssg checkout postgres --service redis 2>&1 || true)
echo "$CONFLICT"
assert_contains "$CONFLICT" "conflicting service name" "mismatched positional+flag values are rejected"

echo ""
echo "=== Step 5: --all combined with a specific service is rejected ==="

BOTH=$("$COAST" ssg checkout postgres --all 2>&1 || true)
echo "$BOTH"
assert_contains "$BOTH" "mutually exclusive" "--all with positional SERVICE is rejected"

BOTH_FLAG=$("$COAST" ssg checkout --service postgres --all 2>&1 || true)
echo "$BOTH_FLAG"
assert_contains "$BOTH_FLAG" "mutually exclusive" "--all with --service is rejected"

# Cleanup.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG CHECKOUT POSITIONAL TESTS PASSED"
echo "==========================================="
