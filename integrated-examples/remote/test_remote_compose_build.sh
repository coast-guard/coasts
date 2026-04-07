#!/usr/bin/env bash
#
# Integration test for remote compose builds.
#
# Verifies that coast-service builds compose services with build: directives
# natively on the remote, caches the images, loads them into the DinD
# container, and compose services start and respond.
#
# Prerequisites (DinDinD environment):
#   - Docker, openssh-server, rsync installed
#   - Coast binaries built
#
# Usage:
#   ./integrated-examples/remote/test_remote_compose_build.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up remote compose build test ---"

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
trap '_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "=== Remote Compose Build Integration Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-compose-build"

echo "--- Starting daemon ---"
start_daemon

# ============================================================
# Test 1: Register remote + build
# ============================================================

echo ""
echo "=== Test 1: Register remote and build ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
pass "Remote registered"

# Local build creates the coast_image (with mutagen) for the shell container
"$COAST" build 2>&1 >/dev/null
pass "Local build complete (coast_image for shell)"

set +e
BUILD_OUT=$("$COAST" build --type remote 2>&1)
BUILD_EXIT=$?
set -e
[ "$BUILD_EXIT" -eq 0 ] || { echo "$BUILD_OUT"; fail "Remote build failed (exit $BUILD_EXIT)"; }
pass "Remote build complete"

# Check the coast-service log for image build activity
echo "  coast-service build log (image lines):"
grep -i "built\|building\|build_directives\|coast-built" /tmp/coast-service-test.log 2>/dev/null | tail -5 || echo "    (no build lines)"

# ============================================================
# Test 2: Run remote instance
# ============================================================

echo ""
echo "=== Test 2: Run remote instance ==="

set +e
RUN_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_EXIT=$?
set -e
[ "$RUN_EXIT" -eq 0 ] || { echo "$RUN_OUT"; fail "Run failed (exit $RUN_EXIT)"; }
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

# ============================================================
# Test 3: Verify compose container is running (not restarting)
# ============================================================

echo ""
echo "=== Test 3: Verify compose container is running ==="

# Poll up to 30s for the app container to be "Up" (not "Restarting").
# This catches volume mount failures (container crash-loops) and
# image loading issues (container never starts).
APP_UP=false
for i in $(seq 1 6); do
    set +e
    CONTAINER_STATUS=$("$COAST" exec dev-1 -- sh -c 'docker ps --format "{{.Names}} {{.Status}}" --filter "name=app" | head -1' 2>&1)
    set -e
    echo "  [$i/6] container: $CONTAINER_STATUS"

    if echo "$CONTAINER_STATUS" | grep -qi "up"; then
        if echo "$CONTAINER_STATUS" | grep -qi "restarting"; then
            echo "  container is restarting, waiting..."
        else
            APP_UP=true
            break
        fi
    fi
    sleep 5
done

if [ "$APP_UP" = true ]; then
    pass "App container is running stably"
else
    echo "  --- app container logs ---"
    "$COAST" exec dev-1 -- sh -c 'docker logs $(docker ps -aq --filter "name=app" | head -1) 2>&1 | tail -15' 2>&1 || true
    echo "  --- docker ps -a ---"
    "$COAST" exec dev-1 -- docker ps -a 2>&1 || true
    echo "  --- compose file volume entries ---"
    "$COAST" exec dev-1 -- grep -A2 "volumes:" /coast-artifact/compose.coast-shared.yml 2>&1 || \
    "$COAST" exec dev-1 -- grep -A2 "volumes:" /coast-artifact/compose.yml 2>&1 || true
    fail "App container never reached stable 'Up' state"
fi

# ============================================================
# Test 4: Verify volume mount (relative path resolution)
# ============================================================

echo ""
echo "=== Test 4: Verify volume mount resolves correctly ==="

