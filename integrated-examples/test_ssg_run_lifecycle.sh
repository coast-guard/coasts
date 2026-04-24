#!/usr/bin/env bash
#
# Integration test: full SSG runtime lifecycle (Phase 3).
#
# Verifies the end-to-end `coast ssg run / stop / start / restart / rm`
# flow against the coast-ssg-multi-service project (postgres + redis,
# both `*-alpine`):
#
# - `coast ssg build` produces a build artifact.
# - `coast ssg run` streams progress events, creates the singleton
#   `coast-ssg` outer container, and populates the state DB.
# - `docker ps` shows `coast-ssg` running.
# - `coast ssg ps` reports the two services.
# - `coast ssg ports` returns the allocated dynamic host ports.
# - `coast ssg stop` stops the outer container; state transitions to
#   `stopped` and the container disappears from `docker ps`.
# - `coast ssg start` brings the services back up.
# - `coast ssg restart` cycles successfully.
# - `coast ssg rm --with-data` removes the outer container and clears
#   state rows.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_run_lifecycle.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) — SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-multi-service"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG state from other runs.
rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

cd "$PROJECTS_DIR/coast-ssg-multi-service"

start_daemon

# ============================================================
# Test 1: build
# ============================================================

echo ""
echo "=== Test 1: coast ssg build ==="

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"
assert_contains "$BUILD_OUT" "Build complete" "ssg build succeeds"
pass "SSG build complete"

# ============================================================
# Test 2: run (streaming progress)
# ============================================================

echo ""
echo "=== Test 2: coast ssg run ==="

RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$RUN_OUT"
assert_contains "$RUN_OUT" "Creating SSG container" "run streams creation step"
assert_contains "$RUN_OUT" "Waiting for inner daemon" "run streams daemon-ready step"
assert_contains "$RUN_OUT" "Starting inner services" "run streams compose-up step"
assert_contains "$RUN_OUT" "SSG running" "run reports success"

# Outer container exists and is running.
DOCKER_PS=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "${SSG_PROJECT}-ssg" "docker ps shows ${SSG_PROJECT}-ssg"

# ============================================================
# Test 3: ps + ports after run
# ============================================================

echo ""
echo "=== Test 3: coast ssg ps / ports ==="

PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT"
assert_contains "$PS_OUT" "postgres" "ps shows postgres"
assert_contains "$PS_OUT" "redis" "ps shows redis"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
assert_contains "$PORTS_OUT" "postgres" "ports shows postgres"
assert_contains "$PORTS_OUT" "5432" "ports shows postgres canonical port"
assert_contains "$PORTS_OUT" "redis" "ports shows redis"
assert_contains "$PORTS_OUT" "6379" "ports shows redis canonical port"
pass "ps + ports reflect running SSG"

# ============================================================
# Test 4: stop
# ============================================================

echo ""
echo "=== Test 4: coast ssg stop ==="

STOP_OUT=$("$COAST" ssg stop 2>&1)
echo "$STOP_OUT"
assert_contains "$STOP_OUT" "SSG stopped" "stop reports success"

# Outer container is no longer in the "running" list.
DOCKER_PS_RUNNING=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_RUNNING" ]; then
    fail "${SSG_PROJECT}-ssg still running after coast ssg stop"
fi
pass "${SSG_PROJECT}-ssg is no longer running"

# Container still exists in `docker ps -a` (we stopped, didn't remove).
DOCKER_PS_ALL=$(docker ps -a --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS_ALL" "${SSG_PROJECT}-ssg" "${SSG_PROJECT}-ssg container preserved"

# ============================================================
# Test 5: start
# ============================================================

echo ""
echo "=== Test 5: coast ssg start ==="

START_OUT=$("$COAST" ssg start 2>&1)
echo "$START_OUT"
assert_contains "$START_OUT" "SSG started" "start reports success"

DOCKER_PS=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "${SSG_PROJECT}-ssg" "${SSG_PROJECT}-ssg running again after start"

# ============================================================
# Test 6: restart
# ============================================================

echo ""
echo "=== Test 6: coast ssg restart ==="

RESTART_OUT=$("$COAST" ssg restart 2>&1)
echo "$RESTART_OUT"
assert_contains "$RESTART_OUT" "SSG started" "restart ends with start"

DOCKER_PS=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "${SSG_PROJECT}-ssg" "${SSG_PROJECT}-ssg running after restart"

# ============================================================
# Test 7: rm --with-data
# ============================================================

echo ""
echo "=== Test 7: coast ssg rm --with-data ==="

RM_OUT=$("$COAST" ssg rm --with-data 2>&1)
echo "$RM_OUT"
assert_contains "$RM_OUT" "SSG removed" "rm reports success"

# Outer container is gone entirely.
DOCKER_PS_ALL=$(docker ps -a --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_ALL" ]; then
    fail "${SSG_PROJECT}-ssg container still exists after rm"
fi
pass "${SSG_PROJECT}-ssg container removed"

# ssg ports returns the "nothing to show" message since state was cleared.
POST_PORTS=$("$COAST" ssg ports 2>&1)
assert_contains "$POST_PORTS" "No SSG services running" "ports cleared after rm"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG RUN LIFECYCLE TESTS PASSED"
echo "==========================================="
