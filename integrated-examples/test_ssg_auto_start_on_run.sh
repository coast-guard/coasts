#!/usr/bin/env bash
#
# Integration test: `coast run` auto-starts the SSG when a consumer
# coast references SSG services (Phase 3.5).
#
# Verifies two flows:
#
#  1. Positive: an SSG build exists (but the SSG is NOT running), and a
#     consumer coast declaring `[shared_services.<name>] from_group =
#     true` runs. `coast run` auto-starts the SSG and the singleton
#     `coast-ssg` container is up when `coast run` returns. The run
#     progress stream includes an `Ensure SSG ready` step.
#
#  2. Negative: no SSG build exists at all. `coast run` on the
#     consumer fails fast with the DESIGN.md §11.1 verbatim error
#     mentioning the consumer project name and the referenced SSG
#     service name.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#
# Usage:
#   ./integrated-examples/test_ssg_auto_start_on_run.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# Phase 25: per-project SSG naming (§23) — SSG container is `{project}-ssg`.
# Under Phase 23's per-project contract, the consumer project owns its
# own SSG. Build the SSG from the consumer's cwd so the auto-start
# path resolves against this project.
SSG_PROJECT="coast-ssg-consumer"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG + consumer state from other runs.
rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

start_daemon

# ============================================================
# Positive case: SSG build exists, SSG not running, consumer run
# triggers auto-start.
# ============================================================

echo ""
echo "=== Positive: SSG build exists, consumer run auto-starts it ==="

# Phase 25: build the SSG from the consumer's cwd so the SSG is
# owned by the consumer's project (Phase 23 per-project contract).
# The consumer fixture now carries its own Coastfile.shared_service_groups
# that mirrors coast-ssg-minimal's postgres service.
cd "$PROJECTS_DIR/coast-ssg-consumer"
BUILD_SSG_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_SSG_OUT" | tail -5
assert_contains "$BUILD_SSG_OUT" "Build complete" "coast ssg build succeeds"

# Sanity: SSG is NOT running before the consumer run.
DOCKER_PS_BEFORE=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_BEFORE" ]; then
    fail "${SSG_PROJECT}-ssg is already running before the consumer run (expected stopped state)"
fi
pass "coast-ssg is not running before the consumer run"

cd "$PROJECTS_DIR/coast-ssg-consumer"
BUILD_CONSUMER_OUT=$("$COAST" build 2>&1)
echo "$BUILD_CONSUMER_OUT" | tail -10
assert_contains "$BUILD_CONSUMER_OUT" "Build" "coast build on consumer succeeds"

CLEANUP_INSTANCES+=("inst-a")
set +e
RUN_OUT=$("$COAST" run inst-a 2>&1)
RUN_EXIT=$?
set -e
echo "$RUN_OUT" | tail -20
if [ "$RUN_EXIT" -ne 0 ]; then
    echo "--- coastd log tail ---"
    tail -60 /tmp/coastd-test.log 2>/dev/null || true
    fail "coast run exited non-zero ($RUN_EXIT) during auto-start"
fi
assert_contains "$RUN_OUT" "Ensure SSG ready" "run output shows the auto-start step"
pass "consumer coast run triggered SSG auto-start"

# Phase 9 SETTLED #35 — the outer `Ensure SSG ready` progress line
# must precede the inner `SSG: ...` prefixed events that the
# auto-start path forwards from the daemon. SsgStarting / SsgStarted
# themselves are daemon-bus events and never reach the CLI; this
# byte-offset ordering on stdout is the user-visible shape of the
# invariant.
#
# Non-TTY `ProgressDisplay` uses `eprint!` without trailing newlines
# for "started" events, so the outer and inner events can end up on
# the same captured line. We compare byte offsets in the raw output
# rather than line numbers.
OUTER_OFFSET=$(printf '%s' "$RUN_OUT" | grep -ob 'Ensure SSG ready' | head -1 | awk -F: '{print $1}')
INNER_OFFSET=$(printf '%s' "$RUN_OUT" | grep -ob 'SSG: ' | head -1 | awk -F: '{print $1}')
if [ -z "$OUTER_OFFSET" ] || [ -z "$INNER_OFFSET" ]; then
    echo "could not locate both outer and inner SSG progress markers; dumping run output:"
    echo "$RUN_OUT"
    fail "missing progress events for auto-start ordering assertion"
