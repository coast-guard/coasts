#!/usr/bin/env bash
#
# Integration test: `coast ssg rm --with-data` + `coast ssg run`
# refreshes the in-dind socat forwarders of already-running consumer
# coasts (Phase 11, DESIGN.md §17-38).
#
# Verifies the bug fix:
#   1. First `ssg run` + consumer run → `psql` works.
#   2. `ssg rm --with-data` + `ssg run` forces a fresh dynamic host
#      port allocation for postgres.
#   3. Without the Phase 11 hook, the consumer's in-dind socat would
#      still forward to the OLD dynamic port; psql would hang/fail.
#   4. With the hook, the lifecycle verb's response message announces
#      the refresh AND psql still works from the SAME running
#      consumer — no rebuild required.
#
# Uses the `coast-ssg-minimal` + `coast-ssg-consumer-basic` fixtures
# (postgres:16-alpine with POSTGRES_PASSWORD=coast and a consumer
# running a long-sleeping postgres:16-alpine as the app service).

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

# ============================================================
# Step 1: first SSG + consumer cycle. Capture the initial
# dynamic host port so we can assert it changes on the rerun.
# ============================================================

echo ""
echo "=== Step 1: first SSG run + consumer run ==="

cd "$PROJECTS_DIR/coast-ssg-minimal"
SSG_BUILD_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
assert_contains "$SSG_BUILD_OUT" "Build complete" "initial ssg build succeeds"

SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "initial ssg run succeeds"

OLD_DYN=$("$COAST" ssg ports 2>&1 | awk '/^  postgres/ {print $3}')
[ -n "$OLD_DYN" ] || fail "could not extract initial SSG postgres dynamic port"
pass "initial SSG postgres dynamic host port = $OLD_DYN"

sleep 5  # initdb inside SSG

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" build >/dev/null 2>&1
CLEANUP_INSTANCES+=("inst-a")
"$COAST" run inst-a >/dev/null 2>&1

sleep 3  # inner compose for the consumer

echo ""
echo "=== Step 2: baseline psql from consumer works ==="

PSQL_BASELINE=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 1 AS baseline;'" 2>&1)
echo "$PSQL_BASELINE"
assert_contains "$PSQL_BASELINE" "baseline" "baseline psql returns column header"
assert_contains "$PSQL_BASELINE" "1" "baseline psql returns the row"

# ============================================================
# Step 3: rip the SSG out with its data and bring a fresh one up.
# This is the only reliable way to force a new dyn port: the port
# allocator probes free ports and may happen to re-pick the same
# one, but `rm --with-data` + `run` typically lands on a new port
# because the Linux kernel's ephemeral range is large. We retry
# up to 5 times if we keep getting the same port.
# ============================================================

echo ""
echo "=== Step 3: ssg rm + ssg run to force fresh dyn port ==="

NEW_DYN=""
for attempt in 1 2 3 4 5; do
    "$COAST" ssg rm --with-data >/dev/null 2>&1 || true
    SSG_RERUN_OUT=$("$COAST" ssg run 2>&1)
    assert_contains "$SSG_RERUN_OUT" "SSG running" "ssg rerun succeeds (attempt $attempt)"
    CANDIDATE=$("$COAST" ssg ports 2>&1 | awk '/^  postgres/ {print $3}')
    [ -n "$CANDIDATE" ] || fail "could not extract SSG postgres dynamic port after rerun"
    if [ "$CANDIDATE" != "$OLD_DYN" ]; then
        NEW_DYN="$CANDIDATE"
        # Preserve this specific rerun output for the refresh
        # assertion below — we need the message from the run that
        # actually found a new port.
        FINAL_RERUN_OUT="$SSG_RERUN_OUT"
        break
    fi
    echo "attempt $attempt: port reallocated to same value ($CANDIDATE); retrying"
done

if [ -z "$NEW_DYN" ]; then
    fail "port never reallocated to a new value across 5 attempts; kernel ephemeral range too narrow"
fi
pass "SSG postgres dynamic host port changed: $OLD_DYN -> $NEW_DYN"

# ============================================================
# Step 4: the lifecycle response MUST announce the refresh.
# Without the Phase 11 hook, `coast ssg run` would finish silently
# and the running consumer would keep pointing at the old port.
# ============================================================

echo ""
echo "=== Step 4: ssg run response announces consumer refresh ==="

echo "$FINAL_RERUN_OUT" | tail -10
assert_contains "$FINAL_RERUN_OUT" "Refreshed shared-service proxies" \
    "ssg run response announces consumer proxy refresh"
assert_contains "$FINAL_RERUN_OUT" "coast-ssg-consumer-basic/inst-a" \
    "refresh message names the consumer instance"

# ============================================================
# Step 5: the running consumer's psql MUST still work even though
# we never touched it directly. This is the whole point of the
# Phase 11 hook: in-dind socat now forwards to NEW_DYN, not OLD_DYN.
# ============================================================

echo ""
echo "=== Step 5: consumer psql works against the NEW SSG without rebuild ==="

sleep 6  # initdb on the fresh postgres

# Use a short connect timeout so a stale forwarder would fail fast
# rather than hanging the test.
PSQL_REFRESHED=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres \
     -c 'SELECT 42 AS refreshed;' -v ON_ERROR_STOP=1 2>&1 || true")
echo "$PSQL_REFRESHED"
assert_contains "$PSQL_REFRESHED" "refreshed" "post-refresh psql returns column header"
assert_contains "$PSQL_REFRESHED" "42" "post-refresh psql returns the row"

pass "consumer psql routed to the NEW SSG dyn port without rebuilding the consumer"

# Cleanup.
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER REFRESH TESTS PASSED"
echo "==========================================="
