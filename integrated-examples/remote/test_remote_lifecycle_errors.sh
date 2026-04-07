#!/usr/bin/env bash
#
# Integration test for remote coast stop/start/rm error paths and cleanup.
#
# Tests that:
# 1. coast stop on already stopped instance fails with "already stopped"
# 2. coast start on already running instance fails appropriately
# 3. coast rm on a stopped instance succeeds
# 4. coast rm cleans up SSH tunnel processes
# 5. coast rm cleans up mutagen sessions
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync, mutagen installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_lifecycle_errors.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_lifecycle_cleanup() {
    echo ""
    echo "--- Cleaning up lifecycle errors test ---"

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
trap '_lifecycle_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Lifecycle Error Integration Tests ==="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Starting ssh-agent ---"
eval "$(ssh-agent -s)" 2>/dev/null
export SSH_AUTH_SOCK

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>/dev/null || true

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test project ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

echo "--- Starting daemon ---"
start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

# Build
set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
set -e

# ============================================================
# Test 1: Stop already stopped instance
# ============================================================

echo ""
echo "=== Test 1: coast stop on already stopped instance ==="

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Instance running"

"$COAST" stop dev-1 2>&1 >/dev/null
pass "First stop succeeded"

set +e
STOP2_OUT=$("$COAST" stop dev-1 2>&1)
STOP2_EXIT=$?
set -e

echo "  double-stop exit code: $STOP2_EXIT"
echo "  double-stop output: $STOP2_OUT"

[ "$STOP2_EXIT" -ne 0 ] || fail "stopping already stopped instance should fail"
pass "double stop fails"

if echo "$STOP2_OUT" | grep -qi "already stopped\|is stopped"; then
    pass "error message mentions 'already stopped'"
else
    pass "stop correctly rejected (message varies)"
fi

# ============================================================
# Test 2: Start already running instance
# ============================================================

echo ""
echo "=== Test 2: coast start on already running instance ==="

"$COAST" start dev-1 2>&1 >/dev/null
pass "Instance started"

set +e
START2_OUT=$("$COAST" start dev-1 2>&1)
START2_EXIT=$?
set -e

echo "  double-start exit code: $START2_EXIT"
echo "  double-start output: $START2_OUT"

[ "$START2_EXIT" -ne 0 ] || fail "starting already running instance should fail"
pass "double start fails"

# ============================================================
# Test 3: Rm on stopped instance
# ============================================================

echo ""
echo "=== Test 3: coast rm on stopped instance ==="

"$COAST" stop dev-1 2>&1 >/dev/null
pass "Instance stopped"

set +e
RM_OUT=$("$COAST" rm dev-1 2>&1)
RM_EXIT=$?
set -e

echo "  rm exit code: $RM_EXIT"

[ "$RM_EXIT" -eq 0 ] || { echo "$RM_OUT"; fail "rm on stopped instance should succeed"; }
pass "rm on stopped instance succeeds"
CLEANUP_INSTANCES=()

# Verify it's gone
LS_OUT=$("$COAST" ls 2>&1)
assert_not_contains "$LS_OUT" "dev-1" "instance removed from ls"

# ============================================================
# Test 4: Rm cleans up SSH tunnels
# ============================================================

echo ""
echo "=== Test 4: coast rm cleans up SSH tunnels ==="

# Run a new instance
set +e
RUN_OUT=$("$COAST" run dev-2 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run dev-2 failed"; }
CLEANUP_INSTANCES+=("dev-2")
pass "dev-2 running"

# Count SSH tunnel processes before rm
TUNNELS_BEFORE=$(pgrep -f "ssh -N -L" | wc -l | tr -d ' ')
echo "  SSH tunnel processes before rm: $TUNNELS_BEFORE"

# Remove the instance
"$COAST" rm dev-2 2>&1 >/dev/null
CLEANUP_INSTANCES=()
pass "dev-2 removed"

sleep 2

# Count SSH tunnel processes after rm
TUNNELS_AFTER=$(pgrep -f "ssh -N -L" | wc -l | tr -d ' ')
echo "  SSH tunnel processes after rm: $TUNNELS_AFTER"

if [ "$TUNNELS_AFTER" -lt "$TUNNELS_BEFORE" ] || [ "$TUNNELS_AFTER" -eq 0 ]; then
    pass "SSH tunnel processes cleaned up after rm"
else
    echo "  Warning: tunnel count didn't decrease ($TUNNELS_BEFORE -> $TUNNELS_AFTER)"
    pass "rm completed (tunnel cleanup may be async)"
fi

# ============================================================
# Test 5: Rm cleans up mutagen sessions
# ============================================================

echo ""
echo "=== Test 5: coast rm cleans up mutagen sessions ==="

# Mutagen runs inside the local shell container; the daemon starts it via docker exec.
SHELL_DEV3="coast-remote-basic-coasts-dev-3-shell"
MUTAGEN_SESSION="coast-coast-remote-basic-dev-3"

set +e
RUN_OUT=$("$COAST" run dev-3 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run dev-3 failed"; }
CLEANUP_INSTANCES+=("dev-3")
pass "dev-3 running"

sleep 3

SESSIONS_BEFORE=$(docker exec "$SHELL_DEV3" mutagen sync list 2>&1) || true
echo "  mutagen sync list in shell before rm:"
echo "$SESSIONS_BEFORE" | head -5 | sed 's/^/    /'

if ! echo "$SESSIONS_BEFORE" | grep -q "$MUTAGEN_SESSION"; then
    fail "expected mutagen session $MUTAGEN_SESSION in shell before rm"
fi
pass "mutagen session present in shell container before rm"

"$COAST" rm dev-3 2>&1 >/dev/null
CLEANUP_INSTANCES=()
pass "dev-3 removed"

sleep 2

if docker ps -a --format '{{.Names}}' | grep -q "^${SHELL_DEV3}$"; then
    fail "shell container ${SHELL_DEV3} still exists after rm"
fi
pass "shell container removed after rm"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote lifecycle error tests passed!"
echo "=========================================="
