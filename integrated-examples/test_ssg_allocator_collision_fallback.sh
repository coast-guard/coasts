#!/usr/bin/env bash
#
# Integration test (Phase 31, DESIGN §24.5): the virtual-port
# allocator skips ports that are already bound by something outside
# Coast and picks the next free port in the band.
#
# Scenario:
#   1. Pick a narrow allocator band via env vars
#      (`COAST_VIRTUAL_PORT_BAND_START` / `_END`).
#   2. Pre-bind the START port using a host-side socat (or python
#      stub) — anything that holds the port for the duration of the
#      test.
#   3. Build + run the SSG. Its single service must allocate a
#      virtual port that is NOT the pre-bound one — typically
#      START+1 (the next free).
#   4. Tear the pre-bound listener down at the end.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-consumer-basic"

# Band picked well above ephemeral ports + the production default
# (42000-43000) so the test can claim its own uncontended slot
# without colliding with parallel test runs.
BAND_START=42400
BAND_END=42410

CLEANUP_INSTANCES=()
BLOCKER_PID=""

_cleanup() {
    echo ""
    echo "--- Cleanup ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    if [ -n "$BLOCKER_PID" ]; then
        kill "$BLOCKER_PID" 2>/dev/null || true
    fi
    "$COAST" ssg rm --with-data --force 2>/dev/null || true
    cleanup_project_ssgs "$SSG_PROJECT"
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    rm -rf "$HOME/.coast/ssg"
}
trap '_cleanup' EXIT

preflight_checks
echo ""
echo "=== Setup ==="
clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"
rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Pre-bind the START port BEFORE the daemon starts so the allocator's
# probe sees it as taken. python3's socketserver is the most portable
# way to hold a TCP port across shells.
python3 -c "
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(('0.0.0.0', $BAND_START))
s.listen(1)
print('blocker bound on $BAND_START', flush=True)
import time
while True:
    time.sleep(60)
" &
BLOCKER_PID=$!
sleep 1

# Sanity check: port is actually bound.
if ! nc -z 127.0.0.1 "$BAND_START" 2>/dev/null && \
   ! python3 -c "import socket; s=socket.socket(); s.settimeout(1); s.connect(('127.0.0.1', $BAND_START))" 2>/dev/null; then
    fail "blocker process did not bind $BAND_START successfully"
fi
pass "pre-bound $BAND_START with PID $BLOCKER_PID"

# Start the daemon AFTER setting the band env vars so the daemon
# inherits them.
COAST_VIRTUAL_PORT_BAND_START=$BAND_START \
COAST_VIRTUAL_PORT_BAND_END=$BAND_END \
    start_daemon

echo ""
echo "=== Step 1: build + run SSG inside the narrow band ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 3

PORTS=$("$COAST" ssg ports 2>&1)
echo "$PORTS"
VPORT=$(echo "$PORTS" | awk '/^  postgres/ {print $4}')
[ -n "$VPORT" ] && [ "$VPORT" != "--" ] \
    || fail "expected an allocated virtual port; got '$VPORT'"

if [ "$VPORT" = "$BAND_START" ]; then
    fail "Phase 26 violation: allocator handed out the pre-bound port $BAND_START; collision skip didn't fire"
fi
if [ "$VPORT" -lt "$BAND_START" ] || [ "$VPORT" -gt "$BAND_END" ]; then
    fail "allocator picked a port outside the configured band [$BAND_START-$BAND_END]: $VPORT"
fi
pass "allocator skipped the pre-bound port $BAND_START and chose $VPORT (within band [$BAND_START-$BAND_END])"

echo ""
echo "==========================================="
echo "  PHASE 31 ALLOCATOR-COLLISION-FALLBACK OK"
echo "==========================================="
