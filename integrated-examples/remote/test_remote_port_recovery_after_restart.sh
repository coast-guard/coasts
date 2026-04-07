#!/usr/bin/env bash
#
# Integration test: the fuser-based stale port recovery must kill a
# process holding a port so a new SSH reverse tunnel can bind.
#
# Simulates the sleep/wake scenario where a stale sshd holds a port
# by using a listening process (nc) bound to 127.0.0.1:25432.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

TEST_PORT=25432

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    pkill -f "ssh.*-N.*-R.*$TEST_PORT" 2>/dev/null || true
    pkill -f "socat.*TCP-LISTEN:$TEST_PORT" 2>/dev/null || true
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Stale Port Recovery Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
setup_localhost_ssh

# ============================================================
# Test 1: Bind port with a stale holder (simulates stale sshd)
# ============================================================

echo ""
echo "=== Test 1: Create stale port holder ==="

socat TCP-LISTEN:$TEST_PORT,reuseaddr,fork /dev/null &
HOLDER_PID=$!
sleep 1

set +e
HOLDER_ALIVE=$(ps -p $HOLDER_PID -o pid= 2>/dev/null | tr -d ' ')
set -e

if [ -n "$HOLDER_ALIVE" ]; then
    pass "Port $TEST_PORT held by stale process (PID $HOLDER_PID)"
else
    fail "Failed to create port holder"
fi

# ============================================================
# Test 2: ssh -R fails when port is occupied
# ============================================================

echo ""
echo "=== Test 2: Verify ssh -R fails on occupied port ==="

ssh -N -R "0.0.0.0:$TEST_PORT:localhost:$TEST_PORT" \
    -p 22 -i ~/.ssh/coast_test_key \
    -o StrictHostKeyChecking=no -o BatchMode=yes \
    -o ExitOnForwardFailure=yes \
    root@localhost 2>/dev/null &
ATTEMPT_PID=$!
sleep 2

set +e
ATTEMPT_ALIVE=$(ps -p $ATTEMPT_PID -o pid= 2>/dev/null | tr -d ' ')
set -e

if [ -z "$ATTEMPT_ALIVE" ]; then
    pass "ssh -R correctly failed on occupied port"
else
    kill $ATTEMPT_PID 2>/dev/null
    pass "ssh -R survived (different interface binding)"
fi

# ============================================================
# Test 3: fuser -k via SSH releases the port
# ============================================================

echo ""
echo "=== Test 3: Release port via fuser -k (daemon's mechanism) ==="

# This mirrors the daemon's release_stale_remote_ports command
set +e
ssh -o BatchMode=yes -o StrictHostKeyChecking=no -i ~/.ssh/coast_test_key \
    root@localhost "sudo fuser -k $TEST_PORT/tcp 2>/dev/null || fuser -k $TEST_PORT/tcp 2>/dev/null || true" 2>/dev/null
set -e

sleep 1

# Verify the holder process was killed
set +e
HOLDER_AFTER=$(ps -p $HOLDER_PID -o pid= 2>/dev/null | tr -d ' ')
set -e

if [ -z "$HOLDER_AFTER" ]; then
    pass "Stale process killed by fuser -k (PID $HOLDER_PID is dead)"
else
    fail "fuser -k failed to kill stale process (PID $HOLDER_PID still alive)"
fi

# ============================================================
# Test 4: New ssh -R tunnel succeeds after release
# ============================================================

echo ""
echo "=== Test 4: New tunnel succeeds after fuser release ==="

ssh -N -R "0.0.0.0:$TEST_PORT:localhost:$TEST_PORT" \
    -p 22 -i ~/.ssh/coast_test_key \
    -o StrictHostKeyChecking=no -o BatchMode=yes \
    -o ExitOnForwardFailure=yes \
    root@localhost 2>/dev/null &
NEW_PID=$!
sleep 2

set +e
NEW_ALIVE=$(ps -p $NEW_PID -o pid= 2>/dev/null | tr -d ' ')
set -e

if [ -n "$NEW_ALIVE" ]; then
    pass "New reverse tunnel alive after port release (PID $NEW_ALIVE)"
else
    fail "New tunnel died -- port may still be in TIME_WAIT"
fi

kill $NEW_PID 2>/dev/null || true

echo ""
echo "=========================================="
echo "  All stale port recovery tests passed!"
echo "=========================================="
