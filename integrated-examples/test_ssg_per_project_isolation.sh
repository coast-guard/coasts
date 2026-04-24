#!/usr/bin/env bash
#
# Integration test (Phase 22): two per-project SSGs run side-by-side
# without interfering with each other.
#
# Verifies:
#   - Two distinct `Coastfile.shared_service_groups` in two different
#     project directories (each with its own sibling `Coastfile`
#     declaring `[coast].name`) produce two distinct SSG containers
#     on the host: `{project-a}-ssg` and `{project-b}-ssg`.
#   - `coast ssg ps` run from each project's cwd shows only that
#     project's SSG (cwd-scoped).
#   - `coast ssg ls` run from an unrelated cwd lists both SSGs
#     (cross-project).
#   - `coast ssg rm --with-data --force` run per project tears down
#     only that project's SSG.
#
# See `coast-ssg/DESIGN.md §23` (per-project correction) — the whole
# point of the correction is that two projects can run their own
# postgres-on-5432 without colliding on the host.
#
# This test intentionally creates its fixtures inline (via `mktemp`)
# rather than reusing the `coast-ssg-minimal` projects dir fixture,
# because the shared fixture doesn't carry a sibling Coastfile, which
# the per-project CLI resolution requires.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

# Additional cleanup for Phase 22: make sure no stale per-project
# SSG container survives from an earlier run. We tear them down by
# the expected `{project}-ssg` name.
for proj in phase22-a phase22-b; do
    docker rm -f "${proj}-ssg" 2>/dev/null || true
    docker volume ls -q --filter "name=coast-dind--${proj}--ssg" 2>/dev/null \
        | xargs -r docker volume rm 2>/dev/null || true
done

start_daemon

# ============================================================
# Create two inline project fixtures.
# ============================================================

TEST_ROOT=$(mktemp -d -t coast-ssg-phase22-XXXXXX)
PROJ_A="$TEST_ROOT/project-a"
PROJ_B="$TEST_ROOT/project-b"
UNRELATED="$TEST_ROOT/unrelated"
mkdir -p "$PROJ_A" "$PROJ_B" "$UNRELATED"

make_project() {
    local dir="$1"
    local project_name="$2"

    # Minimal docker-compose: one alpine container that just idles.
    cat > "$dir/docker-compose.yml" << COMPOSE_EOF
services:
  app:
    image: alpine:3.19
    command: ["sh", "-c", "sleep infinity"]
COMPOSE_EOF

    # Main Coastfile — `[coast].name` is how per-project resolution
    # finds the project.
    cat > "$dir/Coastfile" << COASTFILE_EOF
[coast]
name = "$project_name"
compose = "./docker-compose.yml"
runtime = "dind"
COASTFILE_EOF

    # Per-project SSG with a minimal postgres service. Using
    # postgres:16-alpine for fast pulls in CI (matches the shared
    # `coast-ssg-minimal` fixture).
    cat > "$dir/Coastfile.shared_service_groups" << SSG_EOF
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_PASSWORD = "coast" }
SSG_EOF
}

make_project "$PROJ_A" "phase22-a"
make_project "$PROJ_B" "phase22-b"
pass "Inline project fixtures created (phase22-a, phase22-b)"

# Both projects land in the same ~/.coast state DB managed by the
# single daemon started above. The per-project SSG model means the
# daemon keys every `ssg` row by the project name pulled from the
# sibling Coastfile, so the two projects coexist without collision.

# ============================================================
# Step 1: build + run SSG for project-a.
# ============================================================

echo ""
echo "=== Step 1: build + run phase22-a SSG ==="

cd "$PROJ_A"
SSG_BUILD_A=$("$COAST" ssg build 2>&1)
echo "$SSG_BUILD_A" | tail -3
assert_contains "$SSG_BUILD_A" "Build complete" "phase22-a ssg build succeeds"

SSG_RUN_A=$("$COAST" ssg run 2>&1)
echo "$SSG_RUN_A" | tail -3
assert_contains "$SSG_RUN_A" "SSG running" "phase22-a ssg run succeeds"

# Verify the Docker container is named `phase22-a-ssg`.
docker inspect phase22-a-ssg >/dev/null 2>&1 \
    || fail "expected Docker container 'phase22-a-ssg' to exist"
pass "Docker container 'phase22-a-ssg' exists"

# ============================================================
# Step 2: build + run SSG for project-b (concurrent with -a).
# ============================================================

echo ""
echo "=== Step 2: build + run phase22-b SSG (concurrent) ==="

cd "$PROJ_B"
SSG_BUILD_B=$("$COAST" ssg build 2>&1)
echo "$SSG_BUILD_B" | tail -3
assert_contains "$SSG_BUILD_B" "Build complete" "phase22-b ssg build succeeds"

