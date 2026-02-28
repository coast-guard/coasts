#!/usr/bin/env bash
#
# Benchmark: Coast scaling performance.
#
# Measures:
#   Phase 1 — coast build time
#   Phase 2 — N instance spin-up (sequential coast run, each blocks until healthy)
#   Phase 3 — checkout swap time between instances
#
# Each instance runs on a unique feature branch and returns a unique curl response.
#
# NOT prefixed with test_ — not auto-discovered by test.sh. Run manually:
#   ./integrated_examples/benchmark.sh                            # default 5-instance benchmark
#   COAST_BENCHMARK_COUNT=3 ./integrated_examples/benchmark.sh   # quick smoke test
#   COAST_BENCHMARK_COUNT=50 ./integrated_examples/benchmark.sh  # full 50-instance benchmark
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#   - perl (for sub-millisecond timing on macOS)

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

COUNT="${COAST_BENCHMARK_COUNT:-3}"

# --- Timing helper ---
# macOS date doesn't support %N; use perl for millisecond precision.
now_ms() {
    perl -MTime::HiRes=gettimeofday -e '
        my ($s, $us) = gettimeofday();
        printf "%d\n", $s * 1000 + int($us / 1000);
    '
}

# --- Percentile calculator ---
# Reads newline-delimited integers from stdin, prints stats.
# Usage: echo "$timings" | calc_stats
calc_stats() {
    perl -e '
        my @vals = sort { $a <=> $b } map { chomp; $_ } <STDIN>;
        my $n = scalar @vals;
        exit 0 unless $n;
        my $sum = 0;
        $sum += $_ for @vals;
        my $avg = $sum / $n;
        my $min = $vals[0];
        my $max = $vals[$n - 1];
        sub pct { my ($p) = @_; my $i = int($p / 100 * ($n - 1) + 0.5); return $vals[$i]; }
        printf "  Count:  %d\n", $n;
        printf "  Total:  %d ms\n", $sum;
        printf "  Avg:    %d ms\n", $avg;
        printf "  Min:    %d ms\n", $min;
        printf "  Max:    %d ms\n", $max;
        printf "  P50:    %d ms\n", pct(50);
        printf "  P95:    %d ms\n", pct(95);
        printf "  P99:    %d ms\n", pct(99);
    '
}

# --- Cleanup trap ---
# Custom cleanup that loops through N instances.
benchmark_cleanup() {
    echo ""
    echo "--- Benchmark cleanup ---"
    cd "$PROJECTS_DIR/coast-benchmark" 2>/dev/null || true

    for n in $(seq 1 "$COUNT"); do
        local padded
        padded=$(printf '%02d' "$n")
        "$COAST" rm "feature-$padded" 2>/dev/null || true
    done
    "$COAST" rm main 2>/dev/null || true

    # Kill daemon
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1

    # Kill any orphaned socat
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    # Clean state & volumes
    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs docker volume rm 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Benchmark cleanup complete."
}
trap 'benchmark_cleanup' EXIT

# ============================================================
# Setup
# ============================================================

echo "==========================================="
echo "  COAST BENCHMARK — $COUNT instances"
echo "==========================================="
echo ""

preflight_checks

echo ""
echo "=== Setup ==="
clean_slate

# Initialize examples (includes coast-benchmark with N feature branches)
COAST_BENCHMARK_COUNT="$COUNT" "$HELPERS_DIR/setup.sh"
pass "Examples initialized with $COUNT feature branches"

cd "$PROJECTS_DIR/coast-benchmark"
start_daemon

# ============================================================
# Phase 1: Build
# ============================================================

echo ""
echo "=== Phase 1: Build ==="

T_BUILD_START=$(now_ms)
BUILD_OUT=$("$COAST" build 2>&1)
T_BUILD_END=$(now_ms)
BUILD_MS=$((T_BUILD_END - T_BUILD_START))

assert_contains "$BUILD_OUT" "Built coast image" "coast build succeeds"
echo ""
echo "  Build time: ${BUILD_MS} ms"

# ============================================================
# Phase 2: Run N instances (sequential)
# ============================================================

echo ""
echo "=== Phase 2: Run $COUNT instances ==="

# Arrays to hold per-instance data
declare -a DYN_PORTS
declare -a RUN_TIMINGS

