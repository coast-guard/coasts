#!/usr/bin/env bash
#
# Integration test: HMR (hot-reload) through host-bound worktree.
#
# Verifies that file changes made inside a coast instance's /workspace are
# immediately visible to the running compose service AND to the host worktree.
# In the worktree architecture, /workspace is a bind mount to the host
# worktree (or project root for main), so HMR works naturally.
#
# Uses coast-hmr — a zero-dep Node.js server that re-reads data.json on
# every request, with a volume mount instead of COPY.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_hmr.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

# Only set up coast-hmr (skip others for speed)
setup_coast_hmr() {
    local dir="$PROJECTS_DIR/coast-hmr"
    rm -rf "$dir/.git" "$dir/docker-compose.override.yml"
    # Reset data.json to initial state (previous test run may have modified it)
    cat > "$dir/data.json" <<'DATAJSON'
{"message": "initial", "version": 1}
DATAJSON
    cd "$dir"
    git init -b main
    git add -A
    git commit -m "initial commit: HMR test project"
}
setup_coast_hmr
pass "coast-hmr initialized"

cd "$PROJECTS_DIR/coast-hmr"

start_daemon

# Build
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"

# Run instance
RUN_OUT=$("$COAST" run hmr-slot 2>&1)
CLEANUP_INSTANCES+=("hmr-slot")
assert_contains "$RUN_OUT" "Created coast instance" "coast run hmr-slot succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
wait_for_healthy "$DYN_PORT" 60 || fail "hmr-slot did not become healthy"
pass "hmr-slot healthy on port $DYN_PORT"

# ============================================================
# Test A: Initial response matches data.json
# ============================================================

echo ""
echo "=== Test A: Initial response matches data.json ==="

RESP=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" '"message":"initial"' "initial response contains message:initial"
assert_contains "$RESP" '"version":1' "initial response contains version:1"
pass "initial data.json served correctly"

# ============================================================
# Test B: Write inside DinD /workspace, compose service picks up change
# ============================================================

echo ""
echo "=== Test B: DinD write visible to compose service (HMR) ==="

"$COAST" exec hmr-slot -- sh -c 'printf '"'"'{"message":"updated","version":2}'"'"' > /workspace/data.json' 2>&1

sleep 1

RESP2=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP2" '"message":"updated"' "response reflects updated data.json"
assert_contains "$RESP2" '"version":2' "response reflects version:2"
pass "HMR works — DinD write visible to compose service"

# ============================================================
# Test C: Host sees the change (host-bound via bind mount)
# ============================================================

echo ""
echo "=== Test C: Host sees the DinD edit ==="

HOST_DATA=$(cat data.json)
assert_contains "$HOST_DATA" '"message":"updated"' "host data.json reflects DinD edit"
assert_contains "$HOST_DATA" '"version":2' "host data.json shows version 2"
pass "host-bound sync confirmed — host sees DinD edit"

# ============================================================
# Test D: Host edit visible inside DinD and to compose service
# ============================================================

echo ""
echo "=== Test D: Host edit visible inside DinD ==="

printf '{"message":"host-edit","version":3}' > data.json
sleep 1

RESP3=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP3" '"message":"host-edit"' "response reflects host edit"
assert_contains "$RESP3" '"version":3' "response reflects version:3"
pass "bidirectional HMR — host edit picked up by compose service"

# ============================================================
# Test E: Stop + start preserves HMR capability
# ============================================================

echo ""
echo "=== Test E: Stop + start preserves HMR ==="

STOP_OUT=$("$COAST" stop hmr-slot 2>&1)
assert_contains "$STOP_OUT" "Stopped" "coast stop succeeds"

START_OUT=$("$COAST" start hmr-slot 2>&1)
[ $? -eq 0 ] || fail "coast start failed"

wait_for_healthy "$DYN_PORT" 60 || fail "hmr-slot not healthy after start"
pass "hmr-slot healthy after stop+start"

# After restart, data.json still reflects the host edit (persistent bind mount)
RESP4=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP4" '"message":"host-edit"' "data.json persists across restart"
assert_contains "$RESP4" '"version":3' "version persists across restart"

# Make another edit to confirm HMR still works
"$COAST" exec hmr-slot -- sh -c 'printf '"'"'{"message":"restarted","version":4}'"'"' > /workspace/data.json' 2>&1
sleep 1

RESP5=$(curl -sf "http://localhost:${DYN_PORT}/")
assert_contains "$RESP5" '"message":"restarted"' "response reflects post-restart write"
assert_contains "$RESP5" '"version":4' "response reflects version:4"
pass "HMR works after stop+start cycle"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

git checkout main 2>/dev/null || true
git checkout -- . 2>/dev/null || true
git clean -fd 2>/dev/null || true

"$COAST" rm hmr-slot 2>&1 | grep -q "Removed" || fail "coast rm hmr-slot failed"
CLEANUP_INSTANCES=()

echo ""
echo "==========================================="
echo "  ALL HMR TESTS PASSED"
echo "==========================================="
