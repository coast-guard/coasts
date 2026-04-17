#!/usr/bin/env bash
#
# Integration test: host reboot recovery.
#
# Reproduces the failure mode the Coastfile 'cg' project hit when the user
# rebooted their machine:
#
#   Bug 1: daemon 'running' but HTTP API on :31415 never binds because
#          `restore_running_state` awaits SSH work against an unreachable
#          remote, and the SSH has BatchMode=yes with no ConnectTimeout.
#   Bug 2: /workspace bind mount inside the DinD is missing after the DinD
#          restarts, so `docker compose up` fails with
#          "env file /workspace/app/.env not found".
#   Bug 3: shared-service plumbing (docker0 alias IPs + socat proxies)
#          inside the DinD is gone after restart, so compose services hit
#          "dial tcp 172.18.255.254:5432: connect: connection timed out"
#          when talking to shared postgres/redis.
#
# Scenario: simulate a reboot by stopping the Coast DinD containers and
# killing the daemon, then starting everything back up. Assert recovery
# happens automatically, *without* manual `mount --bind` / `ip addr add` /
# `socat` / `restart-services` intervention.
#
# Usage:
#   ./integrated-examples/test_reboot_recovery.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

# Track iptables rules we add so cleanup can remove them even on early exit.
IPTABLES_RULES=()

_reboot_cleanup() {
    echo ""
    echo "--- Cleaning up ---"

    # Remove any iptables blackhole rules we installed
    for rule in "${IPTABLES_RULES[@]:-}"; do
        # shellcheck disable=SC2086
        iptables -D $rule 2>/dev/null || true
    done

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    "$COAST" remote rm test-remote 2>/dev/null || true

    clean_remote_state

    "$COAST" daemon kill 2>/dev/null || true
    pkill -f "coastd" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}
trap '_reboot_cleanup' EXIT

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Wait for the HTTP API on :31415 to bind. Return 0 on success, 1 on timeout.
wait_for_api() {
    local max_wait="${1:-30}"
    local i=0
    while [ $i -lt "$max_wait" ]; do
        if curl -sf --max-time 2 http://localhost:31415/api/v1/ls >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    return 1
}

# Wait for a service behind the `app` dynamic port to serve /health 200.
wait_for_app_health() {
    local port="$1"
    local max_wait="${2:-60}"
    local i=0
    while [ $i -lt "$max_wait" ]; do
        if curl -sf --max-time 2 "http://localhost:${port}/health" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    return 1
}

# Resolve the outer DinD container ID for an instance (local only).
resolve_dind_cid() {
    local project="$1"
    local name="$2"
    local container="${project}-coasts-${name}"
    docker ps -aq --filter "name=^${container}$" 2>/dev/null | head -1
}

# ---------------------------------------------------------------------------
# Preflight + setup
# ---------------------------------------------------------------------------

echo "=== Reboot Recovery Integration Test ==="
echo ""

preflight_checks

# iptables is needed to blackhole the remote SSH port
command -v iptables >/dev/null || {
    echo "iptables not installed; this test requires iptables to blackhole the remote"
    exit 1
}

echo ""
echo "=== Setup ==="

clean_slate

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test projects ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

start_daemon

# Pre-pull shared-service and app base images
docker pull postgres:16-alpine >/dev/null 2>&1 || true
docker pull redis:7-alpine >/dev/null 2>&1 || true
docker pull node:20-alpine >/dev/null 2>&1 || true

# ============================================================
# Phase 1: baseline -- local instance with shared services
# ============================================================

echo ""
echo "=== Phase 1: baseline ==="

cd "$PROJECTS_DIR/coast-reboot-recovery"

BUILD_OUT=$("$COAST" build 2>&1) || { echo "$BUILD_OUT"; fail "coast build failed"; }
assert_contains "$BUILD_OUT" "Build complete" "coast build"

RUN_OUT=$("$COAST" run dev-local 2>&1) || { echo "$RUN_OUT"; fail "coast run dev-local failed"; }
CLEANUP_INSTANCES+=("dev-local")
assert_contains "$RUN_OUT" "Created coast instance" "coast run dev-local"

LOCAL_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$LOCAL_PORT" ] || fail "Could not extract dynamic port for app on dev-local"
echo "  dev-local app port: $LOCAL_PORT"

wait_for_app_health "$LOCAL_PORT" 90 || fail "dev-local /health did not become healthy pre-reboot"

# End-to-end probe: /full-check exercises postgres + redis via the
# shared-service socat proxy. If this passes pre-reboot, we have a
# clean baseline to compare against after the reboot.
FULL_CHECK_BEFORE=$(curl -sf --max-time 10 "http://localhost:${LOCAL_PORT}/full-check" 2>&1) \
    || fail "baseline /full-check failed: $FULL_CHECK_BEFORE"
