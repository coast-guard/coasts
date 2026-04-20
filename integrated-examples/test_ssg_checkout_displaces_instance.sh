#!/usr/bin/env bash
#
# Integration test: `coast ssg checkout` displaces a coast instance
# that is currently holding the canonical port (Phase 6, DESIGN.md §12).
#
# Setup:
#   1. Run coast `canon-a` whose own inner service listens on
#      canonical port 5432 (from `coast-canonical-5432-app`).
#   2. `coast checkout canon-a` binds localhost:5432 to that coast's
#      dynamic port (its own socat).
#   3. Write a probe row into that postgres so we can identify it.
#
# Trigger:
#   4. Start SSG + `coast ssg checkout postgres`.
#
# Assertions:
#   5. CLI output mentions displacement of the coast.
#   6. psql via localhost:5432 now hits the SSG postgres (different
#      instance — probe row is missing).
#   7. `coast ssg uncheckout postgres` releases the port.
#   8. The displaced coast is NOT auto-restored on canonical 5432 —
#      DESIGN §12 contract. (psql against localhost:5432 fails.)

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
echo "=== Step 1: run the canonical-5432 coast ==="

cd "$PROJECTS_DIR/coast-canonical-5432-app"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("canon-a")
RUN_OUT=$("$COAST" run canon-a 2>&1)
echo "$RUN_OUT" | tail -10
assert_contains "$RUN_OUT" "Created coast instance 'canon-a'" "coast run succeeds"

# Wait for the inner postgres to finish initdb.
sleep 8

echo ""
echo "=== Step 2: coast checkout canon-a binds localhost:5432 ==="

set +e
CHECKOUT_OUT=$("$COAST" checkout canon-a 2>&1)
CHECKOUT_RC=$?
set -e
echo "$CHECKOUT_OUT"
if [ "$CHECKOUT_RC" -ne 0 ]; then
    echo "--- coastd log tail ---"
    tail -30 /tmp/coastd-test.log 2>/dev/null || true
    fail "coast checkout canon-a failed (exit $CHECKOUT_RC)"
fi
# Give the inner postgres time to finish initdb (it has to be
# reachable via the checkout socat's upstream port).
sleep 8

# Probe by reading from the actual TCP socket rather than relying
# on `nc -z`, which has been unreliable under DinDinD. The bash
# `/dev/tcp/127.0.0.1/5432` redirect fails loudly if no listener
# accepts on 5432.
if ! (exec 3<>/dev/tcp/127.0.0.1/5432) 2>/dev/null; then
    echo "--- pgrep socat ---"
    pgrep -af socat 2>&1 || true
    echo "--- coastd log tail ---"
    tail -30 /tmp/coastd-test.log 2>/dev/null || true
    fail "port 5432 should accept TCP after coast checkout canon-a"
fi
exec 3>&-
pass "port 5432 accepts TCP after coast checkout"

# Write a unique probe row so we can tell whose DB we hit later.
# (The harness host lacks psql; borrow one from a throwaway postgres
# container on the host network.)
host_psql() {
    docker run --rm --network=host -e PGPASSWORD=dev \
        postgres:16-alpine psql "postgres://postgres:dev@127.0.0.1:5432/postgres" "$@"
}

PROBE_OUT=$(host_psql \
    -c 'CREATE TABLE IF NOT EXISTS ssg_displacement_probe(name TEXT);' \
    -c "INSERT INTO ssg_displacement_probe VALUES ('canon-a-owner');" 2>&1)
echo "$PROBE_OUT"
# We need this probe to succeed so the displacement assertion is
# meaningful.
assert_contains "$PROBE_OUT" "INSERT" "probe row written to the canon-a database"

echo ""
echo "=== Step 3: SSG up + ssg checkout postgres displaces canon-a ==="

cd "$PROJECTS_DIR/coast-ssg-auto-db"
"$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-auto-db" >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 6

CHECKOUT_OUT=$("$COAST" ssg checkout --service postgres 2>&1)
echo "$CHECKOUT_OUT"
assert_contains "$CHECKOUT_OUT" "Displaced coast" "ssg checkout warns about the displaced coast"
assert_contains "$CHECKOUT_OUT" "canon-a" "warning names the displaced instance"
assert_contains "$CHECKOUT_OUT" "5432" "warning names the canonical port"
assert_contains "$CHECKOUT_OUT" "SSG checkout" "ssg checkout still reports success overall"

sleep 1

echo ""
echo "=== Step 4: localhost:5432 now hits the SSG postgres (not canon-a) ==="

# The SSG postgres has auto_create_db but NO ssg_displacement_probe
# table (that only exists in the canon-a postgres). Querying it
# should fail with a "relation does not exist" error.
set +e
PSQL_PROBE=$(host_psql -c 'SELECT name FROM ssg_displacement_probe;' 2>&1)
PSQL_RC=$?
set -e
echo "$PSQL_PROBE"
if [ "$PSQL_RC" -eq 0 ]; then
    fail "expected probe query to fail (table should not exist in SSG postgres)"
fi
# Accept either relation-not-exist or the current_database check below.
if ! echo "$PSQL_PROBE" | grep -q "does not exist"; then
    echo "unexpected failure mode:"
    echo "$PSQL_PROBE"
    fail "expected 'relation ... does not exist' from SSG postgres"
fi
pass "probe table missing in localhost:5432 — we are hitting the SSG postgres"

# Sanity: the SSG postgres does respond to SELECT 42.
SANITY=$(host_psql -c 'SELECT 42 AS answer;' 2>&1)
assert_contains "$SANITY" "42" "SSG postgres answers SELECT 42 via localhost:5432"

echo ""
echo "=== Step 5: coast ssg uncheckout releases the port ==="

"$COAST" ssg uncheckout --service postgres >/dev/null 2>&1
sleep 1

if nc -z -w1 localhost 5432 2>/dev/null; then
    fail "port 5432 is still bound after uncheckout (no auto-restore expected)"
fi
pass "port 5432 free; displaced coast is NOT auto-restored (DESIGN contract)"

# Cleanup.
"$COAST" rm canon-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  ALL SSG CHECKOUT DISPLACEMENT TESTS PASSED"
echo "==========================================="
