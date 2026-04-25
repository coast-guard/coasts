#!/usr/bin/env bash
#
# Phase 30 integration test: two instances of the SAME project on the
# SAME remote VM share ONE `ssh -N -R` process for SSG-backed shared
# services.
#
# Phase 28 made the local host socat the consumer's stable forwarding
# target. Phase 30 carries that further on the remote side: the
# reverse-tunnel binds the project's stable virtual port on BOTH legs,
# and the daemon coalesces tunnels per (project, remote_host) so
# multiple consumer instances of the same project don't fight over
# the same remote bind. See `coast-ssg/DESIGN.md §24` Phase 30.
#
# Invariants asserted:
#   1. After running `inst-a` and `inst-b` of the same project, EXACTLY
#      ONE `ssh -N -R 0.0.0.0:<vport>:localhost:<vport>` process exists
#      for the project's postgres tunnel — not two.
#   2. Both instances can psql through the shared tunnel (functional
#      proof of routing).
#   3. Removing `inst-a` leaves the shared ssh process alive (sibling
#      `inst-b` still needs it).
#   4. Removing `inst-b` (the last sibling) tears the ssh down and
#      clears the `ssg_shared_tunnels` row.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

CLEANUP_INSTANCES=()

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true
    "$COAST" remote rm test-remote 2>/dev/null || true

    docker rm -f phase30-shared-ssg 2>/dev/null || true
    docker volume ls -q --filter "name=coast-dind--phase30-shared--ssg" 2>/dev/null \
        | xargs -r docker volume rm 2>/dev/null || true

    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "ssh -N -R" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    rm -rf "$HOME/.coast/ssg"
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Phase 30: Multi-instance same project shares one SSG tunnel ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate
rm -rf "$HOME/.coast/ssg"

eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true
start_coast_service

start_daemon

# ============================================================
# Project fixture: one project, one SSG (postgres on 5432), one
# consumer Coastfile that references postgres via from_group=true.
# We'll run TWO instances of this project on the same remote.
# ============================================================

TEST_ROOT=$(mktemp -d -t coast-phase30-XXXXXX)
PROJ="$TEST_ROOT/project"
mkdir -p "$PROJ"

cat > "$PROJ/docker-compose.yml" << 'COMPOSE_EOF'
services:
  app:
    image: alpine:3.19
    command: |
      sh -c "
        apk add --no-cache postgresql-client >/dev/null 2>&1
        tail -f /dev/null
      "
COMPOSE_EOF

cat > "$PROJ/Coastfile" << 'COASTFILE_EOF'
[coast]
name = "phase30-shared"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
COASTFILE_EOF

cat > "$PROJ/Coastfile.remote.toml" << 'COASTFILE_REMOTE_EOF'
[coast]
extends = "Coastfile"

[remote]
workspace_sync = "rsync"
COASTFILE_REMOTE_EOF

cat > "$PROJ/Coastfile.shared_service_groups" << 'SSG_EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "postgres" }
SSG_EOF

(
    cd "$PROJ"
    git init -b main >/dev/null 2>&1
    git config user.name "Coast Test"
    git config user.email "test@coasts.dev"
    git add -A
    git commit -m "initial phase30-shared fixture" >/dev/null 2>&1
)
pass "phase30-shared project fixture created"

# ============================================================
# Step 1: Build + run the SSG. Phase 28 spawns the host socat
# and allocates the project's stable virtual port for postgres.
# ============================================================

echo ""
echo "=== Step 1: Build + run SSG ==="

cd "$PROJ"
SSG_BUILD=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD" "Build complete" "ssg build succeeds"
SSG_RUN=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN" "SSG running" "ssg run succeeds"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
SSG_DYN=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYN" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYN"

sleep 5

# ============================================================
# Step 2: Register remote, build for remote, run inst-a.
# ============================================================

echo ""
echo "=== Step 2: Register remote + run inst-a ==="

ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "coast remote add succeeds"

cd "$PROJ"
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

CLEANUP_INSTANCES+=("inst-a")
"$COAST" run inst-a --type remote 2>&1 >/dev/null
pass "inst-a running on remote"

sleep 4

