#!/usr/bin/env bash
#
# Integration test: `coast ssg stop` refuses while a remote shadow
# coast is consuming the SSG (Phase 4.5, DESIGN.md §20.6).
#
# Reuses the remote consumer fixture (`coast-ssg-consumer-remote`).
# After the remote instance is up, `coast ssg stop` without --force
# must error out listing the blocking shadow by project/name@host,
# and the SSG must remain running.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) — SSG container is `{project}-ssg`.
SSG_PROJECT="coast-ssg-consumer-remote"

_ssg_remote_cleanup() {
    echo ""
    echo "--- Cleaning up ---"
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done
    "$COAST" remote rm test-remote 2>/dev/null || true
    "$COAST" ssg rm --with-data --force 2>/dev/null || true
    clean_remote_state
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    pkill -f "ssh -N -R" 2>/dev/null || true
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    cleanup_project_ssgs "$SSG_PROJECT"
    echo "Cleanup complete."
}
trap '_ssg_remote_cleanup' EXIT

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

setup_localhost_ssh
start_coast_service

"$HELPERS_DIR/setup.sh" 2>/dev/null
pass "Examples initialized"

start_daemon

echo ""
echo "=== Step 1: SSG build + run ==="

# Phase 25.5: build SSG from the consumer's cwd (Phase 23 per-project).
cd "$PROJECTS_DIR/coast-ssg-consumer-remote"
"$COAST" ssg build >/dev/null 2>&1
SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

sleep 5

echo ""
echo "=== Step 2: register remote + run remote consumer ==="

"$COAST" remote add test-remote "root@localhost" --key ~/.ssh/coast_test_key >/dev/null 2>&1
"$COAST" build 2>&1 >/dev/null
"$COAST" build --type remote 2>&1 >/dev/null

CLEANUP_INSTANCES+=("blocked-a")
RUN_OUT=$("$COAST" run blocked-a --type remote 2>&1)
assert_contains "$RUN_OUT" "Created coast instance" "remote consumer up"

sleep 3

echo ""
echo "=== Step 3: coast ssg stop (without --force) must fail ==="

set +e
STOP_OUT=$("$COAST" ssg stop 2>&1)
STOP_RC=$?
set -e
echo "$STOP_OUT"
echo "exit code: $STOP_RC"

[ "$STOP_RC" -ne 0 ] || fail "coast ssg stop must exit non-zero while a remote shadow consumes it"
pass "coast ssg stop exited non-zero"

assert_contains "$STOP_OUT" "coast-ssg-consumer-remote/blocked-a" \
    "error lists the blocking shadow (project/name)"
assert_contains "$STOP_OUT" "@" \
    "error includes the @remote_host marker"
# grep interprets leading `--` as a flag; check via fgrep-style search.
if echo "$STOP_OUT" | grep -F -q -- "--force"; then
    pass "error suggests the --force flag"
else
    echo "  actual: $STOP_OUT"
    fail "error should suggest --force"
fi

echo ""
echo "=== Step 4: SSG is still running (stop was correctly refused) ==="

DOCKER_PS=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS" "${SSG_PROJECT}-ssg" "${SSG_PROJECT}-ssg container is still running"

PS_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_OUT" | head -10
assert_contains "$PS_OUT" "postgres" "ssg ps still shows postgres"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG STOP BLOCKED BY REMOTE TESTS PASSED"
echo "==========================================="
