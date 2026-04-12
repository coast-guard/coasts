#!/usr/bin/env bash
#
# Integration test: coast assign with external worktree directories.
#
# Reproduces probe 05 from aw-studio-app's bug report. The claim was
# that `coast assign -w <branch>` reports success (including
# "[4/7] Switching worktree... ok") but /workspace does NOT actually
# remount to the worktree — files, git branch, and marker writes all
# still point at the project root.
#
# The existing test_assign.sh covers internal worktrees (the default
# .worktrees/ inside the project). This test specifically targets
# external worktrees: worktree_dir entries that point to a SIBLING
# directory outside the project root, which is the layout aw-studio-app
# uses ({repo_parent}/{project}-worktrees/).
#
# Creates a throwaway project inline with:
#   - worktree_dir pointing to a sibling directory
#   - Two branches (main, feature/external) with distinct server.js
#   - Bare service (no compose, no Docker build needed)
#
# Tests:
#   1. Baseline: coast run serves main content
#   2. coast assign -w feature/external remounts /workspace
#   3. Marker file written inside coast lands at external worktree on host
#   4. git branch inside /workspace matches assigned branch
#   5. Unassign returns to project root
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_external_worktree_assign.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Custom cleanup: we create a throwaway project that needs removal
PROJECT_DIR=""
EXT_WT_DIR=""

_custom_cleanup() {
    echo ""
    echo "--- Cleaning up ---"

    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1

    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    if [ -n "$PROJECT_DIR" ] && [ -d "$PROJECT_DIR" ]; then
        # Remove worktrees before deleting the repo
        git -C "$PROJECT_DIR" worktree list 2>/dev/null | grep -v "$PROJECT_DIR " | awk '{print $1}' | while read -r wt; do
            git -C "$PROJECT_DIR" worktree remove --force "$wt" 2>/dev/null || true
        done
        rm -rf "$PROJECT_DIR"
    fi
    [ -n "$EXT_WT_DIR" ] && rm -rf "$EXT_WT_DIR"

    echo "Cleanup complete."
}
trap '_custom_cleanup' EXIT

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup: create throwaway project with external worktree_dir ==="

clean_slate

PROJECT_DIR=$(mktemp -d "${TMPDIR:-/tmp}/coast-ext-wt-XXXXXX")
EXT_WT_DIR="${PROJECT_DIR}-worktrees"

cd "$PROJECT_DIR"

# Main branch server: identifies as "main"
cat > server.js << 'MAIN_EOF'
const http = require("http");
const PORT = process.env.PORT || 41000;

const server = http.createServer((req, res) => {
  const json = (data) => {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };
  if (req.url === "/health") return json({ status: "ok" });
  return json({ service: "ext-wt-test", branch: "main", version: "main" });
});

server.listen(PORT, () => console.log(`Server (main) on :${PORT}`));
MAIN_EOF

cat > Coastfile << COASTFILE_EOF
[coast]
name = "ext-wt-test"
runtime = "dind"

[coast.setup]
packages = ["nodejs", "npm"]

worktree_dir = ["${EXT_WT_DIR}"]

[services.web]
command = "node server.js"
port = 41000
restart = "on-failure"

[ports]
web = 41000
COASTFILE_EOF

git init -b main
git config user.name "Coast Dev"
git config user.email "dev@coasts.dev"
git add -A
git commit -m "initial: main branch server"

# Feature branch with distinct server response
git checkout -b feature/external

cat > server.js << 'FEATURE_EOF'
const http = require("http");
const PORT = process.env.PORT || 41000;

const server = http.createServer((req, res) => {
  const json = (data) => {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(data));
  };
  if (req.url === "/health") return json({ status: "ok" });
  return json({ service: "ext-wt-test", branch: "feature/external", version: "v2-external" });
});

server.listen(PORT, () => console.log(`Server (feature/external) on :${PORT}`));
FEATURE_EOF

git add server.js
git commit -m "feature/external: distinct server response"
git checkout main

# Create the external worktree at sibling directory
mkdir -p "$EXT_WT_DIR"
git worktree add "$EXT_WT_DIR/external" feature/external

pass "Project created at $PROJECT_DIR"
pass "External worktree at $EXT_WT_DIR/external"

