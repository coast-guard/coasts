#!/usr/bin/env bash
#
# Integration test: `[ssg] extends` / `includes` / `[unset]` (Phase 17,
# DESIGN.md §17 SETTLED #42).
#
# Scenarios:
#   1. `Coastfile.shared_service_groups` with `[ssg] extends = "..."`
#      -- parent defines postgres + redis, child adds mongodb and
#      overrides postgres image. `coast ssg build` succeeds, manifest
#      has all three services, postgres image is the child's.
#   2. Inspect `~/.coast/ssg/latest/ssg-coastfile.toml` -- it's the
#      flattened standalone form: no `extends`, no `includes`, no
#      `[unset]`. All three services present.
#   3. `coast ssg run` + `coast ssg ps` confirms every service boots
#      and the child's postgres image is the running image.
#   4. Cycle detection: write two files that extend each other;
#      `coast ssg build` must hard-error with
#      `circular extends/includes dependency`.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

start_daemon

WORKDIR="$(mktemp -d -t coast-ssg-phase17-XXXXXX)"
trap 'rm -rf "$WORKDIR" "$CYCLE_DIR" 2>/dev/null || true; _do_cleanup' EXIT

cat > "$WORKDIR/Coastfile.ssg-base" <<'EOF'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
EOF

cat > "$WORKDIR/Coastfile.shared_service_groups" <<'EOF'
[ssg]
extends = "Coastfile.ssg-base"

[shared_services.postgres]
image = "postgres:17-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast" }

[shared_services.mongodb]
image = "mongo:7"
ports = [27017]
EOF

echo ""
echo "=== Test 1: coast ssg build against the extending Coastfile ==="

cd "$WORKDIR"
BUILD_OUT=$("$COAST" ssg build --working-dir "$WORKDIR" 2>&1)
echo "$BUILD_OUT" | tail -10
assert_contains "$BUILD_OUT" "Build complete" "ssg build succeeds with extends"

BUILD_ID=$(readlink "$HOME/.coast/ssg/latest" | xargs basename)
echo "SSG build id: $BUILD_ID"

# Manifest: 3 services, postgres image is the child's.
MANIFEST="$HOME/.coast/ssg/builds/$BUILD_ID/manifest.json"
[ -f "$MANIFEST" ] || fail "manifest.json missing at $MANIFEST"

SERVICE_NAMES=$(python3 -c "
import json
m = json.load(open('$MANIFEST'))
print(' '.join(sorted(s['name'] for s in m['services'])))
")
echo "services in manifest: $SERVICE_NAMES"
assert_contains "$SERVICE_NAMES" "mongodb" "mongodb (child-only) is in manifest"
assert_contains "$SERVICE_NAMES" "postgres" "postgres is in manifest"
assert_contains "$SERVICE_NAMES" "redis" "redis (parent-only) inherited into manifest"

PG_IMAGE=$(python3 -c "
import json
m = json.load(open('$MANIFEST'))
for s in m['services']:
    if s['name'] == 'postgres':
        print(s['image'])
        break
")
assert_eq "$PG_IMAGE" "postgres:17-alpine" "child's postgres image won the merge"

echo ""
echo "=== Test 2: flattened ssg-coastfile.toml ==="

FLAT="$HOME/.coast/ssg/builds/$BUILD_ID/ssg-coastfile.toml"
[ -f "$FLAT" ] || fail "flattened coastfile missing at $FLAT"

# Must not contain extends / includes / [unset] anywhere in the
# flattened standalone form.
if grep -qE '^[[:space:]]*extends' "$FLAT"; then
    echo "--- flattened file ---"
    cat "$FLAT"
    fail "flattened coastfile still contains 'extends ='"
fi
if grep -qE '^[[:space:]]*includes' "$FLAT"; then
    echo "--- flattened file ---"
    cat "$FLAT"
    fail "flattened coastfile still contains 'includes ='"
fi
if grep -qE '^\[unset\]' "$FLAT"; then
    echo "--- flattened file ---"
    cat "$FLAT"
    fail "flattened coastfile still contains '[unset]' block"
fi
pass "flattened ssg-coastfile.toml has no extends/includes/[unset]"

# All three services appear in the flattened file.
grep -qE '^\[shared_services\.mongodb\]' "$FLAT" || fail "mongodb missing from flattened file"
grep -qE '^\[shared_services\.postgres\]' "$FLAT" || fail "postgres missing from flattened file"
grep -qE '^\[shared_services\.redis\]' "$FLAT" || fail "redis missing from flattened file"
pass "all three merged services present in flattened file"

echo ""
echo "=== Test 3: coast ssg run + ps reports all three services ==="

"$COAST" ssg run >/dev/null 2>&1
sleep 5

PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT"
assert_contains "$PS_OUT" "postgres" "ps reports postgres"
assert_contains "$PS_OUT" "redis" "ps reports redis"
assert_contains "$PS_OUT" "mongodb" "ps reports mongodb"

# Verify the running postgres really is the 17-alpine build the
# child requested (not the parent's 16).
PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
assert_contains "$PORTS_OUT" "postgres" "ports lists postgres"

echo ""
echo "=== Test 4: cycle detection hard-errors ==="

CYCLE_DIR="$(mktemp -d -t coast-ssg-cycle-XXXXXX)"

cat > "$CYCLE_DIR/Coastfile.a" <<'EOF'
[ssg]
extends = "Coastfile.b"

[shared_services.pg]
image = "postgres:16"
EOF

cat > "$CYCLE_DIR/Coastfile.b" <<'EOF'
[ssg]
extends = "Coastfile.a"
EOF

set +e
CYCLE_OUT=$("$COAST" ssg build -f "$CYCLE_DIR/Coastfile.a" 2>&1)
EC=$?
set -e

echo "$CYCLE_OUT" | tail -10
[ "$EC" -ne 0 ] || fail "expected ssg build to fail on cycle, got exit code 0"
assert_contains "$CYCLE_OUT" "circular extends/includes dependency" \
    "cycle hard-error surfaces"

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG COASTFILE INHERITANCE TESTS PASSED"
echo "==========================================="
