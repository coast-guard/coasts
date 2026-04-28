#!/usr/bin/env bash
#
# Regression test for PR #254 (Fix restart-services with autostart=false bare services).
#
# Background:
#   `coast run` short-circuits bare-service setup when the Coastfile has
#   `autostart = false`, so `/coast-supervisor/start-all.sh` and
#   `/coast-supervisor/stop-all.sh` are never written. After a subsequent
#   `coast start` flips the instance to Running, `coast restart-services`
#   re-parses the cached Coastfile, sees `[services.*]`, and used to
#   unconditionally execute the missing `stop-all.sh` — failing with
#   `sh: /coast-supervisor/stop-all.sh: No such file or directory`.
#
#   PR #254 makes restart-services tolerate the missing supervisor on
#   `autostart=false` instances (clean no-op) and surface a clearer error
#   on `autostart=true` instances whose supervisor is unexpectedly absent.
#
# This test exercises the exact bug repro that the PR's Rust unit tests
# only cover via a mock runtime. It would FAIL on `main` and PASS on
# pr-254.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_restart_services_bare_noautostart.sh

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
# Test: bare services + autostart=false, then start, then restart-services
# ============================================================

echo ""
echo "=== Test: restart-services on Running instance with bare services + autostart=false ==="

cd "$PROJECTS_DIR/coast-bare-noautostart"

BUILD_OUTPUT=$($COAST build 2>&1) || { echo "$BUILD_OUTPUT"; fail "coast build failed for coast-bare-noautostart"; }
pass "coast-bare-noautostart build succeeded"

# Phase 1: coast run with autostart=false leaves the instance Idle and
# does NOT write supervisor scripts (run/mod.rs:resolve_coastfile_flags
# returns has_services=false when autostart=false is set).
RUN_OUTPUT=$($COAST run rs-bare-noauto 2>&1) || { echo "$RUN_OUTPUT"; fail "coast run failed for coast-bare-noautostart"; }
CLEANUP_INSTANCES+=("rs-bare-noauto")
pass "coast run rs-bare-noauto succeeded"

LS_AFTER_RUN=$($COAST ls 2>&1)
assert_contains "$LS_AFTER_RUN" "rs-bare-noauto" "instance is listed"
assert_contains "$LS_AFTER_RUN" "idle" "instance is idle (autostart=false)"

# Phase 2: coast start flips the container to Running. start.rs
# calls start_bare_services_if_present, which is best-effort: if
# /coast-supervisor doesn't exist it returns false and does nothing
# (no error). The instance status still becomes Running.
START_OUTPUT=$($COAST start rs-bare-noauto 2>&1) || { echo "$START_OUTPUT"; fail "coast start failed"; }
pass "coast start rs-bare-noauto succeeded"

sleep 5

LS_AFTER_START=$($COAST ls 2>&1)
assert_contains "$LS_AFTER_START" "running" "instance is running after coast start"

# Phase 3: confirm the supervisor stop script genuinely doesn't exist.
# This is the precondition that makes this test meaningful — without
# it, the assertions below would not exercise the bug fix.
EXEC_OUT=$($COAST exec rs-bare-noauto -- sh -c "test -f /coast-supervisor/stop-all.sh && echo PRESENT || echo MISSING" 2>&1)
assert_contains "$EXEC_OUT" "MISSING" "/coast-supervisor/stop-all.sh was never written by coast run"

# Phase 4: THE TEST. On `main` this errors with
#   "stop-all.sh failed in instance 'rs-bare-noauto': sh: /coast-supervisor/stop-all.sh: No such file or directory"
# On pr-254 this is a clean no-op: ensure_bare_stop_script_exists()
# sees the missing file and autostart=false, returns Ok(false), and
# restart_bare_services returns Ok(None).
RESTART_OUTPUT=$($COAST restart-services rs-bare-noauto 2>&1) || {
    echo "----- restart-services output -----"
    echo "$RESTART_OUTPUT"
    echo "------------------------------------"
    fail "restart-services errored on Running instance with bare+autostart=false (PR #254 regression)"
}
pass "coast restart-services rs-bare-noauto succeeded (PR #254 fix is in effect)"

assert_contains "$RESTART_OUTPUT" "ok" "restart-services returned ok"
assert_not_contains "$RESTART_OUTPUT" "(all bare services)" "no bare services were restarted (autostart=false no-op)"
assert_not_contains "$RESTART_OUTPUT" "stop-all.sh" "no reference to stop-all.sh in successful output"

# Phase 5: instance must remain Running afterward.
LS_AFTER_RESTART=$($COAST ls 2>&1)
assert_contains "$LS_AFTER_RESTART" "running" "instance is still running after restart-services no-op"

echo ""
echo "=== restart-services bare+autostart=false regression test passed ==="