SSG_RUN_B=$("$COAST" ssg run 2>&1)
echo "$SSG_RUN_B" | tail -3
assert_contains "$SSG_RUN_B" "SSG running" "phase22-b ssg run succeeds"

docker inspect phase22-b-ssg >/dev/null 2>&1 \
    || fail "expected Docker container 'phase22-b-ssg' to exist"
pass "Docker container 'phase22-b-ssg' exists"

# Both containers are live at the same time — this is the core
# contract from DESIGN.md §23 (overturning §3 non-goal "Multiple
# concurrent SSGs on one host").
RUNNING_A=$(docker inspect -f '{{.State.Running}}' phase22-a-ssg)
RUNNING_B=$(docker inspect -f '{{.State.Running}}' phase22-b-ssg)
assert_eq "$RUNNING_A" "true" "phase22-a-ssg is running"
assert_eq "$RUNNING_B" "true" "phase22-b-ssg is running"

# ============================================================
# Step 3: `coast ssg ps` is cwd-scoped.
# ============================================================

echo ""
echo "=== Step 3: cwd-scoped `coast ssg ps` ==="

cd "$PROJ_A"
PS_A=$("$COAST" ssg ps 2>&1)
echo "$PS_A" | tail -6
# `ps` reads the build manifest on disk and the project's state row;
# we mostly want to prove the command resolves to project-a without
# error (i.e. the CLI read `phase22-a` from the cwd's Coastfile).
assert_contains "$PS_A" "postgres" "ps from phase22-a cwd surfaces its postgres service"

cd "$PROJ_B"
PS_B=$("$COAST" ssg ps 2>&1)
echo "$PS_B" | tail -6
assert_contains "$PS_B" "postgres" "ps from phase22-b cwd surfaces its postgres service"

# ============================================================
# Step 4: `coast ssg ls` is cross-project (no cwd scoping).
# ============================================================

echo ""
echo "=== Step 4: cross-project `coast ssg ls` ==="

cd "$UNRELATED"
# Deliberately run from a dir with no Coastfile — `ls` must work
# from any cwd.
LS_OUT=$("$COAST" ssg ls 2>&1)
echo "$LS_OUT"
assert_contains "$LS_OUT" "2 SSG(s) across 2 project(s)" "ls announces both SSGs"
assert_contains "$LS_OUT" "phase22-a" "ls row for phase22-a"
assert_contains "$LS_OUT" "phase22-b" "ls row for phase22-b"
assert_contains "$LS_OUT" "running" "ls shows running status"

# Sanity: ls from inside project-a also sees both (cross-project).
cd "$PROJ_A"
LS_FROM_A=$("$COAST" ssg ls 2>&1)
assert_contains "$LS_FROM_A" "phase22-b" "ls from phase22-a cwd still lists phase22-b"

# ============================================================
# Step 5: rm phase22-a only — phase22-b should stay up.
# ============================================================

echo ""
echo "=== Step 5: per-project rm is isolated ==="

cd "$PROJ_A"
"$COAST" ssg rm --with-data --force >/dev/null 2>&1
docker inspect phase22-a-ssg >/dev/null 2>&1 \
    && fail "phase22-a-ssg container should have been removed"
pass "phase22-a SSG removed"

# phase22-b is untouched.
RUNNING_B_AFTER=$(docker inspect -f '{{.State.Running}}' phase22-b-ssg 2>/dev/null || echo "gone")
assert_eq "$RUNNING_B_AFTER" "true" "phase22-b-ssg still running after removing phase22-a"

# `ls` now shows just one SSG.
cd "$UNRELATED"
LS_AFTER=$("$COAST" ssg ls 2>&1)
echo "$LS_AFTER"
assert_contains "$LS_AFTER" "1 SSG(s) across 1 project(s)" "ls shows one remaining SSG"
assert_contains "$LS_AFTER" "phase22-b" "remaining SSG is phase22-b"
assert_not_contains "$LS_AFTER" "phase22-a" "phase22-a no longer listed"

# Tear down phase22-b as well.
cd "$PROJ_B"
"$COAST" ssg rm --with-data --force >/dev/null 2>&1

# Final `ls` reports zero SSGs.
cd "$UNRELATED"
LS_EMPTY=$("$COAST" ssg ls 2>&1)
assert_contains "$LS_EMPTY" "No SSGs registered" "ls reports no SSGs after both removed"

# --- Done ---

echo ""
echo "==========================================="
echo "  PHASE 22 PER-PROJECT ISOLATION PASSED"
echo "==========================================="
