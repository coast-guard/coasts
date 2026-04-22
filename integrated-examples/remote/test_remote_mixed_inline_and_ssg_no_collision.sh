#!/usr/bin/env bash
#
# Phase 18 integration test: two consumer coasts on the same remote VM
# declaring the same canonical port (postgres:5432) with DIFFERENT
# local upstreams — one inline, one SSG-backed — must both work
# without collision.
#
# Pre-Phase-18 this scenario silently misrouted: coast B's
# reverse_forward_ports call would fail the bind and the "already
# bound, reusing existing" branch would send B's traffic through A's
# tunnel to the wrong upstream. Phase 18 allocates a distinct
# dynamic `remote_port` per forward so the two tunnels never compete.
#
# Test fixtures:
#   - coast-remote-shared-services: inline `[shared_services.postgres]`
#     on the consumer Coastfile.
#   - coast-ssg-consumer-remote: `[shared_services.postgres] from_group = true`
#     referencing the local SSG's postgres.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true" --filter "name=shell") 2>/dev/null || true
    "$COAST" remote rm test-remote 2>/dev/null || true
    "$COAST" ssg rm --with-data --force 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "ssh -N -R" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    docker rm -f coast-ssg 2>/dev/null || true
    rm -rf "$HOME/.coast/ssg"
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Phase 18: Mixed inline + SSG, no collision ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate
rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

start_daemon

# ============================================================
# Step 1: Stand up the SSG with postgres
# ============================================================

echo ""
echo "=== Step 1: Build + run the SSG ==="

cd "$PROJECTS_DIR/coast-ssg-minimal"
SSG_BUILD_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
assert_contains "$SSG_BUILD_OUT" "Build complete" "ssg build succeeds"
SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
SSG_DYNAMIC=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYNAMIC" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYNAMIC"

sleep 5

# ============================================================
# Step 2: Register the test remote
# ============================================================

echo ""
echo "=== Step 2: Register remote ==="
ADD_OUT=$("$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1)
assert_contains "$ADD_OUT" "added" "coast remote add succeeds"

# ============================================================
# Step 3: Build + run the inline consumer
# ============================================================

echo ""
echo "=== Step 3: Inline-postgres consumer on the remote ==="

cd "$PROJECTS_DIR/remote/coast-remote-shared-services"
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run inline-a --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "inline consumer run failed"
set -e
CLEANUP_INSTANCES+=("inline-a")
pass "inline consumer running"

sleep 3

# ============================================================
# Step 4: Build + run the SSG-backed consumer in parallel
# ============================================================

echo ""
echo "=== Step 4: SSG-backed consumer on the same remote ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-remote"
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
RUN_OUT=$("$COAST" run ssg-b --type remote 2>&1)
RUN_EXIT=$?
set -e
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "$RUN_OUT" | tail -20
    echo "--- coastd log tail ---"
    tail -40 /tmp/coastd-test.log 2>/dev/null || true
    fail "SSG-backed consumer run failed despite inline consumer already owning postgres:5432"
fi
CLEANUP_INSTANCES+=("ssg-b")
pass "SSG-backed consumer running alongside inline consumer"

sleep 3

# ============================================================
# Step 5: Two distinct remote reverse-tunnel ports, different
#         local upstreams
# ============================================================

echo ""
echo "=== Step 5: Distinct remote ports with different local upstreams ==="

ALL_TUNNELS=$(pgrep -af "ssh -N -R 0.0.0.0:" | \
    grep -oE '0\.0\.0\.0:[0-9]+:localhost:[0-9]+')
echo "  sshd reverse listeners:"
echo "$ALL_TUNNELS" | sed 's/^/    /'

REMOTE_PORTS=$(echo "$ALL_TUNNELS" | awk -F: '{print $2}' | sort -u)
REMOTE_COUNT=$(echo "$REMOTE_PORTS" | grep -cv '^$' || echo 0)

LOCAL_UPSTREAMS=$(echo "$ALL_TUNNELS" | awk -F: '{print $NF}' | sort -u)
LOCAL_COUNT=$(echo "$LOCAL_UPSTREAMS" | grep -cv '^$' || echo 0)

echo "  distinct remote ports = $REMOTE_COUNT"
echo "  distinct local upstreams = $LOCAL_COUNT"
echo "$LOCAL_UPSTREAMS" | sed 's/^/    local: /'

if [ "$REMOTE_COUNT" -lt 2 ]; then
    echo "--- daemon log ---"
    tail -60 /tmp/coastd-test.log 2>/dev/null || true
    fail "expected at least 2 distinct remote tunnel ports; got $REMOTE_COUNT"
fi

# The inline consumer's tunnel has local upstream = 5432 (canonical).
# The SSG consumer's tunnel has local upstream = $SSG_DYNAMIC.
# With the Phase 18 symmetric design both live side by side.
echo "$LOCAL_UPSTREAMS" | grep -q "^5432$" \
    || fail "expected inline consumer's tunnel to terminate at localhost:5432"
echo "$LOCAL_UPSTREAMS" | grep -q "^$SSG_DYNAMIC$" \
    || fail "expected SSG consumer's tunnel to terminate at localhost:$SSG_DYNAMIC"
pass "tunnels point at distinct upstreams (5432 inline, $SSG_DYNAMIC SSG)"

# ============================================================
# Step 6: Both instances show running
# ============================================================

echo ""
echo "=== Step 6: Both instances healthy ==="
LS_OUT=$("$COAST" ls 2>&1)
echo "$LS_OUT" | head -8

INLINE_RUNNING=$(echo "$LS_OUT" | grep -c "inline-a.*running" || echo 0)
SSG_RUNNING=$(echo "$LS_OUT" | grep -c "ssg-b.*running" || echo 0)
[ "$INLINE_RUNNING" -eq 1 ] || fail "inline-a should be running"
[ "$SSG_RUNNING" -eq 1 ] || fail "ssg-b should be running"
pass "both instances running on the same remote"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm inline-a 2>&1 >/dev/null || true
"$COAST" rm ssg-b 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  Phase 18 mixed inline+SSG no-collision OK"
echo "=========================================="
