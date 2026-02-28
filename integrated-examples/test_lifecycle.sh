#!/usr/bin/env bash
#
# End-to-end integration test for coast lifecycle operations.
#
# Tests the complete coast lifecycle: build, run, stop, start, exec, ls,
# and rm. Uses coast-demo with shared postgres (data persists
# across instances) and isolated redis (each instance gets fresh redis).
#
# Because postgres locks its data directory, instances sharing a postgres
# volume must run sequentially (stop one before starting another).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated_examples/test_lifecycle.sh

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

# ============================================================
# Test 1: Build
# ============================================================

echo ""
echo "=== Test 1: coast build ==="

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"
assert_contains "$BUILD_OUT" "coast-demo" "build output references project name"
pass "Build complete"

# ============================================================
# Test 2: Run inst-a — products table, Redis marker
# ============================================================

echo ""
echo "=== Test 2: coast run inst-a ==="

RUN_OUT=$("$COAST" run inst-a 2>&1)
CLEANUP_INSTANCES+=("inst-a")
assert_contains "$RUN_OUT" "Created coast instance" "coast run inst-a succeeds"

INST_A_DYN=$(extract_dynamic_port "$RUN_OUT" "app")
[ -n "$INST_A_DYN" ] || fail "Could not extract inst-a app dynamic port"
pass "inst-a dynamic port: $INST_A_DYN"

wait_for_healthy "$INST_A_DYN" 60 || fail "inst-a did not become healthy"
pass "inst-a is healthy"

# Verify homepage
MAIN_RESP=$(curl -s "http://localhost:${INST_A_DYN}/")
assert_contains "$MAIN_RESP" "Hello from Coast!" "inst-a returns correct greeting"

# Check tables — should have products
TABLES=$(curl -s "http://localhost:${INST_A_DYN}/tables")
assert_contains "$TABLES" "products" "inst-a has products table"

# Create a product (persists via shared postgres volume)
CREATE_RESP=$(curl -s -X POST -H "Content-Type: application/json" \
    -d '{"name":"Widget"}' "http://localhost:${INST_A_DYN}/products")
assert_contains "$CREATE_RESP" "Widget" "created product Widget"

# Set Redis marker for inst-a
REDIS_RESP=$(curl -s "http://localhost:${INST_A_DYN}/redis-set-marker")
assert_contains "$REDIS_RESP" '"marker":"main"' "Redis marker set for inst-a"

# Bump redis hit counter a few more times
for _ in 1 2 3 4 5; do
    curl -s "http://localhost:${INST_A_DYN}/" >/dev/null
done

# ============================================================
# Test 3: Stop inst-a (release postgres volume lock)
# ============================================================

echo ""
echo "=== Test 3: coast stop inst-a ==="

"$COAST" stop inst-a 2>&1 | grep -q "Stopped" || fail "coast stop inst-a failed"
pass "coast stop inst-a succeeded"

# Dynamic port should be down
STOP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:${INST_A_DYN}/" 2>&1 || true)
assert_eq "$STOP_CODE" "000" "inst-a dynamic port down after stop"

# ============================================================
# Test 4: Run inst-b — shared pg sees products, redis is fresh
# ============================================================

echo ""
echo "=== Test 4: coast run inst-b ==="

RUN_OUT_B=$("$COAST" run inst-b 2>&1)
CLEANUP_INSTANCES+=("inst-b")
assert_contains "$RUN_OUT_B" "Created coast instance" "coast run inst-b succeeds"

INST_B_DYN=$(extract_dynamic_port "$RUN_OUT_B" "app")
[ -n "$INST_B_DYN" ] || fail "Could not extract inst-b app dynamic port"
pass "inst-b dynamic port: $INST_B_DYN"

wait_for_healthy "$INST_B_DYN" 60 || fail "inst-b did not become healthy"
pass "inst-b is healthy"

# ISOLATED REDIS: check freshness BEFORE any requests that increment the counter
INST_B_REDIS=$(curl -s "http://localhost:${INST_B_DYN}/redis-info")
assert_contains "$INST_B_REDIS" '"instance_marker":null' "inst-b Redis is fresh (isolated)"
assert_contains "$INST_B_REDIS" '"hit_counter":0' "inst-b Redis counter is 0 (isolated)"

