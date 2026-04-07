#!/usr/bin/env bash
#
# Integration test: fresh coast run must sync gitignored generated files
# to the remote, and coast assign must preserve them.
#
# Verifies the rsync P filter fix: generated files that exist locally
# ARE transferred (not excluded by gitignore filtering), and --delete-after
# does NOT remove them (protected by P filter).

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
    cd "$PROJECTS_DIR/remote/coast-remote-basic" 2>/dev/null || true
    rm -rf generated
    git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
    git checkout -- . 2>/dev/null || true
    echo "Cleanup complete."
}
trap '_cleanup' EXIT

echo "=== Remote Run Syncs Generated Files Test ==="
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

cd "$PROJECTS_DIR/remote/coast-remote-basic"

# Create a gitignored generated file in the project
mkdir -p generated/proto
echo 'export const HEALTH = true;' > generated/proto/health_client.ts

pass "Created generated file (gitignored by .gitignore)"

start_daemon

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key 2>&1 >/dev/null
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

# ============================================================
# Test 1: Fresh coast run syncs generated files
# ============================================================

echo ""
echo "=== Test 1: Fresh run syncs generated files ==="

set +e
"$COAST" run dev-1 --type remote 2>&1 >/dev/null
[ $? -eq 0 ] || fail "Run failed"
set -e
CLEANUP_INSTANCES+=("dev-1")
pass "Remote instance running"

sleep 5

set +e
CHECK=$("$COAST" exec dev-1 -- cat /workspace/generated/proto/health_client.ts 2>&1)
set -e

if echo "$CHECK" | grep -q "HEALTH"; then
    pass "Generated file synced to remote on fresh run"
else
    echo "  content: $CHECK"
    fail "Generated file NOT synced -- rsync excluded it (gitignore filter too aggressive)"
fi

# ============================================================
# Test 2: Assign preserves generated files (P filter)
# ============================================================

echo ""
echo "=== Test 2: Assign preserves generated files ==="

mkdir -p .worktrees
git worktree add .worktrees/feature-sync-test feature-sync-test 2>/dev/null || true

"$COAST" assign dev-1 -w feature-sync-test 2>&1 >/dev/null
pass "Assigned to feature-sync-test"

sleep 3

set +e
CHECK2=$("$COAST" exec dev-1 -- cat /workspace/generated/proto/health_client.ts 2>&1)
set -e

if echo "$CHECK2" | grep -q "HEALTH"; then
    pass "Generated file preserved after assign (P filter working)"
else
    echo "  content: $CHECK2"
    fail "Generated file DELETED by assign -- P filter not protecting it"
fi

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="
"$COAST" rm dev-1 2>&1 >/dev/null || true
CLEANUP_INSTANCES=()
git worktree remove .worktrees/feature-sync-test 2>/dev/null || true
pass "Cleaned up"

echo ""
echo "=========================================="
echo "  All run-syncs-generated tests passed!"
echo "=========================================="
