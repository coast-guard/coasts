#!/usr/bin/env bash
#
# Integration test: Phase 28 contract — consumer in-DinD socat argv
# is STABLE across an SSG `rm --with-data` + `run` cycle.
#
# Before Phase 28 (legacy `consumer_refresh.rs` machinery), an SSG
# rebuild reallocated the dynamic host port and the daemon docker-
# execed into every running consumer to rewrite its socat argv. This
# test originally asserted on that machinery: "consumer socat argv
# changed OLD_DYN -> NEW_DYN" and "ssg run response contains
# 'Refreshed shared-service proxies'".
#
# Phase 28 replaced that machinery with a host-side socat supervisor
# (see `coast-ssg/DESIGN.md §24`). The consumer's in-DinD socat
# always forwards to the project's stable VIRTUAL port; only the
# daemon-managed host socat (`coast-daemon::handlers::ssg::host_socat`)
# tracks the SSG's ephemeral dyn port. Result:
#
#   1. Consumer socat argv MUST NOT change across `ssg rm --with-data`
#      + `ssg run` (its upstream is the virtual port, not the dyn).
#   2. The lifecycle response message announces "Refreshed host socats
#      for: <project>/<service>:<port>" (new format) instead of the
#      old "Refreshed shared-service proxies for: ...".
#   3. psql from the consumer still works, even though we never
#      touched the consumer container.
#
# Test rename to something like `test_ssg_consumer_socat_stable_across_rebuild.sh`
# is deferred to Phase 31's integration sweep.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-consumer-basic"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

start_daemon

# ============================================================
# Step 1: first SSG + consumer cycle. Capture the initial dynamic
# host port + the consumer's in-DinD socat upstream port (which
# Phase 28 says is the VIRTUAL port, not the dyn port).
# ============================================================

echo ""
echo "=== Step 1: first SSG run + consumer run ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
SSG_BUILD_OUT=$("$COAST" ssg build 2>&1)
assert_contains "$SSG_BUILD_OUT" "Build complete" "initial ssg build succeeds"

SSG_RUN_OUT=$("$COAST" ssg run 2>&1)
assert_contains "$SSG_RUN_OUT" "SSG running" "initial ssg run succeeds"

OLD_DYN=$("$COAST" ssg ports 2>&1 | awk '/^  postgres/ {print $3}')
[ -n "$OLD_DYN" ] || fail "could not extract initial SSG postgres dynamic port"
pass "initial SSG postgres dynamic host port = $OLD_DYN"

sleep 5  # initdb inside SSG

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" build >/dev/null 2>&1
CLEANUP_INSTANCES+=("inst-a")
"$COAST" run inst-a >/dev/null 2>&1

sleep 3  # inner compose for the consumer

# Phase 28: capture the consumer's in-DinD socat upstream from
# `ps`. The argv looks like:
#   socat TCP-LISTEN:5432,fork,reuseaddr,bind=... TCP:host.docker.internal:<port>
# The `<port>` is the VIRTUAL port — it must stay the same across
# ssg rm/run. We extract it via the `:` after `host.docker.internal`.
#
# `coast-ssg-consumer-basic-inst-a` is the consumer's outer DinD
# container (project-instance naming). We `docker exec` directly on
# the host because `coast exec` only runs INSIDE the inner compose
# service, not on the DinD shell that hosts the in-DinD socat.
CONSUMER_DIND="coast-ssg-consumer-basic-coasts-inst-a"
extract_consumer_socat_upstream() {
    docker exec "$CONSUMER_DIND" sh -c \
        'ps -ef | grep -E "socat .* TCP:host\.docker\.internal:" | grep -v grep' \
        2>/dev/null \
        | grep -oE 'TCP:host\.docker\.internal:[0-9]+' \
        | head -1 \
        | awk -F: '{print $NF}'
}

INITIAL_UPSTREAM=$(extract_consumer_socat_upstream)
[ -n "$INITIAL_UPSTREAM" ] || fail "could not extract consumer socat upstream port from $CONSUMER_DIND"
pass "consumer in-DinD socat upstream port (phase 28: virtual port) = $INITIAL_UPSTREAM"

# Phase 28 sanity: the consumer upstream is a host-owned virtual
# port (band defaults to 42000-43000), NOT the SSG's dyn port.
if [ "$INITIAL_UPSTREAM" = "$OLD_DYN" ]; then
    fail "consumer upstream ($INITIAL_UPSTREAM) matched the SSG dyn port ($OLD_DYN); \
this means Phase 28's host_socat layer is NOT in front of the consumer — \
either reconcile_project never ran or synthesize_configs_for_consumer \
didn't substitute the virtual port"
fi
pass "consumer upstream is distinct from SSG dyn port (phase 28 layering verified)"

echo ""
echo "=== Step 2: baseline psql from consumer works ==="

PSQL_BASELINE=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 1 AS baseline;'" 2>&1)
echo "$PSQL_BASELINE"
assert_contains "$PSQL_BASELINE" "baseline" "baseline psql returns column header"
assert_contains "$PSQL_BASELINE" "1" "baseline psql returns the row"

# ============================================================
# Step 3: rip the SSG out with its data and bring a fresh one up.
# This forces a new dyn port allocation at the SSG side. Phase 28
# contract: the consumer's in-DinD socat upstream (the virtual
# port) must still be the same value afterwards.
# ============================================================

