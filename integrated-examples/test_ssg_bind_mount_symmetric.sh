#!/usr/bin/env bash
#
# Integration test: SSG symmetric-path host bind mount (Phase 3).
#
# Verifies that the same host directory path is visible with the same
# inode inside the outer SSG DinD *and* inside the inner postgres
# service container — the contract described in `coast-ssg/DESIGN.md
# §10.2`. Seeds a marker file on the host, then:
#
#   1. `coast ssg build && coast ssg run` against the coast-ssg-bind-mount project.
#   2. Runs `stat -c %i` on the marker on the host, then the same path
#      inside `coast-ssg` (via `docker exec`), then the same path
#      inside the inner postgres service (via
#      `docker exec coast-ssg docker compose ... exec ...`).
#   3. Asserts the three inode numbers match.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_bind_mount_symmetric.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG state from other runs.
rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

# Pick a host path that's reachable through the dindind test
# container's persistent volume tree. `/tmp` would be a tmpfs inside
# the test container that the outer daemon can't bind-mount through
# into the coast-ssg container. Exported so setup.sh bakes the same
# path into the Coastfile.
export COAST_SSG_BIND_HOST_ROOT="${COAST_SSG_BIND_HOST_ROOT:-$HOME/coast-ssg-bind-mount}"
HOST_PATH="$COAST_SSG_BIND_HOST_ROOT/pg-data"
rm -rf "$HOST_PATH"
mkdir -p "$HOST_PATH"
# alpine postgres runs as UID 70; make sure the bind target is
# writable by that user. Fallback to chmod 777 when chown isn't
# possible (rootless host).
chown 70:70 "$HOST_PATH" 2>/dev/null || chmod 777 "$HOST_PATH"

cd "$PROJECTS_DIR/coast-ssg-bind-mount"

start_daemon

# ============================================================
# Test 1: build + run
# ============================================================

echo ""
echo "=== Test 1: build + run ==="

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"
assert_contains "$BUILD_OUT" "Build complete" "build succeeds"

RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$RUN_OUT"
assert_contains "$RUN_OUT" "SSG running" "run succeeds"

DOCKER_PS=$(docker ps --filter "name=^coast-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "coast-ssg" "coast-ssg container is running"

# Postgres needs time to initdb into the fresh bind directory.
sleep 10

# After initdb completes, postgres has written PG_VERSION into the
# shared directory. We use that file (always present after initdb) as
# our inode probe so we don't race with postgres claiming ownership
# of the data dir.
MARKER_NAME="PG_VERSION"
MARKER_HOST_PATH="$HOST_PATH/$MARKER_NAME"

# Wait for PG_VERSION to appear (postgres can take a while on first
# boot in a resource-constrained CI environment).
for i in $(seq 1 30); do
    if [ -f "$MARKER_HOST_PATH" ]; then break; fi
    sleep 1
done
[ -f "$MARKER_HOST_PATH" ] || fail "postgres never created $MARKER_HOST_PATH inside host bind dir"
pass "postgres wrote $MARKER_NAME into the host bind directory"

# ============================================================
# Test 2: host and outer-DinD see the same inode
# ============================================================

echo ""
echo "=== Test 2: host vs outer DinD inode ==="

HOST_INODE=$(stat -c %i "$MARKER_HOST_PATH" 2>/dev/null || stat -f %i "$MARKER_HOST_PATH")
pass "host inode = $HOST_INODE"

OUTER_INODE=$(docker exec coast-ssg stat -c %i "$MARKER_HOST_PATH")
OUTER_INODE=$(echo "$OUTER_INODE" | tr -d '\r' | tr -d '[:space:]')
pass "outer DinD inode = $OUTER_INODE"

assert_eq "$OUTER_INODE" "$HOST_INODE" "host bind visible with same inode inside coast-ssg"

# ============================================================
# Test 3: inner postgres container sees the same inode
# ============================================================

echo ""
echo "=== Test 3: inner postgres inode (symmetric path, remapped to data dir) ==="

# Symmetric-path plan: the outer bind dst equals the host src, then
# the inner compose binds that path to /var/lib/postgresql/data. We
# verify by statting the remapped inner path.

INNER_INODE=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml \
    -p coast-ssg \
    exec -T postgres stat -c %i "/var/lib/postgresql/data/$MARKER_NAME")
INNER_INODE=$(echo "$INNER_INODE" | tr -d '\r' | tr -d '[:space:]')
pass "inner postgres inode = $INNER_INODE"

assert_eq "$INNER_INODE" "$HOST_INODE" "inner postgres sees the same inode (symmetric path)"

# Double-check by reading PG_VERSION contents from all three places.
HOST_CONTENT=$(cat "$MARKER_HOST_PATH" | tr -d '\n')
OUTER_CONTENT=$(docker exec coast-ssg cat "$MARKER_HOST_PATH" | tr -d '\n')
INNER_CONTENT=$(docker exec coast-ssg docker compose \
    -f /coast-artifact/compose.yml \
    -p coast-ssg \
    exec -T postgres cat "/var/lib/postgresql/data/$MARKER_NAME" | tr -d '\r' | tr -d '\n')

assert_eq "$OUTER_CONTENT" "$HOST_CONTENT" "host + outer DinD see identical PG_VERSION contents"
assert_eq "$INNER_CONTENT" "$HOST_CONTENT" "inner postgres sees identical PG_VERSION contents"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG BIND MOUNT SYMMETRIC TESTS PASSED"
echo "==========================================="