fi
if [ "$OUTER_OFFSET" -ge "$INNER_OFFSET" ]; then
    echo "outer_offset=$OUTER_OFFSET inner_offset=$INNER_OFFSET"
    fail "'Ensure SSG ready' must appear before any 'SSG:' prefixed inner event"
fi
pass "SSG auto-start ordering: 'Ensure SSG ready' (byte $OUTER_OFFSET) precedes 'SSG:' inner event (byte $INNER_OFFSET)"

DOCKER_PS_AFTER=$(docker ps --filter "name=^${SSG_PROJECT}-ssg$" --format "{{.Names}}")
assert_eq "$DOCKER_PS_AFTER" "${SSG_PROJECT}-ssg" "${SSG_PROJECT}-ssg container is running after consumer run"

PS_SSG_OUT=$("$COAST" ssg ps 2>&1)
echo "$PS_SSG_OUT"
assert_contains "$PS_SSG_OUT" "postgres" "coast ssg ps shows postgres"

# Cleanup inst-a before the negative case so clean_slate can proceed.
"$COAST" rm inst-a >/dev/null 2>&1 || true
CLEANUP_INSTANCES=()
"$COAST" ssg rm --with-data >/dev/null 2>&1 || true

# ============================================================
# Negative case A: no SSG build exists; `coast build` on the
# consumer hard-errors at build time (DESIGN.md §6). This is the
# new Phase 9 behavior: consumers cannot silently build without an
# active SSG since that would weaken drift detection.
# ============================================================

echo ""
echo "=== Negative A: no SSG build -> coast build hard-errors ==="

# Wipe the SSG state AND the consumer's prior build artifact so we
# start from a truly clean slate.
rm -rf "$HOME/.coast/ssg"
rm -rf "$HOME/.coast/images/coast-ssg-consumer"
cleanup_project_ssgs "$SSG_PROJECT"

cd "$PROJECTS_DIR/coast-ssg-consumer"
BUILD_NEG_OUT=$("$COAST" build 2>&1 || true)
echo "$BUILD_NEG_OUT" | tail -10

assert_contains "$BUILD_NEG_OUT" "no SSG build exists" \
    "coast build error mentions the missing SSG build"
assert_contains "$BUILD_NEG_OUT" "coast ssg build" \
    "coast build error directs the user to run coast ssg build first"
assert_contains "$BUILD_NEG_OUT" "postgres" \
    "coast build error names the referenced SSG service"

# ============================================================
# Negative case B: consumer has a stale build artifact but the
# SSG was removed between build and run. The drift validator
# catches this and fails `coast run` with the DESIGN §6.1 error.
# ============================================================

echo ""
echo "=== Negative B: stale consumer build + missing SSG -> drift error ==="

# Rebuild the SSG + consumer so the consumer has a valid artifact
# with an `ssg` block pointing at the active SSG. Phase 25: build
# SSG from the consumer's own cwd (per-project contract).
cd "$PROJECTS_DIR/coast-ssg-consumer"
"$COAST" ssg build >/dev/null 2>&1
"$COAST" build >/dev/null 2>&1

# Now wipe the SSG so the consumer has a stale reference.
rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

CLEANUP_INSTANCES+=("inst-b")
NEG_OUT=$("$COAST" run inst-b 2>&1 || true)
echo "$NEG_OUT" | tail -15

assert_contains "$NEG_OUT" "SSG has changed since this coast was built" \
    "drift error fires (DESIGN.md \u00a76.1 verbatim prefix)"
assert_contains "$NEG_OUT" "no SSG build exists now" \
    "drift error explains the SSG is gone"

# The instance should NOT have been created on the host.
DOCKER_PS_NEG=$(docker ps -a --filter "name=^coast-ssg-consumer-coasts-inst-b$" --format "{{.Names}}")
if [ -n "$DOCKER_PS_NEG" ]; then
    fail "consumer instance container still exists after negative-case run"
fi
pass "consumer run aborted before creating an instance container"

# --- Done ---

echo ""
echo "==========================================="
echo "  ALL SSG AUTO-START TESTS PASSED"
echo "==========================================="
