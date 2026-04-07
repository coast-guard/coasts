#!/usr/bin/env bash
#
# Integration test: second remote instance on same remote must reuse
# existing shared service tunnels instead of failing on port conflicts.
#
# Verifies is_remote_port_listening: when reverse tunnel ports are
# already bound on the remote, new tunnel creation is skipped.

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

echo "=== Remote Shared Tunnel Reuse Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-shared-services"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

# ============================================================
# Test 1: First instance creates tunnels
# ============================================================

echo ""
echo "=== Test 1: Run first instance ==="

set +e
"$COAST" run shared-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "First run failed"
set -e
CLEANUP_INSTANCES+=("shared-1")
pass "First instance running"

sleep 3

echo "  Daemon log (tunnel creation):"
grep -E "reverse.*forward|shared.*tunnel|port already bound|skipping" /tmp/coastd-test.log 2>/dev/null | tail -5 || true

# ============================================================
# Test 2: Second instance succeeds (no port conflict)
# ============================================================

echo ""
echo "=== Test 2: Run second instance (same remote) ==="

set +e
RUN2_OUT=$("$COAST" run shared-2 --type remote 2>&1)
RUN2_EXIT=$?
set -e

if [ "$RUN2_EXIT" -ne 0 ]; then
    echo "  Output: $RUN2_OUT"
    fail "Second run failed -- shared service tunnel port conflict"
fi
CLEANUP_INSTANCES+=("shared-2")
pass "Second instance created (no tunnel conflict)"

sleep 3

echo "  Daemon log (tunnel reuse):"
grep -E "port already bound|skipping|tunnel.*reuse\|reverse.*forward" /tmp/coastd-test.log 2>/dev/null | tail -5 || true

# ============================================================
# Test 3: Both instances listed and running
# ============================================================

echo ""
echo "=== Test 3: Both instances running ==="

LS_OUT=$("$COAST" ls 2>&1)
RUNNING=$(echo "$LS_OUT" | grep -c "remote.*running" || echo 0)
echo "  Running remote instances: $RUNNING"

if [ "$RUNNING" -ge 2 ]; then
    pass "Both instances running"
else
    echo "$LS_OUT" | head -5
    fail "Expected 2+ running remote instances"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm shared-1 2>&1 >/dev/null || true
"$COAST" rm shared-2 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All shared tunnel reuse tests passed!"
echo "=========================================="