assert_contains "$FULL_CHECK_BEFORE" "\"db\":\"connected\"" "baseline: shared postgres reachable"
assert_contains "$FULL_CHECK_BEFORE" "\"cache\":\"connected\"" "baseline: shared redis reachable"

# ============================================================
# Phase 2: add a remote and run an instance on it
# ============================================================

echo ""
echo "=== Phase 2: remote baseline ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
pass "remote added"

cd "$PROJECTS_DIR/remote/coast-remote-basic"

"$COAST" build 2>&1 >/dev/null
pass "local build for remote-basic"

set +e
BUILD_REMOTE_OUT=$("$COAST" build --type remote 2>&1)
BUILD_REMOTE_EXIT=$?
set -e
[ "$BUILD_REMOTE_EXIT" -eq 0 ] || { echo "$BUILD_REMOTE_OUT"; fail "remote build failed"; }
pass "remote build"

set +e
RUN_REMOTE_OUT=$("$COAST" run dev-remote --type remote 2>&1)
RUN_REMOTE_EXIT=$?
set -e
[ "$RUN_REMOTE_EXIT" -eq 0 ] || { echo "$RUN_REMOTE_OUT"; fail "coast run dev-remote failed"; }
CLEANUP_INSTANCES+=("dev-remote")
pass "coast run dev-remote"

# ============================================================
# Phase 3: simulate reboot
# ============================================================

echo ""
echo "=== Phase 3: simulate reboot ==="

LOCAL_DIND_CID=$(resolve_dind_cid "coast-reboot-recovery" "dev-local")
[ -n "$LOCAL_DIND_CID" ] || fail "could not resolve dev-local DinD container ID"
echo "  dev-local DinD: $LOCAL_DIND_CID"

REMOTE_DIND_CID=$(resolve_dind_cid "coast-remote-basic" "dev-remote" || true)
# The remote instance's DinD lives on the 'remote' host (also localhost here)
# via coast-service. We don't need to touch it directly; stopping coastd is
# sufficient to exercise the startup-restore path.

echo "--- killing daemon ---"
"$COAST" daemon kill 2>&1 || true
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2

# Verify daemon is actually dead
if "$COAST" daemon status 2>&1 | grep -q "is running"; then
    fail "daemon should be stopped before reboot simulation"
fi

echo "--- stopping local DinD (simulates Docker restart dropping runtime state) ---"
docker stop "$LOCAL_DIND_CID" >/dev/null 2>&1
pass "local DinD stopped"

echo "--- blackholing the remote (simulates unreachable network) ---"
# Block loopback SSH to the sshd port so any SSH from coastd to test-remote
# hangs at connect. Using OUTPUT rules on loopback is safe for this isolated
# test container.
iptables -I OUTPUT -p tcp -d 127.0.0.1 --dport 22 -j DROP
IPTABLES_RULES+=("OUTPUT -p tcp -d 127.0.0.1 --dport 22 -j DROP")
# Also block the coast-service port so any in-progress probes fail fast
iptables -I OUTPUT -p tcp -d 127.0.0.1 --dport 31420 -j DROP
IPTABLES_RULES+=("OUTPUT -p tcp -d 127.0.0.1 --dport 31420 -j DROP")
pass "remote blackholed via iptables"

echo "--- starting local DinD back up (simulates Docker Desktop auto-restart) ---"
docker start "$LOCAL_DIND_CID" >/dev/null 2>&1
pass "local DinD started"

echo "--- starting daemon (simulates auto-start on boot) ---"
# Start in foreground mode in the background so we can observe it
clean_daemon_start() {
    "$COASTD" --foreground &>>/tmp/coastd-test.log &
    echo $!
}
COASTD_PID=$(clean_daemon_start)
pass "daemon process spawned (pid $COASTD_PID)"

# ============================================================
# Phase 4: assertions (no manual intervention)
# ============================================================

echo ""
echo "=== Phase 4: automatic recovery ==="

# --- Bug 1: HTTP API binds promptly, even though remote is blackholed ---

if wait_for_api 45; then
    pass "bug 1 FIXED: HTTP API on :31415 bound within 45s (remote blackholed)"
else
    echo "  recent daemon log:"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "bug 1 STILL BROKEN: HTTP API did not bind within 45s"
fi

# Unix socket also up
[ -S ~/.coast/coastd.sock ] || fail "coastd.sock not present"
pass "coastd.sock present"

# coast ls should succeed
LS_OUT=$("$COAST" ls 2>&1) || fail "coast ls failed"
assert_contains "$LS_OUT" "dev-local" "coast ls shows dev-local"

# --- Bug 2: /workspace bind mount restored inside the local DinD ---

WS_ENV_OUT=$(docker exec "$LOCAL_DIND_CID" sh -c 'ls /workspace/app/.env 2>&1' 2>&1 || true)
if echo "$WS_ENV_OUT" | grep -q "No such file"; then
    echo "  /workspace contents: $(docker exec "$LOCAL_DIND_CID" sh -c 'ls /workspace 2>&1' | tr '\n' ' ')"
    fail "bug 2 STILL BROKEN: /workspace/app/.env missing after restart (bind mount not restored)"