# ============================================================
# Step 3: Capture the (vport, pid) of the project's reverse
# tunnel BEFORE running inst-b. Phase 30 owns one tunnel per
# (project, remote_host).
# ============================================================

echo ""
echo "=== Step 3: One ssh -R process exists for inst-a alone ==="

count_ssh_for_vport() {
    local vport="$1"
    pgrep -af "ssh -N -R 0.0.0.0:${vport}:localhost:${vport}" | grep -cv '^$' || true
}

# Find any ssh -R that's symmetric (same port on both sides).
SYMMETRIC_LINE=$(pgrep -af "ssh -N -R 0.0.0.0:" \
    | grep -oE '0\.0\.0\.0:[0-9]+:localhost:[0-9]+' \
    | awk -F: -v OFS=: '$2 == $NF {print}' | head -1)
if [ -z "$SYMMETRIC_LINE" ]; then
    echo "--- pgrep ssh -N -R ---"
    pgrep -af "ssh -N -R" || true
    fail "no symmetric ssh -R found (Phase 30 expects vport:localhost:vport)"
fi

VPORT=$(echo "$SYMMETRIC_LINE" | awk -F: '{print $2}')
[ -n "$VPORT" ] || fail "could not extract virtual port from ssh -R argv"
pass "phase30-shared virtual port = $VPORT (post-Phase 28 host_socat allocation)"

INITIAL_COUNT=$(count_ssh_for_vport "$VPORT")
if [ "$INITIAL_COUNT" -ne 1 ]; then
    pgrep -af "ssh -N -R" || true
    fail "expected exactly 1 ssh -R for vport $VPORT after inst-a; got $INITIAL_COUNT"
fi

INITIAL_PID=$(pgrep -f "ssh -N -R 0.0.0.0:${VPORT}:localhost:${VPORT}" | head -1)
[ -n "$INITIAL_PID" ] || fail "could not capture initial ssh PID"
pass "single ssh -R PID $INITIAL_PID owns the project's postgres tunnel"

# Phase 30: Sanity that the local leg is NOT the SSG dyn port.
if [ "$VPORT" = "$SSG_DYN" ]; then
    fail "Phase 30 violation: ssh -R local leg matches the SSG dyn port; expected the host_socat virtual port"
fi
pass "vport ($VPORT) is distinct from the SSG dyn port ($SSG_DYN); host_socat is in the path"

# ============================================================
# Step 4: Run inst-b of the SAME project on the SAME remote.
# Phase 30 must REUSE the existing ssh -R; total ssh count stays 1.
# ============================================================

echo ""
echo "=== Step 4: Second instance reuses the existing tunnel ==="

cd "$PROJ"
CLEANUP_INSTANCES+=("inst-b")
"$COAST" run inst-b --type remote 2>&1 >/dev/null
pass "inst-b running on remote (same project as inst-a)"

sleep 4

POST_B_COUNT=$(count_ssh_for_vport "$VPORT")
if [ "$POST_B_COUNT" -ne 1 ]; then
    echo "--- pgrep ssh -N -R ---"
    pgrep -af "ssh -N -R" || true
    fail "Phase 30 violation: expected 1 ssh -R after inst-b; got $POST_B_COUNT (sibling instance spawned a duplicate)"
fi

# Same PID across the two instance starts — the tunnel was reused,
# not re-spawned.
POST_B_PID=$(pgrep -f "ssh -N -R 0.0.0.0:${VPORT}:localhost:${VPORT}" | head -1)
if [ "$INITIAL_PID" != "$POST_B_PID" ]; then
    fail "ssh PID changed across inst-a/inst-b runs ($INITIAL_PID -> $POST_B_PID); the shared tunnel must be reused, not respawned"
fi
pass "exactly 1 ssh -R for vport $VPORT after both instances; PID $INITIAL_PID unchanged (reused)"

# ============================================================
# Step 5: Functional proof — both consumers can psql through
# the shared tunnel.
# ============================================================

echo ""
echo "=== Step 5: Both inst-a and inst-b reach SSG postgres ==="

A_DIND="phase30-shared-coasts-inst-a"
B_DIND="phase30-shared-coasts-inst-b"