T_PHASE2_START=$(now_ms)

for n in $(seq 1 "$COUNT"); do
    PADDED=$(printf '%02d' "$n")
    T_RUN_START=$(now_ms)
    RUN_OUT=$("$COAST" run "feature-$PADDED" --worktree "feature-$PADDED" 2>&1)
    T_RUN_END=$(now_ms)
    RUN_MS=$((T_RUN_END - T_RUN_START))
    RUN_TIMINGS+=("$RUN_MS")

    DYN=$(extract_dynamic_port "$RUN_OUT" "app")
    if [ -z "$DYN" ]; then
        echo "  FAIL: Could not extract dynamic port for feature-$PADDED"
        echo "  Output: $RUN_OUT"
        exit 1
    fi
    DYN_PORTS+=("$DYN")

    echo "  feature-$PADDED: started in ${RUN_MS}ms (dynamic port $DYN)"
done

T_PHASE2_END=$(now_ms)
PHASE2_TOTAL=$((T_PHASE2_END - T_PHASE2_START))

echo ""
echo "--- Verifying all instances healthy ---"

for idx in $(seq 0 $((COUNT - 1))); do
    NUM=$(printf '%02d' $((idx + 1)))
    PORT="${DYN_PORTS[$idx]}"
    wait_for_healthy "$PORT" 120 || fail "feature-$NUM did not become healthy on port $PORT"
done
pass "All $COUNT instances healthy"

echo ""
echo "--- Verifying unique responses on dynamic ports ---"

for idx in $(seq 0 $((COUNT - 1))); do
    NUM=$(printf '%02d' $((idx + 1)))
    PORT="${DYN_PORTS[$idx]}"
    RESP=$(curl -sf "http://localhost:${PORT}/" 2>&1)
    assert_contains "$RESP" "\"feature\":\"feature-$NUM\"" "feature-$NUM returns correct feature on dynamic port"
done

echo ""
echo "--- Phase 2 Stats (coast run) ---"
printf '%s\n' "${RUN_TIMINGS[@]}" | calc_stats
echo "  Wall clock: ${PHASE2_TOTAL} ms"

# ============================================================
# Phase 3: Checkout swap
# ============================================================

echo ""
echo "=== Phase 3: Checkout swap ($COUNT instances) ==="

declare -a CHECKOUT_TIMINGS

T_PHASE3_START=$(now_ms)

for n in $(seq 1 "$COUNT"); do
    PADDED=$(printf '%02d' "$n")
    T_CO_START=$(now_ms)
    CO_OUT=$("$COAST" checkout "feature-$PADDED" 2>&1)
    T_CO_END=$(now_ms)
    CO_MS=$((T_CO_END - T_CO_START))
    CHECKOUT_TIMINGS+=("$CO_MS")

    # Brief pause for socat to bind
    sleep 0.1

    RESP=$(curl -sf "http://localhost:39000/" 2>&1)
    assert_contains "$RESP" "\"feature\":\"feature-$PADDED\"" "checkout feature-$PADDED: canonical port returns correct feature"
done

T_PHASE3_END=$(now_ms)
PHASE3_TOTAL=$((T_PHASE3_END - T_PHASE3_START))

echo ""
echo "--- Phase 3 Stats (coast checkout) ---"
printf '%s\n' "${CHECKOUT_TIMINGS[@]}" | calc_stats
echo "  Wall clock: ${PHASE3_TOTAL} ms"

# ============================================================
# Phase 4: Assign (branch swap without DinD restart)
# ============================================================

echo ""
echo "=== Phase 4: Assign (branch swap via coast assign) ==="

# Clear the checkout from Phase 3 so all instances are in Running state.
# (Checked-out instances cannot be assigned — the developer controls the branch directly.)
"$COAST" checkout --none 2>&1 >/dev/null

# Rotate branches: assign feature-01's branch to feature-02, feature-02's to feature-03, etc.
# This demonstrates the fast branch-switch path.
declare -a ASSIGN_TIMINGS

# First, collect the original branches for each instance
# We'll rotate: instance feature-01 gets branch feature-02, feature-02 gets feature-03, ...
# feature-N gets branch feature-01.

T_PHASE4_START=$(now_ms)

