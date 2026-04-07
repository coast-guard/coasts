#!/usr/bin/env bash
#
# Integration test: inner compose services should be healed by the
# coast-service reconciler when they die while the outer DinD container
# is still running.
#
# Reproduces the bug where services without `restart: unless-stopped`
# (like test-redis) stay dead after crashing, because the reconciler
# only acts when the outer container state changes, not when inner
# compose services die.
#
# Steps:
#   1. Run a remote coast with two compose services:
#      - "app" (has restart: unless-stopped)
#      - "fragile-cache" (no restart policy)
#   2. Verify both services running
#   3. Kill the fragile-cache container inside the inner DinD
#   4. Verify fragile-cache is down
#   5. Wait for the reconciler to heal it
#   6. Assert fragile-cache is back up

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

DAEMON_API="http://localhost:6100"

_cleanup() {
    echo ""
    echo "--- Cleaning up inner service heal test ---"
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
trap '_cleanup' EXIT

echo "=== Remote Inner Service Heal Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate

setup_localhost_ssh
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-compose"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 15

# ============================================================
# Test 1: Baseline -- both services running
# ============================================================

echo ""
echo "=== Test 1: Baseline -- both services running ==="

set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
echo "  ps exit code: $PS_EXIT"
echo "  ps output:"
echo "$PS_OUT" | head -10

if echo "$PS_OUT" | grep -q "fragile-cache.*running"; then
    pass "fragile-cache is running"
else
    echo "  WARNING: fragile-cache not running at baseline, waiting longer"
    sleep 15
    set +e
    PS_OUT=$("$COAST" ps dev-1 2>&1)
    set -e
    echo "  ps retry:"
    echo "$PS_OUT" | head -10
    if echo "$PS_OUT" | grep -q "fragile-cache.*running"; then
        pass "fragile-cache is running (after extra wait)"
    else
        fail "fragile-cache never started"
    fi
fi

# ============================================================
# Test 2: Kill fragile-cache inside the inner DinD
# ============================================================

echo ""
echo "=== Test 2: Kill fragile-cache inside inner DinD ==="

# Find the outer DinD container
DIND_NAME=$(docker ps --format '{{.Names}}' | grep "coast-remote-compose.*coasts.*dev-1" | grep -v shell | head -1)
if [ -z "$DIND_NAME" ]; then
    echo "  Available containers:"
    docker ps --format '{{.Names}}'
    fail "DinD container not found"
fi
echo "  DinD container: $DIND_NAME"

# Find and kill the fragile-cache container inside the inner DinD
INNER_FRAGILE=$(docker exec "$DIND_NAME" docker ps --format '{{.Names}}' 2>/dev/null | grep "fragile" | head -1)
if [ -z "$INNER_FRAGILE" ]; then
    echo "  Inner containers:"
    docker exec "$DIND_NAME" docker ps --format '{{.Names}}' 2>/dev/null
    fail "fragile-cache container not found inside DinD"
fi
echo "  Inner fragile container: $INNER_FRAGILE"

docker exec "$DIND_NAME" docker rm -f "$INNER_FRAGILE" 2>/dev/null
pass "Killed fragile-cache inside inner DinD"

sleep 3

# ============================================================
# Test 3: Verify fragile-cache is down
# ============================================================

echo ""
echo "=== Test 3: Verify fragile-cache is down ==="

PS_DOWN=$("$COAST" ps dev-1 2>&1)
echo "  ps after kill:"
echo "$PS_DOWN" | head -10

if echo "$PS_DOWN" | grep -q "fragile-cache.*running"; then
    fail "fragile-cache should be down after kill"
else
    pass "fragile-cache is down (expected)"
fi

# ============================================================
# Test 4: Wait for reconciler to heal it
# ============================================================

echo ""
echo "=== Test 4: Wait for reconciler to heal fragile-cache ==="

HEALED=false
for attempt in $(seq 1 6); do
    sleep 5
    PS_CHECK=$("$COAST" ps dev-1 2>&1)
    if echo "$PS_CHECK" | grep -q "fragile-cache.*running"; then
        HEALED=true
        break
    fi
    echo "  attempt $attempt: still down"
done

if [ "$HEALED" = true ]; then
    pass "fragile-cache healed by reconciler (attempt $attempt)"
else
    echo "  Final ps:"
    "$COAST" ps dev-1 2>&1 | head -10
    fail "fragile-cache NOT healed -- reconciler did not restart dead inner compose service"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All inner service heal tests passed!"
echo "=========================================="
