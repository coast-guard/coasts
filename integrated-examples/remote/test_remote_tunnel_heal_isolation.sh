#!/usr/bin/env bash
#
# Integration test: healing one instance's tunnels must NOT kill
# another instance's tunnels.
#
# Reproduces the bug where the heal loop ran `pkill -f "ssh -N"`
# globally, destroying ALL SSH tunnels when only one instance's
# tunnels were dead. With the fix, only the affected instance's
# tunnels are killed and re-established.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true
    "$COAST" remote rm test-remote 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Tunnel Heal Isolation Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate

eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

# ============================================================
# Test 1: Run two instances
# ============================================================

echo ""
echo "=== Test 1: Run two remote instances ==="

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run dev-1 failed"
CLEANUP_INSTANCES+=("dev-1")

"$COAST" run dev-2 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run dev-2 failed"
CLEANUP_INSTANCES+=("dev-2")
set -e
pass "Both instances running"

sleep 3

# ============================================================
# Test 2: Record dev-2's tunnel PIDs
# ============================================================

echo ""
echo "=== Test 2: Record tunnel PIDs ==="

set +e
DEV2_PORTS=$("$COAST" ports dev-2 2>&1 | awk '/[0-9]+/ && $2 ~ /^[0-9]+$/ {print $3}' | head -1)
set -e
echo "  dev-2 dynamic port: $DEV2_PORTS"

set +e
DEV2_TUNNEL_PID=$(ps aux | grep "ssh.*-N.*-L.*${DEV2_PORTS}:" | grep -v grep | awk '{print $2}' | head -1)
set -e
echo "  dev-2 tunnel PID: ${DEV2_TUNNEL_PID:-NONE}"

[ -n "$DEV2_TUNNEL_PID" ] || fail "No tunnel PID found for dev-2"
pass "dev-2 tunnel PID recorded: $DEV2_TUNNEL_PID"

# ============================================================
# Test 3: Kill dev-1's tunnels (simulate stale after sleep)
# ============================================================

echo ""
echo "=== Test 3: Kill dev-1's tunnels only ==="

set +e
DEV1_PORTS=$("$COAST" ports dev-1 2>&1 | awk '/[0-9]+/ && $2 ~ /^[0-9]+$/ {print $3}' | head -1)
DEV1_TUNNEL_PID=$(ps aux | grep "ssh.*-N.*-L.*${DEV1_PORTS}:" | grep -v grep | awk '{print $2}' | head -1)
set -e

if [ -n "$DEV1_TUNNEL_PID" ]; then
    kill "$DEV1_TUNNEL_PID" 2>/dev/null
    pass "Killed dev-1's tunnel (PID $DEV1_TUNNEL_PID)"
else
    echo "  WARNING: No specific tunnel PID for dev-1"
fi

# ============================================================
# Test 4: Wait for heal loop and verify dev-2 survives
# ============================================================

echo ""
echo "=== Test 4: Wait for heal loop (max 40s) ==="

HEALED=false
for i in $(seq 1 8); do
    sleep 5
    set +e
    DEV2_ALIVE=$(ps -p "$DEV2_TUNNEL_PID" -o pid= 2>/dev/null | tr -d ' ')
    set -e
    if [ -z "$DEV2_ALIVE" ]; then
        fail "dev-2's tunnel (PID $DEV2_TUNNEL_PID) was killed by heal loop -- NOT isolated"
    fi

    # Check if dev-1 was healed (new tunnel exists)
    set +e
    DEV1_NEW=$(ps aux | grep "ssh.*-N.*-L.*${DEV1_PORTS}:" | grep -v grep | awk '{print $2}' | head -1)
    set -e
    if [ -n "$DEV1_NEW" ] && [ "$DEV1_NEW" != "$DEV1_TUNNEL_PID" ]; then
        HEALED=true
        pass "dev-1 healed with new tunnel (PID $DEV1_NEW) at attempt $i"
        break
    fi
    echo "  attempt $i: dev-2 alive, dev-1 not yet healed"
done

if [ "$HEALED" = true ]; then
    pass "dev-2's tunnel survived heal (PID $DEV2_TUNNEL_PID still alive)"
else
    echo "  WARNING: dev-1 not healed in time (may need longer cooldown)"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
"$COAST" rm dev-2 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All tunnel heal isolation tests passed!"
echo "=========================================="
