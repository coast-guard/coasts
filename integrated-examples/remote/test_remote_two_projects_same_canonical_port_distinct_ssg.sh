#!/usr/bin/env bash
#
# Phase 24 integration test: two DIFFERENT projects each with their
# own SSG on the local host, both consumer coasts running on the SAME
# remote VM, both declaring `postgres:5432` via `from_group = true`.
# Each consumer must reach its OWN project's postgres (not the other
# project's). Plus a daemon-restart leg that exercises the Phase 24
# fix to `restore_shared_service_tunnels` (removed the
# host-keyed dedup that would have silently dropped project B's
# tunnel restoration).
#
# See `coast-ssg/DESIGN.md §23.3 item 7` and Phase 24 plan.
#
# Differences vs `test_remote_mixed_inline_and_ssg_no_collision.sh`:
#   - Two SSGs (one per project), not one.
#   - Two *projects*, not two instances of one project.
#   - Provable image isolation via different postgres major versions.

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

    # Two SSGs to tear down this time (one per project).
    for proj in phase24-a phase24-b; do
        docker rm -f "${proj}-ssg" 2>/dev/null || true
        docker volume ls -q --filter "name=coast-dind--${proj}--ssg" 2>/dev/null \
            | xargs -r docker volume rm 2>/dev/null || true
    done

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

echo "=== Phase 24: Two projects, distinct SSGs, one shared remote ==="
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
# Create two inline project fixtures (A and B) under a tempdir.
# ============================================================

TEST_ROOT=$(mktemp -d -t coast-phase24-XXXXXX)
PROJ_A="$TEST_ROOT/project-a"
PROJ_B="$TEST_ROOT/project-b"
mkdir -p "$PROJ_A" "$PROJ_B"

# Each project:
#   - declares its own SSG with postgres on canonical 5432 but a
#     DIFFERENT postgres major version so routing is provable
#     (version 15 vs 16).
#   - its consumer Coastfile declares `from_group = true` for
#     `postgres` and has a tiny app service with psql installed that
#     writes its connected server's version into a file inside the
#     container so we can assert it.
#   - has a [remote] section so `coast run --type remote` works.

make_project() {
    local dir="$1"
    local project_name="$2"
    local pg_version="$3"

    # Consumer compose: alpine with psql, writes the server version to
    # /var/coast-version so we can exec-read it. `extra_hosts` +
    # socat routing inside the remote DinD get set up by coast-service
    # from the `[shared_services.postgres] from_group = true` ref.
    cat > "$dir/docker-compose.yml" << COMPOSE_EOF
services:
  app:
    image: alpine:3.19
    command: |
      sh -c "
        apk add --no-cache postgresql-client >/dev/null 2>&1
        while true; do
          V=\$\$(PGPASSWORD=coast psql -h postgres -U coast -d postgres -tAc 'SHOW server_version;' 2>/dev/null || true)
          if [ -n \"\$\$V\" ]; then
            echo \"\$\$V\" > /var/coast-version
            break
          fi
          sleep 1
        done
        tail -f /dev/null
      "
COMPOSE_EOF

    # Main Coastfile: project name drives ALL per-project routing.
    cat > "$dir/Coastfile" << COASTFILE_EOF
[coast]
name = "$project_name"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
COASTFILE_EOF

    # Remote variant extends the base Coastfile and declares [remote]
    # (naming convention: Coastfile.remote.toml + [coast] extends).
    cat > "$dir/Coastfile.remote.toml" << COASTFILE_REMOTE_EOF
[coast]
extends = "Coastfile"

[remote]
workspace_sync = "rsync"
COASTFILE_REMOTE_EOF

    # Per-project SSG: same canonical port (5432), different image.
    # Phase 24's whole point is that these don't collide because
    # each SSG publishes on its own dynamic host port.
    cat > "$dir/Coastfile.shared_service_groups" << SSG_EOF
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:${pg_version}-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "postgres" }
SSG_EOF

    # `coast run --type remote` needs a git repo (reads current
    # branch for worktree/state metadata). Match the pattern used
    # by the local equivalent test.
    (
        cd "$dir"
        git init -b main >/dev/null 2>&1
        git config user.name "Coast Test"
        git config user.email "test@coasts.dev"
        git add -A
        git commit -m "initial $project_name fixture" >/dev/null 2>&1
    )
}

make_project "$PROJ_A" "phase24-a" "15"
make_project "$PROJ_B" "phase24-b" "16"
pass "Inline project fixtures created (phase24-a pg15, phase24-b pg16)"

# ============================================================
# Step 1: Build + run both per-project SSGs.
# ============================================================

echo ""
echo "=== Step 1: Build + run both SSGs ==="

