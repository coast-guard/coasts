#!/usr/bin/env bash
#
# Integration test: `coast ssg doctor` (Phase 8).
#
# Exercises the read-only permission check against an SSG build whose
# postgres:16 service has a host bind-mount at
# `$COAST_SSG_DOCTOR_HOST_ROOT/pg-data`. Three cases:
#
#   1. Directory owned by root:root (UID 0) -> `warn` finding citing
#      the expected 999:999 ownership.
#   2. Directory missing -> `info` finding.
#   3. Directory owned by 999:999 -> `ok` finding.
#
# Doctor never modifies anything — the test flips the ownership
# between cases with `chown` on the host.
#
# Prerequisites:
#   - Docker running
#   - socat installed
#   - Coast binaries built (cargo build --release)
#   - Running as root (dindind harness default)
#
# Usage:
#   ./integrated-examples/test_ssg_doctor.sh

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

register_cleanup

preflight_checks

echo ""
echo "=== Setup ==="

clean_slate

"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

# Reset any prior SSG state from other runs.
rm -rf "$HOME/.coast/ssg"
docker rm -f coast-ssg 2>/dev/null || true

# Host root for the doctor fixture's bind mount. Same pattern as
# test_ssg_bind_mount_symmetric.sh.
export COAST_SSG_DOCTOR_HOST_ROOT="${COAST_SSG_DOCTOR_HOST_ROOT:-$HOME/coast-ssg-doctor}"
HOST_PATH="$COAST_SSG_DOCTOR_HOST_ROOT/pg-data"

cd "$PROJECTS_DIR/coast-ssg-doctor"

start_daemon

# ============================================================
# Build the SSG once. Doctor only needs the manifest; it does not
# start the container.
# ============================================================

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT"
assert_contains "$BUILD_OUT" "Build complete" "coast ssg build succeeds"

# ============================================================
# Case 1: directory missing -> info finding
# ============================================================

echo ""
echo "=== Case 1: host bind dir missing -> info ==="

rm -rf "$HOST_PATH"
[ ! -e "$HOST_PATH" ] || fail "host bind dir should not exist for case 1"

INFO_OUT=$("$COAST" ssg doctor 2>&1)
echo "$INFO_OUT"
assert_contains "$INFO_OUT" "info" "doctor emits an info finding"
assert_contains "$INFO_OUT" "postgres" "info finding names the service"
assert_contains "$INFO_OUT" "does not exist" "info finding explains why"

# ============================================================
# Case 2: directory owned by root -> warn finding
# ============================================================

echo ""
echo "=== Case 2: host bind dir owned by root -> warn ==="

mkdir -p "$HOST_PATH"
chown 0:0 "$HOST_PATH"
STAT_OWNER=$(stat -c '%u:%g' "$HOST_PATH")
assert_eq "$STAT_OWNER" "0:0" "root ownership set"

WARN_OUT=$("$COAST" ssg doctor 2>&1)
echo "$WARN_OUT"
assert_contains "$WARN_OUT" "warn" "doctor emits a warn finding"
assert_contains "$WARN_OUT" "999:999" "warn finding cites expected UID/GID"
assert_contains "$WARN_OUT" "sudo chown" "warn finding includes the chown remediation"

# ============================================================
# Case 3: directory owned by 999:999 -> ok finding
# ============================================================

echo ""
echo "=== Case 3: host bind dir owned by 999:999 -> ok ==="

chown 999:999 "$HOST_PATH"
STAT_OWNER=$(stat -c '%u:%g' "$HOST_PATH")
assert_eq "$STAT_OWNER" "999:999" "postgres ownership set"

OK_OUT=$("$COAST" ssg doctor 2>&1)
echo "$OK_OUT"
assert_contains "$OK_OUT" "ok" "doctor emits an ok finding"
assert_contains "$OK_OUT" "Owner matches 999:999" "ok finding cites the match"
# Must not mention 'warn' anywhere — the whole point of ok is no warnings.
if echo "$OK_OUT" | grep -q "warn"; then
    fail "ok output should not contain any 'warn' finding"
fi
pass "no warnings when ownership matches"

# ============================================================
# Bonus: doctor is read-only (file mtime stays put across runs)
# ============================================================

echo ""
echo "=== Bonus: doctor does not modify the bind dir ==="

BEFORE=$(stat -c '%Y' "$HOST_PATH")
"$COAST" ssg doctor >/dev/null 2>&1
sleep 1
AFTER=$(stat -c '%Y' "$HOST_PATH")
assert_eq "$BEFORE" "$AFTER" "doctor never touches the bind directory"

echo ""
echo "==========================================="
echo "  ALL SSG DOCTOR TESTS PASSED"
echo "==========================================="
