#!/usr/bin/env bash
#
# Integration test: daemon cwd does NOT leak into /workspace.
#
# Reproduces probes 03+04 from aw-studio-app's bug report. The claim was
# that coastd captures its startup cwd and uses it as /host-project for
# all instances, so running `coast run` from project B while the daemon
# was started from project A would give /workspace = project A's files.
#
# This test starts coastd from coast-demo's directory, then cd's to
# coast-benchmark and runs an instance there. If the daemon's cwd leaks,
# /workspace will contain coast-demo's files (docker-compose.yml with
# postgres/redis, server.js with pg/redis imports). If cwd is properly
# isolated, /workspace will contain coast-benchmark's files (simple
# server.js with no dependencies).
#
# Uses coast-demo (compose + pg + redis) and coast-benchmark (zero deps).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_daemon_cwd_isolation.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

COAST_BENCHMARK_COUNT=1 "$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# ============================================================
# Test 1: Start daemon from coast-demo's directory, then build
# ============================================================

echo ""
echo "=== Test 1: Start daemon from coast-demo, build both projects ==="

cd "$PROJECTS_DIR/coast-demo"
start_daemon
pass "Daemon started from coast-demo directory ($(pwd))"

DEMO_BUILD=$("$COAST" build 2>&1)
assert_contains "$DEMO_BUILD" "coast-demo" "coast-demo build succeeds"

cd "$PROJECTS_DIR/coast-benchmark"
BENCH_BUILD=$("$COAST" build 2>&1)
assert_contains "$BENCH_BUILD" "coast-benchmark" "coast-benchmark build succeeds"

# ============================================================
# Test 2: Run coast-benchmark instance (daemon cwd = coast-demo)
# ============================================================

echo ""
echo "=== Test 2: Run coast-benchmark instance ==="

cd "$PROJECTS_DIR/coast-benchmark"
RUN_OUT=$("$COAST" run cwd-test 2>&1)
CLEANUP_INSTANCES+=("cwd-test")
assert_contains "$RUN_OUT" "Created coast instance" "coast run cwd-test succeeds"

DYN_PORT=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$DYN_PORT" ] || fail "Could not extract dynamic port"
pass "Dynamic port: $DYN_PORT"

wait_for_healthy "$DYN_PORT" 60 || fail "cwd-test did not become healthy"
pass "cwd-test is healthy"

# ============================================================
# Test 3: Verify /workspace contains coast-benchmark files
# ============================================================

echo ""
echo "=== Test 3: Verify /workspace is coast-benchmark (not coast-demo) ==="

RESP=$(curl -s "http://localhost:${DYN_PORT}/")
assert_contains "$RESP" "coast-benchmark" "response identifies as coast-benchmark"
assert_not_contains "$RESP" "coast-demo" "response is NOT from coast-demo"

WORKSPACE_FILES=$("$COAST" exec cwd-test -- ls /workspace 2>&1)
assert_contains "$WORKSPACE_FILES" "Coastfile" "/workspace has Coastfile"
assert_contains "$WORKSPACE_FILES" "server.js" "/workspace has server.js"
assert_not_contains "$WORKSPACE_FILES" "package.json" "/workspace does NOT have package.json (coast-demo artifact)"

COASTFILE_CONTENT=$("$COAST" exec cwd-test -- cat /workspace/Coastfile 2>&1)
assert_contains "$COASTFILE_CONTENT" "coast-benchmark" "Coastfile identifies as coast-benchmark"
assert_not_contains "$COASTFILE_CONTENT" "coast-demo" "Coastfile is NOT coast-demo's"

# ============================================================
# Test 4: Marker file round-trip lands at coast-benchmark on host
# ============================================================

echo ""
echo "=== Test 4: Marker file round-trip ==="

"$COAST" exec cwd-test -- sh -c 'echo BENCHMARK_MARKER > /workspace/MARKER.txt'

[ -f "$PROJECTS_DIR/coast-benchmark/MARKER.txt" ] || fail "MARKER.txt not found at coast-benchmark on host"
MARKER_CONTENT=$(cat "$PROJECTS_DIR/coast-benchmark/MARKER.txt")
assert_eq "$MARKER_CONTENT" "BENCHMARK_MARKER" "marker file content correct at coast-benchmark"

if [ -f "$PROJECTS_DIR/coast-demo/MARKER.txt" ]; then
    fail "MARKER.txt found at coast-demo — daemon cwd leaked into /workspace!"
fi
pass "marker file landed at coast-benchmark, not coast-demo"

rm -f "$PROJECTS_DIR/coast-benchmark/MARKER.txt"

# ============================================================
# Test 5: Also verify coast-demo still works from same daemon
# ============================================================

echo ""
echo "=== Test 5: coast-demo instance on same daemon ==="

cd "$PROJECTS_DIR/coast-demo"
DEMO_RUN=$("$COAST" run demo-verify 2>&1)
CLEANUP_INSTANCES+=("demo-verify")
assert_contains "$DEMO_RUN" "Created coast instance" "coast-demo instance started"

DEMO_PORT=$(extract_dynamic_port "$DEMO_RUN" "app")
[ -n "$DEMO_PORT" ] || fail "Could not extract coast-demo dynamic port"

wait_for_healthy "$DEMO_PORT" 60 || fail "coast-demo did not become healthy"
pass "coast-demo is healthy"

DEMO_RESP=$(curl -s "http://localhost:${DEMO_PORT}/")
assert_contains "$DEMO_RESP" "Hello from Coast!" "coast-demo responds correctly"
assert_contains "$DEMO_RESP" '"branch":"main"' "coast-demo is on main branch"

DEMO_COASTFILE=$("$COAST" exec demo-verify -- cat /workspace/Coastfile 2>&1)
assert_contains "$DEMO_COASTFILE" "coast-demo" "coast-demo /workspace has correct Coastfile"
assert_not_contains "$DEMO_COASTFILE" "coast-benchmark" "coast-demo Coastfile is NOT coast-benchmark's"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

cd "$PROJECTS_DIR/coast-benchmark"
"$COAST" rm cwd-test 2>&1 | grep -q "Removed" || fail "coast rm cwd-test failed"
pass "cwd-test removed"

cd "$PROJECTS_DIR/coast-demo"
"$COAST" rm demo-verify 2>&1 | grep -q "Removed" || fail "coast rm demo-verify failed"
pass "demo-verify removed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  ALL DAEMON CWD ISOLATION TESTS PASSED"
echo "==========================================="