cd "$PROJ_A"
set +e
SSG_BUILD_A=$("$COAST" ssg build 2>&1)
BUILD_A_EXIT=$?
set -e
echo "$SSG_BUILD_A" | tail -5
if [ $BUILD_A_EXIT -ne 0 ]; then
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "phase24-a ssg build exited non-zero (exit=$BUILD_A_EXIT)"
fi
assert_contains "$SSG_BUILD_A" "Build complete" "phase24-a ssg build succeeds"
SSG_RUN_A=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_A" "SSG running" "phase24-a ssg run succeeds"
PORTS_A_OUT=$("$COAST" ssg ports 2>&1)
SSG_A_DYNAMIC=$(echo "$PORTS_A_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_A_DYNAMIC" ] || fail "could not extract phase24-a SSG dynamic port"
pass "phase24-a SSG postgres dynamic host port = $SSG_A_DYNAMIC"

cd "$PROJ_B"
SSG_BUILD_B=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD_B" "Build complete" "phase24-b ssg build succeeds"
SSG_RUN_B=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_B" "SSG running" "phase24-b ssg run succeeds"
PORTS_B_OUT=$("$COAST" ssg ports 2>&1)
SSG_B_DYNAMIC=$(echo "$PORTS_B_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_B_DYNAMIC" ] || fail "could not extract phase24-b SSG dynamic port"
pass "phase24-b SSG postgres dynamic host port = $SSG_B_DYNAMIC"

[ "$SSG_A_DYNAMIC" != "$SSG_B_DYNAMIC" ] \
    || fail "each project's SSG must publish on a distinct dynamic port (got $SSG_A_DYNAMIC for both)"

# Both Docker containers exist side by side — Phase 23 correction.
docker inspect phase24-a-ssg >/dev/null 2>&1 \
    || fail "phase24-a-ssg container should exist"
docker inspect phase24-b-ssg >/dev/null 2>&1 \
    || fail "phase24-b-ssg container should exist"
pass "Both per-project SSG containers running concurrently"

sleep 5

# ============================================================
# Step 2: Register the shared remote.
# ============================================================

echo ""
echo "=== Step 2: Register remote ==="
ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "coast remote add succeeds"

# ============================================================
# Step 3: Run phase24-a's consumer coast on the remote.
# ============================================================

echo ""
echo "=== Step 3: phase24-a remote consumer ==="

cd "$PROJ_A"
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
RUN_A_EXIT=$?
set -e
[ $RUN_A_EXIT -eq 0 ] || fail "phase24-a remote consumer run failed"
CLEANUP_INSTANCES+=("dev-1")
pass "phase24-a consumer 'dev-1' running on remote"

sleep 3

# ============================================================
# Step 4: Run phase24-b's consumer coast on the SAME remote,
#         with the SAME instance name 'dev-1'. Project key
#         disambiguates.
# ============================================================

echo ""
echo "=== Step 4: phase24-b remote consumer (same instance name, different project) ==="

cd "$PROJ_B"
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_B_OUT=$("$COAST" run dev-1 --type remote 2>&1)
RUN_B_EXIT=$?
set -e
if [ "$RUN_B_EXIT" -ne 0 ]; then
    echo "$RUN_B_OUT" | tail -20
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "phase24-b remote consumer run failed — cross-project collision?"
fi
# Note: CLEANUP_INSTANCES treats names as globally unique in its
# plain `coast rm <name>` call. For per-project cleanup we skip
# appending here and handle phase24-b's dev-1 in the cleanup block
# below explicitly via cwd-based dispatch.
pass "phase24-b consumer 'dev-1' running on remote alongside phase24-a's"

sleep 3

# ============================================================
# Step 5: Assert distinct remote_port + distinct local upstream per
#         project's tunnel.
#
# Phase 30 (DESIGN.md §24): each project's SSG-backed reverse
# tunnel is now SYMMETRIC on the project's stable VIRTUAL port —
# both `ssh -R` legs are `<vport>:localhost:<vport>`. The local
# leg terminates at the Phase 28 host socat (which forwards to
# whatever dyn port the SSG currently publishes), so the local
# upstream a packet capture would show is the virtual port, NOT
# the SSG dyn port the test originally asserted on.
#
# Per-project isolation is preserved because each project has its
# own virtual port from `ssg_virtual_ports` — and (project A's
# vport) != (project B's vport) by construction.
# ============================================================

echo ""
echo "=== Step 5: Distinct remote ports + project-scoped virtual ports ==="

ALL_TUNNELS=$(pgrep -af "ssh -N -R 0.0.0.0:" | \
    grep -oE '0\.0\.0\.0:[0-9]+:localhost:[0-9]+' || true)
echo "  sshd reverse listeners:"
echo "$ALL_TUNNELS" | sed 's/^/    /'