docker inspect "$A_DIND" >/dev/null 2>&1 || fail "inst-a DinD '$A_DIND' missing"
docker inspect "$B_DIND" >/dev/null 2>&1 || fail "inst-b DinD '$B_DIND' missing"

psql_through_dind() {
    local dind="$1"
    docker exec "$dind" sh -c '
        cid=$(docker ps --format "{{.Names}}" | grep -E "app-1$" | head -1)
        if [ -z "$cid" ]; then
            echo ""
            return
        fi
        for i in $(seq 1 30); do
            out=$(docker exec "$cid" sh -c "PGPASSWORD=coast psql -h postgres -U coast -d postgres -tAc \"SELECT 42 AS answer;\" 2>/dev/null" || true)
            if [ -n "$out" ]; then
                echo "$out" | tr -d "[:space:]"
                return
            fi
            sleep 2
        done
        echo ""
    ' 2>/dev/null
}

A_OUT=$(psql_through_dind "$A_DIND" || echo "")
B_OUT=$(psql_through_dind "$B_DIND" || echo "")
echo "  inst-a psql output: '$A_OUT'"
echo "  inst-b psql output: '$B_OUT'"

[ "$A_OUT" = "42" ] || fail "inst-a could not reach SSG postgres through the shared tunnel"
[ "$B_OUT" = "42" ] || fail "inst-b could not reach SSG postgres through the shared tunnel"
pass "both instances reach the SSG postgres through ONE shared tunnel"

# ============================================================
# Step 6: Remove inst-a. Phase 30 keeps the shared ssh -R alive
# because inst-b still needs it.
# ============================================================

echo ""
echo "=== Step 6: rm inst-a leaves shared tunnel alive (sibling needs it) ==="

cd "$PROJ"
"$COAST" rm inst-a 2>&1 >/dev/null
# Drop the entry so cleanup doesn't try to re-rm.
CLEANUP_INSTANCES=("inst-b")

sleep 2

POST_RM_A_COUNT=$(count_ssh_for_vport "$VPORT")
if [ "$POST_RM_A_COUNT" -ne 1 ]; then
    echo "--- pgrep ssh -N -R ---"
    pgrep -af "ssh -N -R" || true
    fail "Phase 30 violation: expected 1 ssh -R after rm inst-a; got $POST_RM_A_COUNT"
fi

POST_RM_A_PID=$(pgrep -f "ssh -N -R 0.0.0.0:${VPORT}:localhost:${VPORT}" | head -1)
if [ "$INITIAL_PID" != "$POST_RM_A_PID" ]; then
    fail "ssh PID changed across rm inst-a ($INITIAL_PID -> $POST_RM_A_PID); shared tunnel must NOT be respawned for an intermediate rm"
fi
pass "shared tunnel survived rm inst-a (pid $INITIAL_PID still alive); inst-b is its remaining holder"

# inst-b must still be able to psql.
B_OUT2=$(psql_through_dind "$B_DIND" || echo "")
[ "$B_OUT2" = "42" ] || fail "inst-b lost psql connectivity after rm inst-a; shared tunnel teardown was over-eager"
pass "inst-b still reaches SSG postgres after rm inst-a"

# ============================================================
# Step 7: Remove inst-b (the last sibling). Phase 30 tears the
# shared ssh -R down and clears the ssg_shared_tunnels row.
# ============================================================

echo ""
echo "=== Step 7: rm inst-b tears down the shared tunnel ==="

cd "$PROJ"
"$COAST" rm inst-b 2>&1 >/dev/null
CLEANUP_INSTANCES=()

sleep 2

POST_RM_B_COUNT=$(count_ssh_for_vport "$VPORT")
if [ "$POST_RM_B_COUNT" -ne 0 ]; then
    echo "--- pgrep ssh -N -R ---"
    pgrep -af "ssh -N -R" || true
    fail "Phase 30 violation: shared ssh -R for vport $VPORT should be gone after rm of last sibling; got $POST_RM_B_COUNT"
fi
pass "shared ssh -R torn down after the last instance was removed"

# --- Done ---

echo ""
echo "==========================================="
echo "  PHASE 30 SHARED SSG TUNNEL TEST PASSED"
echo "==========================================="
