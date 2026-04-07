#!/usr/bin/env bash
#
# Integration test: rsync during coast assign should reuse SSH connections
# via ControlMaster, not create new SSH tunnel processes.
#
# Without ControlMaster flags on the rsync -e "ssh ..." command, each
# rsync invocation creates its own SSH connection. With ControlMaster,
# rsync piggybacks on the existing control socket, avoiding new SSH
# process creation.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    "$COAST" remote rm test-remote 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    cd "$PROJECTS_DIR/remote/coast-remote-basic" 2>/dev/null || true
    git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Rsync ControlMaster Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-basic"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 5

# ============================================================
# Test 1: Assign to worktree and check SSH tunnel churn
# ============================================================

echo ""
echo "=== Test 1: Assign with ControlMaster (no new SSH tunnels) ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

# Snapshot daemon log before assign
BEFORE_LINES=$(wc -l < /tmp/coastd-test.log | tr -d ' ')
echo "  Daemon log lines before assign: $BEFORE_LINES"

"$COAST" assign dev-1 -w feature-sync-test 2>&1 >/dev/null
pass "Assign completed"

# Count new "SSH tunnel established" entries during assign
TUNNEL_COUNT=$(tail -n +$((BEFORE_LINES + 1)) /tmp/coastd-test.log | grep -c "SSH tunnel established" || true)
TUNNEL_COUNT=$(echo "$TUNNEL_COUNT" | tr -d '[:space:]')
echo "  New SSH tunnel entries during assign: $TUNNEL_COUNT"

# With ControlMaster, rsync reuses the existing connection.
# The assign creates 1 tunnel for RemoteClient::connect (cached).
# Without ControlMaster on rsync, we'd see additional tunnel entries
# from the rsync ssh subprocess. With it, rsync reuses the control socket.
# Allow up to 2 (1 for the RemoteClient tunnel + 1 for sudo probe).
if [ "$TUNNEL_COUNT" -gt 2 ]; then
    fail "rsync created extra SSH connections: $TUNNEL_COUNT tunnel entries (expected <=2 with ControlMaster)"
else
    pass "rsync reused SSH via ControlMaster ($TUNNEL_COUNT tunnel entries)"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All rsync ControlMaster tests passed!"
echo "=========================================="
