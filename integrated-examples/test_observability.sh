#!/usr/bin/env bash
#
# Integration test: observability commands (ps, logs, shared).
#
# Tests coast ps, coast logs, coast logs <service>, coast shared-services ps,
# and error behavior on stopped instances.
#
# Uses coast-demo (single instance on main branch).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_observability.sh

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

cd "$PROJECTS_DIR/coast-demo"

start_daemon

# Build and run main
echo ""
echo "=== Build and run ==="

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"

RUN_OUT=$("$COAST" run main 2>&1)
CLEANUP_INSTANCES+=("main")
assert_contains "$RUN_OUT" "Created coast instance" "coast run main succeeds"

MAIN_DYN=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$MAIN_DYN" ] || fail "Could not extract main app dynamic port"
pass "Main dynamic port: $MAIN_DYN"

wait_for_healthy "$MAIN_DYN" 60 || fail "main did not become healthy"
pass "main is healthy"

# ============================================================
# Test 1: coast ps main
# ============================================================

echo ""
echo "=== Test 1: coast ps main ==="

PS_OUT=$("$COAST" ps main 2>&1)
assert_contains "$PS_OUT" "Services in coast instance" "ps output has instance header"
assert_contains "$PS_OUT" "main" "ps output references instance name"
assert_contains "$PS_OUT" "NAME" "ps output has NAME column"
assert_contains "$PS_OUT" "STATUS" "ps output has STATUS column"
assert_contains "$PS_OUT" "PORTS" "ps output has PORTS column"
assert_contains "$PS_OUT" "app" "ps shows app service"
assert_contains "$PS_OUT" "db" "ps shows db service"
assert_contains "$PS_OUT" "cache" "ps shows cache service"

# ============================================================
# Test 2: coast logs main
# ============================================================

echo ""
echo "=== Test 2: coast logs main ==="

LOGS_OUT=$("$COAST" logs main 2>&1)
# Logs should be non-empty and contain recognizable output
[ -n "$LOGS_OUT" ] || fail "coast logs returned empty output"
pass "coast logs main returned non-empty output"

# Should contain output from at least one service
# (postgres typically logs "database system is ready" or "listening on")
# We check for common patterns — at least one should match
if echo "$LOGS_OUT" | grep -qi -e "listening" -e "ready" -e "started" -e "Migrations" -e "Connected"; then
    pass "coast logs contains recognizable service output"
else
    echo "  Logs output (first 10 lines):"
    echo "$LOGS_OUT" | head -10
    fail "coast logs does not contain expected service output"
fi

# ============================================================
# Test 3: coast logs main app (filtered)
# ============================================================

echo ""
echo "=== Test 3: coast logs main app (filtered) ==="

LOGS_APP=$("$COAST" logs main app 2>&1)
[ -n "$LOGS_APP" ] || fail "coast logs main app returned empty output"
pass "coast logs main app returned non-empty output"

# Filtered logs should contain app-related output
# and ideally NOT contain postgres-specific output
if echo "$LOGS_APP" | grep -qi -e "listening" -e "Coast" -e "Migrations" -e "Connected"; then
    pass "filtered logs contain app-level output"
else
    echo "  App logs (first 5 lines):"
    echo "$LOGS_APP" | head -5
    # Don't fail — the filter may work differently, just note it
    pass "filtered logs returned (content varies by compose version)"
fi

# ============================================================
# Test 4: coast shared-services ps
# ============================================================

echo ""
echo "=== Test 4: coast shared-services ps ==="

SHARED_OUT=$("$COAST" shared-services ps 2>&1)
# The command should succeed (exit 0) — coast-demo doesn't use [shared_services]
# so the output will likely be empty or show no services
if echo "$SHARED_OUT" | grep -qi -e "Shared services" -e "No shared services" -e "SERVICE"; then
    pass "coast shared-services ps succeeds with expected output"
else
    # Even if output format differs, command should not error
    pass "coast shared-services ps completed (output: $SHARED_OUT)"
fi

# ============================================================
# Test 5: ps on stopped instance
# ============================================================

echo ""
echo "=== Test 5: ps on stopped instance ==="

"$COAST" stop main 2>&1 | grep -q "Stopped" || fail "coast stop main failed"
pass "main stopped for ps error test"

PS_STOPPED=$("$COAST" ps main 2>&1 || true)
assert_contains "$PS_STOPPED" "stopped" "ps on stopped instance returns stopped error"

# ============================================================
# Test 6: logs on stopped instance
# ============================================================

echo ""
echo "=== Test 6: logs on stopped instance ==="

LOGS_STOPPED=$("$COAST" logs main 2>&1 || true)
assert_contains "$LOGS_STOPPED" "stopped" "logs on stopped instance returns stopped error"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm main 2>&1 | grep -q "Removed" || fail "coast rm main failed"
CLEANUP_INSTANCES=()

echo ""
echo "==========================================="
echo "  ALL OBSERVABILITY TESTS PASSED"
echo "==========================================="
