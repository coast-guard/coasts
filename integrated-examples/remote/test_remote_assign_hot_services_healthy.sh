#!/usr/bin/env bash
#
# Integration test: remote assign with "hot" services must ensure all
# compose services are running after assign completes, even if a service
# crashed (e.g., from hot-reload failure during file sync).
#
# The real-world scenario: air (Go hot-reloader) detects the rsync file
# storm during assign, kills the running server, tries to rebuild mid-
# transfer, fails, and the service stays dead. The "hot" assign strategy
# skips compose restart entirely, so nothing recovers it.
#
# We simulate this by killing a service inside the DinD before assign,
# then verifying the assign ensures it's running afterward.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/helpers.sh"

_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    "$COAST" remote rm test-remote 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "mutagen" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Assign Hot Services Recovery Test ==="
echo ""
preflight_checks
echo ""
echo "=== Setup ==="
clean_slate

eval "$(ssh-agent -s)"
export SSH_AUTH_SOCK
setup_localhost_ssh
ssh-add ~/.ssh/coast_test_key 2>&1 || true
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

cd "$PROJECTS_DIR/remote/coast-remote-compose"

# Add [assign] with hot strategy
cat >> Coastfile <<'EOF'

[assign]
default = "none"

[assign.services]
app = "hot"
fragile-cache = "hot"
EOF

git checkout -b feature-hot-test 2>/dev/null || git checkout feature-hot-test 2>/dev/null || true
echo "# hot test" >> docker-compose.yml
git add -A && git commit -m "feature: hot test" 2>/dev/null || true
git checkout main 2>/dev/null || true

mkdir -p .worktrees
git worktree add .worktrees/feature-hot-test feature-hot-test 2>/dev/null || true

start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 8

# ============================================================
# Test 1: Baseline -- both services running
# ============================================================

echo ""
echo "=== Test 1: Baseline ==="

set +e
PS_OUT=$("$COAST" ps dev-1 2>&1)
set -e
echo "$PS_OUT" | head -6

if echo "$PS_OUT" | grep -q "fragile-cache.*running"; then
    pass "fragile-cache running at baseline"
else
    fail "fragile-cache not running at baseline"
fi

# ============================================================
# Test 2: Kill fragile-cache (simulates hot-reload crash)
# ============================================================

echo ""
echo "=== Test 2: Kill fragile-cache (simulate hot-reload crash) ==="

DIND_NAME=$(docker ps --format '{{.Names}}' | grep "coast-remote-compose.*coasts.*dev-1" | grep -v shell | head -1)
[ -n "$DIND_NAME" ] || fail "DinD container not found"

INNER=$(docker exec "$DIND_NAME" docker ps --format '{{.Names}}' 2>/dev/null | grep "fragile" | head -1)
[ -n "$INNER" ] || fail "fragile-cache not found inside DinD"

docker exec "$DIND_NAME" docker rm -f "$INNER" 2>/dev/null
pass "Killed fragile-cache (simulating hot-reload crash)"

sleep 2

set +e
PS_DOWN=$("$COAST" ps dev-1 2>&1)
set -e
if echo "$PS_DOWN" | grep -q "fragile-cache.*running"; then
    fail "fragile-cache should be down after kill"
else
    pass "fragile-cache confirmed down"
fi

# ============================================================
# Test 3: Assign with hot strategy -- should recover dead service
# ============================================================

echo ""
echo "=== Test 3: Assign to feature-hot-test (should recover dead service) ==="

"$COAST" assign dev-1 -w feature-hot-test 2>&1 >/dev/null
pass "Assign completed"

sleep 2

set +e
PS_AFTER=$("$COAST" ps dev-1 2>&1)
set -e
echo "$PS_AFTER" | head -6

if echo "$PS_AFTER" | grep -q "fragile-cache.*running"; then
    pass "fragile-cache recovered after hot assign"
else
    fail "fragile-cache NOT recovered -- hot assign did not ensure service health"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-hot-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All hot-assign-recovery tests passed!"
echo "=========================================="
