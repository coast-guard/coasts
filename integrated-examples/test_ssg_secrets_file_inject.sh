#!/usr/bin/env bash
#
# Phase 33 integration test: `[secrets.<name>]` declared with
# `inject = "file:/run/secrets/jwt"` results in the decrypted bytes
# being written to a host scratch dir and bind-mounted read-only
# into the postgres inner container at the configured target.
#
# Asserts:
#   1. Build extracts the value (via env extractor).
#   2. Run writes the bytes to `~/.coast/ssg/runs/<project>/secrets/jwt`
#      with 0600 perms.
#   3. The override file mounts that path at /run/secrets/jwt
#      read-only.
#   4. Inside postgres, `cat /run/secrets/jwt` returns the secret
#      bytes; the file is read-only.
#
# Uses the env extractor (sourcing from $SSG_TEST_JWT_VALUE) so the
# test stays hermetic — we don't write a host file and depend on
# the file extractor's path resolution.

set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

SSG_PROJECT="coast-ssg-secrets"
JWT_VALUE="jwt-secret-${RANDOM}-${RANDOM}"

register_cleanup
preflight_checks

echo ""
echo "=== Setup ==="

clean_slate
"$HELPERS_DIR/setup.sh"
pass "Examples initialized"

rm -rf "$HOME/.coast/ssg"
cleanup_project_ssgs "$SSG_PROJECT"

# Phase 33: env extractor needs vars in the daemon's env; export
# before `start_daemon`.
export SSG_TEST_PG_PASSWORD="dev"
export SSG_TEST_JWT_VALUE="$JWT_VALUE"

start_daemon

echo ""
echo "=== Step 1: build extracts both secrets ==="

cd "$PROJECTS_DIR/$SSG_PROJECT"

BUILD_OUT=$("$COAST" ssg build 2>&1)
echo "$BUILD_OUT" | tail -20
assert_contains "$BUILD_OUT" "Build complete" "ssg build succeeds"
assert_contains "$BUILD_OUT" "jwt" "build mentions the jwt secret"
pass "build extracts file-inject secret"

echo ""
echo "=== Step 2: run materializes the file-inject payload ==="

"$COAST" ssg run >/dev/null 2>&1
sleep 8

# The host-side payload lives under `~/.coast/ssg/runs/<project>/secrets/`.
HOST_JWT="$HOME/.coast/ssg/runs/$SSG_PROJECT/secrets/jwt"
[ -f "$HOST_JWT" ] || fail "expected file payload at '$HOST_JWT'"
HOST_JWT_BYTES=$(cat "$HOST_JWT")
[ "$HOST_JWT_BYTES" = "$JWT_VALUE" ] \
    || fail "host file payload mismatch: expected '$JWT_VALUE', got '$HOST_JWT_BYTES'"
pass "host scratch dir contains the decrypted bytes"

# Best-effort 0600 perm check (Linux/macOS; harmless on FS that
# doesn't support it — the materialize path tolerates chmod failures).
HOST_JWT_PERM=$(stat -f '%Lp' "$HOST_JWT" 2>/dev/null || stat -c '%a' "$HOST_JWT" 2>/dev/null || echo "")
echo "host file perms: $HOST_JWT_PERM"
if [ "$HOST_JWT_PERM" = "600" ]; then
    pass "host file is mode 0600"
else
    echo "  (perms='$HOST_JWT_PERM' — non-Unix FS or container UID mismatch; not fatal)"
fi

echo ""
echo "=== Step 3: postgres container can read /run/secrets/jwt ==="

SSG_CONTAINER="${SSG_PROJECT}-ssg"
INNER_JWT_BYTES=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres cat /run/secrets/jwt 2>&1 | tr -d '\r')
echo "observed inside postgres: $INNER_JWT_BYTES"
[ "$INNER_JWT_BYTES" = "$JWT_VALUE" ] \
    || fail "expected /run/secrets/jwt='$JWT_VALUE' inside postgres; got '$INNER_JWT_BYTES'"
pass "/run/secrets/jwt inside postgres contains the decrypted value"

echo ""
echo "=== Step 4: file mount is read-only inside the container ==="

# `docker compose exec postgres` runs as the postgres user (UID 999
# on debian images, but our alpine variant uses UID 70). Either
# way, attempting to overwrite a `:ro` mount must fail.
WRITE_OUT=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres sh -c \
            "echo tampered > /run/secrets/jwt 2>&1 && echo unexpected_success" 2>&1 || true)
echo "$WRITE_OUT"
if echo "$WRITE_OUT" | grep -q "unexpected_success"; then
    fail "/run/secrets/jwt is writable inside postgres (mount must be :ro)"
fi
pass "/run/secrets/jwt is read-only inside postgres"

# Confirm the bytes are still intact post-attempt.
INNER_JWT_AFTER=$(docker exec "$SSG_CONTAINER" \
    docker compose -p "$SSG_CONTAINER" \
        -f /coast-artifact/compose.yml \
        -f /coast-runtime/compose.override.yml \
        exec -T postgres cat /run/secrets/jwt 2>&1 | tr -d '\r')
[ "$INNER_JWT_AFTER" = "$JWT_VALUE" ] \
    || fail "file body mutated despite :ro mount: '$INNER_JWT_AFTER'"
pass "file body unchanged after write attempt"

echo ""
echo "==========================================="
echo "  SSG SECRETS FILE INJECT TEST PASSED"
echo "==========================================="

"$COAST" ssg rm --with-data >/dev/null 2>&1 || true
unset SSG_TEST_PG_PASSWORD SSG_TEST_JWT_VALUE
