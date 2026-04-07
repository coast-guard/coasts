#!/usr/bin/env bash
#
# Integration test: a second remote instance on the same remote host
# must run successfully without failing on shared service tunnel port
# conflicts.
#
# Reproduces the bug where the second `coast run --type remote` on the
# same remote host failed because reverse_forward_ports tried to bind
# ports already occupied by the first instance's tunnels. With the fix,
# existing tunnels are detected and reused.

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

echo "=== Remote Second Instance Shared Services Test ==="
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
# Test 1: Run first instance
# ============================================================

echo ""
echo "=== Test 1: Run first remote instance ==="

set +e
"$COAST" run shared-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "First run failed"
set -e
CLEANUP_INSTANCES+=("shared-1")
pass "First remote instance running"

sleep 3

# ============================================================
# Test 2: Run second instance on same remote (must not fail)
# ============================================================

echo ""
echo "=== Test 2: Run second remote instance (same remote) ==="

set +e
RUN2_OUT=$("$COAST" run shared-2 --type remote 2>&1)
RUN2_EXIT=$?
set -e

if [ "$RUN2_EXIT" -ne 0 ]; then
    echo "  Second run output: $RUN2_OUT"
    fail "Second run failed -- shared service tunnel port conflict"
fi
CLEANUP_INSTANCES+=("shared-2")
pass "Second remote instance created successfully"

sleep 5

# ============================================================
# Test 3: Verify both instances are running
# ============================================================

echo ""
echo "=== Test 3: Verify both instances running ==="

LS_OUT=$("$COAST" ls 2>&1)
echo "$LS_OUT" | head -5

INST_COUNT=$(echo "$LS_OUT" | grep -c "remote.*running" || echo 0)
echo "  Running remote instances: $INST_COUNT"

if [ "$INST_COUNT" -ge 2 ]; then
    pass "Both remote instances running"
else
    fail "Expected 2 running remote instances, got $INST_COUNT"
fi

# ============================================================
# Test 4: Verify daemon logged tunnel reuse (not conflict)
# ============================================================

echo ""
echo "=== Test 4: Verify tunnel reuse logged ==="

if grep -q "already exist.*reusing" /tmp/coastd-test.log 2>/dev/null; then
    pass "Daemon correctly reused existing shared service tunnels"
elif grep -q "shared service reverse tunnels created" /tmp/coastd-test.log 2>/dev/null; then
    pass "Shared service tunnels created (no prior tunnels)"
else
    echo "  WARNING: No tunnel reuse or creation logged"
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
echo "  All second-instance shared services tests passed!"
echo "=========================================="