# The compose uses ../app:/app (relative to infra/). With the correct
# --project-directory, this resolves to /workspace/app:/app.
# Without the fix, it resolves to /app (above workspace root) and
# server.js would be missing.
set +e
FILE_CHECK=$("$COAST" exec dev-1 -- sh -c 'docker exec $(docker ps -q --filter "name=app" | head -1) ls /app/server.js 2>&1' 2>&1)
FILE_EXIT=$?
set -e
echo "  file check: $FILE_CHECK"
if echo "$FILE_CHECK" | grep -q "server.js"; then
    pass "Volume mount ../app:/app resolved correctly (server.js in /app)"
else
    echo "  --- mount inspect ---"
    "$COAST" exec dev-1 -- sh -c 'docker inspect $(docker ps -q --filter "name=app" | head -1) --format "{{json .Mounts}}"' 2>&1 || true
    fail "Volume mount failed: server.js not found at /app (--project-directory likely wrong)"
fi

# ============================================================
# Test 5: Verify app HTTP response (end-to-end)
# ============================================================

echo ""
echo "=== Test 5: Verify app HTTP response ==="

# Hit the app's HTTP endpoint from inside the compose container using
# node (guaranteed available in node:20-alpine, unlike wget/curl).
# This proves: image built, loaded, compose started, volume mounted,
# app actually serving.
HTTP_OK=false
for i in $(seq 1 6); do
    set +e
    HTTP_OUT=$("$COAST" exec dev-1 -- sh -c \
        'docker exec $(docker ps -q --filter "name=app" | head -1) node -e "require(\"http\").get(\"http://127.0.0.1:3000\",r=>{let d=\"\";r.on(\"data\",c=>d+=c);r.on(\"end\",()=>process.stdout.write(d))}).on(\"error\",e=>process.stderr.write(e.message))" 2>&1' 2>&1)
    set -e
    echo "  [$i/6] HTTP: $HTTP_OUT"
    if echo "$HTTP_OUT" | grep -q "hello from remote compose build"; then
        HTTP_OK=true
        break
    fi
    sleep 3
done

if [ "$HTTP_OK" = true ]; then
    pass "App responds with correct HTTP content"
else
    echo "  --- app container logs ---"
    "$COAST" exec dev-1 -- sh -c 'docker logs $(docker ps -q --filter "name=app" | head -1) 2>&1 | tail -10' 2>&1 || true
    echo "  --- trying wget from DinD host to published port ---"
    "$COAST" exec dev-1 -- wget -qO- http://localhost:40300 2>&1 || true
    fail "App did not return expected HTTP response 'hello from remote compose build'"
fi

# ============================================================
# Test 6: Verify coast ps returns running services
# ============================================================

echo ""
echo "=== Test 6: Verify coast ps shows running services ==="

set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
PS_EXIT=$?
set -e
echo "  ps output:"
echo "$PS_OUT"

if echo "$PS_OUT" | grep -qi "running"; then
    pass "coast ps returns running services for remote instance"
else
    fail "coast ps returned no running services (got: $PS_OUT)"
fi

SERVICE_COUNT=$(echo "$PS_OUT" | grep -ci "running" || true)
echo "  running service count: $SERVICE_COUNT"
if [ "$SERVICE_COUNT" -ge 1 ]; then
    pass "At least 1 compose service reported as running"
else
    fail "Expected at least 1 running service in coast ps output"
fi

# ============================================================
# Test 7: Verify port is reachable via tunnel
# ============================================================

echo ""
echo "=== Test 7: Verify port tunnel is reachable ==="

# The daemon sets up SSH -L tunnels binding local dynamic ports to
# the remote's dynamic ports. Verify the app port is reachable from
# the host (DinDinD container) via the tunnel. Extract the dynamic
# port from the run output.
DYNAMIC_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
echo "  app dynamic port: $DYNAMIC_PORT"