REMOTE_PORTS=$(echo "$ALL_TUNNELS" | awk -F: '{print $2}' | sort -u)
REMOTE_COUNT=$(echo "$REMOTE_PORTS" | grep -cv '^$' || echo 0)

LOCAL_UPSTREAMS=$(echo "$ALL_TUNNELS" | awk -F: '{print $NF}' | sort -u)

echo "  distinct remote ports = $REMOTE_COUNT"
echo "  distinct local upstreams:"
echo "$LOCAL_UPSTREAMS" | sed 's/^/    /'

if [ "$REMOTE_COUNT" -lt 2 ]; then
    echo "--- daemon log ---"
    tail -60 /tmp/coastd-test.log 2>/dev/null || true
    fail "expected at least 2 distinct remote tunnel ports (one per project); got $REMOTE_COUNT"
fi

# Phase 30: each remote port equals the project's virtual port,
# and so does the local-side leg (symmetric). Different projects
# have distinct virtual ports → distinct (remote, local) pairs.
LOCAL_COUNT=$(echo "$LOCAL_UPSTREAMS" | grep -cv '^$' || echo 0)
if [ "$LOCAL_COUNT" -lt 2 ]; then
    fail "expected at least 2 distinct local upstreams (one virtual port per project); got $LOCAL_COUNT"
fi

# The OLD assertion was "local upstream contains $SSG_X_DYNAMIC".
# Phase 30 inverts that contract: SSG dyn ports MUST NOT appear
# in the ssh -R argv anymore — only the virtual port shows there.
# A regression to the dyn port (e.g. someone reverts the
# rewrite_reverse_tunnel_pairs change) would make this assertion
# fail in exactly the right place.
if echo "$LOCAL_UPSTREAMS" | grep -q "^${SSG_A_DYNAMIC}$"; then
    fail "Phase 30 violation: ssh -R local upstream $SSG_A_DYNAMIC is the SSG dyn port; expected the virtual port"
fi
if echo "$LOCAL_UPSTREAMS" | grep -q "^${SSG_B_DYNAMIC}$"; then
    fail "Phase 30 violation: ssh -R local upstream $SSG_B_DYNAMIC is the SSG dyn port; expected the virtual port"
fi

# Phase 30 symmetry: for every tunnel, the remote and local
# halves are equal (vport:localhost:vport). Verify by parsing
# each line individually — any line where remote != local is a
# Phase 30 contract violation (the fall-through inline shape).
while IFS= read -r line; do
    [ -n "$line" ] || continue
    R=$(echo "$line" | awk -F: '{print $2}')
    L=$(echo "$line" | awk -F: '{print $NF}')
    if [ "$R" != "$L" ]; then
        fail "Phase 30 violation: ssh -R '$line' is asymmetric (remote=$R != local=$L) for an SSG-backed tunnel"
    fi
done <<<"$ALL_TUNNELS"

pass "every SSG tunnel is symmetric (remote == local) and projects' virtual ports are distinct"

# ============================================================
# Step 6: Functional proof — each consumer's psql sees its own
#         project's postgres major version. Nothing short of
#         correct routing makes this work.
# ============================================================

echo ""
echo "=== Step 6: Per-project image isolation (psql SHOW server_version) ==="

# Find each consumer's app container on the remote and read the
# version file. The instance container name convention is
# `{instance}-{service}-1` under docker compose.
wait_for_file_in_container() {
    local container="$1"
    local path="$2"
    local tries=30
    while [ $tries -gt 0 ]; do
        if docker exec "$container" test -f "$path" 2>/dev/null; then
            return 0
        fi
        sleep 2
        tries=$((tries - 1))
    done
    return 1
}

# For each remote instance, the coast-service side starts a nested
# DinD with the consumer's compose; we exec THROUGH that DinD to the
# app container. Since both `dev-1` instances exist (different
# projects), we identify them by the DinD container name — the
# canonical naming is `{project}-coasts-{instance}`
# (see coast-docker/src/runtime.rs::container_name).
A_DIND="phase24-a-coasts-dev-1"
B_DIND="phase24-b-coasts-dev-1"

docker inspect "$A_DIND" >/dev/null 2>&1 \
    || fail "expected DinD container '$A_DIND' to exist on shared remote"
docker inspect "$B_DIND" >/dev/null 2>&1 \
    || fail "expected DinD container '$B_DIND' to exist on shared remote"

