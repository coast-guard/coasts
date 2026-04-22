#!/usr/bin/env bash
#
# Phase 18 integration test: two remote instances of the same project
# on the same remote VM must each get their OWN dynamic `remote_port`
# for every shared-service reverse tunnel.
#
# Pre-Phase-18 the second run's reverse_forward_ports call would fail
# the bind and the daemon would log "already bound, reusing existing",
# silently aliasing coast B's traffic onto coast A's tunnel. Phase 18
# allocates a unique `remote_port` per forward so each coast's sshd
# listener sits on a distinct port.
#
# Replaces the pre-Phase-18 `test_remote_shared_tunnel_reuse.sh`.

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
    pkill -f "ssh -N -R" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Phase 18: Multi-instance independent tunnels ==="
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
# Test 1: First instance creates a tunnel on some dynamic port
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

FIRST_REMOTE_PORT=$(pgrep -af "ssh -N -R 0.0.0.0:" | \
    grep -oE '0\.0\.0\.0:[0-9]+:localhost:5432' | \
    head -1 | cut -d: -f2)
[ -n "$FIRST_REMOTE_PORT" ] || fail "could not identify first instance's reverse-tunnel remote port"
pass "First instance tunnel: remote_port=$FIRST_REMOTE_PORT -> localhost:5432 (canonical, inline)"

# ============================================================
# Test 2: Second instance succeeds with a DIFFERENT remote port
# ============================================================

echo ""
echo "=== Test 2: Run second instance on same remote ==="

set +e
RUN2_OUT=$("$COAST" run shared-2 --type remote 2>&1)
RUN2_EXIT=$?
set -e

if [ "$RUN2_EXIT" -ne 0 ]; then
    echo "  Output: $RUN2_OUT"
    fail "Second run failed -- shared-service tunnel allocation collided"
fi
CLEANUP_INSTANCES+=("shared-2")
pass "Second instance created"

sleep 3

# ============================================================
# Test 3: Each instance owns a distinct remote tunnel port
# ============================================================

echo ""
echo "=== Test 3: Distinct remote tunnel ports ==="

ALL_REMOTE_PORTS=$(pgrep -af "ssh -N -R 0.0.0.0:" | \
    grep -oE '0\.0\.0\.0:[0-9]+:localhost:5432' | \
    awk -F: '{print $2}' | sort -u)

PORT_COUNT=$(echo "$ALL_REMOTE_PORTS" | grep -cv '^$' || echo 0)
echo "  Distinct remote ports bound by sshd: $PORT_COUNT"
echo "$ALL_REMOTE_PORTS" | sed 's/^/    /'

if [ "$PORT_COUNT" -lt 2 ]; then
    echo "--- daemon log tail ---"
    tail -60 /tmp/coastd-test.log 2>/dev/null || true
    fail "Phase 18 requires each instance to bind its own remote port; got $PORT_COUNT"
fi
pass "Both instances have independent reverse-tunnel remote ports"

# ============================================================
# Test 4: Both instances listed and running
# ============================================================

echo ""
echo "=== Test 4: Both instances running ==="

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
echo "  Phase 18 multi-instance independence OK"
echo "=========================================="
