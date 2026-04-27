#!/usr/bin/env bash
#
# Integration test (Phase 31, DESIGN §24.5): when the SSG container
# is restarted OUTSIDE the daemon (via plain `docker restart`), the
# DYN port likely changes but the project's virtual port stays the
# same. The next `coast ssg lifecycle` verb (here `ssg restart`)
# triggers `host_socat::reconcile_project` which detects the dyn
# port change and refreshes the host socat's upstream — consumers
# resume working without rebuilds.
#
# Scenario:
#   1. Build + run SSG. Capture VPORT + DYN_OLD.
#   2. `docker restart {project}-ssg`. Wait for it to come up.
#      The `dynamic_host_port` Docker assigns may differ.
#   3. Capture DYN_NEW from `docker inspect`. If it equals DYN_OLD,
#      skip the assertion (kernel reused the slot) — this test is
#      best-effort on that front.
#   4. `coast ssg restart` → reconcile_project re-runs.
#   5. After the refresh, `coast ssg ports` virtual port is still
#      the SAME (VPORT). The host_socat's argv now references the
#      new dyn port if it changed.
#   6. From a consumer, psql still works — that's the user-visible
#      success criterion.

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

echo ""
echo "=== Step 1: SSG build + run + consumer up ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 5

PORTS_INITIAL=$("$COAST" ssg ports 2>&1)
DYN_OLD=$(echo "$PORTS_INITIAL" | awk '/^  postgres/ {print $3}')
VPORT=$(echo "$PORTS_INITIAL" | awk '/^  postgres/ {print $4}')
[ -n "$VPORT" ] && [ "$VPORT" != "--" ] || fail "missing virtual port"
pass "initial: dyn=$DYN_OLD virtual=$VPORT"

"$COAST" build >/dev/null 2>&1
CLEANUP_INSTANCES+=("inst-a")
"$COAST" run inst-a >/dev/null 2>&1
sleep 4

# Sanity: psql works through the consumer before we mess with
# Docker.
PSQL_BEFORE=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 1 AS pre;'" 2>&1)
echo "$PSQL_BEFORE" | tail -3
echo "$PSQL_BEFORE" | grep -q "pre" || fail "baseline psql failed"
pass "baseline psql works (pre-restart)"

echo ""
echo "=== Step 2: docker restart the SSG container OUTSIDE the daemon ==="

docker restart "${SSG_PROJECT}-ssg" >/dev/null
# Inner DinD + postgres need a moment to come up.
sleep 8

# Inspect the host port Docker now reports for the SSG's published
# 5432/tcp. This is the `dynamic_host_port` post-restart.
DYN_NEW=$(docker inspect "${SSG_PROJECT}-ssg" \
    --format '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' 2>/dev/null \
    | tr -d '[:space:]')
if [ -z "$DYN_NEW" ]; then
    fail "could not inspect new dyn port from ${SSG_PROJECT}-ssg after docker restart"
fi
echo "post-restart dyn port from docker: $DYN_NEW"

if [ "$DYN_NEW" = "$DYN_OLD" ]; then
    echo "(NOTE: kernel reused the same dyn port across the restart; the host_socat argv refresh path won't be exercised but the test still validates that virtual port + consumer still work)"
fi

echo ""
echo "=== Step 3: ssg restart -> reconcile_project refreshes host socat ==="

"$COAST" ssg restart >/dev/null 2>&1
sleep 3

PORTS_AFTER=$("$COAST" ssg ports 2>&1)
echo "$PORTS_AFTER"
VPORT_AFTER=$(echo "$PORTS_AFTER" | awk '/^  postgres/ {print $4}')
DYN_AFTER=$(echo "$PORTS_AFTER" | awk '/^  postgres/ {print $3}')

[ "$VPORT_AFTER" = "$VPORT" ] \
    || fail "virtual port changed across docker restart + ssg restart: $VPORT -> $VPORT_AFTER"
pass "virtual port stable: $VPORT (Phase 28 contract)"

# Phase 28: host_socat's argv sidecar should now reference the
# (possibly-new) dyn port. Read the argvfile and look for the dyn
# value the daemon recorded post-restart.
ARGVFILE="$HOME/.coast/socats/${SSG_PROJECT}--postgres--5432.pid.argv"
if [ -f "$ARGVFILE" ]; then
    if ! grep -q "host.docker.internal:${DYN_AFTER}" "$ARGVFILE"; then
        fail "host socat argv $ARGVFILE doesn't reference the post-restart dyn ($DYN_AFTER): $(cat "$ARGVFILE")"
    fi
    pass "host_socat argv references the daemon's post-restart dyn ($DYN_AFTER)"
fi

echo ""
echo "=== Step 4: consumer psql still works through the refreshed tunnel ==="

# Give postgres a moment to finish recovery inside the restarted SSG.
sleep 4

PSQL_AFTER=$("$COAST" exec inst-a --service app -- sh -c \
    "PGPASSWORD=coast psql -h postgres -U postgres -d postgres -c 'SELECT 42 AS post;' -v ON_ERROR_STOP=1" 2>&1 || true)
echo "$PSQL_AFTER" | tail -5
echo "$PSQL_AFTER" | grep -q "post" \
    || fail "consumer psql failed after docker restart + ssg restart; reconcile_project did not refresh routing"
pass "consumer psql works after docker-side restart + ssg restart"

# Cleanup
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  PHASE 31 DOCKER-RESTART-OUTSIDE-DAEMON OK"
echo "==========================================="