# The app container inside each DinD is named `<compose>-app-1`. The
# compose project label comes from the instance name. Exec a
# version-check via the inner docker of each DinD.
read_version_inside() {
    local dind="$1"
    docker exec "$dind" sh -c '
        cid=$(docker ps --format "{{.Names}}" | grep -E "app-1$" | head -1)
        if [ -z "$cid" ]; then
            echo ""
            return
        fi
        # Wait up to ~60s for the version file to appear.
        for i in $(seq 1 30); do
            v=$(docker exec "$cid" cat /var/coast-version 2>/dev/null || true)
            if [ -n "$v" ]; then
                echo "$v"
                return
            fi
            sleep 2
        done
        echo ""
    ' 2>/dev/null
}

A_VERSION=$(read_version_inside "$A_DIND" || echo "")
B_VERSION=$(read_version_inside "$B_DIND" || echo "")

echo "  phase24-a reported server_version: '$A_VERSION'"
echo "  phase24-b reported server_version: '$B_VERSION'"

[ -n "$A_VERSION" ] || fail "phase24-a's app could not reach its postgres"
[ -n "$B_VERSION" ] || fail "phase24-b's app could not reach its postgres"

# postgres:15-alpine reports a version string starting with "15."
# postgres:16-alpine reports one starting with "16.". If routing
# leaked between projects we'd see the same version twice — or the
# wrong version per project.
echo "$A_VERSION" | grep -q '^15\.' \
    || fail "phase24-a expected server_version starting with 15., got '$A_VERSION' — routing leak?"
echo "$B_VERSION" | grep -q '^16\.' \
    || fail "phase24-b expected server_version starting with 16., got '$B_VERSION' — routing leak?"
pass "each consumer's psql reached its own project's postgres (15 vs 16)"

# ============================================================
# Step 7: Daemon-restart leg — exercises the Phase 24 fix to
#         `restore_shared_service_tunnels` (dropped the
#         `restored_hosts` host-keyed skip).
# ============================================================

echo ""
echo "=== Step 7: Daemon restart preserves both projects' tunnels ==="

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 2
start_daemon
sleep 5

# After restart, both reverse tunnels must be re-established. Before
# Phase 24, the second project's instance on the shared remote was
# silently skipped because the first instance's host_key was already
# in `restored_hosts`.
#
# Phase 30: each project's tunnel is symmetric on its own virtual
# port. The post-restart shape must include exactly the same set
# of (vport, vport) pairs that existed pre-restart, so we compare
# the SORTED unique remote-port sets across the restart.
ALL_TUNNELS_AFTER=$(pgrep -af "ssh -N -R 0.0.0.0:" | \
    grep -oE '0\.0\.0\.0:[0-9]+:localhost:[0-9]+' || true)
REMOTE_PORTS_AFTER=$(echo "$ALL_TUNNELS_AFTER" | awk -F: '{print $2}' | sort -u)
echo "  post-restart remote ports:"
echo "$REMOTE_PORTS_AFTER" | sed 's/^/    /'

# Compare with the pre-restart REMOTE_PORTS captured in step 5.
# Both sets must be equal — Phase 30 keeps virtual ports stable
# across daemon restart.
PRE_REMOTE_SORTED=$(echo "$REMOTE_PORTS" | sort -u)
POST_REMOTE_SORTED=$(echo "$REMOTE_PORTS_AFTER" | sort -u)
if [ "$PRE_REMOTE_SORTED" != "$POST_REMOTE_SORTED" ]; then
    echo "  pre-restart : $(echo "$PRE_REMOTE_SORTED" | tr '\n' ' ')"
    echo "  post-restart: $(echo "$POST_REMOTE_SORTED" | tr '\n' ' ')"
    fail "Phase 30 violation: SSG shared tunnels' virtual ports MUST be stable across daemon restart"
fi

# Symmetry must hold post-restart too.
while IFS= read -r line; do
    [ -n "$line" ] || continue
    R=$(echo "$line" | awk -F: '{print $2}')
    L=$(echo "$line" | awk -F: '{print $NF}')
    if [ "$R" != "$L" ]; then
        fail "post-restart Phase 30 violation: ssh -R '$line' is asymmetric (remote=$R != local=$L)"
    fi
done <<<"$ALL_TUNNELS_AFTER"

pass "both projects' tunnels restored after daemon restart with stable virtual ports"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
# Remove each project's instance from its own cwd so the CLI resolves
# the correct project key (both instances are named `dev-1`).
cd "$PROJ_A"
"$COAST" rm dev-1 2>&1 >/dev/null || true
cd "$PROJ_B"
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()

# Tear down each SSG too so subsequent test runs start clean.
cd "$PROJ_A"
"$COAST" ssg rm --with-data --force 2>/dev/null || true
cd "$PROJ_B"
"$COAST" ssg rm --with-data --force 2>/dev/null || true

pass "Cleaned up"

echo ""
echo "==========================================="
echo "  PHASE 24 TWO-PROJECTS-ONE-REMOTE OK"
echo "==========================================="
