#!/usr/bin/env bash
# test-release.sh — validate build + packaging locally before pushing to GHA.
#
# Prerequisites:
#   rustup target add x86_64-apple-darwin   (for cross-compile on arm64 mac)
#
# Usage:
#   ./scripts/test-release.sh [--skip-cross]
set -euo pipefail

SKIP_CROSS="${1:-}"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "=== Coast release test — v${VERSION} ==="

# Step 1: Run all tests
echo ""
echo "--- Step 1: cargo test --workspace ---"
cargo test --workspace
echo "PASS: all tests"

# Step 2: Lint check
echo ""
echo "--- Step 2: make lint ---"
make lint
echo "PASS: lint clean"

# Step 3: Build native target (aarch64-apple-darwin on M-series, x86_64 on Intel)
echo ""
echo "--- Step 3: Build native release ---"
NATIVE_TARGET=$(rustc -vV | grep host | awk '{print $2}')
cargo build --release --target "${NATIVE_TARGET}"

COAST_BIN="target/${NATIVE_TARGET}/release/coast"
COASTD_BIN="target/${NATIVE_TARGET}/release/coastd"

if [ ! -f "${COAST_BIN}" ] || [ ! -f "${COASTD_BIN}" ]; then
    echo "FAIL: missing binaries after build"
    exit 1
fi
echo "PASS: native build (${NATIVE_TARGET})"

# Step 4: Cross-compile x86_64-apple-darwin (if on arm64 mac)
if [ "${SKIP_CROSS}" != "--skip-cross" ] && [ "${NATIVE_TARGET}" = "aarch64-apple-darwin" ]; then
    echo ""
    echo "--- Step 4: Cross-compile x86_64-apple-darwin ---"
    cargo build --release --target x86_64-apple-darwin
    echo "PASS: cross-compile x86_64-apple-darwin"
else
    echo ""
    echo "--- Step 4: Cross-compile (skipped) ---"
fi

# Step 5: Strip and package native tarball
echo ""
echo "--- Step 5: Package tarball ---"
strip "${COAST_BIN}"
strip "${COASTD_BIN}"

# Determine asset name
case "${NATIVE_TARGET}" in
    aarch64-apple-darwin)  ASSET="darwin-arm64" ;;
    x86_64-apple-darwin)   ASSET="darwin-amd64" ;;
    x86_64-unknown-linux*) ASSET="linux-amd64" ;;
    aarch64-unknown-linux*) ASSET="linux-arm64" ;;
    *) ASSET="unknown" ;;
esac

TARBALL="coast-v${VERSION}-${ASSET}.tar.gz"
tar czf "${TARBALL}" -C "target/${NATIVE_TARGET}/release" coast coastd
echo "Created: ${TARBALL}"

# Step 6: Verify tarball contents
echo ""
echo "--- Step 6: Verify tarball ---"
CONTENTS=$(tar tzf "${TARBALL}")
if echo "${CONTENTS}" | grep -q "^coast$" && echo "${CONTENTS}" | grep -q "^coastd$"; then
    echo "PASS: tarball contains coast + coastd"
else
    echo "FAIL: tarball missing expected binaries"
    echo "Contents: ${CONTENTS}"
    exit 1
fi

# Step 7: Verify version output
echo ""
echo "--- Step 7: Verify version ---"
TMPDIR=$(mktemp -d)
tar xzf "${TARBALL}" -C "${TMPDIR}"
ACTUAL=$("${TMPDIR}/coast" --version 2>&1 || true)
if echo "${ACTUAL}" | grep -q "${VERSION}"; then
    echo "PASS: coast --version shows ${VERSION}"
else
    echo "FAIL: expected version ${VERSION}, got: ${ACTUAL}"
    rm -rf "${TMPDIR}"
    exit 1
fi
rm -rf "${TMPDIR}"

# Cleanup
rm -f "${TARBALL}"

echo ""
echo "=== All release checks passed ==="
