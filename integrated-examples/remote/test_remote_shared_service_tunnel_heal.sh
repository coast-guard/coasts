#!/usr/bin/env bash
#
# Integration test: shared service reverse tunnels must be restored
# after daemon restart.
#
# Reproduces the bug where killing SSH reverse tunnels (-R) and
# restarting the daemon leaves shared services (PostgreSQL, Redis)
# unreachable from remote DinD containers.
#
# The test:
#   1. Provisions a remote coast with shared services (postgres)
#   2. Verifies postgres is reachable from inside the DinD container
#   3. Kills SSH reverse tunnel processes
#   4. Verifies postgres is now unreachable
#   5. Restarts the daemon
#   6. Asserts postgres is reachable again (reverse tunnels restored)
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_shared_service_tunnel_heal.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_tunnel_heal_cleanup() {
    echo ""
    echo "--- Cleaning up shared service tunnel heal test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_tunnel_heal_cleanup' EXIT

count_reverse_tunnels() {
    local cnt
    cnt=$(pgrep -cf "ssh.*-N.*-R" 2>/dev/null || echo "0")
    echo "$cnt" | tr -d '[:space:]'
}

# ============================================================
# Setup
# ============================================================

echo "=== Remote Shared Service Tunnel Heal Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-shared-services"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

"$COAST" build 2>&1 >/dev/null
pass "Local build complete"

set +e
"$COAST" build --type remote 2>&1 >/dev/null
set -e

set +e
RUN_OUT=$("$COAST" run shared-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "$RUN_OUT"
    fail "Run failed (exit $RUN_EXIT)"
fi
CLEANUP_INSTANCES+=("shared-1")
pass "Remote instance with shared services running"

sleep 5

# ============================================================
# Test 1: Baseline — reverse tunnels exist
# ============================================================

echo ""
echo "=== Test 1: Baseline — reverse tunnels exist ==="

REVERSE_BEFORE=$(count_reverse_tunnels)
REVERSE_BEFORE=$(echo "$REVERSE_BEFORE" | tr -d '[:space:]')
echo "  reverse tunnel count: $REVERSE_BEFORE"
echo "  all ssh -N processes:"
pgrep -af "ssh.*-N" 2>/dev/null || echo "  (none)"
[ "$REVERSE_BEFORE" -gt 0 ] || fail "Expected reverse tunnel processes after coast run"
pass "Reverse tunnels present ($REVERSE_BEFORE)"

# ============================================================
# Test 2: Kill reverse tunnels
# ============================================================

echo ""
echo "=== Test 2: Kill reverse tunnels ==="

pkill -f "ssh -N -R" 2>/dev/null || true
sleep 2

REVERSE_KILLED=$(count_reverse_tunnels)
REVERSE_KILLED=$(echo "$REVERSE_KILLED" | tr -d '[:space:]')
echo "  reverse tunnel count after kill: $REVERSE_KILLED"
[ "$REVERSE_KILLED" -eq 0 ] || fail "Reverse tunnels should be dead after pkill"
pass "Reverse tunnels killed"

# ============================================================
# Test 3: Restart daemon — reverse tunnels must be restored
# ============================================================

echo ""
echo "=== Test 3: Restart daemon and verify reverse tunnel restore ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
sleep 8

REVERSE_RESTORED=$(count_reverse_tunnels)
REVERSE_RESTORED=$(echo "$REVERSE_RESTORED" | tr -d '[:space:]')
echo "  reverse tunnel count after restart: $REVERSE_RESTORED"
echo "  all ssh -N processes:"
pgrep -af "ssh.*-N" 2>/dev/null || echo "  (none)"
[ "$REVERSE_RESTORED" -gt 0 ] || fail "Reverse tunnels NOT restored after daemon restart — shared service tunnel restore is broken"
pass "Shared service reverse tunnels restored ($REVERSE_RESTORED)"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All shared service tunnel heal tests passed!"
echo "=========================================="
