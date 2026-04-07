#!/usr/bin/env bash
#
# Integration test for remote exec session scoping.
#
# Tests that:
# 1. Sessions created with scope=remote-exec:<remote> only appear in that scope
# 2. Sessions created with scope=remote-instance-exec:<project>:<instance> only
#    appear in that scope
# 3. Sessions without a scope default to filtering by remote name (backward compat)
# 4. Different scopes on the same remote are fully isolated
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#   - python3 available
#
# Usage:
#   ./integrated-examples/remote/test_remote_exec_session_scope.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

DAEMON_API="http://localhost:31415"

_scope_cleanup() {
    echo ""
    echo "--- Cleaning up exec session scope test ---"

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
trap '_scope_cleanup' EXIT

# Helper: open a WS connection to the daemon remote exec endpoint,
# read the session_id from the init message, then close.
# Usage: ws_create_session "query_string"
# Prints the session_id on stdout.
ws_create_session() {
    local qs="$1"
    python3 -c "
import socket, hashlib, base64, struct, json, sys, os

host, port = '127.0.0.1', 31415
path = '/api/v1/remote/exec/interactive?${qs}'
key = base64.b64encode(os.urandom(16)).decode()

sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
sock.settimeout(15)
sock.connect((host, port))

req = (
    f'GET {path} HTTP/1.1\r\n'
    f'Host: {host}:{port}\r\n'
    f'Upgrade: websocket\r\n'
    f'Connection: Upgrade\r\n'
    f'Sec-WebSocket-Key: {key}\r\n'
    f'Sec-WebSocket-Version: 13\r\n'
    f'\r\n'
)
sock.sendall(req.encode())

resp = b''
while b'\r\n\r\n' not in resp:
    chunk = sock.recv(4096)
    if not chunk:
        break
    resp += chunk

status_line = resp.split(b'\r\n')[0].decode()
if '101' not in status_line:
    print(f'WS upgrade failed: {status_line}', file=sys.stderr)
    sock.close()
    sys.exit(1)

# Read a single WS text frame (the session init message)
after_headers = resp.split(b'\r\n\r\n', 1)[1] if b'\r\n\r\n' in resp else b''
buf = bytearray(after_headers)
while len(buf) < 2:
    buf.extend(sock.recv(4096))

b0, b1 = buf[0], buf[1]
payload_len = b1 & 0x7F
offset = 2
if payload_len == 126:
    while len(buf) < 4:
        buf.extend(sock.recv(4096))
    payload_len = struct.unpack('!H', buf[2:4])[0]
    offset = 4
elif payload_len == 127:
    while len(buf) < 10:
        buf.extend(sock.recv(4096))
    payload_len = struct.unpack('!Q', buf[2:10])[0]
    offset = 10

while len(buf) < offset + payload_len:
    buf.extend(sock.recv(4096))

payload = buf[offset:offset+payload_len].decode()
msg = json.loads(payload)
print(msg.get('session_id', ''))

# Send a close frame and disconnect
close_frame = struct.pack('!BBH', 0x88, 0x82, 1000) + os.urandom(4)
try:
    sock.sendall(close_frame)
except:
    pass
sock.close()
"
}

# ============================================================
# Preflight
# ============================================================

echo "=== Remote Exec Session Scope Test ==="
echo ""

preflight_checks

# ============================================================
# Setup
# ============================================================

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

echo "--- Registering remote ---"
ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "remote registered"

# ============================================================
# Test 1: Create session with scope=remote-exec:test-remote
# ============================================================

echo ""
echo "=== Test 1: Create bare remote session (scope=remote-exec:test-remote) ==="

SCOPE_A="remote-exec:test-remote"
SID_A=$(ws_create_session "name=test-remote&scope=${SCOPE_A}")
[ -n "$SID_A" ] || fail "Failed to create session with scope $SCOPE_A"
pass "Session A created: $SID_A (scope=$SCOPE_A)"

# ============================================================
# Test 2: Create session with scope=remote-instance-exec:proj:r1
# ============================================================

echo ""
echo "=== Test 2: Create instance session (scope=remote-instance-exec:proj:r1) ==="

SCOPE_B="remote-instance-exec:proj:r1"
SID_B=$(ws_create_session "name=test-remote&scope=${SCOPE_B}")
[ -n "$SID_B" ] || fail "Failed to create session with scope $SCOPE_B"
pass "Session B created: $SID_B (scope=$SCOPE_B)"

# ============================================================
# Test 3: List sessions with scope A — only session A
# ============================================================

echo ""
echo "=== Test 3: List sessions with scope=$SCOPE_A ==="

RESP_A=$(curl -sS "${DAEMON_API}/api/v1/remote/exec/sessions?name=test-remote&scope=${SCOPE_A}" 2>&1)
COUNT_A=$(echo "$RESP_A" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
assert_eq "$COUNT_A" "1" "scope A returns exactly 1 session"
assert_contains "$RESP_A" "$SID_A" "scope A contains session A"
assert_not_contains "$RESP_A" "$SID_B" "scope A does NOT contain session B"
pass "Scope A isolation verified"

# ============================================================
# Test 4: List sessions with scope B — only session B
# ============================================================

echo ""
echo "=== Test 4: List sessions with scope=$SCOPE_B ==="

RESP_B=$(curl -sS "${DAEMON_API}/api/v1/remote/exec/sessions?name=test-remote&scope=${SCOPE_B}" 2>&1)
COUNT_B=$(echo "$RESP_B" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
assert_eq "$COUNT_B" "1" "scope B returns exactly 1 session"
assert_contains "$RESP_B" "$SID_B" "scope B contains session B"
assert_not_contains "$RESP_B" "$SID_A" "scope B does NOT contain session A"
pass "Scope B isolation verified"

# ============================================================
# Test 5: List sessions without scope — falls back to name match
# ============================================================

echo ""
echo "=== Test 5: List sessions without scope (backward compat) ==="

RESP_NO_SCOPE=$(curl -sS "${DAEMON_API}/api/v1/remote/exec/sessions?name=test-remote" 2>&1)
COUNT_NO_SCOPE=$(echo "$RESP_NO_SCOPE" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")

# Without scope, filter is s.project == "test-remote". Neither session A nor B
# has project == "test-remote" (they have scoped project values), so count is 0.
# This is correct: old clients that don't send scope won't see scoped sessions.
assert_eq "$COUNT_NO_SCOPE" "0" "no-scope query returns 0 (scoped sessions hidden from legacy)"
pass "Backward compat verified (scoped sessions not leaked)"

# ============================================================
# Test 6: Create a legacy (no scope) session — visible without scope
# ============================================================

echo ""
echo "=== Test 6: Legacy session (no scope param) ==="

SID_LEGACY=$(ws_create_session "name=test-remote")
[ -n "$SID_LEGACY" ] || fail "Failed to create legacy session"
pass "Legacy session created: $SID_LEGACY"

RESP_LEGACY=$(curl -sS "${DAEMON_API}/api/v1/remote/exec/sessions?name=test-remote" 2>&1)
COUNT_LEGACY=$(echo "$RESP_LEGACY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))")
assert_eq "$COUNT_LEGACY" "1" "no-scope query returns 1 (the legacy session)"
assert_contains "$RESP_LEGACY" "$SID_LEGACY" "legacy session visible without scope"
assert_not_contains "$RESP_LEGACY" "$SID_A" "scoped session A not in legacy list"
assert_not_contains "$RESP_LEGACY" "$SID_B" "scoped session B not in legacy list"
pass "Legacy session isolation verified"

# ============================================================
# Done
# ============================================================

echo ""
echo "=== All session scope tests passed ==="
