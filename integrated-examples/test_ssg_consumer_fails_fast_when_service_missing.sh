#!/usr/bin/env bash
#
# Integration test (Phase 31, DESIGN §24.4): a consumer that
# references an SSG service which subsequently disappears from the
# SSG fails FAST at connect time (TCP refused / DNS-style failure)
# rather than via the pre-Phase-29 build-time drift audit.
#
# Phase 29 deleted `validate_ssg_drift`. Phase 24 (§24.4) replaces it
# with "drift becomes a runtime concern": when the SSG no longer
# publishes a service the consumer references, the consumer's
# in-DinD socat targets a virtual port that is no longer being
# forwarded by the host socat. The connection fails — that's the
# user-visible signal.
#
# Scenario:
#   1. Build SSG A with services [postgres, redis]. Run it.
#   2. Build a consumer that references both via `from_group = true`.
#      Run the consumer. Confirm both services reachable from inside.
#   3. Mutate SSG Coastfile to drop redis. `ssg build` + `ssg run`.
#      Phase 28's `clear_ssg_services` + reconcile_project tears
#      down redis's host socat; postgres's stays.
#   4. From inside the still-running consumer, attempt to connect to
#      redis canonical port (6379). Expect a connect failure.
#      postgres (5432) still works.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

CLEANUP_INSTANCES=()

_cleanup() {
    echo ""
    echo "--- Cleanup ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    docker rm -f phase31-failsfast-ssg 2>/dev/null || true
    docker volume ls -q --filter "name=coast-dind--phase31-failsfast--ssg" 2>/dev/null \
        | xargs -r docker volume rm 2>/dev/null || true
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    rm -rf "$HOME/.coast/ssg"
}
trap '_cleanup' EXIT

preflight_checks
echo ""
echo "=== Setup ==="
clean_slate
rm -rf "$HOME/.coast/ssg"
start_daemon

# ============================================================
# Self-contained project fixture under a tempdir so we can mutate
# its Coastfile.shared_service_groups freely between steps.
# ============================================================

TEST_ROOT=$(mktemp -d -t coast-phase31-failsfast-XXXXXX)
PROJ="$TEST_ROOT/project"
mkdir -p "$PROJ"

cat > "$PROJ/docker-compose.yml" << 'COMPOSE_EOF'
services:
  app:
    image: alpine:3.19
    command: |
      sh -c "
        apk add --no-cache postgresql-client redis netcat-openbsd >/dev/null 2>&1
        tail -f /dev/null
      "
COMPOSE_EOF

cat > "$PROJ/Coastfile" << 'COASTFILE_EOF'
[coast]
name = "phase31-failsfast"
compose = "./docker-compose.yml"
runtime = "dind"

[shared_services.postgres]
from_group = true

[shared_services.redis]
from_group = true
COASTFILE_EOF

cat > "$PROJ/Coastfile.shared_service_groups" << 'SSG_INITIAL'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "postgres" }

[shared_services.redis]
image = "redis:7-alpine"
ports = [6379]
SSG_INITIAL

(
    cd "$PROJ"
    git init -b main >/dev/null 2>&1
    git config user.name "Coast Test"
    git config user.email "test@coasts.dev"
    git add -A
    git commit -m "phase31 failsfast initial fixture" >/dev/null 2>&1
)
pass "phase31-failsfast fixture created"

echo ""
echo "=== Step 1: SSG with postgres + redis, both reachable ==="

cd "$PROJ"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 5

"$COAST" build >/dev/null 2>&1
CLEANUP_INSTANCES+=("dev-1")
"$COAST" run dev-1 >/dev/null 2>&1
sleep 4

# nc -z probes the inner alias-IP socat. Both ports must accept
# the TCP handshake before we mutate the SSG.
NC_BOTH=$("$COAST" exec dev-1 --service app -- sh -c \
    'nc -z -w 2 postgres 5432 && echo "pg-ok"; nc -z -w 2 redis 6379 && echo "redis-ok"' 2>&1 || true)
echo "$NC_BOTH"
echo "$NC_BOTH" | grep -q '^pg-ok$' || fail "postgres unreachable from inst-1 before mutation"
echo "$NC_BOTH" | grep -q '^redis-ok$' || fail "redis unreachable from inst-1 before mutation"
pass "both services reachable initially"

echo ""
echo "=== Step 2: rebuild SSG WITHOUT redis ==="

cat > "$PROJ/Coastfile.shared_service_groups" << 'SSG_NO_REDIS'
[ssg]
runtime = "dind"

[shared_services.postgres]
image = "postgres:16-alpine"
ports = [5432]
env = { POSTGRES_USER = "coast", POSTGRES_PASSWORD = "coast", POSTGRES_DB = "postgres" }
SSG_NO_REDIS

set +e
"$COAST" ssg build >/dev/null 2>&1
BUILD_EC=$?
set -e
[ "$BUILD_EC" -eq 0 ] || fail "ssg build (without redis) failed; the consumer's Coastfile still references redis but Phase 29 was supposed to remove the build-time drift audit"

# Phase 31: the consumer's `from_group = true` for redis should have
# been caught by the static parse-time check in build/manifest.rs
# (`build_ssg_manifest_block`) at the consumer's NEXT `coast build`.
# But the SSG-side `ssg build` itself is project-scoped to the SSG
# Coastfile and shouldn't fail just because the consumer references
# redis. (We DO expect the consumer's eventual `coast build` to fail
# if it's re-run after this point — out of scope for this test.)
pass "ssg build succeeded after dropping redis (no runtime drift audit)"

"$COAST" ssg run >/dev/null 2>&1
sleep 4

echo ""
echo "=== Step 3: from running consumer, redis fails fast; postgres still works ==="

NC_AFTER=$("$COAST" exec dev-1 --service app -- sh -c \
    'nc -z -w 2 postgres 5432 && echo "pg-ok"; nc -z -w 2 redis 6379 && echo "redis-ok" || echo "redis-failed"' 2>&1 || true)
echo "$NC_AFTER"

echo "$NC_AFTER" | grep -q '^pg-ok$' \
    || fail "postgres became unreachable after the SSG rebuild (its host socat should have survived)"

# Phase 31's contract: redis is gone from the SSG, so its host socat
# was killed. The consumer's in-DinD socat still listens on 6379 and
# forwards to host.docker.internal:<vport_redis>, but nothing is
# bound on that vport anymore. The connection attempt fails fast.
echo "$NC_AFTER" | grep -q '^redis-failed$' \
    || fail "redis should have been unreachable post-mutation; got: $NC_AFTER"
pass "redis fails fast; postgres still works (Phase 31 §24.4 fail-fast contract)"

# --- Done ---
"$COAST" rm dev-1 >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()

echo ""
echo "==========================================="
echo "  PHASE 31 CONSUMER-FAILS-FAST OK"
echo "==========================================="
