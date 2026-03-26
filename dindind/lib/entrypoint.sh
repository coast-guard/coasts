#!/usr/bin/env bash
# Shared entrypoint for DinDinD test containers.
# Starts the Docker daemon (via sudo if non-root), waits for readiness,
# then either runs the supplied command or drops to an interactive shell.
set -eu

DOCKER_READY_TIMEOUT="${DOCKER_READY_TIMEOUT:-60}"
DOCKERD_LOG="/tmp/dockerd.log"

# Use a non-default bridge subnet so the outer DinD's networks don't collide
# with the inner Coast DinD's default docker0 (172.17.0.0/16). Without this,
# shared service proxy alias IPs on the inner docker0 overlap with the outer
# daemon's bridge, breaking routing in DinDinD.
mkdir -p /etc/docker
cat > /etc/docker/daemon.json <<'DAEMONJSON'
{
  "bip": "10.200.0.1/16",
  "default-address-pools": [{"base": "10.201.0.0/16", "size": 24}],
  "storage-driver": "overlay2"
}
DAEMONJSON

echo "==> Starting Docker daemon..."
if [ "$(id -u)" -eq 0 ]; then
  dockerd > "$DOCKERD_LOG" 2>&1 &
else
  sudo dockerd > "$DOCKERD_LOG" 2>&1 &
fi
DOCKERD_PID=$!

elapsed=0
while ! docker info >/dev/null 2>&1; do
  if ! kill -0 "$DOCKERD_PID" 2>/dev/null && ! sudo kill -0 "$DOCKERD_PID" 2>/dev/null; then
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

if [ $# -gt 0 ]; then
  exec "$@"
else
  exec bash -l
fi
