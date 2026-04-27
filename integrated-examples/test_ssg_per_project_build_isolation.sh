#!/usr/bin/env bash
#
# Integration test (Phase 23): a project's SSG build is invisible to
# a different project's consumer. Before Phase 23, `coast ssg build`
# updated a single global `~/.coast/ssg/latest` symlink and every
# project's consumer fell through to it on resolution — that leaked
# builds across projects. Under Phase 23 the consumer resolver reads
# `ssg.latest_build_id` scoped to the consumer's own project, and
# hard-errors when the project has never run `ssg build`.
#
# Verifies:
#   1. Project A runs `ssg build` — success.
#   2. Project B (which references `from_group = true` but has never
#      run `ssg build`) hard-errors on `coast build` with a message
#      naming project B (NOT silently picking up A's build).
#   3. Project B then runs its own `ssg build` — `coast build` in B
#      succeeds.
#
# See `coast-ssg/DESIGN.md §23.3`.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

# Additional cleanup: make sure no stale per-project SSG container
# survives from an earlier run.
for proj in phase23-a phase23-b; do
    docker rm -f "${proj}-ssg" 2>/dev/null || true
    docker volume ls -q --filter "name=coast-dind--${proj}--ssg" 2>/dev/null \
        | xargs -r docker volume rm 2>/dev/null || true
done

start_daemon

TEST_ROOT=$(mktemp -d -t coast-ssg-phase23-XXXXXX)
PROJ_A="$TEST_ROOT/project-a"
PROJ_B="$TEST_ROOT/project-b"
mkdir -p "$PROJ_A" "$PROJ_B"

make_consumer_project() {
    local dir="$1"
    local project_name="$2"

    cat > "$dir/docker-compose.yml" << COMPOSE_EOF
services:
  app:
    image: alpine:3.19
    command: ["sh", "-c", "sleep infinity"]
COMPOSE_EOF

    cat > "$dir/Coastfile" << COASTFILE_EOF
[coast]
name = "$project_name"
compose = "./docker-compose.yml"
runtime = "dind"

# Phase 23: declares `from_group = true` so `coast build` tries to
# snapshot the project's own SSG manifest into the consumer manifest.
[shared_services.postgres]
from_group = true
COASTFILE_EOF

    cat > "$dir/Coastfile.shared_service_groups" << SSG_EOF
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_PASSWORD = "coast" }
SSG_EOF
}

make_consumer_project "$PROJ_A" "phase23-a"
make_consumer_project "$PROJ_B" "phase23-b"
pass "Inline project fixtures created (phase23-a, phase23-b)"

# ============================================================
# Step 1: project A runs `ssg build`.
# ============================================================

echo ""
echo "=== Step 1: phase23-a runs ssg build ==="

cd "$PROJ_A"
SSG_BUILD_A=$("$COAST" ssg build 2>&1)
echo "$SSG_BUILD_A" | tail -3
assert_contains "$SSG_BUILD_A" "Build complete" "phase23-a ssg build succeeds"

# ============================================================
# Step 2: project B's `coast build` must hard-error — A's build
# must NOT leak into B's consumer manifest.
# ============================================================

echo ""
echo "=== Step 2: phase23-b coast build hard-errors (no cross-project leak) ==="

cd "$PROJ_B"
# `coast build` (not `ssg build`) reads the consumer's Coastfile; the
# `from_group = true` ref triggers the "which SSG build?" resolution
# and hard-errors when project B has no build.
BUILD_B_EXIT=0
BUILD_B_OUT=$("$COAST" build 2>&1) || BUILD_B_EXIT=$?
echo "$BUILD_B_OUT" | tail -8

if [ "$BUILD_B_EXIT" = "0" ]; then
    fail "phase23-b coast build must exit non-zero when no SSG build exists"
else
    pass "phase23-b coast build exits non-zero (exit=$BUILD_B_EXIT)"
fi
assert_contains "$BUILD_B_OUT" "no SSG build exists" \
    "error must mention missing SSG build"
assert_contains "$BUILD_B_OUT" "phase23-b" \
    "error must name project 'phase23-b', not silently pick up phase23-a"
assert_not_contains "$BUILD_B_OUT" "phase23-a" \
    "error must NOT reference project 'phase23-a' (that would be a leak)"

# ============================================================
# Step 3: after B runs its own ssg build, `coast build` in B works.
# ============================================================

echo ""
echo "=== Step 3: phase23-b ssg build + coast build succeeds ==="

cd "$PROJ_B"
SSG_BUILD_B=$("$COAST" ssg build 2>&1)
echo "$SSG_BUILD_B" | tail -3
assert_contains "$SSG_BUILD_B" "Build complete" "phase23-b ssg build succeeds"

# Now `coast build` in B succeeds because B has its own build.
BUILD_B_OK=$("$COAST" build 2>&1)
echo "$BUILD_B_OK" | tail -5
assert_contains "$BUILD_B_OK" "Build complete" \
    "phase23-b coast build succeeds once B has its own SSG build"

# --- Cleanup ---
# Tear down any SSG containers started as part of this test.
for proj in phase23-a phase23-b; do
    cd "$TEST_ROOT/project-${proj#phase23-}"
    "$COAST" ssg rm --with-data --force >/dev/null 2>&1 || true
done

echo ""
echo "==========================================="
echo "  PHASE 23 BUILD ISOLATION PASSED"
echo "==========================================="
