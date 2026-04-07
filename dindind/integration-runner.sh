#!/usr/bin/env bash
# integration-runner.sh -- Run integrated-examples tests inside a DinD container.
#
# Usage:
#   ./dindind/integration-runner.sh test_egress   Run a single test
#   ./dindind/integration-runner.sh all           Run every test in integration.yaml
#   ./dindind/integration-runner.sh list          List available tests
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
YAML_FILE="${SCRIPT_DIR}/integration.yaml"
BASE_IMAGE="coast-dindind-base"
INTEGRATION_IMAGE="coast-dindind-integration"

# ---------------------------------------------------------------------------
# YAML parsing (grep/awk, no yq dependency)
# ---------------------------------------------------------------------------

list_tests() {
  awk '/^  test_/{gsub(/:$/,"",$1); print $1}' "$YAML_FILE"
}

get_test_script() {
  local name="$1"
  awk -v test="  ${name}:" '
    $0 == test { found=1; next }
    found && /script:/ { gsub(/^ *script: */, ""); print; exit }
    found && /^  [^ ]/ { exit }
  ' "$YAML_FILE"
}

# ---------------------------------------------------------------------------
# Image building
# ---------------------------------------------------------------------------

build_images() {
  echo "==> Building base image..."
  docker build -t "$BASE_IMAGE" \
    -f "${SCRIPT_DIR}/lib/base.Dockerfile" \
    "$SCRIPT_DIR" 2>&1

  echo ""
  echo "==> Building integration image (base + Rust)..."
  docker build -t "$INTEGRATION_IMAGE" \
    -f "${SCRIPT_DIR}/lib/integration.Dockerfile" \
    "$SCRIPT_DIR" 2>&1
}

# ---------------------------------------------------------------------------
# Run a single test
# ---------------------------------------------------------------------------