echo "  Worktrees:"
git worktree list

# --- Start daemon and build ---

start_daemon

echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# ============================================================
# Test 1: Baseline — coast run serves main
# ============================================================

echo ""
echo "=== Test 1: coast run (baseline, expect main) ==="

RUN_OUT=$("$COAST" run slot-1 2>&1)
CLEANUP_INSTANCES+=("slot-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run slot-1 succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "web")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
pass "Dynamic port: $DYN_PORT"

sleep 5

RESP=$(curl -sf "http://localhost:${DYN_PORT}/" 2>&1 || echo '{"error":"no response"}')
assert_contains "$RESP" '"branch":"main"' "baseline serves main branch"
assert_contains "$RESP" '"version":"main"' "baseline version is main"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "main" "/workspace is on main branch"

# ============================================================
# Test 2: Assign to feature/external (external worktree)
# ============================================================

echo ""
echo "=== Test 2: coast assign -w feature/external ==="

ASSIGN_OUT=$("$COAST" assign slot-1 --worktree feature/external 2>&1)
assert_contains "$ASSIGN_OUT" "Assigned worktree" "assign to feature/external succeeded"
pass "assign reported success"

sleep 5

# ============================================================
# Test 3: Verify /workspace remounted to external worktree
# ============================================================

echo ""
echo "=== Test 3: Verify /workspace is on feature/external ==="

RESP_EXT=$(curl -sf "http://localhost:${DYN_PORT}/" 2>&1 || echo '{"error":"no response"}')
assert_contains "$RESP_EXT" '"branch":"feature/external"' "server reports feature/external branch"
assert_contains "$RESP_EXT" '"version":"v2-external"' "server reports v2-external version"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "feature/external" "/workspace git branch is feature/external"

# ============================================================
# Test 4: Marker file persists in /workspace after write
# ============================================================

echo ""
echo "=== Test 4: Marker file write inside coast ==="

"$COAST" exec slot-1 -- sh -c 'echo EXT_WT_MARKER > /workspace/MARKER.txt'

MARKER_READ=$("$COAST" exec slot-1 -- cat /workspace/MARKER.txt 2>&1)
assert_eq "$MARKER_READ" "EXT_WT_MARKER" "marker file readable inside coast /workspace"

MARKER_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$MARKER_BRANCH" "feature/external" "/workspace still on feature/external after write"

"$COAST" exec slot-1 -- rm -f /workspace/MARKER.txt 2>/dev/null || true

# ============================================================
# Test 5: Unassign returns to project root
# ============================================================

echo ""
echo "=== Test 5: coast unassign returns to main ==="

UNASSIGN_OUT=$("$COAST" unassign slot-1 2>&1)
pass "unassign completed"

sleep 8

RESP_MAIN=$(curl -sf "http://localhost:${DYN_PORT}/" 2>&1 || echo '{"error":"no response"}')
assert_contains "$RESP_MAIN" '"branch":"main"' "server returns main after unassign"
assert_contains "$RESP_MAIN" '"version":"main"' "version is main after unassign"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "main" "/workspace reverted to main after unassign"

# ============================================================
# Test 6: Re-assign works (bidirectional)
# ============================================================

echo ""
echo "=== Test 6: Re-assign to feature/external (bidirectional) ==="

ASSIGN2_OUT=$("$COAST" assign slot-1 --worktree feature/external 2>&1)
assert_contains "$ASSIGN2_OUT" "Assigned worktree" "re-assign to feature/external succeeded"

sleep 5

RESP_RE=$(curl -sf "http://localhost:${DYN_PORT}/" 2>&1 || echo '{"error":"no response"}')
assert_contains "$RESP_RE" '"branch":"feature/external"' "re-assign serves feature/external"

WORKSPACE_BRANCH=$("$COAST" exec slot-1 -- git -C /workspace rev-parse --abbrev-ref HEAD 2>&1)
assert_eq "$WORKSPACE_BRANCH" "feature/external" "/workspace is feature/external after re-assign"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm slot-1 2>&1 | grep -q "Removed" || fail "coast rm slot-1 failed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  ALL EXTERNAL WORKTREE ASSIGN TESTS PASSED"
echo "==========================================="
