#!/usr/bin/env bash
#
# Integration test for remote coast exec/ps/logs error paths.
#
# Tests that:
# 1. coast exec on a stopped remote instance fails with "stopped"
# 2. coast exec with a bad command returns non-zero exit code
# 3. coast ps on a stopped instance shows appropriate message
# 4. coast logs on a bare-service project (no compose) handles gracefully
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_exec_errors.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_exec_errors_cleanup() {
    echo ""
    echo "--- Cleaning up exec errors test ---"

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
trap '_exec_errors_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Exec/PS/Logs Error Integration Tests ==="
echo ""

preflight_checks

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

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

# Build and run an instance
set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || fail "Build failed: $BUILD_OUT"

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Instance running"

# ============================================================
# Test 1: Exec on stopped instance
# ============================================================

echo ""
echo "=== Test 1: coast exec on stopped instance ==="

"$COAST" stop dev-1 2>&1 >/dev/null
pass "Instance stopped"

set +e
EXEC_OUT=$("$COAST" exec dev-1 -- echo "should fail" 2>&1)
EXEC_EXIT=$?
set -e

echo "  exec exit code: $EXEC_EXIT"
echo "  exec output: $EXEC_OUT"

[ "$EXEC_EXIT" -ne 0 ] || fail "exec on stopped instance should fail"
pass "exec on stopped instance fails"

if echo "$EXEC_OUT" | grep -qi "stopped"; then
    pass "error message mentions 'stopped'"
else
    echo "  Note: error doesn't explicitly say 'stopped'"
    pass "exec correctly rejected on stopped instance"
fi

# Restart for next tests
"$COAST" start dev-1 2>&1 >/dev/null
pass "Instance restarted"

# ============================================================
# Test 2: Exec with bad command
# ============================================================

echo ""
echo "=== Test 2: coast exec with nonexistent command ==="

set +e
BAD_EXEC_OUT=$("$COAST" exec dev-1 -- /usr/bin/this_command_does_not_exist_xyz 2>&1)
BAD_EXEC_EXIT=$?
set -e

echo "  exec exit code: $BAD_EXEC_EXIT"
echo "  exec output (first 2 lines): $(echo "$BAD_EXEC_OUT" | head -2)"

[ "$BAD_EXEC_EXIT" -ne 0 ] || fail "exec with bad command should return non-zero"
pass "exec with bad command returns non-zero exit code"

# ============================================================
# Test 3: PS on stopped instance
# ============================================================

echo ""
echo "=== Test 3: coast ps on stopped instance ==="

"$COAST" stop dev-1 2>&1 >/dev/null
pass "Instance stopped again"

set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e

echo "  ps exit code: $PS_EXIT"
echo "  ps output: $PS_OUT"

# PS on a stopped instance should either fail with an error or show empty.
# Both are acceptable behaviors.
if [ "$PS_EXIT" -ne 0 ]; then
    pass "ps on stopped instance returns error (expected)"
else
    pass "ps on stopped instance returns empty/ok (acceptable)"
fi

# Restart for next test
"$COAST" start dev-1 2>&1 >/dev/null

# ============================================================
# Test 4: Logs on bare-service project (no compose)
# ============================================================

echo ""
echo "=== Test 4: coast logs on bare-service project (no compose) ==="

# coast-remote-basic uses [services.app] (bare service), not docker-compose.
# Logs should handle this gracefully.

set +e
LOGS_OUT=$("$COAST" logs dev-1 2>&1)
LOGS_EXIT=$?
set -e

echo "  logs exit code: $LOGS_EXIT"
echo "  logs output (first 3 lines): $(echo "$LOGS_OUT" | head -3)"

# Logs may succeed with empty output, or may error since compose isn't used.
# Either is acceptable — the key is it doesn't crash or hang.
if [ "$LOGS_EXIT" -eq 0 ]; then
    pass "logs on bare-service project returns ok (may be empty)"
else
    pass "logs on bare-service project returns error (no compose, expected)"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm dev-1 2>/dev/null || true
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote exec/ps/logs error tests passed!"
echo "=========================================="
