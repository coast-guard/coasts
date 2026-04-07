#!/usr/bin/env bash
set -eu

DOCKER_READY_TIMEOUT="${DOCKER_READY_TIMEOUT:-60}"
DOCKERD_LOG="/tmp/dockerd.log"

cleanup() {
    echo ""
    echo "==> Shutting down..."
    kill -TERM "$WATCH_PID" 2>/dev/null || true
    # Kill the entire process group to catch cargo run + coast-service
    kill -TERM 0 2>/dev/null || true
    wait "$WATCH_PID" 2>/dev/null || true
    echo "==> Stopped."
    exit 0
}
trap cleanup INT TERM

# Use a non-default bridge subnet so DinD networks don't collide with the
# host Docker or any inner Coast DinD containers.
mkdir -p /etc/docker
cat > /etc/docker/daemon.json <<'DAEMONJSON'
{
  "bip": "10.200.0.1/16",
  "default-address-pools": [{"base": "10.201.0.0/16", "size": 24}],
  "storage-driver": "overlay2"
}
DAEMONJSON

echo "==> Starting Docker daemon..."
dockerd > "$DOCKERD_LOG" 2>&1 &
DOCKERD_PID=$!

elapsed=0
while ! docker info >/dev/null 2>&1; do
  if ! kill -0 "$DOCKERD_PID" 2>/dev/null; then
    echo "error: dockerd exited unexpectedly. Last 20 lines of log:"
    tail -20 "$DOCKERD_LOG"
    exit 1
  fi
  if [ "$elapsed" -ge "$DOCKER_READY_TIMEOUT" ]; then
    echo "error: Docker daemon did not become ready within ${DOCKER_READY_TIMEOUT}s"
    tail -20 "$DOCKERD_LOG"
    exit 1
  fi
  sleep 1
  elapsed=$((elapsed + 1))
done
echo "==> Docker daemon ready (${elapsed}s)"

echo "==> Starting sshd..."
mkdir -p /run/sshd
chmod 700 /root/.ssh 2>/dev/null || true
echo "GatewayPorts clientspecified" >> /etc/ssh/sshd_config
/usr/sbin/sshd
echo "==> sshd started (GatewayPorts clientspecified)"

echo "==> Starting cargo-watch (coast-service)..."
cargo watch \
  -w coast-core/src \
  -w coast-service/src \
  -w coast-docker/src \
  -w coast-secrets/src \
  -x "run -p coast-service" &
WATCH_PID=$!
wait "$WATCH_PID"
