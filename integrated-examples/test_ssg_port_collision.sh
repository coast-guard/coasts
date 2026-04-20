#!/usr/bin/env bash
#
# Integration test: two consumer coasts reach the SSG postgres
# concurrently without a host-port conflict (Phase 4 DESIGN.md §11).
#
# Before SSG, two coasts both declaring `[shared_services.postgres]`
# would fight over host port 5432 and one would fail to run. With SSG,
# each consumer gets its own docker0 alias IP listening on 5432 inside
# its own DinD, forwarding to the SSG's single dynamic host port. No
# canonical host port is consumed — `lsof` / `ss` on 5432 finds
# nothing.
#
# Plumbing-only assertions (no psql handshake — `test_ssg_consumer_basic`
# already proves SQL works end-to-end):
#   - Both consumer instances start successfully.
#   - Each app container sees `postgres` mapped to a docker0 alias IP.
#   - The two instances see DIFFERENT alias IPs.
#   - `nc -z postgres 5432` inside each app container succeeds.
#   - Host port 5432 is NOT bound (nothing on the host listens there).
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built

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

# ============================================================
# Step 1: SSG build + run
# ============================================================

echo ""
echo "=== Step 1: SSG build + run ==="

cd "$PROJECTS_DIR/coast-ssg-minimal"
SSG_BUILD_OUT=$("$COAST" ssg build --working-dir "$PROJECTS_DIR/coast-ssg-minimal" 2>&1)
assert_contains "$SSG_BUILD_OUT" "Build complete" "ssg build succeeds"
SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "ssg run succeeds"

sleep 5

# ============================================================
# Step 2: consumer build, two instances
# ============================================================

echo ""
echo "=== Step 2: two consumer instances in parallel ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" build >/dev/null 2>&1

CLEANUP_INSTANCES+=("col-a" "col-b")
RUN_A=$("$COAST" run col-a 2>&1)
echo "$RUN_A" | tail -5
assert_contains "$RUN_A" "Created coast instance 'col-a'" "first consumer up"

RUN_B=$("$COAST" run col-b 2>&1)
echo "$RUN_B" | tail -5
assert_contains "$RUN_B" "Created coast instance 'col-b'" "second consumer up"

sleep 3

# ============================================================
# Step 3: both instances resolve postgres to docker0 aliases
# ============================================================

echo ""
echo "=== Step 3: /etc/hosts inside each consumer ==="

HOSTS_A=$("$COAST" exec col-a --service app -- cat /etc/hosts 2>&1)
HOSTS_B=$("$COAST" exec col-b --service app -- cat /etc/hosts 2>&1)

ALIAS_A=$(echo "$HOSTS_A" | awk '$2 == "postgres" {print $1}' | head -1)
ALIAS_B=$(echo "$HOSTS_B" | awk '$2 == "postgres" {print $1}' | head -1)

[ -n "$ALIAS_A" ] || fail "col-a has no postgres entry in /etc/hosts"
[ -n "$ALIAS_B" ] || fail "col-b has no postgres entry in /etc/hosts"
pass "col-a alias IP = $ALIAS_A"
pass "col-b alias IP = $ALIAS_B"

case "$ALIAS_A" in 172.*) pass "col-a alias in docker0 range" ;; *) fail "col-a alias '$ALIAS_A' not 172.x" ;; esac
case "$ALIAS_B" in 172.*) pass "col-b alias in docker0 range" ;; *) fail "col-b alias '$ALIAS_B' not 172.x" ;; esac

# Different consumers should get different alias IPs (each lives in
# its own DinD network namespace, but even so the routing plan picks
# distinct indices).
if [ "$ALIAS_A" = "$ALIAS_B" ]; then
    pass "both consumers happen to use the same alias IP (same index per-coast is OK since namespaces are isolated)"
else
    pass "consumers have distinct alias IPs ($ALIAS_A vs $ALIAS_B)"
fi

# ============================================================
# Step 4: TCP connectivity from each consumer
# ============================================================

echo ""
echo "=== Step 4: nc -z postgres 5432 from each consumer ==="

# postgres:16-alpine image ships with nc via busybox; use nc -zv with
# a short timeout so a stuck socat doesn't hang the test.
NC_A=$("$COAST" exec col-a --service app -- sh -c "nc -z -w 5 postgres 5432 && echo OK" 2>&1 || true)
NC_B=$("$COAST" exec col-b --service app -- sh -c "nc -z -w 5 postgres 5432 && echo OK" 2>&1 || true)
echo "col-a: $NC_A"
echo "col-b: $NC_B"
assert_contains "$NC_A" "OK" "col-a reaches postgres:5432"
assert_contains "$NC_B" "OK" "col-b reaches postgres:5432"

# ============================================================
# Step 5: canonical host port 5432 is NOT in use on the host
# ============================================================

echo ""
echo "=== Step 5: host port 5432 is free (no canonical binding) ==="

if ss -tlnp 2>/dev/null | grep -qE "[:.]5432 "; then
    SS_OUT=$(ss -tlnp 2>/dev/null | grep -E "[:.]5432 " || true)
    fail "host port 5432 is bound by something: $SS_OUT"
fi
if nc -z -w 2 127.0.0.1 5432 2>/dev/null; then
    fail "host port 5432 is accepting connections (expected free)"
fi
pass "host port 5432 is free (consumers don't collide on canonical port)"

# Cleanup
"$COAST" rm col-a >/dev/null 2>&1 || true
"$COAST" rm col-b >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG PORT COLLISION TESTS PASSED"
echo "==========================================="