for n in $(seq 1 "$COUNT"); do
    PADDED=$(printf '%02d' "$n")
    # Assign the "next" branch circularly
    NEXT_N=$(( (n % COUNT) + 1 ))
    NEXT_PADDED=$(printf '%02d' "$NEXT_N")

    T_ASSIGN_START=$(now_ms)
    ASSIGN_OUT=$("$COAST" assign "feature-$PADDED" --worktree "feature-$NEXT_PADDED" 2>&1)
    T_ASSIGN_END=$(now_ms)
    ASSIGN_MS=$((T_ASSIGN_END - T_ASSIGN_START))
    ASSIGN_TIMINGS+=("$ASSIGN_MS")

    assert_contains "$ASSIGN_OUT" "Assigned branch" "coast assign feature-$PADDED -> feature-$NEXT_PADDED"
    echo "  feature-$PADDED -> branch feature-$NEXT_PADDED: ${ASSIGN_MS}ms"
done

T_PHASE4_END=$(now_ms)
PHASE4_TOTAL=$((T_PHASE4_END - T_PHASE4_START))

echo ""
echo "--- Verifying reassigned instances return correct responses ---"

for n in $(seq 1 "$COUNT"); do
    PADDED=$(printf '%02d' "$n")
    NEXT_N=$(( (n % COUNT) + 1 ))
    NEXT_PADDED=$(printf '%02d' "$NEXT_N")
    PORT="${DYN_PORTS[$((n-1))]}"
    wait_for_healthy "$PORT" 120 || fail "feature-$PADDED did not become healthy after assign"
    RESP=$(curl -sf "http://localhost:${PORT}/" 2>&1)
    assert_contains "$RESP" "\"feature\":\"feature-$NEXT_PADDED\"" "feature-$PADDED now returns feature-$NEXT_PADDED after assign"
done

echo ""
echo "--- Phase 4 Stats (coast assign) ---"
printf '%s\n' "${ASSIGN_TIMINGS[@]}" | calc_stats
echo "  Wall clock: ${PHASE4_TOTAL} ms"

# ============================================================
# Summary
# ============================================================

echo ""
echo "==========================================="
echo "  BENCHMARK SUMMARY — $COUNT instances"
echo "==========================================="
echo ""
echo "  Phase 1 (build):          ${BUILD_MS} ms"
echo "  Phase 2 (run $COUNT):       ${PHASE2_TOTAL} ms"
printf '%s\n' "${RUN_TIMINGS[@]}" | perl -e '
    my @v = sort { $a <=> $b } map { chomp; $_ } <STDIN>;
    my $n = scalar @v;
    my $sum = 0; $sum += $_ for @v;
    printf "    avg: %d ms, min: %d ms, max: %d ms, P50: %d ms, P95: %d ms\n",
        $sum/$n, $v[0], $v[$n-1],
        $v[int(0.50*($n-1)+0.5)],
        $v[int(0.95*($n-1)+0.5)];
'
echo "  Phase 3 (checkout $COUNT):  ${PHASE3_TOTAL} ms"
printf '%s\n' "${CHECKOUT_TIMINGS[@]}" | perl -e '
    my @v = sort { $a <=> $b } map { chomp; $_ } <STDIN>;
    my $n = scalar @v;
    my $sum = 0; $sum += $_ for @v;
    printf "    avg: %d ms, min: %d ms, max: %d ms, P50: %d ms, P95: %d ms\n",
        $sum/$n, $v[0], $v[$n-1],
        $v[int(0.50*($n-1)+0.5)],
        $v[int(0.95*($n-1)+0.5)];
'
echo "  Phase 4 (assign $COUNT):   ${PHASE4_TOTAL} ms"
printf '%s\n' "${ASSIGN_TIMINGS[@]}" | perl -e '
    my @v = sort { $a <=> $b } map { chomp; $_ } <STDIN>;
    my $n = scalar @v;
    my $sum = 0; $sum += $_ for @v;
    printf "    avg: %d ms, min: %d ms, max: %d ms, P50: %d ms, P95: %d ms\n",
        $sum/$n, $v[0], $v[$n-1],
        $v[int(0.50*($n-1)+0.5)],
        $v[int(0.95*($n-1)+0.5)];
'
echo ""
echo "  ALL BENCHMARK ASSERTIONS PASSED"
echo "==========================================="
