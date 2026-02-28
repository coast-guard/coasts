#!/usr/bin/env bash
#
# Run all coast integration tests.
#
# Discovers and runs every test_*.sh script in this directory.
# Each test runs in isolation (its own cleanup trap handles teardown).
#
# Usage:
#   ./integrated_examples/test.sh                    # run all tests
#   ./integrated_examples/test.sh --include-keychain  # include macOS Keychain test
#   ./integrated_examples/test.sh test_assign         # run a specific test
#   ./integrated_examples/test.sh test_assign test_checkout  # run multiple
#
# The test_claude.sh test requires macOS Keychain access and is skipped by
# default. Pass --include-keychain to include it.
#
# Prerequisites:
#   - Docker running
#   - socat installed (brew install socat)
#   - Coast binaries built (cargo build --release)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# --- Parse arguments ---

INCLUDE_KEYCHAIN=false
SPECIFIC_TESTS=()

for arg in "$@"; do
    case "$arg" in
        --include-keychain)
            INCLUDE_KEYCHAIN=true
            ;;
        *)
            SPECIFIC_TESTS+=("$arg")
            ;;
    esac
done

# --- Discover tests ---

ALL_TESTS=()
for f in "$SCRIPT_DIR"/test_*.sh; do
    [ -f "$f" ] || continue
    name="$(basename "$f" .sh)"

    # Skip keychain test unless explicitly included
    if [ "$name" = "test_claude" ] && [ "$INCLUDE_KEYCHAIN" = "false" ]; then
        continue
    fi

    ALL_TESTS+=("$name")
done

# Filter to specific tests if provided
if [ ${#SPECIFIC_TESTS[@]} -gt 0 ]; then
    TESTS_TO_RUN=()
    for requested in "${SPECIFIC_TESTS[@]}"; do
        # Allow with or without test_ prefix
        normalized="$requested"
        [[ "$normalized" == test_* ]] || normalized="test_$normalized"
        found=false
        for available in "${ALL_TESTS[@]}"; do
            if [ "$available" = "$normalized" ]; then
                TESTS_TO_RUN+=("$available")
                found=true
                break
            fi
        done
        if [ "$found" = "false" ]; then
            echo "ERROR: Unknown test '$requested'"
            echo "Available tests:"
            for t in "${ALL_TESTS[@]}"; do
                echo "  $t"
            done
            exit 1
        fi
    done
else
    TESTS_TO_RUN=("${ALL_TESTS[@]}")
fi

# --- Run tests ---

TOTAL=${#TESTS_TO_RUN[@]}
PASSED=0
FAILED=0
SKIPPED=0
FAILED_NAMES=()

echo "==========================================="
echo "  COAST INTEGRATION TESTS"
echo "==========================================="
echo ""
echo "  Tests to run: $TOTAL"
if [ "$INCLUDE_KEYCHAIN" = "false" ]; then
    echo "  (test_claude skipped — pass --include-keychain to include)"
fi
echo ""

for test_name in "${TESTS_TO_RUN[@]}"; do
    script="$SCRIPT_DIR/${test_name}.sh"
    echo "==========================================="
    echo "  Running: $test_name"
    echo "==========================================="

    # Run each test in a subshell so its EXIT trap doesn't kill us
    set +e
    (
        cd "$REPO_ROOT"
        bash "$script"
    )
    exit_code=$?
    set -e

    if [ $exit_code -eq 0 ]; then
        PASSED=$((PASSED + 1))
        echo ""
        echo "  >>> $test_name: PASSED"
    else
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("$test_name")
        echo ""
        echo "  >>> $test_name: FAILED (exit code $exit_code)"
    fi
    echo ""
done

# --- Summary ---

echo "==========================================="
echo "  SUMMARY"
echo "==========================================="
echo ""
echo "  Total:   $TOTAL"
echo "  Passed:  $PASSED"
echo "  Failed:  $FAILED"

if [ "$INCLUDE_KEYCHAIN" = "false" ]; then
    echo "  Skipped: 1 (test_claude — use --include-keychain)"
fi

if [ $FAILED -gt 0 ]; then
    echo ""
    echo "  Failed tests:"
    for name in "${FAILED_NAMES[@]}"; do
        echo "    - $name"
    done
    echo ""
    echo "  SOME TESTS FAILED"
    echo "==========================================="
    exit 1
else
    echo ""
    echo "  ALL TESTS PASSED"
    echo "==========================================="
    exit 0
fi
