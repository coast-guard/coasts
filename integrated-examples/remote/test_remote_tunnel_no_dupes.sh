#!/usr/bin/env bash
#
# Integration test: SSH tunnel processes must not duplicate on daemon restart.
#
# Reproduces the bug where every `coastd` restart spawns a new batch of
# `ssh -N -L` port-forwarding processes without killing the old ones,
# leading to dozens of zombie SSH tunnels over time.
#
# The test:
#   1. Provisions a remote coast instance (creates SSH tunnels)
#   2. Counts SSH tunnel processes
#   3. Restarts the daemon (kills coastd, starts it again)
#   4. Counts SSH tunnel processes again
#   5. Asserts the count did NOT increase
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_tunnel_no_dupes.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_tunnel_cleanup() {
    echo ""
    echo "--- Cleaning up tunnel test ---"

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
trap '_tunnel_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Tunnel No-Dupes Integration Test ==="
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
# Test 1: Baseline — count SSH tunnel processes
# ============================================================

echo ""
echo "=== Test 1: Count SSH tunnels before restart ==="

sleep 2

BEFORE=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  SSH tunnel processes before restart: $BEFORE"

[ "$BEFORE" -gt 0 ] || fail "Expected at least one SSH tunnel process after run"
pass "SSH tunnels exist ($BEFORE processes)"

# ============================================================
# Test 2: Restart daemon and count again
# ============================================================

echo ""
echo "=== Test 2: Restart daemon, count SSH tunnels ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
sleep 3

AFTER=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  SSH tunnel processes after restart: $AFTER"

if [ "$AFTER" -gt "$BEFORE" ]; then
    echo "  DETAIL: before=$BEFORE after=$AFTER (duplicates created!)"
    echo "  SSH processes:"
    pgrep -lf "ssh -N -L" 2>/dev/null || true
    fail "Daemon restart created duplicate SSH tunnels ($BEFORE -> $AFTER)"
fi
pass "No duplicate tunnels after restart ($BEFORE -> $AFTER)"

# ============================================================
# Test 3: Restart a second time — still no growth
# ============================================================

echo ""
echo "=== Test 3: Second restart, count again ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
sleep 3

AFTER2=$(pgrep -c -f "ssh -N -L" 2>/dev/null || echo 0)
echo "  SSH tunnel processes after second restart: $AFTER2"

if [ "$AFTER2" -gt "$BEFORE" ]; then
    fail "Second restart created duplicate SSH tunnels ($BEFORE -> $AFTER2)"
fi
pass "No duplicate tunnels after second restart ($BEFORE -> $AFTER2)"

# ============================================================
# Test 4: Verify tunnels still work (exec into remote coast)
# ============================================================

echo ""
echo "=== Test 4: Tunnels still functional ==="

set +e
EXEC_OUT=$(timeout 15 "$COAST" exec dev-1 -- echo "tunnel-alive" 2>&1)
EXEC_EXIT=$?
set -e

[ "$EXEC_EXIT" -ne 124 ] || fail "exec hung (timed out)"
assert_contains "$EXEC_OUT" "tunnel-alive" "exec works through restored tunnel"
pass "Tunnels functional after restarts"

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
echo "  All tunnel no-dupes tests passed!"
echo "=========================================="
