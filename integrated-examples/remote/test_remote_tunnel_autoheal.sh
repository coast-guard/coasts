#!/usr/bin/env bash
#
# Integration test: SSH tunnels must auto-recover after being killed.
#
# Reproduces the bug where killing SSH tunnel processes (simulating a
# connection drop or coast-service restart) leaves remote ports dead
# until the user manually restarts the daemon.
#
# The test:
#   1. Provisions a remote coast instance (creates SSH tunnels)
#   2. Verifies exec works through the tunnel (baseline)
#   3. Kills all SSH tunnel processes (simulates connection death)
#   4. Waits up to 45 seconds for auto-recovery (no daemon restart)
#   5. Asserts exec works again through the restored tunnel
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_tunnel_autoheal.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_autoheal_cleanup() {
    echo ""
    echo "--- Cleaning up autoheal test ---"

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
trap '_autoheal_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Tunnel Auto-heal Integration Test ==="
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
BUILD_OUT=$("$COAST" build --type remote 2>&1)
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
# Test 1: Baseline — exec works through tunnel
# ============================================================

echo ""
echo "=== Test 1: Baseline — exec works ==="

sleep 3

set +e
EXEC_BEFORE=$("$COAST" exec dev-1 -- echo "tunnel-baseline" 2>&1)
EXEC_BEFORE_EXIT=$?
set -e

[ "$EXEC_BEFORE_EXIT" -eq 0 ] || fail "Baseline exec failed (exit $EXEC_BEFORE_EXIT): $EXEC_BEFORE"
assert_contains "$EXEC_BEFORE" "tunnel-baseline" "exec works before kill"
pass "Baseline exec works"

TUNNEL_COUNT_BEFORE=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  SSH -L tunnel count before kill: $TUNNEL_COUNT_BEFORE"
[ "$TUNNEL_COUNT_BEFORE" -gt 0 ] || fail "Expected SSH tunnels to exist"

# ============================================================
# Test 2: Kill SSH tunnels (simulate connection death)
# ============================================================

echo ""
echo "=== Test 2: Kill SSH tunnels ==="

pkill -f "ssh -N" 2>/dev/null || true
sleep 2

TUNNEL_COUNT_AFTER_KILL=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  SSH -L tunnel count after kill: $TUNNEL_COUNT_AFTER_KILL"
[ "$TUNNEL_COUNT_AFTER_KILL" -eq 0 ] || fail "SSH tunnels should be dead after pkill"
pass "All SSH tunnels killed"

# ============================================================
# Test 3: Wait for auto-recovery (NO daemon restart)
# ============================================================

echo ""
echo "=== Test 3: Wait for auto-recovery (max 45s) ==="

RECOVERED=false
for i in $(seq 1 9); do
    sleep 5
    TUNNEL_COUNT_NOW=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
    echo "  [${i}] After $((i * 5))s: $TUNNEL_COUNT_NOW SSH -L tunnels"

    if [ "$TUNNEL_COUNT_NOW" -gt 0 ]; then
        set +e
        EXEC_AFTER=$("$COAST" exec dev-1 -- echo "tunnel-recovered" 2>&1)
        EXEC_AFTER_EXIT=$?
        set -e

        if [ "$EXEC_AFTER_EXIT" -eq 0 ] && echo "$EXEC_AFTER" | grep -q "tunnel-recovered"; then
            RECOVERED=true
            echo "  Recovered after $((i * 5)) seconds"
            break
        fi
    fi
done

if [ "$RECOVERED" = false ]; then
    echo "  SSH tunnel processes:"
    pgrep -lf "ssh -N" 2>/dev/null || echo "  (none)"
    echo "  coastd log tail:"
    tail -30 /tmp/coastd-test.log 2>/dev/null || true
    fail "Tunnels did not auto-recover within 45 seconds (no daemon restart was performed)"
fi
pass "Tunnels auto-recovered without daemon restart"

# ============================================================
# Test 4: No duplicate tunnels after recovery
# ============================================================

echo ""
echo "=== Test 4: No duplicate tunnels ==="

TUNNEL_COUNT_FINAL=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  Final SSH -L tunnel count: $TUNNEL_COUNT_FINAL"

if [ "$TUNNEL_COUNT_FINAL" -gt "$TUNNEL_COUNT_BEFORE" ]; then
    fail "More tunnels after recovery ($TUNNEL_COUNT_FINAL) than before ($TUNNEL_COUNT_BEFORE)"
fi
pass "No duplicate tunnels ($TUNNEL_COUNT_FINAL <= $TUNNEL_COUNT_BEFORE)"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>/dev/null || true
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All tunnel auto-heal tests passed!"
echo "=========================================="