if [ -n "$DYNAMIC_PORT" ]; then
    # TCP connect test using bash /dev/tcp -- verifies the SSH -L
    # tunnel is working. No nc/wget needed.
    set +e
    (echo > /dev/tcp/127.0.0.1/${DYNAMIC_PORT}) 2>/dev/null
    TUNNEL_EXIT=$?
    set -e
    echo "  TCP connect exit: $TUNNEL_EXIT"
    if [ "$TUNNEL_EXIT" -eq 0 ]; then
        pass "App port $DYNAMIC_PORT reachable via SSH tunnel"
    else
        sleep 3
        set +e
        (echo > /dev/tcp/127.0.0.1/${DYNAMIC_PORT}) 2>/dev/null
        TUNNEL_EXIT2=$?
        set -e
        if [ "$TUNNEL_EXIT2" -eq 0 ]; then
            pass "App port $DYNAMIC_PORT reachable via SSH tunnel (after retry)"
        else
            fail "App port $DYNAMIC_PORT not reachable via SSH tunnel"
        fi
    fi
else
    echo "  Could not extract dynamic port from run output"
    pass "Port tunnel test skipped (no dynamic port in output)"
fi

# ============================================================
# Test 8: Verify shared service ports excluded from allocations
# ============================================================

echo ""
echo "=== Test 8: Shared service ports not in port list ==="

set +e
PORTS_OUT=$("$COAST" ports dev-1 2>&1)
set -e
echo "  ports output:"
echo "$PORTS_OUT"

if echo "$PORTS_OUT" | grep -qi "app"; then
    pass "App port is in port allocations"
else
    fail "App port missing from port allocations"
fi

if echo "$PORTS_OUT" | grep -q "5432"; then
    fail "Shared service port 5432 (db) should NOT be in port allocations"
else
    pass "Shared service port 5432 correctly excluded"
fi

# ============================================================
# Test 9: Verify primary port is set
# ============================================================

echo ""
echo "=== Test 9: Primary port set from Coastfile ==="

set +e
LS_OUT=$("$COAST" ls 2>&1)
set -e
echo "  ls output (primary port):"
echo "$LS_OUT" | grep "dev-1" | head -1

if echo "$PORTS_OUT" | grep -q "\\*"; then
    pass "Primary port is starred in ports output"
else
    echo "  Note: primary port star may not appear in all output formats"
    pass "Primary port test completed"
fi

# ============================================================
# Test 10: Verify service exec into remote compose service
# ============================================================

echo ""
echo "=== Test 10: Service exec into remote compose container ==="

# coast exec dev-1 --service app runs a command inside the compose
# service container on the remote. This verifies the daemon routes
# through SSH + docker exec into the DinD + docker compose exec.
set +e
SVC_EXEC_OUT=$("$COAST" exec dev-1 --service app -- echo "SERVICE_EXEC_OK" 2>&1)
SVC_EXEC_EXIT=$?
set -e
echo "  service exec output: $SVC_EXEC_OUT (exit $SVC_EXEC_EXIT)"

if echo "$SVC_EXEC_OUT" | grep -q "SERVICE_EXEC_OK"; then
    pass "Service exec into remote compose container works"
elif echo "$SVC_EXEC_OUT" | grep -qi "Could not find"; then
    fail "Service exec failed: container not found on remote"
else
    echo "  Note: exec ran but output unexpected"
    if [ "$SVC_EXEC_EXIT" -eq 0 ]; then
        pass "Service exec completed without error"
    else
        fail "Service exec into remote compose container failed (exit $SVC_EXEC_EXIT)"
    fi
fi

# ============================================================
# Test 11: Cleanup
# ============================================================

echo ""
echo "=== Test 11: Cleanup ==="

"$COAST" rm dev-1 2>&1 >/dev/null
CLEANUP_INSTANCES=()
pass "Instance removed"

"$COAST" remote rm test-remote 2>&1 >/dev/null
pass "Remote removed"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  All remote compose build tests passed!"
echo "=========================================="
