#!/usr/bin/env bash
#
# Integration test (Phase 25): two DIFFERENT projects each with their
# own SSG on the LOCAL host, both consumer coasts running locally,
# both declaring `postgres:5432` via `from_group = true`. Each
# consumer must reach its OWN project's postgres (not the other
# project's).
#
# Local analogue of the Phase 24 remote-side proof
# (`test_remote_two_projects_same_canonical_port_distinct_ssg.sh`).
# Proves the DESIGN.md §23 per-project SSG contract in a pure local
# topology: no reverse tunnels, no remote VMs, just two SSGs side by
# side on the host Docker daemon.
#
# Differences vs `test_ssg_per_project_isolation.sh`:
#   - That test only proves the SSGs can coexist (distinct containers,
#     distinct dynamic ports, cwd-scoped `ssg ps`).
#   - This test adds the functional probe: each consumer's app runs
#     `psql SHOW server_version` against its own `from_group = true`
#     postgres, and the returned version matches that project's own
#     SSG image (postgres:15 vs postgres:16). Nothing short of
#     correct per-project routing makes this work.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

CLEANUP_INSTANCES=()

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true") 2>/dev/null || true
    # Two SSGs to tear down, one per project.
    cleanup_project_ssgs "phase25-a" "phase25-b"
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    rm -rf "$HOME/.coast/ssg"
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Phase 25: Two projects, distinct local SSGs, no collision ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate
rm -rf "$HOME/.coast/ssg"

start_daemon

# ============================================================
# Create two inline project fixtures (A and B) under a tempdir.
# Each fixture is a self-contained per-project SSG: its own
# `Coastfile` (consumer) + `Coastfile.shared_service_groups` (SSG).
# ============================================================

TEST_ROOT=$(mktemp -d -t coast-phase25-XXXXXX)
PROJ_A="$TEST_ROOT/project-a"
PROJ_B="$TEST_ROOT/project-b"
mkdir -p "$PROJ_A" "$PROJ_B"

make_project() {
    local dir="$1"
    local project_name="$2"
    local pg_version="$3"

    # Consumer compose: alpine with psql, writes server_version to
    # /var/coast-version for the test to exec-read.
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

    cat > "$dir/Coastfile" << COASTFILE_EOF
[coast]
name = "$project_name"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true
COASTFILE_EOF

    cat > "$dir/Coastfile.shared_service_groups" << SSG_EOF
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:${pg_version}-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "postgres" }
SSG_EOF

    # `coast run` requires a git repo (it reads the current branch
    # for worktree/state metadata). Initialize one so the mktemp
    # fixture is usable by the run path, not just `ssg build/run`.
    (
        cd "$dir"
        git init -b main >/dev/null 2>&1
        git config user.name "Coast Test"
        git config user.email "test@coasts.dev"
        git add -A
        git commit -m "initial $project_name fixture" >/dev/null 2>&1
    )
}

make_project "$PROJ_A" "phase25-a" "15"
make_project "$PROJ_B" "phase25-b" "16"
pass "Inline project fixtures created (phase25-a pg15, phase25-b pg16)"

# ============================================================
# Step 1: Build + run both per-project SSGs locally.
# ============================================================

echo ""
echo "=== Step 1: Build + run both SSGs locally ==="

cd "$PROJ_A"
SSG_BUILD_A=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD_A" "Build complete" "phase25-a ssg build succeeds"
SSG_RUN_A=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_A" "SSG running" "phase25-a ssg run succeeds"
PORTS_A_OUT=$("$COAST" ssg ports 2>&1)
SSG_A_DYNAMIC=$(echo "$PORTS_A_OUT" | awk '/^  postgres/ {print $3}')
SSG_A_VIRTUAL=$(echo "$PORTS_A_OUT" | awk '/^  postgres/ {print $4}')
[ -n "$SSG_A_DYNAMIC" ] || fail "could not extract phase25-a SSG dynamic port"
[ -n "$SSG_A_VIRTUAL" ] && [ "$SSG_A_VIRTUAL" != "--" ] \
    || fail "could not extract phase25-a SSG virtual port (col 4 of ssg ports). Got '$SSG_A_VIRTUAL'"
pass "phase25-a SSG postgres dynamic=$SSG_A_DYNAMIC virtual=$SSG_A_VIRTUAL"

cd "$PROJ_B"
SSG_BUILD_B=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD_B" "Build complete" "phase25-b ssg build succeeds"
SSG_RUN_B=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_B" "SSG running" "phase25-b ssg run succeeds"
PORTS_B_OUT=$("$COAST" ssg ports 2>&1)
SSG_B_DYNAMIC=$(echo "$PORTS_B_OUT" | awk '/^  postgres/ {print $3}')
SSG_B_VIRTUAL=$(echo "$PORTS_B_OUT" | awk '/^  postgres/ {print $4}')
[ -n "$SSG_B_DYNAMIC" ] || fail "could not extract phase25-b SSG dynamic port"
[ -n "$SSG_B_VIRTUAL" ] && [ "$SSG_B_VIRTUAL" != "--" ] \
    || fail "could not extract phase25-b SSG virtual port (col 4 of ssg ports). Got '$SSG_B_VIRTUAL'"
