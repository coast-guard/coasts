#!/usr/bin/env bash
#
# Integration test (Phase 31, DESIGN §24.5): the project's virtual
# port is stable across `ssg build` + `ssg run` cycles. Rebuilding
# the SSG image does NOT release the virtual port allocation, so
# consumer in-DinD socats keep pointing at the same upstream forever
# (until `ssg rm --with-data`).
#
# Scenario:
#   1. Build + run SSG. Capture VPORT_INITIAL from `coast ssg ports`.
#   2. Mutate the SSG Coastfile (force a new build_id) + `ssg build`
#      + `ssg run`. The container is recreated; the dyn port likely
#      changes. Capture VPORT_AFTER_BUILD.
#   3. Assert VPORT_INITIAL == VPORT_AFTER_BUILD.
#   4. `ssg stop` then `ssg run` (no rebuild). Capture
#      VPORT_AFTER_RESTART. Assert it matches.
#   5. Sanity: ssh -R / consumer socat are NOT exercised here — those
#      are covered by the remote and rm-run-refreshes tests; this
#      test isolates the virtual_port allocation persistence
#      contract.

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
echo "=== Step 1: initial SSG build + run ==="

cd "$PROJECTS_DIR/coast-ssg-consumer-basic"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 3

PORTS_INITIAL=$("$COAST" ssg ports 2>&1)
echo "$PORTS_INITIAL"
DYN_INITIAL=$(echo "$PORTS_INITIAL" | awk '/^  postgres/ {print $3}')
VPORT_INITIAL=$(echo "$PORTS_INITIAL" | awk '/^  postgres/ {print $4}')
[ -n "$VPORT_INITIAL" ] && [ "$VPORT_INITIAL" != "--" ] \
    || fail "expected a virtual port from `coast ssg ports` col 4; got '$VPORT_INITIAL'"
pass "initial: dyn=$DYN_INITIAL virtual=$VPORT_INITIAL"

echo ""
echo "=== Step 2: rebuild SSG with a mutated Coastfile -> dyn changes, virtual stable ==="

# Append a comment line to force a fresh build_id.
echo "" >> Coastfile.shared_service_groups
echo "# phase31 vport-stable-across-rebuild marker" >> Coastfile.shared_service_groups
"$COAST" ssg build >/dev/null 2>&1
"$COAST" ssg run >/dev/null 2>&1
sleep 3

PORTS_AFTER_BUILD=$("$COAST" ssg ports 2>&1)
echo "$PORTS_AFTER_BUILD"
DYN_AFTER_BUILD=$(echo "$PORTS_AFTER_BUILD" | awk '/^  postgres/ {print $3}')
VPORT_AFTER_BUILD=$(echo "$PORTS_AFTER_BUILD" | awk '/^  postgres/ {print $4}')
[ -n "$VPORT_AFTER_BUILD" ] && [ "$VPORT_AFTER_BUILD" != "--" ] \
    || fail "expected a virtual port post-rebuild; got '$VPORT_AFTER_BUILD'"

[ "$VPORT_AFTER_BUILD" = "$VPORT_INITIAL" ] \
    || fail "Phase 31 violation: virtual port changed across `ssg build` + `ssg run` ($VPORT_INITIAL -> $VPORT_AFTER_BUILD); the host-owned allocation must persist"
pass "virtual port stable across rebuild: $VPORT_INITIAL"

if [ "$DYN_AFTER_BUILD" = "$DYN_INITIAL" ]; then
    echo "(dyn port unchanged at $DYN_AFTER_BUILD — kernel reused the ephemeral slot; not a concern for this test)"
else
    echo "(dyn port changed: $DYN_INITIAL -> $DYN_AFTER_BUILD; the host socat absorbed the swap)"
fi

echo ""
echo "=== Step 3: ssg stop + ssg run -> virtual still stable ==="

"$COAST" ssg stop >/dev/null 2>&1 || true
sleep 1
"$COAST" ssg run >/dev/null 2>&1
sleep 3

PORTS_AFTER_RESTART=$("$COAST" ssg ports 2>&1)
VPORT_AFTER_RESTART=$(echo "$PORTS_AFTER_RESTART" | awk '/^  postgres/ {print $4}')
[ "$VPORT_AFTER_RESTART" = "$VPORT_INITIAL" ] \
    || fail "Phase 31 violation: virtual port changed across `ssg stop`/`ssg run` ($VPORT_INITIAL -> $VPORT_AFTER_RESTART)"
pass "virtual port stable across stop+run: $VPORT_INITIAL"

# Restore Coastfile so other tests don't see the marker line.
git -C . checkout -- Coastfile.shared_service_groups 2>/dev/null || \
    sed -i.bak '/# phase31 vport-stable-across-rebuild marker/d' Coastfile.shared_service_groups
rm -f Coastfile.shared_service_groups.bak

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

echo ""
echo "==========================================="
echo "  PHASE 31 VPORT-STABLE-ACROSS-REBUILD OK"
echo "==========================================="
