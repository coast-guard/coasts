#!/usr/bin/env bash
#
# Integration test: remote coast reaches local SSG postgres via
# reverse SSH tunnel (Phase 4.5, DESIGN.md §20).
#
# Flow tested end-to-end:
#
#     remote app container (inside coast-service DinD)
#       -> postgres:5432 (DNS via compose_rewrite extra_hosts)
#       -> remote docker host-gateway
#       -> reverse SSH tunnel (ssh -R 0.0.0.0:5432:localhost:<dyn>)
#       -> local host :dynamic_host_port
#       -> SSG DinD :dynamic_host_port -> inner postgres :5432
#
# The key Phase 4.5 assertion is that the tunnel pair has a REWRITTEN
# local side — `localhost:<dynamic>`, not `localhost:5432` — proving
# `rewrite_reverse_tunnel_pairs` is wired into `setup_shared_service_tunnels`.
#
# Uses the localhost-as-remote harness (`setup_localhost_ssh` +
# `start_coast_service`).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) -- SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-remote"

# Custom cleanup for remote tests: additional SSG + remote teardown.
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

echo "--- Setting up localhost SSH ---"
setup_localhost_ssh

echo "--- Starting coast-service ---"
start_coast_service

echo "--- Initializing test projects ---"
"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

start_daemon

echo ""
echo "=== Step 1: SSG build + run ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-remote"
SSG_BUILD_OUT=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD_OUT" "Build complete" "ssg build succeeds"
SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
SSG_DYNAMIC=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYNAMIC" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYNAMIC"

sleep 5

echo ""
echo "=== Step 2: register the remote ==="

ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "coast remote add succeeds"

echo ""
echo "=== Step 3: build consumer locally + for remote ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-remote"

"$COAST" build 2>&1 >/dev/null
pass "local base build complete (coast_image for shell)"

REMOTE_BUILD_OUT=$("$COAST" build --type remote 2>&1)
echo "$REMOTE_BUILD_OUT" | tail -10
assert_contains "$REMOTE_BUILD_OUT" "Build complete" "coast build --type remote succeeds"

echo ""
echo "=== Step 4: run remote consumer ==="

CLEANUP_INSTANCES+=("remote-a")
set +e
RUN_OUT=$("$COAST" run remote-a --type remote 2>&1)
RUN_EXIT=$?
set -e
echo "$RUN_OUT" | tail -25
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    echo "--- coast-service log tail ---"
    tail -40 /tmp/coast-service-test.log 2>/dev/null || true
    fail "coast run --type remote failed (exit $RUN_EXIT)"
fi
assert_contains "$RUN_OUT" "Created coast instance" "remote run succeeds"

sleep 5

echo ""
echo "=== Step 5: reverse tunnel uses SSG dynamic port on local side ==="

# Phase 18: the remote side of the reverse tunnel is a dynamic port
# (not canonical 5432); the local side is still the SSG's dynamic
# port because rewrite_reverse_tunnel_pairs still maps SSG forwards.
PGREP_OUT=$(pgrep -af "ssh -N -R 0.0.0.0:" 2>&1 || true)
echo "$PGREP_OUT"
if ! echo "$PGREP_OUT" | grep -qE "ssh -N -R 0\.0\.0\.0:[0-9]+:localhost:$SSG_DYNAMIC"; then
    fail "reverse ssh tunnel should bind a dynamic remote port and terminate at localhost:$SSG_DYNAMIC"
fi
pass "reverse ssh tunnel targets SSG dynamic port ($SSG_DYNAMIC) locally (Phase 18)"

echo ""
echo "=== Step 6: app container inside remote coast reaches SSG postgres ==="

# Exec into the inner app service via the remote DinD's docker
# client. Bypasses any ambiguity in `coast exec --service app` about
# which container is targeted — we go directly to the inner compose
# project's app service.
#
# The remote is localhost in this dindind test, so the outer DinD
# container is reachable from the harness.
REMOTE_DIND="coast-ssg-consumer-remote-coasts-remote-a"
DOCKER_PS_REMOTE=$(docker ps --filter "name=^${REMOTE_DIND}$" --format "{{.Names}}")
assert_eq "$DOCKER_PS_REMOTE" "$REMOTE_DIND" "remote DinD container is running"

# Give the inner compose a moment to settle.
sleep 5

# coast-service cd's to /workspace before `docker compose up`, so the
# compose project name is "workspace" (dir basename). The inner app
# container's name is therefore `workspace-app-1`.
INNER_APP="workspace-app-1"
INNER_PS=$(docker exec "$REMOTE_DIND" docker ps --filter "name=^${INNER_APP}$" --format "{{.Names}}")
assert_eq "$INNER_PS" "$INNER_APP" "inner app container is running in the remote DinD"

set +e
PSQL_OUT=$(docker exec "$REMOTE_DIND" docker exec "$INNER_APP" sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 42 AS answer;'" 2>&1)
PSQL_EXIT=$?
set -e
echo "$PSQL_OUT"
if [ "$PSQL_EXIT" -ne 0 ]; then
    echo "--- inner container /etc/hosts ---"
    docker exec "$REMOTE_DIND" docker exec "$INNER_APP" cat /etc/hosts 2>&1 || true
    echo "--- coast-service log tail ---"
    tail -30 /tmp/coast-service-test.log 2>/dev/null || true
    fail "psql through remote tunnel failed (exit $PSQL_EXIT)"
fi
assert_contains "$PSQL_OUT" "answer" "psql returned the column header"
assert_contains "$PSQL_OUT" "42" "psql returned the answer row"

pass "remote consumer app reached SSG postgres through the reverse tunnel"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG REMOTE REVERSE TUNNEL TESTS PASSED"
echo "==========================================="