# SHARED POSTGRES: products table AND Widget data from inst-a should still be here
INST_B_TABLES=$(curl -s "http://localhost:${INST_B_DYN}/tables")
assert_contains "$INST_B_TABLES" "products" "inst-b sees products table (shared pg)"

INST_B_PRODUCTS=$(curl -s "http://localhost:${INST_B_DYN}/products")
assert_contains "$INST_B_PRODUCTS" "Widget" "inst-b sees Widget from inst-a (shared pg)"

# Create another product from inst-b
curl -s -X POST -H "Content-Type: application/json" \
    -d '{"name":"Gadget"}' "http://localhost:${INST_B_DYN}/products" >/dev/null
PRODUCTS=$(curl -s "http://localhost:${INST_B_DYN}/products")
assert_contains "$PRODUCTS" "Widget" "inst-b still sees Widget"
assert_contains "$PRODUCTS" "Gadget" "inst-b created Gadget"

# Set inst-b redis marker
curl -s "http://localhost:${INST_B_DYN}/redis-set-marker" >/dev/null

# ============================================================
# Test 5: Stop inst-b
# ============================================================

echo ""
echo "=== Test 5: coast stop inst-b ==="

"$COAST" stop inst-b 2>&1 | grep -q "Stopped" || fail "coast stop inst-b failed"
pass "coast stop inst-b succeeded"

# ============================================================
# Test 6: Restart inst-a — verify accumulated postgres state + redis isolation
# ============================================================

echo ""
echo "=== Test 6: coast start inst-a (verify accumulated state) ==="

"$COAST" start inst-a 2>&1 | grep -q "Started" || fail "coast start inst-a failed"
pass "coast start inst-a succeeded"

wait_for_healthy "$INST_A_DYN" 60 || fail "inst-a did not recover after start"
pass "inst-a recovered after start"

# SHARED POSTGRES: inst-a should see products accumulated from both instances
RESTARTED_PRODUCTS=$(curl -s "http://localhost:${INST_A_DYN}/products")
assert_contains "$RESTARTED_PRODUCTS" "Widget" "restarted inst-a sees Widget"
assert_contains "$RESTARTED_PRODUCTS" "Gadget" "restarted inst-a sees Gadget (from inst-b via shared pg!)"

# ISOLATED REDIS: inst-a's Redis data should be preserved from before stop
MAIN_REDIS=$(curl -s "http://localhost:${INST_A_DYN}/redis-info")
assert_contains "$MAIN_REDIS" '"instance_marker":"main"' "inst-a Redis marker preserved across stop/start"
MAIN_COUNTER=$(echo "$MAIN_REDIS" | python3 -c "import sys,json; print(json.load(sys.stdin)['hit_counter'])")
[ "$MAIN_COUNTER" -gt 0 ] || fail "inst-a Redis counter should be > 0 after stop/start"
pass "inst-a Redis state preserved (hit_counter=$MAIN_COUNTER)"

# ============================================================
# Test 7: Exec into running instance — internal port access
# ============================================================

echo ""
echo "=== Test 7: coast exec (internal port access) ==="

INTERNAL_RESP=$("$COAST" exec inst-a -- wget -qO- http://localhost:33000/ 2>&1)
assert_contains "$INTERNAL_RESP" "Hello from Coast!" "exec: internal port 33000 works inside coast"

# ============================================================
# Test 8: coast ls
# ============================================================

echo ""
echo "=== Test 8: coast ls ==="

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "inst-a" "ls shows inst-a"
assert_contains "$LS_OUT" "inst-b" "ls shows inst-b"

# ============================================================
# Test 9: coast rm — clean removal
# ============================================================

echo ""
echo "=== Test 9: coast rm ==="

"$COAST" rm inst-a 2>&1 | grep -q "Removed" || fail "coast rm inst-a failed"
pass "coast rm inst-a succeeded"

"$COAST" rm inst-b 2>&1 | grep -q "Removed" || fail "coast rm inst-b failed"
pass "coast rm inst-b succeeded"
CLEANUP_INSTANCES=()

FINAL_LS=$("$COAST" ls 2>&1)
assert_contains "$FINAL_LS" "No coast instances" "all instances removed"

# Verify no coast containers remain
REMAINING=$(docker ps -q --filter "label=coast.managed=true" 2>/dev/null)
assert_eq "${REMAINING:-}" "" "no coast containers remain"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL LIFECYCLE TESTS PASSED"
echo "==========================================="
