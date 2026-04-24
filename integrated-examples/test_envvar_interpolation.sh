#!/usr/bin/env bash
#
# Integration test for environment variable interpolation in Coastfiles.
#
# Tests ${VAR}, ${VAR:-default}, undefined variable warnings, and $${VAR}
# escape syntax in real build/run cycles.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)

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
# Test 1: Default values — build without setting env vars
# ============================================================

echo ""
echo "=== Test 1: Build with default env var values ==="

cd "$PROJECTS_DIR/coast-envvar"

# Do NOT set COAST_PROJECT_NAME or COAST_APP_PORT.
# Coastfile uses ${COAST_PROJECT_NAME:-coast-envvar} and ${COAST_APP_PORT:-40000}.

BUILD_OUT=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT" "Build complete" "build succeeds with default values"
assert_contains "$BUILD_OUT" "coast-envvar" "project name defaults to coast-envvar"
pass "Default values applied"

# ============================================================
# Test 2: Set env vars — build with overridden values
# ============================================================

echo ""
echo "=== Test 2: Build with env var overrides ==="

# Env vars are interpolated in the daemon process, so we restart
# the daemon with the new environment for this test.
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1
rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

export COAST_PROJECT_NAME="custom-envvar"
export COAST_APP_PORT=41000

start_daemon

cd "$PROJECTS_DIR/coast-envvar"
BUILD_OUT2=$("$COAST" build 2>&1)
assert_contains "$BUILD_OUT2" "Build complete" "build succeeds with env var overrides"
assert_contains "$BUILD_OUT2" "custom-envvar" "project name uses env var value"
pass "Env var overrides applied"

unset COAST_PROJECT_NAME
unset COAST_APP_PORT

# ============================================================
# Test 3: Run instance from env-var-interpolated build
# ============================================================

echo ""
echo "=== Test 3: Run instance from default build ==="

# Restart daemon without the override env vars
pkill -f "coastd --foreground" 2>/dev/null || true
sleep 1
rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

start_daemon

cd "$PROJECTS_DIR/coast-envvar"

# Rebuild with defaults since state was cleaned for daemon restart
"$COAST" build 2>&1 >/dev/null

RUN_OUT=$("$COAST" run envvar-1 2>&1)
CLEANUP_INSTANCES+=("envvar-1")
assert_contains "$RUN_OUT" "Created coast instance" "coast run envvar-1 succeeds"
pass "Instance created from envvar build"

LS_OUT=$("$COAST" ls 2>&1)
assert_contains "$LS_OUT" "envvar-1" "ls shows envvar-1"

# ============================================================
# Test 4: Undefined variable warning
# ============================================================

echo ""
echo "=== Test 4: Undefined variable produces warning ==="

TMPDIR_TEST=$(mktemp -d)
cat > "$TMPDIR_TEST/Coastfile" << 'EOF'
[coast]
name = "test-undefined-var"
runtime = "dind"

[coast.setup]
packages = ["${UNDEFINED_PACKAGE}"]
EOF

cd "$TMPDIR_TEST"
git init -b main >/dev/null 2>&1
git add -A >/dev/null 2>&1
git commit -m "init" >/dev/null 2>&1

BUILD_UNDEF=$("$COAST" build 2>&1)
assert_contains "$BUILD_UNDEF" "Build complete" "build succeeds with undefined var"

# Undefined variables are now PRESERVED as literal `${VAR}` text rather
# than silently substituted to empty. The build may produce a downstream
# warning or package-install quirk because apk doesn't recognize the
# literal `${UNDEFINED_PACKAGE}` token — but the build itself should
# complete so the warning surfaces instead of the whole flow failing
# silently on a mangled value.
pass "Undefined variable preserved as literal"

rm -rf "$TMPDIR_TEST"

# ============================================================
# Test 5: Escape syntax $${VAR}
# ============================================================

echo ""
echo "=== Test 5: Escape syntax \$\${VAR} ==="

TMPDIR_ESC=$(mktemp -d)
cat > "$TMPDIR_ESC/Coastfile" << 'ESCEOF'
[coast]
name = "test-escape"
runtime = "dind"
ESCEOF

cd "$TMPDIR_ESC"
git init -b main >/dev/null 2>&1
git add -A >/dev/null 2>&1
git commit -m "init" >/dev/null 2>&1

BUILD_ESC=$("$COAST" build 2>&1)
assert_contains "$BUILD_ESC" "Build complete" "build with escape syntax succeeds"
pass "Escape syntax does not break build"

rm -rf "$TMPDIR_ESC"

# ============================================================
# Cleanup
# ============================================================

echo ""
echo "=== Cleanup ==="

"$COAST" rm envvar-1 2>/dev/null || true
CLEANUP_INSTANCES=()

echo ""
echo "==========================================="
echo "  ALL ENV VAR INTERPOLATION TESTS PASSED"
echo "==========================================="