run_test() {
  local test_name="$1"
  local script
  script="$(get_test_script "$test_name")"

  if [ -z "$script" ]; then
    echo "error: test '${test_name}' not found in ${YAML_FILE}" >&2
    echo "Available tests:"
    list_tests | sed 's/^/  /'
    return 1
  fi

  echo ""
  echo "======================================================"
  echo "  Running: ${test_name}"
  echo "  Script:  ${script}"
  echo "======================================================"
  echo ""

  local container_name="coast-integration-${test_name}-$$"

  docker run --rm \
    --privileged \
    --name "$container_name" \
    -v "${REPO_ROOT}:/coast-repo:ro" \
    -v coast-dindind-cargo-registry:/root/.cargo/registry \
    -v coast-dindind-cargo-git:/root/.cargo/git \
    -v coast-dindind-target:/workspace/target \
    -v coast-dindind-coast-home:/root/.coast \
    -v coast-dindind-docker:/var/lib/docker \
    -e HOME=/root \
    -e SHELL=/bin/bash \
    -e DIND_TEST_SCRIPT="$script" \
    "$INTEGRATION_IMAGE" \
    bash -l -c '
set -euo pipefail

# Copy source to a container-only path so we never write to the host mount.
# target/ is a named volume that persists across runs for build caching.
# Exclude .coasts/ dirs (stale worktree data from host runs with wrong absolute paths).
echo "==> Copying project into container..."
mkdir -p /workspace
rsync -a --exclude target --exclude .coasts --exclude .worktrees /coast-repo/ /workspace/
cd /workspace

echo "==> Building coast binaries (cargo build --release)..."
# Remove stale wrapper symlinks or non-ELF files from cached target volume.
if [ -L /workspace/target/release/coast ] || { [ -f /workspace/target/release/coast ] && ! file /workspace/target/release/coast 2>/dev/null | grep -q ELF; }; then
  rm -f /workspace/target/release/coast
fi
cargo build --release 2>&1

# Skip auto-update (dev build flag)
export COAST_HOME=/root/.coast
# Increase compose health timeout for DinDinD (services start slower with nested Docker)
export COAST_COMPOSE_HEALTH_TIMEOUT=300
# Use BuildKit to avoid legacy builder deprecation warnings that break host builds
export DOCKER_BUILDKIT=1

# Set up git identity (needed for setup.sh git commits)
git config --global user.email "test@coasts.dev"
git config --global user.name "Coast Test"

# Clean stale state from previous test runs. The docker volume persists
# containers and state from killed runs.
rm -f /root/.coast/state.db /root/.coast/state.db-wal /root/.coast/state.db-shm
rm -f /root/.coast/coastd.sock /root/.coast/coastd.pid
docker rm -f $(docker ps -aq --filter "label=coast.managed=true") 2>/dev/null || true
docker volume ls -q --filter "name=coast-shared--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
docker volume ls -q --filter "name=coast-dind--" 2>/dev/null | xargs -r docker volume rm 2>/dev/null || true
docker network ls -q --filter "name=coast-shared-" 2>/dev/null | xargs -r docker network rm 2>/dev/null || true

# Install coast wrapper for egress forwarding in nested DinD.
# Put the real binary and wrapper in /opt/coast/ -- never touch the cached
# target/ volume so cargo caching works correctly across runs.
echo "==> Installing coast wrapper..."
mkdir -p /opt/coast
cp /workspace/target/release/coast /opt/coast/coast
cp /workspace/target/release/coastd /opt/coast/coastd
cp /workspace/target/release/coast-service /opt/coast/coast-service 2>/dev/null || true
cp /workspace/dindind/lib/coast-wrapper.sh /opt/coast/coast-wrapper
chmod +x /opt/coast/coast-wrapper
# Replace target/release/coast with a symlink to the wrapper
ln -sf /opt/coast/coast-wrapper /workspace/target/release/coast
export REAL_COAST=/opt/coast/coast

# Seed the inner Docker daemon with images from /coast-repo/.dindind-image-cache
# if available (populated by the host). Fall back to pulling from registry.
echo "==> Seeding Docker image cache..."
if [ -d /coast-repo/.dindind-image-cache ]; then
  for tarball in /coast-repo/.dindind-image-cache/*.tar; do
    [ -f "$tarball" ] && docker load < "$tarball" 2>/dev/null && echo "    loaded $(basename "$tarball")" || true
  done
fi
for img in docker:dind alpine:3.20 postgres:16 postgres:15 postgres:16-alpine redis:7-alpine redis:7 node:18-alpine node:20-alpine node:22-alpine; do
  if ! docker image inspect "$img" >/dev/null 2>&1; then
    echo "    pulling $img..."
    docker pull "$img" 2>&1 | tail -1 || true
  fi
done

echo ""
echo "==> Running test: ${DIND_TEST_SCRIPT}"
exec bash "${DIND_TEST_SCRIPT}"
'
}

# ---------------------------------------------------------------------------
# Run all tests
# ---------------------------------------------------------------------------

run_all() {
  local tests
  tests="$(list_tests)"
  local pass=0
  local fail=0
  local failed_tests=()

  for test_name in $tests; do
    if run_test "$test_name"; then
      pass=$((pass + 1))
    else
      fail=$((fail + 1))
      failed_tests+=("$test_name")
    fi
  done

  echo ""
  echo "======================================================"
  echo "  Integration Results: ${pass} passed, ${fail} failed"
  if [ ${#failed_tests[@]} -gt 0 ]; then
    echo "  Failed: ${failed_tests[*]}"
  fi
  echo "======================================================"

  [ "$fail" -eq 0 ]
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

ACTION="${1:-}"

case "$ACTION" in
  list)
    list_tests
    ;;
  all)
    build_images
    run_all
    ;;
  "")
    echo "Usage: $0 {test_name|all|list}"
    echo ""
    echo "  test_name   Run a single integration test in DinD"
    echo "  all         Run all tests defined in integration.yaml"
    echo "  list        List available test names"
    exit 1
    ;;
  *)
    build_images
    run_test "$ACTION"
    ;;
esac
