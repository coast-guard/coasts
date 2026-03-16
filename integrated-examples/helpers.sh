#!/usr/bin/env bash
#
# Shared test helpers for coast integration tests.
#
# Source this file at the top of each test script:
#   source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/helpers.sh"

# --- Path variables ---

HELPERS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_DIR="$HELPERS_DIR/projects"
REPO_ROOT="$(cd "$HELPERS_DIR/.." && pwd)"
COAST="$REPO_ROOT/target/release/coast"
COASTD="$REPO_ROOT/target/release/coastd"

# Track instances for cleanup
CLEANUP_INSTANCES=()

# --- Output helpers ---

pass() {
    echo "  PASS: $1"
}

fail() {
    echo "  FAIL: $1"
    exit 1
}

# --- Assertions ---

assert_contains() {
    local actual="$1"
    local expected="$2"
    local msg="$3"
    if echo "$actual" | grep -q "$expected"; then
        pass "$msg"
    else
        echo "  Expected to contain: $expected"
        echo "  Actual: $actual"
        fail "$msg"
    fi
}

assert_not_contains() {
    local actual="$1"
    local unexpected="$2"
    local msg="$3"
    if echo "$actual" | grep -q "$unexpected"; then
        echo "  Expected NOT to contain: $unexpected"
        echo "  Actual: $actual"
        fail "$msg"
    else
        pass "$msg"
    fi
}

assert_eq() {
    local actual="$1"
    local expected="$2"
    local msg="$3"
    if [ "$actual" = "$expected" ]; then
        pass "$msg"
    else
        echo "  Expected: $expected"
        echo "  Actual: $actual"
        fail "$msg"
    fi
}

# --- Polling ---

wait_for_healthy() {
    local port="$1"
    local max_wait="${2:-30}"
    local i=0
    while [ $i -lt "$max_wait" ]; do
        if curl -sf "http://localhost:${port}/health" >/dev/null 2>&1; then
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    return 1
}

# --- Preflight checks ---

preflight_checks() {
    echo "=== Preflight checks ==="
    [ -f "$COAST" ] || { echo "coast binary not found at $COAST. Run: cargo build --release"; exit 1; }
    [ -f "$COASTD" ] || { echo "coastd binary not found at $COASTD. Run: cargo build --release"; exit 1; }
    command -v socat >/dev/null || { echo "socat not installed. Run: brew install socat (macOS) or sudo apt-get install socat (Ubuntu)"; exit 1; }
    command -v docker >/dev/null || { echo "docker not installed"; exit 1; }
    docker info >/dev/null 2>&1 || { echo "Docker daemon not running"; exit 1; }
    pass "All prerequisites met"
}

# --- Environment management ---

clean_slate() {
    echo "--- Cleaning slate ---"

    # Kill any existing daemon and socat
    pkill -f "coastd" 2>/dev/null || true
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true
    sleep 1

    # Remove state files (including SQLite WAL/SHM)
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid
    rm -f ~/.coast/keystore.db ~/.coast/keystore.key

    # Remove any leftover coast containers and volumes
    docker rm -f $(docker ps -aq --filter "label=coast.managed=true") 2>/dev/null || true
    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

    echo "  Slate clean."
}

start_daemon() {
    "$COASTD" --foreground &>/tmp/coastd-test.log &
    sleep 2
    pass "Daemon started"
}

# --- Cleanup trap helper ---

# Call register_cleanup in your test script to set up the EXIT trap.
# It will rm any instances in CLEANUP_INSTANCES, kill daemon/socat, clean state.
register_cleanup() {
    trap '_do_cleanup' EXIT
}

_do_cleanup() {
    echo ""
    echo "--- Cleaning up ---"

    # Remove any instances we created
    for inst in "${CLEANUP_INSTANCES[@]:-}"; do
        "$COAST" rm "$inst" 2>/dev/null || true
    done

    # Clean up volumes
    docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
    docker volume ls -q --filter "name=coast--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true

    # Kill daemon
    pkill -f "coastd --foreground" 2>/dev/null || true
    sleep 1

    # Kill any orphaned socat
    pkill -f "socat TCP-LISTEN.*fork,reuseaddr" 2>/dev/null || true

    # Clean state (including SQLite WAL/SHM)
    rm -f ~/.coast/state.db ~/.coast/state.db-wal ~/.coast/state.db-shm
    rm -f ~/.coast/coastd.sock ~/.coast/coastd.pid

    echo "Cleanup complete."
}

# --- Dynamic port extraction ---

# Extract the dynamic port for a given service name from coast run output.
# Usage: extract_dynamic_port "$RUN_OUTPUT" "app"
extract_dynamic_port() {
    local output="$1"
    local service="$2"
    # Match only port table rows where the first field is exactly the service name
    echo "$output" | awk -v svc="$service" '$1 == svc {print $3}'
}
