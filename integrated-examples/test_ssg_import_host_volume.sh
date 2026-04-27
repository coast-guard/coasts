#!/usr/bin/env bash
#
# Integration test: `coast ssg import-host-volume` — zero-copy
# migration of existing host Docker named volumes into SSG Coastfile
# bind-mount entries (Phase 15, DESIGN.md §10.7).
#
# Asserts:
#   1. Snippet mode prints the canonical `<mountpoint>:<mount>` bind
#      line for an existing host volume.
#   2. `--apply` rewrites the SSG Coastfile in place and creates a
#      `.bak` backup with the original bytes.
#   3. Duplicate `--mount` paths on the same service are rejected.
#   4. Missing host volumes produce a clear error.
#   5. `coast ssg build` still succeeds on the rewritten Coastfile.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

# Additional cleanup for the host volume this test creates.
VOLUME_NAME="coast-phase15-test-vol"
cleanup_volume() {
    docker volume rm -f "$VOLUME_NAME" >/dev/null 2>&1 || true
}
trap 'cleanup_volume' EXIT

echo ""
echo "=== Setup ==="

clean_slate
cleanup_volume

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"

# Create a host Docker named volume with real bytes so `inspect_volume`
# returns a valid Mountpoint.
docker volume create "$VOLUME_NAME" >/dev/null
docker run --rm -v "$VOLUME_NAME":/vol alpine \
    sh -c 'echo "phase-15-marker-$(date +%s)" > /vol/marker.txt' >/dev/null
pass "Created host volume '$VOLUME_NAME' with marker data"

start_daemon

# ============================================================
# Test 1: snippet mode prints a zero-copy bind line
# ============================================================

echo ""
echo "=== Test 1: snippet mode ==="

# Stage the minimal SSG fixture into a scratch dir so --apply below
# rewrites a file we own, not the shared fixture.
WORKDIR="$(mktemp -d)"
cp "$PROJECTS_DIR/coast-ssg-minimal/Coastfile.shared_service_groups" "$WORKDIR/"

# Phase 23: import-host-volume resolves the project via the sibling
# Coastfile, same as `ssg build`. Stamp a minimal one into the
# scratch so resolution doesn't walk up to the dindind workspace.
cat > "$WORKDIR/Coastfile" << 'IMPORT_TEST_COASTFILE_EOF'
[coast]
name = "coast-ssg-import-test"
runtime = "dind"
compose = "docker-compose.yml"
IMPORT_TEST_COASTFILE_EOF
cat > "$WORKDIR/docker-compose.yml" << 'IMPORT_TEST_COMPOSE_EOF'
services:
  app:
    image: alpine:3
IMPORT_TEST_COMPOSE_EOF

SNIPPET=$("$COAST" ssg import-host-volume "$VOLUME_NAME" \
    --service postgres \
    --mount /var/lib/postgresql/data-new \
    --working-dir "$WORKDIR" 2>&1)
echo "$SNIPPET"

# The Mountpoint Docker reports is typically
# `/var/lib/docker/volumes/<name>/_data` — bollard just hands back
# whatever the daemon returns. Assert on the $VOLUME_NAME substring
# and the container path rather than the full Docker-root path so
# this also works on non-standard Docker roots.
assert_contains "$SNIPPET" "$VOLUME_NAME" "snippet references the host volume name in the path"
assert_contains "$SNIPPET" "_data:/var/lib/postgresql/data-new" \
    "snippet contains the zero-copy <mountpoint>:<mount> line"
assert_contains "$SNIPPET" "[shared_services.postgres]" "snippet headers the target service"

# Snippet mode must NOT have mutated the Coastfile on disk.
if ! diff -q "$PROJECTS_DIR/coast-ssg-minimal/Coastfile.shared_service_groups" \
    "$WORKDIR/Coastfile.shared_service_groups" >/dev/null; then
    fail "snippet mode should not modify the on-disk Coastfile"
fi
pass "snippet mode left the Coastfile unmodified"

# ============================================================
# Test 2: --apply rewrites in place with a .bak backup
# ============================================================

echo ""
echo "=== Test 2: --apply writes file + .bak ==="

cp "$WORKDIR/Coastfile.shared_service_groups" "$WORKDIR/Coastfile.shared_service_groups.pre-apply"

APPLY_OUT=$("$COAST" ssg import-host-volume "$VOLUME_NAME" \
    --service postgres \
    --mount /srv/imported/data \
    --working-dir "$WORKDIR" \
    --apply 2>&1)
echo "$APPLY_OUT"
assert_contains "$APPLY_OUT" "applied" "apply response announces success"
assert_contains "$APPLY_OUT" "backup" "apply response references the backup path"

# .bak must match the original.
[ -f "$WORKDIR/Coastfile.shared_service_groups.bak" ] \
    || fail ".bak backup file is missing"
if ! diff -q "$WORKDIR/Coastfile.shared_service_groups.pre-apply" \
    "$WORKDIR/Coastfile.shared_service_groups.bak" >/dev/null; then
    fail ".bak should contain the pre-apply Coastfile bytes"
fi
pass ".bak contains the original Coastfile bytes"

# Rewritten file must contain the new bind line.
grep -q "_data:/srv/imported/data" "$WORKDIR/Coastfile.shared_service_groups" \
    || fail "rewritten Coastfile is missing the new bind line"
# Pre-existing volume entry must still be preserved.
grep -q "pg_data:/var/lib/postgresql/data" "$WORKDIR/Coastfile.shared_service_groups" \
    || fail "rewritten Coastfile dropped the pre-existing pg_data volume"
pass "rewritten Coastfile has new bind line + preserves existing entries"

# ============================================================
# Test 3: duplicate mount path is rejected
# ============================================================

echo ""
echo "=== Test 3: duplicate --mount rejected ==="

DUP_OUT=$("$COAST" ssg import-host-volume "$VOLUME_NAME" \
    --service postgres \
    --mount /srv/imported/data \
    --working-dir "$WORKDIR" 2>&1 || true)
echo "$DUP_OUT"
assert_contains "$DUP_OUT" "already declares a volume" "duplicate mount path hard-errors"

# ============================================================
# Test 4: missing host volume produces a clear error
# ============================================================

echo ""
echo "=== Test 4: missing host volume errors cleanly ==="

MISSING_OUT=$("$COAST" ssg import-host-volume nope-does-not-exist \
    --service postgres \
    --mount /srv/imported/other \
    --working-dir "$WORKDIR" 2>&1 || true)
echo "$MISSING_OUT"
assert_contains "$MISSING_OUT" "no volume named" "missing volume produces a clear error message"

# ============================================================
# Test 5: rewritten Coastfile still builds
# ============================================================

echo ""
echo "=== Test 5: coast ssg build succeeds on the rewritten Coastfile ==="

# Pull the postgres image into the host cache first (build pulls too,
# but being explicit keeps the test deterministic on fresh hosts).
BUILD_OUT=$("$COAST" ssg build --working-dir "$WORKDIR" 2>&1)
echo "$BUILD_OUT" | tail -5
assert_contains "$BUILD_OUT" "Build complete" "ssg build succeeds on imported-volume Coastfile"

# Cleanup.
rm -rf "$WORKDIR"
cleanup_volume

echo ""
echo "==========================================="
echo "  ALL SSG IMPORT-HOST-VOLUME TESTS PASSED"
echo "==========================================="
