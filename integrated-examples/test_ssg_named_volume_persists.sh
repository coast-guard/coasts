#!/usr/bin/env bash
#
# Integration test: SSG inner named volume persists across stop/start (Phase 3).
#
# The coast-ssg-minimal project declares `pg_data:/var/lib/postgresql/data`
# as an inner named volume. Postgres writes to that path on first boot,
# so a marker file dropped there must survive a full stop/start cycle.
#
# Steps:
#
#   1. `coast ssg build && coast ssg run`
#   2. Write a marker file under `/var/lib/postgresql/data/` via
#      `docker exec coast-ssg docker compose ... exec postgres ...`.
#   3. `coast ssg stop && coast ssg start`.
#   4. Read the marker back — content must match.
#   5. As a negative control, `coast ssg rm --with-data` removes the
#      inner named volume so a subsequent `coast ssg run` starts clean.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_named_volume_persists.sh

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
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

cd "$PROJECTS_DIR/coast-ssg-minimal"

start_daemon

# ============================================================
# Test 1: build + run
# ============================================================

echo ""
echo "=== Test 1: build + run ==="

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT" | tail -5
assert_contains "$BUILD_OUT" "Build complete" "build succeeds"

RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$RUN_OUT" | tail -10
assert_contains "$RUN_OUT" "SSG running" "run succeeds"

# postgres needs a moment to finish initdb on first boot.
sleep 5

# ============================================================
# Test 2: write a marker inside the inner postgres data dir
# ============================================================

echo ""
echo "=== Test 2: write marker into pg_data named volume ==="

MARKER_CONTENT="named-vol-$(date +%s%N)"

# Write the marker via `docker exec` chained into the inner container.
docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml \
    -p coast-ssg \
    exec -T postgres sh -c "echo '$MARKER_CONTENT' > /var/lib/postgresql/data/.coast-marker"

pass "marker written into inner named volume"

# ============================================================
# Test 3: stop + start, verify marker survives
# ============================================================

echo ""
echo "=== Test 3: stop + start ==="

STOP_OUT=$("$COAST" ssg stop 2>&1)
assert_contains "$STOP_OUT" "SSG stopped" "stop succeeds"

START_OUT=$("$COAST" ssg start 2>&1)
assert_contains "$START_OUT" "SSG started" "start succeeds"

sleep 5

POST_MARKER=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml \
    -p coast-ssg \
    exec -T postgres cat /var/lib/postgresql/data/.coast-marker | tr -d '\r')

assert_eq "$POST_MARKER" "$MARKER_CONTENT" "marker survives stop+start (named volume persists)"

# ============================================================
# Test 4: rm --with-data removes the inner named volume
# ============================================================

echo ""
echo "=== Test 4: rm --with-data clears inner volume ==="

RM_OUT=$("$COAST" ssg rm --with-data 2>&1)
assert_contains "$RM_OUT" "SSG removed" "rm reports success"

# Re-run and check the marker is gone (fresh initdb).
"$COAST" ssg run >/dev/null 2>&1
sleep 8

if docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml \
    -p coast-ssg \
    exec -T postgres test -f /var/lib/postgresql/data/.coast-marker 2>/dev/null; then
    fail "marker still exists after rm --with-data (named volume was not cleared)"
fi
pass "named volume was removed by rm --with-data"

# Clean up this test's final SSG.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG NAMED VOLUME PERSISTS TESTS PASSED"
echo "==========================================="
