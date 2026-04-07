#!/usr/bin/env bash
#
# Integration test: remote shared service port forwarding.
#
# Verifies that a remote coast instance can reach locally-running shared
# services (postgres) through SSH reverse tunnels set up by the daemon.
#
# Flow:
# 1. Set up SSH, coast-service, daemon
# 2. Register a remote
# 3. Build + run a remote coast with shared_services.postgres
# 4. Verify postgres is reachable from inside the remote DinD container
# 5. Verify the compose override has the correct extra_hosts entries
# 6. Clean up

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_remote_cleanup() {
    echo ""
    echo "--- Cleaning up remote shared services test ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_remote_cleanup' EXIT

# ============================================================
# Preflight
# ============================================================

echo "=== Remote Shared Services Integration Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-shared-services"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Register remote
# ============================================================

echo ""
echo "=== Test 1: Register remote ==="

REMOTE_ADD_OUT=$("$COAST" remote add test-remote root@localhost \
    --key ~/.ssh/coast_test_key --port 22 --sync mutagen 2>&1)
assert_contains "$REMOTE_ADD_OUT" "added" "remote add succeeded"

# ============================================================
# Test 2: Build + Run remote coast with shared services
# ============================================================

echo ""
echo "=== Test 2: Build + Run remote with shared services ==="

RUN_OUT=$("$COAST" run shared-1 --type remote --remote test-remote 2>&1) || {
    echo "Run output: $RUN_OUT"
    fail "coast run --type remote failed"
}
CLEANUP_INSTANCES+=("shared-1")

assert_contains "$RUN_OUT" "ok" "remote run with shared services succeeded"

# ============================================================
# Test 3: Verify shared services started locally
# ============================================================

echo ""
echo "=== Test 3: Verify local shared services ==="

SHARED_PS=$("$COAST" shared ps 2>&1) || true
assert_contains "$SHARED_PS" "postgres" "postgres shared service is running locally"

# ============================================================
# Test 4: Verify PostgreSQL is reachable from inside the DinD
# ============================================================

echo ""
echo "=== Test 4: Verify PostgreSQL reachable via host.docker.internal ==="

sleep 5

set +e
PG_CHECK=$("$COAST" exec shared-1 -- sh -c '
  GATEWAY=$(grep host.docker.internal /etc/hosts 2>/dev/null | awk "{print \$1}" | head -1)
  echo "host.docker.internal=$GATEWAY"
  # Try TCP connect to PostgreSQL via host.docker.internal
  (echo "" | nc -w 2 host.docker.internal 5432 >/dev/null 2>&1) && echo "PG_REACHABLE=yes" || echo "PG_REACHABLE=no"
  # Also try via the gateway IP directly
  if [ -n "$GATEWAY" ]; then
    (echo "" | nc -w 2 $GATEWAY 5432 >/dev/null 2>&1) && echo "PG_GATEWAY_REACHABLE=yes" || echo "PG_GATEWAY_REACHABLE=no"
  fi
' 2>&1)
set -e

echo "  $PG_CHECK"
assert_contains "$PG_CHECK" "PG_REACHABLE=yes" "PostgreSQL reachable at host.docker.internal:5432 from DinD"

# ============================================================
# Test 5: Verify host-gateway was replaced in compose file
# ============================================================

echo ""
echo "=== Test 5: Verify host-gateway replaced in compose file ==="

set +e
COMPOSE_CHECK=$("$COAST" exec shared-1 -- sh -c '
  if [ -f /coast-artifact/compose.coast-shared.yml ]; then
    if grep -q "host-gateway" /coast-artifact/compose.coast-shared.yml; then
      echo "STILL_HAS_HOST_GATEWAY=yes"
    else
      echo "STILL_HAS_HOST_GATEWAY=no"
    fi
  else
    echo "NO_SHARED_COMPOSE=true"
  fi
' 2>&1)
set -e

echo "  $COMPOSE_CHECK"
if echo "$COMPOSE_CHECK" | grep -q "NO_SHARED_COMPOSE"; then
    echo "  SKIP: no shared compose file (bare service project)"
else
    assert_contains "$COMPOSE_CHECK" "STILL_HAS_HOST_GATEWAY=no" "host-gateway replaced with actual IP in compose file"
fi

# ============================================================
# Test 6: Verify instance is listed as remote
# ============================================================

echo ""
echo "=== Test 6: Verify instance listing ==="

LS_OUT=$("$COAST" ls 2>&1) || true
assert_contains "$LS_OUT" "shared-1" "instance appears in ls"
assert_contains "$LS_OUT" "test-remote" "instance shows remote host"

# ============================================================
# Test 7: Clean up remote instance
# ============================================================

echo ""
echo "=== Test 7: Remove remote instance ==="

RM_OUT=$("$COAST" rm shared-1 2>&1) || true
assert_contains "$RM_OUT" "removed\|Removed\|ok" "instance removed"

echo ""
echo "=== All remote shared services tests passed ==="
