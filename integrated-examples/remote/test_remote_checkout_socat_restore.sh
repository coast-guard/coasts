#!/usr/bin/env bash
#
# Integration test: socat canonical port forwarders must be restored
# for remote instances after a daemon restart.
#
# Reproduces the bug where `restore_socat_forwarding` skips remote
# instances entirely, leaving canonical ports dead after daemon restart
# or laptop sleep/wake.
#
# The test:
#   1. Provisions a remote coast instance
#   2. Checks it out (spawns socat canonical forwarders)
#   3. Verifies canonical port is listening (baseline)
#   4. Kills socat and restarts the daemon (simulates sleep/wake)
#   5. Asserts canonical port is listening again after restore
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync, socat installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_checkout_socat_restore.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

CANONICAL_PORT=40100

_socat_restore_cleanup() {
    echo ""
    echo "--- Cleaning up socat restore test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_socat_restore_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Checkout Socat Restore Test ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
"$COAST" build --type remote 2>&1 >/dev/null
set -e

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "$RUN_OUT"
    fail "Run failed (exit $RUN_EXIT)"
fi
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

# ============================================================
# Test 1: Checkout — canonical port is listening
# ============================================================

echo ""
echo "=== Test 1: Checkout and verify canonical port ==="

CO_OUT=$("$COAST" checkout dev-1 2>&1)
assert_contains "$CO_OUT" "Checked out" "checkout succeeded"
pass "Instance checked out"

sleep 3

SOCAT_COUNT_BEFORE=$(pgrep -cx socat 2>/dev/null || echo "0")
SOCAT_COUNT_BEFORE=$(echo "$SOCAT_COUNT_BEFORE" | tr -d '[:space:]')
echo "  socat process count: $SOCAT_COUNT_BEFORE"
[ "$SOCAT_COUNT_BEFORE" -gt 0 ] || fail "Expected socat processes after checkout"

CANONICAL_SOCAT=$(pgrep -af "socat.*TCP-LISTEN:${CANONICAL_PORT}" 2>/dev/null || true)
echo "  canonical socat: ${CANONICAL_SOCAT:-none}"
[ -n "$CANONICAL_SOCAT" ] || fail "Canonical port $CANONICAL_PORT socat not running after checkout"
pass "Canonical port $CANONICAL_PORT socat is running (baseline)"

# ============================================================
# Test 2: Kill socat — canonical port goes dead
# ============================================================

echo ""
echo "=== Test 2: Kill socat processes ==="

pkill -x socat 2>/dev/null || true
sleep 1

CANONICAL_SOCAT_DEAD=$(pgrep -af "socat.*TCP-LISTEN:${CANONICAL_PORT}" 2>/dev/null || true)
echo "  canonical socat after kill: ${CANONICAL_SOCAT_DEAD:-dead}"
[ -z "$CANONICAL_SOCAT_DEAD" ] || fail "Canonical socat should be dead after pkill"
pass "socat killed, canonical port dead"

# ============================================================
# Test 3: Restart daemon — canonical port must be restored
# ============================================================

echo ""
echo "=== Test 3: Restart daemon and verify socat restore ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
sleep 5

LS_AFTER=$("$COAST" ls 2>&1)
assert_contains "$LS_AFTER" "checked_out" "instance still shows checked_out after restart"
pass "Instance status preserved"

SOCAT_COUNT_RESTORED=$(pgrep -cx socat 2>/dev/null || echo "0")
SOCAT_COUNT_RESTORED=$(echo "$SOCAT_COUNT_RESTORED" | tr -d '[:space:]')
echo "  socat count after restart: $SOCAT_COUNT_RESTORED"

CANONICAL_SOCAT_RESTORED=$(pgrep -af "socat.*TCP-LISTEN:${CANONICAL_PORT}" 2>/dev/null || true)
echo "  canonical socat after restart: ${CANONICAL_SOCAT_RESTORED:-none}"
[ -n "$CANONICAL_SOCAT_RESTORED" ] || fail "Canonical port $CANONICAL_PORT socat NOT restored after daemon restart — socat restore for remote instances is broken"
pass "Canonical port $CANONICAL_PORT socat restored after daemon restart"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All socat restore tests passed!"
echo "=========================================="
