#!/usr/bin/env bash
#
# Integration test for MCP server declarations in the Coastfile.
#
# Verifies that:
#   1. coast build succeeds with [mcp.*] and [mcp_clients.*] sections
#   2. coast run creates an instance with MCP installed at /mcp/<name>/
#   3. Internal MCP source + install pipeline works (/mcp/echo/ has node_modules)
#   4. Internal MCP server is executable inside the coast
#   5. Claude Code MCP client config (~/.claude/mcp_servers.json) is generated
#   6. coast mcp ls shows servers with correct types and statuses
#   7. coast mcp locations shows client config paths
#   8. Invalid MCP config (host-proxied with install) is rejected at build time
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_mcp.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

start_daemon

# ============================================================
# Phase 1: Build with MCP config
# ============================================================

echo ""
echo "=== Phase 1: Build with MCP config ==="

cd "$PROJECTS_DIR/coast-mcp"

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "coast-mcp" "coast build references project name"
pass "coast build succeeded with MCP sections"

# ============================================================
# Phase 2: Run instance with MCP
# ============================================================

echo ""
echo "=== Phase 2: Run instance with MCP ==="

RUN_OUT=$("$COAST" run mcp-1 2>&1)
CLEANUP_INSTANCES+=("mcp-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run mcp-1 succeeds"
pass "Instance mcp-1 created"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
if [ -z "$DYN_PORT" ]; then
    fail "Could not extract dynamic port for app service"
fi
pass "Dynamic port for app: $DYN_PORT"

if wait_for_healthy "$DYN_PORT" 60; then
    pass "Inner app service is healthy"
else
    fail "Inner app service did not become healthy within 60s"
fi

# ============================================================
# Phase 3: Verify internal MCP installed
# ============================================================

echo ""
echo "=== Phase 3: Verify internal MCP installed at /mcp/echo/ ==="

MCP_LS=$("$COAST" exec mcp-1 -- ls /mcp/echo/ 2>&1 || true)
assert_contains "$MCP_LS" "server.js" "MCP echo server.js present at /mcp/echo/"
assert_contains "$MCP_LS" "node_modules" "MCP echo node_modules installed at /mcp/echo/"

# ============================================================
# Phase 4: Verify internal MCP is runnable
# ============================================================

echo ""
echo "=== Phase 4: Verify internal MCP is executable ==="

MCP_VERSION=$("$COAST" exec mcp-1 -- node /mcp/echo/server.js --version 2>&1 || true)
assert_contains "$MCP_VERSION" "mcp-echo 1.0.0" "MCP echo server runs and reports version"

# ============================================================
# Phase 5: Verify MCP client config was generated
# ============================================================

echo ""
echo "=== Phase 5: Verify Claude Code MCP config written ==="

MCP_CONFIG=$("$COAST" exec mcp-1 -- cat /root/.claude/mcp_servers.json 2>&1 || true)
assert_contains "$MCP_CONFIG" "mcpServers" "Claude Code config contains mcpServers key"
assert_contains "$MCP_CONFIG" "echo" "Claude Code config includes echo MCP server"
assert_contains "$MCP_CONFIG" "/mcp/echo/" "Claude Code config has correct cwd for internal MCP"
pass "Claude Code MCP client config correctly generated"

# ============================================================
# Phase 6: Verify coast mcp ls shows installed status
# ============================================================

echo ""
echo "=== Phase 6: Verify coast mcp ls ==="

MCP_LS_OUT=$("$COAST" mcp mcp-1 ls 2>&1 || true)
assert_contains "$MCP_LS_OUT" "echo" "mcp ls shows echo server"
assert_contains "$MCP_LS_OUT" "internal" "mcp ls shows internal type"
assert_contains "$MCP_LS_OUT" "host-echo" "mcp ls shows host-echo server"
pass "coast mcp ls output correct"

# ============================================================
# Phase 7: Verify coast mcp locations
# ============================================================

echo ""
echo "=== Phase 7: Verify coast mcp locations ==="

MCP_LOC_OUT=$("$COAST" mcp mcp-1 locations 2>&1 || true)
assert_contains "$MCP_LOC_OUT" "claude-code" "mcp locations shows claude-code client"
assert_contains "$MCP_LOC_OUT" "/root/.claude/mcp_servers.json" "mcp locations shows correct path"
pass "coast mcp locations output correct"

# ============================================================
# Phase 8: Verify host-proxied MCP rejected with install
# ============================================================

echo ""
echo "=== Phase 5: Verify bad MCP config rejected ==="

BAD_COASTFILE=$(mktemp)
cat > "$BAD_COASTFILE" << 'EOF'
[coast]
name = "coast-mcp-bad"
compose = "./docker-compose.yml"
runtime = "dind"

[ports]
app = 49500

[mcp.bad-server]
proxy = "host"
install = ["npm install something"]
EOF

# Try building with the bad coastfile
BAD_BUILD=$("$COAST" build -f "$BAD_COASTFILE" 2>&1 || true)
assert_contains "$BAD_BUILD" "install" "Error message mentions 'install' field"
rm -f "$BAD_COASTFILE"
pass "Bad MCP config correctly rejected"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm mcp-1 2>&1 || true
CLEANUP_INSTANCES=()

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL MCP TESTS PASSED"
echo "==========================================="