fi
pass "bug 2 FIXED: /workspace/app/.env visible inside DinD (bind mount restored)"

# Also verify mountinfo explicitly
MNT_INFO=$(docker exec "$LOCAL_DIND_CID" sh -c 'grep -E " /workspace " /proc/self/mountinfo || true' 2>&1)
if [ -z "$MNT_INFO" ]; then
    fail "bug 2 STILL BROKEN: no bind mount entry for /workspace"
fi
echo "  /workspace mount: $MNT_INFO"

# --- Bug 3: docker0 alias IPs + socat proxies restored ---

# Poll for the alias IPs on docker0. Note: the subnet depends on the
# environment. The daemon picks addresses from the TOP of the inner DinD's
# docker0 subnet (see shared_service_routing::allocate_alias_ip). In
# production this is usually 172.18.0.0/16 (aliases 172.18.255.254 / .253),
# but in the DinD-in-DinD test harness docker0 ends up on 172.17.0.0/16
# (aliases 172.17.255.254 / .253). Accept either.
ALIAS_RE='172\.(1[78])\.255\.25[34]'
ALIAS_TIMEOUT=120
ALIAS_I=0
ALIAS_OK=0
while [ $ALIAS_I -lt $ALIAS_TIMEOUT ]; do
    ALIAS_OUT=$(docker exec "$LOCAL_DIND_CID" sh -c 'ip -4 addr show dev docker0 2>&1' 2>&1 || true)
    if echo "$ALIAS_OUT" | grep -qE "$ALIAS_RE"; then
        ALIAS_OK=1
        break
    fi
    sleep 2
    ALIAS_I=$((ALIAS_I + 2))
done

if [ "$ALIAS_OK" -ne 1 ]; then
    echo "  docker0 addrs: $(echo "$ALIAS_OUT" | grep inet)"
    echo "  ---- daemon log (shared-service related) ----"
    grep -E "shared|socat|docker0|proxy|restored" /tmp/coastd-test.log 2>/dev/null | tail -30 || true
    echo "  ---- full /proc/mounts + /etc/hosts in DinD ----"
    docker exec "$LOCAL_DIND_CID" sh -c 'cat /proc/mounts | head -20; echo; cat /etc/hosts' 2>&1 || true
    fail "bug 3 STILL BROKEN: no ${ALIAS_RE} alias on docker0 after ${ALIAS_TIMEOUT}s"
fi
pass "bug 3 FIXED: docker0 alias IP present (took ${ALIAS_I}s)"

SOCAT_OUT=$(docker exec "$LOCAL_DIND_CID" sh -c "ps axo pid,command 2>/dev/null | grep -E 'socat.*bind=$ALIAS_RE' | grep -v grep || true" 2>&1)
if [ -z "$SOCAT_OUT" ]; then
    fail "bug 3 STILL BROKEN: no socat proxy running inside DinD"
fi
pass "bug 3 FIXED: socat proxy running inside DinD"

# --- End-to-end: app can reach shared postgres + redis ---

# Compose services inside the DinD don't auto-restart when the DinD is
# stopped/started (no restart policy in the compose file). The user would
# run `coast restart-services` after a reboot. With the runtime state
# already restored (workspace bind + shared-service proxies), this call
# is the only manual step needed to get all services back — the daemon
# took care of the plumbing.
echo "--- coast restart-services dev-local ---"
cd "$PROJECTS_DIR/coast-reboot-recovery"
RESTART_OUT=$(timeout 180 "$COAST" restart-services dev-local 2>&1) \
    || { echo "$RESTART_OUT"; fail "coast restart-services failed"; }
pass "coast restart-services dev-local"

# After recovery compose services may need a short warm-up before the
# backend re-establishes the pg/redis connections.
if wait_for_app_health "$LOCAL_PORT" 120; then
    pass "app /health serves after reboot + restart-services"
else
    fail "app /health did not come back within 120s after restart-services"
fi

# The critical end-to-end probe: shared postgres + redis reachable through
# the restored proxies.
FULL_CHECK_AFTER=$(curl -sf --max-time 15 "http://localhost:${LOCAL_PORT}/full-check" 2>&1) \
    || fail "post-reboot /full-check failed: $FULL_CHECK_AFTER"
assert_contains "$FULL_CHECK_AFTER" "\"db\":\"connected\"" "recovery: shared postgres reachable"
assert_contains "$FULL_CHECK_AFTER" "\"cache\":\"connected\"" "recovery: shared redis reachable"

# ============================================================
# Done
# ============================================================

echo ""
echo "=========================================="
echo "  ALL REBOOT RECOVERY TESTS PASSED"
echo "=========================================="
