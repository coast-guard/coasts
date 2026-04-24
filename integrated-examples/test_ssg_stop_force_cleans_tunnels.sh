#!/usr/bin/env bash
#
# Integration test: `coast ssg stop --force` tears down reverse ssh
# tunnels for shadow consumers before stopping the SSG (Phase 4.5,
# DESIGN.md §20.6).
#
# Uses the remote consumer fixture. Asserts:
#   1. The reverse-tunnel ssh children exist before the stop.
#   2. `coast ssg stop --force` exits 0.
#   3. The coast-ssg container is gone.
#   4. The previously-recorded ssh PIDs are no longer alive (the
#      --force teardown actually killed them).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) — the SSG container is
# `{project}-ssg`. Under Phase 23 per-project, the consumer project
# owns its SSG, so the SSG is built from the consumer's cwd.
SSG_PROJECT="coast-ssg-consumer-remote"

_ssg_remote_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    "$COAST" remote rm test-remote 2>/dev/null || true
    "$COAST" ssg rm --with-data --force 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "ssh -N -R" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    cleanup_project_ssgs "$SSG_PROJECT"
    echo "Cleanup complete."
}
trap '_ssg_remote_cleanup' EXIT

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

setup_localhost_ssh
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

start_daemon

echo ""
echo "=== Step 1: SSG + remote consumer up ==="

# Phase 25: build SSG from the consumer's cwd so the SSG is owned by
# the consumer's project (Phase 23 per-project contract).
cd "$PROJECTS_DIR/coast-ssg-consumer-remote"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1

sleep 5

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key >/dev/null 2>&1

"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

CLEANUP_INSTANCES+=("force-a")
RUN_OUT=$("$COAST" run force-a --type remote 2>&1)
assert_contains "$RUN_OUT" "Created coast instance" "remote consumer up"

sleep 3

echo ""
echo "=== Step 2: capture the reverse ssh PIDs before --force ==="

BEFORE_PIDS=$(pgrep -f "ssh -N -R 0.0.0.0:" 2>/dev/null || true)
echo "reverse ssh PIDs before: '$BEFORE_PIDS'"
[ -n "$BEFORE_PIDS" ] || fail "expected at least one reverse ssh child before --force"
pass "found reverse ssh children: $BEFORE_PIDS"

echo ""
echo "=== Step 3: coast ssg stop --force ==="

set +e
STOP_OUT=$("$COAST" ssg stop --force 2>&1)
STOP_RC=$?
set -e
echo "$STOP_OUT"
echo "exit code: $STOP_RC"

[ "$STOP_RC" -eq 0 ] || fail "coast ssg stop --force must succeed"
assert_contains "$STOP_OUT" "SSG stopped" "stop reported success"

echo ""
echo "=== Step 4: outer SSG container is gone ==="

DOCKER_PS_RUNNING=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_RUNNING" ]; then
    fail "${SSG_PROJECT}-ssg container still running after --force stop"
fi
pass "coast-ssg container is no longer running"

echo ""
echo "=== Step 5: the captured ssh PIDs are dead ==="

# Give SIGTERM a beat to propagate.
sleep 2

REMAINING=""
for pid in $BEFORE_PIDS; do
    if kill -0 "$pid" 2>/dev/null; then
        REMAINING="$REMAINING $pid"
    fi
done
REMAINING=$(echo "$REMAINING" | xargs)
if [ -n "$REMAINING" ]; then
    echo "still-alive ssh PIDs: $REMAINING"
    echo "--- ps --- "
    pgrep -af "ssh -N -R" 2>/dev/null || true
    fail "reverse ssh children survived --force stop: $REMAINING"
fi
pass "all captured reverse ssh children are dead"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG STOP FORCE CLEANS TUNNELS TESTS PASSED"
echo "==========================================="
