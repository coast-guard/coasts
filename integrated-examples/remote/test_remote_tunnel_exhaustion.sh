#!/usr/bin/env bash
#
# Integration test: SSH tunnel exhaustion with rapid connection churn.
#
# Reproduces the bug where the daemon creates a new RemoteClient::connect()
# (which establishes a new SSH tunnel) for every single API call to
# coast-service. Each tunnel creates a new SSH process, uses it for one
# request, then drops it.
#
# Detection: Count "SSH tunnel established" log entries in the daemon log.
# After 50 coast ps calls (one per instance = 100 total), a healthy system
# with connection reuse should show ~2 tunnel entries (one per instance).
# With the bug, we see ~100 entries (one per request).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up tunnel exhaustion test ---"
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
trap '_cleanup' EXIT

echo "=== Remote Tunnel Exhaustion Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate

setup_localhost_ssh
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"
start_daemon

# ============================================================
# Test 1: Run two remote instances
# ============================================================

echo ""
echo "=== Test 1: Run two remote instances ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run dev-1 failed"
CLEANUP_INSTANCES+=("dev-1")
pass "dev-1 running"

"$COAST" run dev-2 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run dev-2 failed"
CLEANUP_INSTANCES+=("dev-2")
pass "dev-2 running"
set -e

# ============================================================
# Test 2: Count tunnel creations from rapid coast ps calls
# ============================================================

echo ""
echo "=== Test 2: Detect tunnel churn via daemon log ==="

# Snapshot the log line count before the polling burst
BEFORE_LINES=$(wc -l < /tmp/coastd-test.log)
echo "  Daemon log lines before polling: $BEFORE_LINES"

# Run 50 iterations of coast ps for both instances (100 total calls)
for i in $(seq 1 50); do
    "$COAST" ps dev-1 >/dev/null 2>&1
    "$COAST" ps dev-2 >/dev/null 2>&1
done
pass "50 iterations of coast ps completed"

# Count new "SSH tunnel established" messages since the burst started
TUNNEL_COUNT=$(tail -n +$((BEFORE_LINES + 1)) /tmp/coastd-test.log | grep -c "SSH tunnel established" || true)
TUNNEL_COUNT=${TUNNEL_COUNT:-0}
TUNNEL_COUNT=$(echo "$TUNNEL_COUNT" | tr -d '[:space:]')
echo "  New 'SSH tunnel established' entries: $TUNNEL_COUNT"

# With connection reuse, we'd expect 0 new tunnels (reusing persistent ones).
# With the bug, each coast ps creates a new tunnel = ~100 entries.
# Threshold: >10 is clearly a per-request pattern.
if [ "$TUNNEL_COUNT" -gt 10 ]; then
    fail "SSH tunnel churn detected: $TUNNEL_COUNT new tunnels created for 100 ps calls (expected <=10 with connection reuse)"
else
    pass "SSH tunnels are being reused ($TUNNEL_COUNT new tunnels)"
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
echo "  All tunnel exhaustion tests passed!"
echo "=========================================="
