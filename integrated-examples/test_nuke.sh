#!/usr/bin/env bash
#
# Integration test for `coast nuke`.
#
# Verifies that `coast nuke --force` wipes all state (DB, images, caches)
# while preserving the CLI binary — even when the binary lives inside
# $COAST_HOME (the production installer layout).
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_nuke.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

TEST_HOME=""
TEST_HOME2=""

_do_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    [ -n "$TEST_HOME" ] && rm -rf "$TEST_HOME" 2>/dev/null || true
    [ -n "$TEST_HOME2" ] && rm -rf "$TEST_HOME2" 2>/dev/null || true
    unset COAST_HOME 2>/dev/null || true
    echo "Cleanup complete."
}

trap '_do_cleanup' EXIT

echo "=== test_nuke.sh — coast nuke preserves CLI binary ==="
echo ""

preflight_checks

# ============================================================
# Test 1: nuke with binary OUTSIDE $COAST_HOME
# ============================================================

echo ""
echo "=== Test 1: nuke with binary outside COAST_HOME ==="

TEST_HOME="$(mktemp -d)"
export COAST_HOME="$TEST_HOME"

mkdir -p "$TEST_HOME/images/test-project"
echo "fake-db" > "$TEST_HOME/state.db"
echo "fake" > "$TEST_HOME/keystore.db"
echo "fake" > "$TEST_HOME/keystore.key"
mkdir -p "$TEST_HOME/image-cache"
echo "cached" > "$TEST_HOME/image-cache/test.tar"

"$COASTD" --foreground &>/tmp/coastd-nuke-test.log &
sleep 2

"$COAST" nuke --force 2>&1 || true

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1

[ -f "$COAST" ] || fail "coast binary missing after nuke"
[ -f "$COASTD" ] || fail "coastd binary missing after nuke"
pass "coast and coastd binaries intact after nuke"

[ ! -d "$TEST_HOME/images/test-project" ] || fail "images dir not wiped"
[ ! -f "$TEST_HOME/image-cache/test.tar" ] || fail "image-cache not wiped"
[ ! -f "$TEST_HOME/keystore.key" ] || fail "keystore.key not wiped"
pass "State files wiped by nuke"

VERSION_OUT=$("$COAST" --version 2>&1) || true
assert_contains "$VERSION_OUT" "coast" "coast --version works after nuke"

pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1
rm -rf "$TEST_HOME"
TEST_HOME=""
unset COAST_HOME

pass "Test 1 passed: nuke with external binary"

# ============================================================
# Test 2: nuke with binary INSIDE $COAST_HOME
# ============================================================

echo ""
echo "=== Test 2: nuke with binary INSIDE COAST_HOME ==="

TEST_HOME2="$(mktemp -d)"
export COAST_HOME="$TEST_HOME2"

mkdir -p "$TEST_HOME2/bin"
# Use the real ELF binary, not the DinD wrapper script, so current_exe()
# resolves to the copy inside COAST_HOME and bin_dir_inside() preserves it.
COAST_BIN="${REAL_COAST:-$COAST}"
COASTD_BIN="${REAL_COASTD:-$COASTD}"
cp "$COAST_BIN" "$TEST_HOME2/bin/coast"
cp "$COASTD_BIN" "$TEST_HOME2/bin/coastd"
chmod +x "$TEST_HOME2/bin/coast" "$TEST_HOME2/bin/coastd"

# Seed state
mkdir -p "$TEST_HOME2/images/test-project"
echo "fake-db" > "$TEST_HOME2/state.db"
echo "fake" > "$TEST_HOME2/keystore.db"
echo "fake" > "$TEST_HOME2/keystore.key"
mkdir -p "$TEST_HOME2/image-cache"
echo "cached" > "$TEST_HOME2/image-cache/test.tar"

# Start daemon from COAST_HOME/bin
"$TEST_HOME2/bin/coastd" --foreground &>/tmp/coastd-nuke-test2.log &
sleep 2

# Run nuke from the binary inside COAST_HOME — this is the bug scenario
"$TEST_HOME2/bin/coast" nuke --force 2>&1 || true

# Kill the restarted daemon
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1

# CRITICAL: binary must survive
[ -f "$TEST_HOME2/bin/coast" ] || fail "coast binary inside COAST_HOME deleted by nuke"
[ -f "$TEST_HOME2/bin/coastd" ] || fail "coastd binary inside COAST_HOME deleted by nuke"
pass "Binaries inside COAST_HOME survive nuke"

# State should be wiped (check files the daemon does NOT auto-recreate)
[ ! -d "$TEST_HOME2/images" ] || fail "images dir not wiped (binary-inside case)"
[ ! -f "$TEST_HOME2/image-cache/test.tar" ] || fail "image-cache not wiped (binary-inside case)"
[ ! -f "$TEST_HOME2/keystore.key" ] || fail "keystore.key not wiped (binary-inside case)"
pass "State wiped despite binary being inside COAST_HOME"

# CLI should still work
VERSION_OUT2=$("$TEST_HOME2/bin/coast" --version 2>&1) || true
assert_contains "$VERSION_OUT2" "coast" "coast --version works after nuke (binary inside COAST_HOME)"

# Clean up test 2
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1
rm -rf "$TEST_HOME2"
TEST_HOME2=""
unset COAST_HOME

pass "Test 2 passed: nuke preserves CLI binary inside COAST_HOME"

echo ""
echo "=== All nuke tests passed ==="
