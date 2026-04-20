#!/usr/bin/env bash
#
# Integration test: `coast ssg checkout / uncheckout` + stop/start
# cycle (Phase 6, DESIGN.md §12).
#
# Verifies the host-side canonical-port socat:
#   1. `coast ssg checkout postgres` binds `localhost:5432`.
#   2. `psql "postgres://...@localhost:5432/..."` works through the
#       socat -> SSG dynamic port -> inner postgres path.
#   3. `coast ssg ports` shows `(checked out)`.
#   4. `coast ssg stop` kills the socat (psql fails again).
#   5. `coast ssg start` re-spawns it automatically (psql works).
#   6. `coast ssg uncheckout postgres` frees the port.
#
# Uses the Phase 5 `coast-ssg-auto-db` fixture (has POSTGRES_PASSWORD=dev
# which matches the canonical `postgres:dev@` credential used everywhere).

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--coast--ssg" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

start_daemon

echo ""
echo "=== Step 1: SSG up ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
RUN_OUT=$("$COAST" ssg run 2>&1)
echo "$RUN_OUT" | tail -5
assert_contains "$RUN_OUT" "SSG running" "ssg run succeeds"

PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
SSG_DYNAMIC=$(echo "$PORTS_OUT" | awk '/^  postgres/ {print $3}')
[ -n "$SSG_DYNAMIC" ] || fail "could not extract SSG postgres dynamic port"
pass "SSG postgres dynamic host port = $SSG_DYNAMIC"

# Wait for postgres initdb to finish inside the SSG.
sleep 6

echo ""
echo "=== Step 2: before checkout, localhost:5432 should be free ==="

# nc returns 0 if the port accepts a connection; we expect it to fail.
if nc -z -w1 localhost 5432 2>/dev/null; then
    fail "port 5432 is already in use on the host before checkout"
fi
pass "port 5432 is free"

echo ""
echo "=== Step 3: coast ssg checkout postgres ==="

CHECKOUT_OUT=$("$COAST" ssg checkout --service postgres 2>&1)
echo "$CHECKOUT_OUT"
assert_contains "$CHECKOUT_OUT" "SSG checkout" "checkout command returns success summary"
assert_contains "$CHECKOUT_OUT" "postgres on canonical 5432" "checkout lists applied service"

# Give the socat a beat to bind.
sleep 1

echo ""
echo "=== Step 4: psql via canonical localhost:5432 succeeds ==="

# The harness host doesn't ship with psql; use a throwaway postgres
# container on the host network to get one.
host_psql() {
    docker run --rm --network=host -e PGPASSWORD=dev \
        postgres:16-alpine psql "postgres://postgres:dev@127.0.0.1:5432/postgres" "$@"
}

PSQL_OUT=$(host_psql -c 'SELECT 42 AS answer;' 2>&1)
echo "$PSQL_OUT"
assert_contains "$PSQL_OUT" "answer" "psql returns column header through checkout"
assert_contains "$PSQL_OUT" "42" "psql returns the answer row"

echo ""
echo "=== Step 5: coast ssg ports annotates the checked-out row ==="

PORTS_OUT=$("$COAST" ssg ports 2>&1)
echo "$PORTS_OUT"
assert_contains "$PORTS_OUT" "(checked out)" "ports output marks postgres as checked out"

echo ""
echo "=== Step 6: coast ssg stop kills the socat ==="

"$COAST" ssg stop >/dev/null 2>&1
sleep 1

if nc -z -w1 localhost 5432 2>/dev/null; then
    fail "port 5432 is still bound after ssg stop (socat should have been killed)"
fi
pass "port 5432 is free after stop"

echo ""
echo "=== Step 7: coast ssg start re-spawns the checkout socat ==="

"$COAST" ssg start >/dev/null 2>&1
sleep 6

PSQL_AFTER_START=$(host_psql -c 'SELECT 42 AS answer;' 2>&1)
echo "$PSQL_AFTER_START"
assert_contains "$PSQL_AFTER_START" "42" "psql works again after start re-spawns the socat"

echo ""
echo "=== Step 8: coast ssg uncheckout postgres ==="

UNCHECKOUT_OUT=$("$COAST" ssg uncheckout --service postgres 2>&1)
echo "$UNCHECKOUT_OUT"
assert_contains "$UNCHECKOUT_OUT" "SSG uncheckout complete" "uncheckout succeeds"

sleep 1

if nc -z -w1 localhost 5432 2>/dev/null; then
    fail "port 5432 is still bound after uncheckout"
fi
pass "port 5432 is free after uncheckout"

PORTS_FINAL=$("$COAST" ssg ports 2>&1)
if echo "$PORTS_FINAL" | grep -q "(checked out)"; then
    fail "ports output still contains '(checked out)' after uncheckout"
fi
pass "ports output cleared the checkout annotation"

# Cleanup.
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG HOST CHECKOUT TESTS PASSED"
echo "==========================================="