pass "phase25-b SSG postgres dynamic=$SSG_B_DYNAMIC virtual=$SSG_B_VIRTUAL"

[ "$SSG_A_DYNAMIC" != "$SSG_B_DYNAMIC" ] \
    || fail "each project's SSG must publish on a distinct dynamic port (got $SSG_A_DYNAMIC for both)"

# Phase 31: each project's virtual port is also distinct — virtual
# ports are allocated per `(project, service, container_port)`, so
# two different projects on the same host MUST get different
# virtual ports even though they share the canonical 5432.
[ "$SSG_A_VIRTUAL" != "$SSG_B_VIRTUAL" ] \
    || fail "each project's SSG must allocate a distinct virtual port (got $SSG_A_VIRTUAL for both)"
pass "phase25-a and phase25-b have distinct virtual ports ($SSG_A_VIRTUAL vs $SSG_B_VIRTUAL)"

# Both SSG containers exist concurrently — Phase 23 correction.
docker inspect phase25-a-ssg >/dev/null 2>&1 \
    || fail "phase25-a-ssg container should exist"
docker inspect phase25-b-ssg >/dev/null 2>&1 \
    || fail "phase25-b-ssg container should exist"
pass "Both per-project SSG containers running concurrently (no host-port collision on 5432)"

sleep 5  # let each inner postgres initdb complete

# ============================================================
# Step 2: Run each project's consumer locally, using SAME instance
#         name `dev-1`. Project key disambiguates.
# ============================================================

echo ""
echo "=== Step 2: run phase25-a consumer 'dev-1' locally ==="

cd "$PROJ_A"
"$COAST" build 2>&1 | tail -5
set +e
RUN_A_OUT=$("$COAST" run dev-1 2>&1)
RUN_A_EXIT=$?
set -e
if [ $RUN_A_EXIT -ne 0 ]; then
    echo "$RUN_A_OUT"
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "phase25-a consumer local run failed"
fi
CLEANUP_INSTANCES+=("dev-1")
pass "phase25-a consumer 'dev-1' running locally"

echo ""
echo "=== Step 3: run phase25-b consumer 'dev-1' locally (same instance name, different project) ==="

cd "$PROJ_B"
"$COAST" build >/dev/null 2>&1
set +e
RUN_B_OUT=$("$COAST" run dev-1 2>&1)
RUN_B_EXIT=$?
set -e
if [ "$RUN_B_EXIT" -ne 0 ]; then
    echo "$RUN_B_OUT" | tail -20
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "phase25-b consumer local run failed — per-project scoping regression?"
fi
pass "phase25-b consumer 'dev-1' running locally alongside phase25-a's"

sleep 5

# ============================================================
# Step 4: Functional proof — each consumer's psql sees ITS OWN
#         project's postgres major version.
# ============================================================

echo ""
echo "=== Step 4: Per-project image isolation (psql SHOW server_version) ==="

wait_for_version_file() {
    local instance="$1"
    local container="${instance}-app-1"
    local dind="phase25-${instance#dev-1-}-dev-1"   # unused helper retained for clarity
    local tries=30
    while [ $tries -gt 0 ]; do
        # `coast exec` enters the instance's app service regardless of
        # where DinD lives; simpler than exec-through-the-DinD.
        if V=$("$COAST" exec "$instance" --service app -- cat /var/coast-version 2>/dev/null); then
            if [ -n "$V" ]; then
                echo "$V"
                return 0
            fi
        fi
        sleep 2
        tries=$((tries - 1))
    done
    echo ""
    return 1
}

# For project-scoped exec, the same instance name `dev-1` exists
# under two distinct projects. `coast exec` uses cwd to resolve
# the project, so `cd` into each before exec.
cd "$PROJ_A"
A_VERSION=$(wait_for_version_file "dev-1" || true)

cd "$PROJ_B"
B_VERSION=$(wait_for_version_file "dev-1" || true)

echo "  phase25-a reported server_version: '$A_VERSION'"
echo "  phase25-b reported server_version: '$B_VERSION'"

[ -n "$A_VERSION" ] || fail "phase25-a's app could not reach its postgres"
[ -n "$B_VERSION" ] || fail "phase25-b's app could not reach its postgres"

# postgres:15-alpine reports a version starting with "15."; 16 with "16.".
# If routing leaked between projects we'd see the same version twice.
echo "$A_VERSION" | grep -q '^15\.' \
    || fail "phase25-a expected server_version starting with 15., got '$A_VERSION' — routing leak?"
echo "$B_VERSION" | grep -q '^16\.' \
    || fail "phase25-b expected server_version starting with 16., got '$B_VERSION' — routing leak?"
pass "each consumer's psql reached its own project's postgres (15 vs 16)"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
cd "$PROJ_A"
"$COAST" rm dev-1 >/dev/null 2>&1 || true
cd "$PROJ_B"
"$COAST" rm dev-1 >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

cd "$PROJ_A"
"$COAST" ssg rm --with-data --force 2>/dev/null || true
cd "$PROJ_B"
"$COAST" ssg rm --with-data --force 2>/dev/null || true

pass "Cleaned up"

echo ""
echo "==========================================="
echo "  PHASE 25 TWO-PROJECTS LOCAL OK"
echo "==========================================="
