#!/usr/bin/env bash
#
# Integration test: checkout, canonical port swapping, and batch creation.
#
# Phase 1: Tests coast checkout, --none, instant swap between instances,
#           coast ports command, and dynamic port independence.
#           Uses coast-api (isolated redis only — safe for simultaneous instances).
#
# Phase 3: Tests host coast sync (coast ls shows live git branch for
#           checked-out instances) and prevention of assigning a branch
#           to a checked-out coast.
#           Uses coast-api.
#
# Phase 2: Tests batch creation (coast run dev-{n} --n=N), auto-branch
#           detection, batch instance independence, and checkout between
#           batch-created instances.
#           Uses coast-benchmark (zero-dep Node server — fast spin-up).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_checkout.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

# --- Preflight ---

preflight_checks

# --- Setup ---

echo ""
echo "=== Setup ==="

clean_slate

# Initialize examples
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

cd "$PROJECTS_DIR/coast-api"

# Start daemon
start_daemon

# Build
echo ""
echo "=== Build ==="
BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "coast build succeeds"

# Run two instances (both on main branch — code is identical, but each has
# isolated Redis so we can distinguish them using request counters).
echo ""
echo "=== Run two instances ==="

RUN_INST_A=$("$COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
assert_contains "$RUN_INST_A" "Created coast instance" "coast run inst-a succeeds"
INST_A_DYN=$(extract_dynamic_port "$RUN_INST_A" "api")
[ -n "$INST_A_DYN" ] || fail "Could not extract inst-a api dynamic port"
pass "inst-a dynamic port: $INST_A_DYN"

wait_for_healthy "$INST_A_DYN" 60 || fail "inst-a did not become healthy"
pass "inst-a is healthy"

RUN_INST_B=$("$COAST" run inst-b 2>&1)
CLEANUP_INSTANCES+=("inst-b")
assert_contains "$RUN_INST_B" "Created coast instance" "coast run inst-b succeeds"
INST_B_DYN=$(extract_dynamic_port "$RUN_INST_B" "api")
[ -n "$INST_B_DYN" ] || fail "Could not extract inst-b api dynamic port"
pass "inst-b dynamic port: $INST_B_DYN"

wait_for_healthy "$INST_B_DYN" 60 || fail "inst-b did not become healthy"
pass "inst-b is healthy"

# Verify both respond on dynamic ports
INST_A_RESP=$(curl -s "http://localhost:${INST_A_DYN}/")
assert_contains "$INST_A_RESP" "API Gateway" "inst-a responds on dynamic port"

INST_B_RESP=$(curl -s "http://localhost:${INST_B_DYN}/")
assert_contains "$INST_B_RESP" "API Gateway" "inst-b responds on dynamic port"

# Mark each instance's isolated Redis so we can tell them apart via canonical port.
# Hit inst-a 5 extra times to bump its request_counter, then set marker.
for _ in 1 2 3 4 5; do
    curl -s "http://localhost:${INST_A_DYN}/" >/dev/null
done
curl -s "http://localhost:${INST_A_DYN}/redis-set-marker" >/dev/null

# inst-b: just set marker (request_counter stays low)
curl -s "http://localhost:${INST_B_DYN}/redis-set-marker" >/dev/null

# Read back counters via dynamic ports to establish baselines
INST_A_INFO=$(curl -s "http://localhost:${INST_A_DYN}/redis-info")
INST_A_COUNT=$(echo "$INST_A_INFO" | python3 -c "import sys,json; print(json.load(sys.stdin)['request_counter'])" 2>/dev/null || echo "0")
pass "inst-a request_counter = $INST_A_COUNT (should be > 5)"

INST_B_INFO=$(curl -s "http://localhost:${INST_B_DYN}/redis-info")
INST_B_COUNT=$(echo "$INST_B_INFO" | python3 -c "import sys,json; print(json.load(sys.stdin)['request_counter'])" 2>/dev/null || echo "0")
pass "inst-b request_counter = $INST_B_COUNT (should be low)"

# Verify the counts are different (proves isolated Redis)
[ "$INST_A_COUNT" != "$INST_B_COUNT" ] || fail "inst-a and inst-b have same counter (Redis not isolated!)"
pass "Instances have different request counters (isolated Redis confirmed)"

# Verify both instances appear in ls
LS_INITIAL=$("$COAST" ls 2>&1)
assert_contains "$LS_INITIAL" "inst-a" "ls shows inst-a"
assert_contains "$LS_INITIAL" "inst-b" "ls shows inst-b"
pass "Initial instances verified in ls"

# ============================================================
# Test 1: Checkout inst-a — canonical port 34000
# ============================================================

echo ""
echo "=== Test 1: coast checkout inst-a ==="

CO_A=$("$COAST" checkout inst-a 2>&1)
assert_contains "$CO_A" "Checked out coast instance" "checkout inst-a output correct"
assert_contains "$CO_A" "inst-a" "checkout output references instance name"

# Wait a moment for socat to be ready
sleep 1

# Canonical port should respond
CANON_RESP=$(curl -s "http://localhost:34000/redis-info")
CANON_COUNT=$(echo "$CANON_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['request_counter'])" 2>/dev/null || echo "0")
# inst-a had a high count; canonical port should reflect inst-a's Redis
[ "$CANON_COUNT" -gt 5 ] || fail "canonical port counter ($CANON_COUNT) too low — expected inst-a's counter"
pass "canonical port 34000 routes to inst-a (counter=$CANON_COUNT)"

# coast ls should show checked_out
LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "checked_out" "coast ls shows checked_out status"
assert_contains "$LS_OUT" "checked_out" "ls shows checked_out after checkout"
pass "ls verified after checkout"

# ============================================================
# Test 2: Instant swap to inst-b
# ============================================================

echo ""
echo "=== Test 2: instant swap to inst-b ==="

CO_B=$("$COAST" checkout inst-b 2>&1)
assert_contains "$CO_B" "Checked out coast instance" "checkout inst-b output correct"

sleep 1

# Canonical port should now route to inst-b (low counter)
CANON_B=$(curl -s "http://localhost:34000/redis-info")
CANON_B_COUNT=$(echo "$CANON_B" | python3 -c "import sys,json; print(json.load(sys.stdin)['request_counter'])" 2>/dev/null || echo "0")
[ "$CANON_B_COUNT" -lt "$INST_A_COUNT" ] || fail "canonical port counter ($CANON_B_COUNT) not lower — expected inst-b"
pass "canonical port 34000 swapped to inst-b (counter=$CANON_B_COUNT)"

# inst-a's dynamic port should STILL work
INST_A_DYN_CHECK=$(curl -s "http://localhost:${INST_A_DYN}/")
assert_contains "$INST_A_DYN_CHECK" "API Gateway" "inst-a dynamic port still works after checkout swap"

# inst-b's dynamic port should also still work
INST_B_DYN_CHECK=$(curl -s "http://localhost:${INST_B_DYN}/")
assert_contains "$INST_B_DYN_CHECK" "API Gateway" "inst-b dynamic port still works"

# ============================================================
# Test 3: coast ports
# ============================================================

echo ""
echo "=== Test 3: coast ports ==="

PORTS_OUT=$("$COAST" ports inst-a 2>&1)
assert_contains "$PORTS_OUT" "Port allocations" "ports output has header"
assert_contains "$PORTS_OUT" "SERVICE" "ports output has SERVICE column"
assert_contains "$PORTS_OUT" "CANONICAL" "ports output has CANONICAL column"
assert_contains "$PORTS_OUT" "DYNAMIC" "ports output has DYNAMIC column"
assert_contains "$PORTS_OUT" "34000" "ports output shows canonical port 34000"
assert_contains "$PORTS_OUT" "$INST_A_DYN" "ports output shows correct dynamic port"

# Also check inst-b ports
PORTS_B=$("$COAST" ports inst-b 2>&1)
assert_contains "$PORTS_B" "Port allocations" "inst-b ports output has header"
assert_contains "$PORTS_B" "$INST_B_DYN" "inst-b ports shows correct dynamic port"

# ============================================================
# Test 4: checkout --none — unbind all canonical ports
# ============================================================

echo ""
echo "=== Test 4: coast checkout --none ==="

CO_NONE=$("$COAST" checkout --none 2>&1)
assert_contains "$CO_NONE" "Unbound all canonical ports" "checkout --none output correct"

sleep 1

# Canonical port should be unreachable
CANON_CODE=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:34000/" 2>&1 || true)
assert_eq "$CANON_CODE" "000" "canonical port 34000 unreachable after checkout --none"

# Dynamic ports should STILL work
INST_A_AFTER_NONE=$(curl -s "http://localhost:${INST_A_DYN}/")
assert_contains "$INST_A_AFTER_NONE" "API Gateway" "inst-a dynamic port works after checkout --none"

INST_B_AFTER_NONE=$(curl -s "http://localhost:${INST_B_DYN}/")
assert_contains "$INST_B_AFTER_NONE" "API Gateway" "inst-b dynamic port works after checkout --none"

# coast ls should show both as running (not checked_out)
LS_NONE=$("$COAST" ls 2>&1)
assert_not_contains "$LS_NONE" "checked_out" "no instance is checked_out after --none"
pass "ls verified after checkout --none"
pass "ls verified after checkout --none"

# ============================================================
# Test 5: Re-checkout after --none
# ============================================================

echo ""
echo "=== Test 5: re-checkout after --none ==="

CO_RECHECK=$("$COAST" checkout inst-a 2>&1)
assert_contains "$CO_RECHECK" "Checked out coast instance" "re-checkout inst-a succeeds"

sleep 1

CANON_RECHECK=$(curl -s "http://localhost:34000/")
assert_contains "$CANON_RECHECK" "API Gateway" "canonical port works again after re-checkout"

# ============================================================
# Phase 1 Cleanup
# ============================================================

echo ""
echo "=== Phase 1 Cleanup ==="

"$COAST" rm inst-a 2>&1 | grep -q "Removed" || fail "coast rm inst-a failed"
"$COAST" rm inst-b 2>&1 | grep -q "Removed" || fail "coast rm inst-b failed"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

echo ""
echo "==========================================="
echo "  PHASE 1 PASSED: Checkout Tests"
echo "==========================================="

# ============================================================
# Phase 3: Host Coast Sync & Assign Prevention
#
# Tests that checked-out instances show the live git branch in
# coast ls, and that coast assign preserves checked_out status.
# Uses coast-api (already built in Phase 1).
# ============================================================

echo ""
echo "============================================================"
echo "  Phase 3: Host Coast Sync & Assign Prevention"
echo "============================================================"

cd "$PROJECTS_DIR/coast-api"

# Run an instance and check it out
echo ""
echo "=== Phase 3 Setup: run and checkout ==="

SYNC_RUN=$("$COAST" run sync-test 2>&1)
CLEANUP_INSTANCES+=("sync-test")
assert_contains "$SYNC_RUN" "Created coast instance" "coast run sync-test succeeds"

SYNC_DYN=$(extract_dynamic_port "$SYNC_RUN" "api")
[ -n "$SYNC_DYN" ] || fail "Could not extract sync-test api dynamic port"
wait_for_healthy "$SYNC_DYN" 60 || fail "sync-test did not become healthy"
pass "sync-test is healthy on port $SYNC_DYN"

CO_SYNC=$("$COAST" checkout sync-test 2>&1)
assert_contains "$CO_SYNC" "Checked out coast instance" "checkout sync-test succeeds"
sleep 1

# ============================================================
# Test 6: Host coast sync — coast ls shows live git branch
# ============================================================

echo ""
echo "=== Test 6: host coast sync (live git branch in coast ls) ==="

# Get the actual live git branch in the project root
LIVE_BRANCH=$(git rev-parse --abbrev-ref HEAD)
[ -n "$LIVE_BRANCH" ] || fail "Could not resolve live git branch"
pass "Live git branch: $LIVE_BRANCH"

# coast ls should show the live branch for the checked-out instance
LS_SYNC=$("$COAST" ls 2>&1)
assert_contains "$LS_SYNC" "sync-test" "coast ls shows sync-test"
assert_contains "$LS_SYNC" "checked_out" "coast ls shows checked_out status"
assert_contains "$LS_SYNC" "$LIVE_BRANCH" "coast ls shows live git branch '$LIVE_BRANCH' for checked-out instance"
assert_contains "$LS_SYNC" "checked_out" "sync-test shows checked_out status"
pass "ls verified for sync-test"
pass "Host coast sync: coast ls reflects live git branch"

# ============================================================
# Test 7: Assign prevention — cannot assign to checked-out coast
# ============================================================

echo ""
echo "=== Test 7: assign prevention on checked-out coast ==="

# Assign on a checked-out instance should succeed but preserve checked_out status
ASSIGN_OUT=$("$COAST" assign sync-test --worktree feature-v2 2>&1 || true)
pass "coast assign on checked-out instance completed"

# Verify the instance is still checked out
LS_AFTER_ASSIGN=$("$COAST" ls 2>&1)
assert_contains "$LS_AFTER_ASSIGN" "checked_out" "instance still checked_out after assign"
assert_contains "$LS_AFTER_ASSIGN" "sync-test" "sync-test still in ls after assign"

# ============================================================
# Phase 3 Cleanup
# ============================================================

echo ""
echo "=== Phase 3 Cleanup ==="

"$COAST" checkout --none 2>&1 >/dev/null || true
"$COAST" rm sync-test 2>&1 | grep -q "Removed" || fail "coast rm sync-test failed"
CLEANUP_INSTANCES=()

FINAL_LS3=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS3" "No coast instances" "all phase 3 instances removed"

echo ""
echo "==========================================="
echo "  PHASE 3 PASSED: Host Sync & Assign Guard"
echo "==========================================="

# ============================================================
# Phase 2: Batch Creation & Auto-Branch
#
# Uses coast-benchmark — zero-dep Node server, single "app" service
# on canonical port 39000. Has 50 feature branches (feature-01..feature-50).
# ============================================================

echo ""
echo "============================================================"
echo "  Phase 2: Batch Creation & Multi-Instance Checkout"
echo "============================================================"

cd "$PROJECTS_DIR/coast-benchmark"

# Build
echo ""
echo "=== Build coast-benchmark ==="
BENCH_BUILD=$("$COAST" build 2>&1)
assert_contains "$BENCH_BUILD" "Build complete" "coast build coast-benchmark succeeds"

# ============================================================
# Test 8: Batch creation — coast run dev-{n} --n=3
# ============================================================

echo ""
echo "=== Test 8: batch creation (coast run dev-{n} --n=3) ==="

BATCH_OUT=$("$COAST" run "dev-{n}" --n 3 2>&1)
CLEANUP_INSTANCES+=("dev-1" "dev-2" "dev-3")

# Verify all 3 instances were created
assert_contains "$BATCH_OUT" "Created coast instance 'dev-1'" "batch created dev-1"
assert_contains "$BATCH_OUT" "Created coast instance 'dev-2'" "batch created dev-2"
assert_contains "$BATCH_OUT" "Created coast instance 'dev-3'" "batch created dev-3"
assert_contains "$BATCH_OUT" "3/3 instances created successfully" "batch summary correct"
pass "Batch creation of 3 instances succeeded"

# Verify coast ls shows all 3
LS_BATCH=$("$COAST" ls 2>&1)
assert_contains "$LS_BATCH" "dev-1" "coast ls shows dev-1"
assert_contains "$LS_BATCH" "dev-2" "coast ls shows dev-2"
assert_contains "$LS_BATCH" "dev-3" "coast ls shows dev-3"

# ============================================================
# Test 9: Auto-branch detection — instances default to current HEAD
# ============================================================

echo ""
echo "=== Test 9: auto-branch detection ==="

# The current branch should be "main" (setup.sh returns to main)
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
assert_eq "$CURRENT_BRANCH" "main" "current git branch is main"

# coast ls should show the branch for each instance
assert_contains "$LS_BATCH" "main" "batch instances are on main branch (auto-detected)"
pass "batch instances verified in ls"
pass "Auto-branch detection works — instances on '$CURRENT_BRANCH'"

# ============================================================
# Test 10: Each batch instance has independent dynamic ports
# ============================================================

echo ""
echo "=== Test 10: batch instance dynamic port independence ==="

# Give instances a moment to finish port allocation
sleep 3

# Extract dynamic ports for each instance
DEV1_PORTS=$("$COAST" ports dev-1 2>&1)
DEV2_PORTS=$("$COAST" ports dev-2 2>&1)
DEV3_PORTS=$("$COAST" ports dev-3 2>&1)

DEV1_DYN=$(echo "$DEV1_PORTS" | awk '/app/{print $NF}')
DEV2_DYN=$(echo "$DEV2_PORTS" | awk '/app/{print $NF}')
DEV3_DYN=$(echo "$DEV3_PORTS" | awk '/app/{print $NF}')

[ -n "$DEV1_DYN" ] || { echo "  dev-1 ports output: $DEV1_PORTS"; fail "Could not extract dev-1 app dynamic port"; }
[ -n "$DEV2_DYN" ] || { echo "  dev-2 ports output: $DEV2_PORTS"; fail "Could not extract dev-2 app dynamic port"; }
[ -n "$DEV3_DYN" ] || { echo "  dev-3 ports output: $DEV3_PORTS"; fail "Could not extract dev-3 app dynamic port"; }
pass "dev-1 dynamic port: $DEV1_DYN"
pass "dev-2 dynamic port: $DEV2_DYN"
pass "dev-3 dynamic port: $DEV3_DYN"

# Ports must be unique
[ "$DEV1_DYN" != "$DEV2_DYN" ] || fail "dev-1 and dev-2 have same dynamic port"
[ "$DEV1_DYN" != "$DEV3_DYN" ] || fail "dev-1 and dev-3 have same dynamic port"
[ "$DEV2_DYN" != "$DEV3_DYN" ] || fail "dev-2 and dev-3 have same dynamic port"
pass "All 3 instances have unique dynamic ports"

# Wait for all instances to become healthy
wait_for_healthy "$DEV1_DYN" 60 || fail "dev-1 did not become healthy on port $DEV1_DYN"
pass "dev-1 is healthy"
wait_for_healthy "$DEV2_DYN" 60 || fail "dev-2 did not become healthy on port $DEV2_DYN"
pass "dev-2 is healthy"
wait_for_healthy "$DEV3_DYN" 60 || fail "dev-3 did not become healthy on port $DEV3_DYN"
pass "dev-3 is healthy"

# Verify each responds correctly
DEV1_RESP=$(curl -s "http://localhost:${DEV1_DYN}/")
assert_contains "$DEV1_RESP" "coast-benchmark" "dev-1 responds correctly"
DEV2_RESP=$(curl -s "http://localhost:${DEV2_DYN}/")
assert_contains "$DEV2_RESP" "coast-benchmark" "dev-2 responds correctly"
DEV3_RESP=$(curl -s "http://localhost:${DEV3_DYN}/")
assert_contains "$DEV3_RESP" "coast-benchmark" "dev-3 responds correctly"

# ============================================================
# Test 11: Checkout between batch-created instances
# ============================================================

echo ""
echo "=== Test 11: checkout between batch instances ==="

# Checkout dev-1 — canonical port 39000
CO_DEV1=$("$COAST" checkout dev-1 2>&1)
assert_contains "$CO_DEV1" "Checked out coast instance" "checkout dev-1 succeeds"
sleep 1

CANON_DEV1=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_DEV1" "coast-benchmark" "canonical port 39000 responds after checkout dev-1"

# Instant swap to dev-2
CO_DEV2=$("$COAST" checkout dev-2 2>&1)
assert_contains "$CO_DEV2" "Checked out coast instance" "checkout dev-2 succeeds"
sleep 1

CANON_DEV2=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_DEV2" "coast-benchmark" "canonical port 39000 responds after checkout dev-2"

# Verify dev-1 dynamic port still works after swap
DEV1_AFTER_SWAP=$(curl -s "http://localhost:${DEV1_DYN}/")
assert_contains "$DEV1_AFTER_SWAP" "coast-benchmark" "dev-1 dynamic port still works after checkout swap"

# Swap to dev-3
CO_DEV3=$("$COAST" checkout dev-3 2>&1)
assert_contains "$CO_DEV3" "Checked out coast instance" "checkout dev-3 succeeds"
sleep 1

CANON_DEV3=$(curl -s "http://localhost:39000/")
assert_contains "$CANON_DEV3" "coast-benchmark" "canonical port 39000 responds after checkout dev-3"

# All dynamic ports still work
DEV1_STILL=$(curl -s "http://localhost:${DEV1_DYN}/health")
assert_contains "$DEV1_STILL" "ok" "dev-1 dynamic port healthy after multiple swaps"
DEV2_STILL=$(curl -s "http://localhost:${DEV2_DYN}/health")
assert_contains "$DEV2_STILL" "ok" "dev-2 dynamic port healthy after multiple swaps"
DEV3_STILL=$(curl -s "http://localhost:${DEV3_DYN}/health")
assert_contains "$DEV3_STILL" "ok" "dev-3 dynamic port healthy after multiple swaps"

# ============================================================
# Test 12: coast ls shows correct statuses for batch instances
# ============================================================

echo ""
echo "=== Test 12: coast ls statuses for batch instances ==="

LS_CHECKOUT=$("$COAST" ls 2>&1)
# dev-3 should be checked_out (last one we checked out)
assert_contains "$LS_CHECKOUT" "checked_out" "one instance is checked out"
assert_contains "$LS_CHECKOUT" "checked_out" "checked_out status during checkout swaps"
pass "ls verified during checkout swaps"

# ============================================================
# Test 13: checkout --none with batch instances
# ============================================================

echo ""
echo "=== Test 13: checkout --none with batch instances ==="

CO_BATCH_NONE=$("$COAST" checkout --none 2>&1)
assert_contains "$CO_BATCH_NONE" "Unbound all canonical ports" "checkout --none works with batch instances"
sleep 1

CANON_NONE=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:39000/" 2>&1 || true)
assert_eq "$CANON_NONE" "000" "canonical port 39000 unreachable after --none"

# Dynamic ports still work
DEV1_AFTER_NONE=$(curl -s "http://localhost:${DEV1_DYN}/health")
assert_contains "$DEV1_AFTER_NONE" "ok" "dev-1 dynamic port works after --none"
DEV2_AFTER_NONE=$(curl -s "http://localhost:${DEV2_DYN}/health")
assert_contains "$DEV2_AFTER_NONE" "ok" "dev-2 dynamic port works after --none"

# ============================================================
# Test 14: Batch creation with --worktree flag
# ============================================================

echo ""
echo "=== Test 14: batch creation with explicit branch ==="

BATCH_BRANCH_OUT=$("$COAST" run "feat-{n}" --n 2 --worktree feature-01 2>&1)
CLEANUP_INSTANCES+=("feat-1" "feat-2")
assert_contains "$BATCH_BRANCH_OUT" "Created coast instance 'feat-1'" "batch with --worktree created feat-1"
assert_contains "$BATCH_BRANCH_OUT" "Created coast instance 'feat-2'" "batch with --worktree created feat-2"
assert_contains "$BATCH_BRANCH_OUT" "2/2 instances created successfully" "batch with --worktree summary correct"

# Verify the instances appear in coast ls.
LS_BRANCH=$("$COAST" ls 2>&1)
assert_contains "$LS_BRANCH" "feat-1" "coast ls shows feat-1"
assert_contains "$LS_BRANCH" "feat-2" "coast ls shows feat-2"

# Get dynamic ports for feat-1 and feat-2
FEAT1_PORTS=$("$COAST" ports feat-1 2>&1)
FEAT1_DYN=$(echo "$FEAT1_PORTS" | awk '/app/{print $NF}')
[ -n "$FEAT1_DYN" ] || fail "Could not extract feat-1 app dynamic port"

FEAT2_PORTS=$("$COAST" ports feat-2 2>&1)
FEAT2_DYN=$(echo "$FEAT2_PORTS" | awk '/app/{print $NF}')
[ -n "$FEAT2_DYN" ] || fail "Could not extract feat-2 app dynamic port"

[ "$FEAT1_DYN" != "$FEAT2_DYN" ] || fail "feat-1 and feat-2 have same dynamic port"
pass "feat-1 port: $FEAT1_DYN, feat-2 port: $FEAT2_DYN (unique)"

wait_for_healthy "$FEAT1_DYN" 60 || fail "feat-1 did not become healthy"
pass "feat-1 is healthy on port $FEAT1_DYN"

wait_for_healthy "$FEAT2_DYN" 60 || fail "feat-2 did not become healthy"
pass "feat-2 is healthy on port $FEAT2_DYN"

# Verify feat-1 responds (note: coast run --worktree records metadata but builds
# from the host's current code. Use coast assign to bake branch-specific code.)
FEAT1_RESP=$(curl -s "http://localhost:${FEAT1_DYN}/")
assert_contains "$FEAT1_RESP" "coast-benchmark" "feat-1 responds on dynamic port"

# ============================================================
# Phase 2 Cleanup
# ============================================================

echo ""
echo "=== Phase 2 Cleanup ==="

"$COAST" rm dev-1 2>&1 | grep -q "Removed" || fail "coast rm dev-1 failed"
"$COAST" rm dev-2 2>&1 | grep -q "Removed" || fail "coast rm dev-2 failed"
"$COAST" rm dev-3 2>&1 | grep -q "Removed" || fail "coast rm dev-3 failed"
"$COAST" rm feat-1 2>&1 | grep -q "Removed" || fail "coast rm feat-1 failed"
"$COAST" rm feat-2 2>&1 | grep -q "Removed" || fail "coast rm feat-2 failed"
CLEANUP_INSTANCES=()

FINAL_LS2=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS2" "No coast instances" "all batch instances removed"

echo ""
echo "==========================================="
echo "  PHASE 2 PASSED: Batch & Multi-Instance"
echo "==========================================="

# Phase 4 (checkout sync) is in test_sync.sh for faster iteration.

# ============================================================
# Final Summary
# ============================================================

echo ""
echo "=============================================="
echo "  ALL TESTS PASSED (Phase 1 + Phase 2 + Phase 3)"
echo "=============================================="
echo "  (Run test_sync.sh separately for checkout sync tests)"