echo ""
echo "=== Step 3: ssg rm + ssg run to force fresh dyn port ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"

NEW_DYN=""
for attempt in 1 2 3 4 5; do
    "$COAST" ssg rm --with-data >/dev/null 2>&1 || true
    "$COAST" ssg build >/dev/null 2>&1
    SSG_RERUN_OUT=$("$COAST" ssg run 2>&1)
    assert_contains "$SSG_RERUN_OUT" "SSG running" "ssg rerun succeeds (attempt $attempt)"
    CANDIDATE=$("$COAST" ssg ports 2>&1 | awk '/^  postgres/ {print $3}')
    [ -n "$CANDIDATE" ] || fail "could not extract SSG postgres dynamic port after rerun"
    if [ "$CANDIDATE" != "$OLD_DYN" ]; then
        NEW_DYN="$CANDIDATE"
        FINAL_RERUN_OUT="$SSG_RERUN_OUT"
        break
    fi
    echo "attempt $attempt: dyn port reallocated to same value ($CANDIDATE); retrying"
done

if [ -z "$NEW_DYN" ]; then
    fail "SSG dyn port never reallocated across 5 attempts; kernel ephemeral range too narrow"
fi
pass "SSG postgres dynamic host port changed: $OLD_DYN -> $NEW_DYN"

# Phase 28: rm --with-data clears the virtual port allocation, so
# a brand-new virtual port may have been chosen on the rerun. Either
# value is acceptable — the test instead asserts that AFTER the
# rerun completes, the consumer's socat upstream still points at
# whatever virtual port the project owns. The legacy contract was
# the inverse: assert the upstream had CHANGED to a new dyn port.

echo ""
echo "=== Step 4: ssg run response announces host socat refresh ==="

echo "$FINAL_RERUN_OUT" | tail -10
assert_contains "$FINAL_RERUN_OUT" "Refreshed host socats for" \
    "ssg run response announces host socat refresh (Phase 28)"
assert_contains "$FINAL_RERUN_OUT" "${SSG_PROJECT}/postgres:5432" \
    "host socat refresh names the (project/service:container_port) triple"
# Phase 28: the legacy "Refreshed shared-service proxies" string
# must NOT appear — the docker-exec-into-consumer machinery is
# gone.
if echo "$FINAL_RERUN_OUT" | grep -q "Refreshed shared-service proxies"; then
    fail "legacy 'Refreshed shared-service proxies' message appeared; consumer_refresh \
machinery should be deleted in Phase 28"
fi
pass "legacy consumer-refresh message absent (consumer_refresh.rs deletion verified)"

# ============================================================
# Step 5: the running consumer's psql still works without ANY
# refresh into the consumer container. Phase 28's host socat
# now forwards the consumer's stable virtual port to the new
# dyn port; the consumer never knew anything changed.
# ============================================================

echo ""
echo "=== Step 5: consumer psql works against the NEW SSG without consumer refresh ==="

sleep 6  # initdb on the fresh postgres

PSQL_REFRESHED=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres \
     -c 'SELECT 42 AS refreshed;' -v ON_ERROR_STOP=1 2>&1 || true")
echo "$PSQL_REFRESHED"
assert_contains "$PSQL_REFRESHED" "refreshed" "post-refresh psql returns column header"
assert_contains "$PSQL_REFRESHED" "42" "post-refresh psql returns the row"

pass "consumer psql routed to the NEW SSG dyn port without rebuilding or re-execing the consumer"

# ============================================================
# Step 6: the consumer's in-DinD socat upstream is UNCHANGED
# across the rebuild — the whole point of Phase 28. Compare the
# port we captured before the rerun with the current value.
# ============================================================

echo ""
echo "=== Step 6: consumer socat argv stable (Phase 28 invariant) ==="

POST_UPSTREAM=$(extract_consumer_socat_upstream)
[ -n "$POST_UPSTREAM" ] || fail "could not extract consumer socat upstream port post-rerun"

# Phase 28: `rm --with-data` cleared `ssg_virtual_ports`, so a
# fresh virtual port may have been allocated. The consumer was
# already running, so its in-DinD socat still has the OLD
# upstream — the rebound notice in `FINAL_RERUN_OUT` warns the
# user about exactly this case. If the SSG run didn't rebind
# (the same virtual port was reissued), then the upstreams must
# match.
REBOUND_DETECTED=$(echo "$FINAL_RERUN_OUT" | grep -E "WARNING:.*virtual port rebound" || true)
if [ -n "$REBOUND_DETECTED" ]; then
    echo "rebound warning was emitted ($REBOUND_DETECTED) — consumer upstream may be stale"
    echo "this is the documented Phase 28 collision-rebind path (DESIGN §24); test \
exits success because the warning surfaced correctly"
else
    if [ "$POST_UPSTREAM" != "$INITIAL_UPSTREAM" ]; then
        fail "consumer in-DinD socat upstream changed without a rebind notice: \
$INITIAL_UPSTREAM -> $POST_UPSTREAM (Phase 28 contract violated)"
    fi
    pass "consumer in-DinD socat upstream unchanged: $INITIAL_UPSTREAM (Phase 28 contract upheld)"
fi

# Cleanup.
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG CONSUMER STABLE-SOCAT TESTS PASSED"
echo "==========================================="
