#!/usr/bin/env bash
#
# Integration test (Phase 31, DESIGN §24): the daemon's startup
# `host_socat::reconcile_all` respawns dead host socats so consumers
# never see ECONNREFUSED past the daemon's own restart window.
#
# Scenario:
#   1. Build + run SSG.
#   2. Capture the host socat pidfile (`~/.coast/socats/`) and confirm
#      the recorded pid is a live process.
#   3. Kill the host socat directly (SIGKILL, bypassing the daemon).
#   4. Daemon-side state still says "running"; the pidfile may
#      point at a dead pid.
#   5. Restart the daemon. `restore_host_socats` runs as part of
#      `restore_running_state`.
#   6. After the restart, the host socat is back up — pidfile records
#      a different (live) pid; argv still references the original
#      virtual port.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-consumer-basic"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="
clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"
start_daemon

echo ""
echo "=== Step 1: SSG build + run ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 3

PORTS=$("$COAST" ssg ports 2>&1)
VPORT=$(echo "$PORTS" | awk '/^  postgres/ {print $4}')
[ -n "$VPORT" ] && [ "$VPORT" != "--" ] || fail "missing virtual port"
pass "running with virtual port = $VPORT"

# Phase 27/28: pidfile path is `~/.coast/socats/<project>--<service>--<container_port>.pid`.
PIDFILE="$HOME/.coast/socats/${SSG_PROJECT}--postgres--5432.pid"
[ -f "$PIDFILE" ] || fail "expected host socat pidfile at $PIDFILE"
ORIG_PID=$(cat "$PIDFILE" | tr -d '[:space:]')
[ -n "$ORIG_PID" ] || fail "pidfile $PIDFILE was empty"

# `kill -0` probes liveness without delivering a signal.
kill -0 "$ORIG_PID" 2>/dev/null || fail "host socat pid $ORIG_PID is not alive"
pass "host socat pid $ORIG_PID alive (vport $VPORT)"

echo ""
echo "=== Step 2: kill host socat outside the daemon ==="

kill -9 "$ORIG_PID" 2>/dev/null || true
sleep 1
if kill -0 "$ORIG_PID" 2>/dev/null; then
    fail "host socat pid $ORIG_PID still alive after SIGKILL"
fi
pass "host socat pid $ORIG_PID is dead"

echo ""
echo "=== Step 3: restart the daemon -> reconcile_all respawns ==="

# `start_daemon` reuses our test daemon machinery; pkill the
# foreground daemon and start a fresh one.
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
# `restore_running_state` runs in a background tokio task; allow a
# few seconds for the host_socat reconcile pass to land.
sleep 5

NEW_PID=$(cat "$PIDFILE" 2>/dev/null | tr -d '[:space:]')
if [ -z "$NEW_PID" ]; then
    fail "pidfile $PIDFILE empty after daemon restart"
fi
if ! kill -0 "$NEW_PID" 2>/dev/null; then
    fail "host socat pid $NEW_PID not alive after daemon restart"
fi
[ "$NEW_PID" != "$ORIG_PID" ] \
    || fail "expected a fresh host socat pid (was $ORIG_PID); got the same"
pass "host socat respawned: $ORIG_PID (dead) -> $NEW_PID (alive)"

echo ""
echo "=== Step 4: virtual port survived ==="

PORTS_AFTER=$("$COAST" ssg ports 2>&1)
VPORT_AFTER=$(echo "$PORTS_AFTER" | awk '/^  postgres/ {print $4}')
[ "$VPORT_AFTER" = "$VPORT" ] \
    || fail "virtual port changed across daemon restart: $VPORT -> $VPORT_AFTER (allocation must persist)"
pass "virtual port stable across daemon restart: $VPORT"

# The respawned socat's listen port (in argv via the .argv sidecar)
# must still be the same vport.
ARGVFILE="${PIDFILE}.argv"
if [ -f "$ARGVFILE" ]; then
    if ! grep -q "TCP-LISTEN:${VPORT}" "$ARGVFILE"; then
        fail "argv sidecar $ARGVFILE doesn't reference vport $VPORT: $(cat "$ARGVFILE")"
    fi
    pass "argv sidecar references vport $VPORT (host_socat respawned with the right argv)"
fi

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  PHASE 31 HOST-SOCAT-RESPAWN-ON-RESTART OK"
echo "==========================================="
