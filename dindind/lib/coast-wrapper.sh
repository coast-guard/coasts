#!/bin/bash
# Wrapper around the coast binary for DinDinD integration tests.
# After a successful `coast run`, sets up socat egress forwarders inside the
# Coast DinD container so inner compose services can reach host services
# through host.docker.internal.
REAL_COAST="${REAL_COAST:-/tmp/coast}"

if [ ! -x "$REAL_COAST" ]; then
  echo "coast-wrapper: real binary not found at $REAL_COAST" >&2
  exit 1
fi

# For non-run commands, pass through directly
case "${1:-}" in
  run) ;;
  *)   exec "$REAL_COAST" "$@" ;;
esac

# Parse ports that need forwarding BEFORE running coast run.
# We need to set up socat in the background while coast run is in progress,
# because coast run blocks waiting for compose health which needs the forwarding.
forward_ports=()
current_section=""
if [ -f Coastfile ]; then
  while IFS= read -r line; do
    case "$line" in
      "["*) current_section="$line" ; continue ;;
    esac
    case "$current_section" in
      "[egress]"*)
        port=$(echo "$line" | grep -oE '[0-9]+' | head -1)
        [ -n "$port" ] && forward_ports+=("$port")
        ;;
      "[shared_services."*)
        if echo "$line" | grep -q "ports"; then
          for p in $(echo "$line" | grep -oE '[0-9]+'); do
            forward_ports+=("$p")
          done
        fi
        ;;
    esac
  done < Coastfile
  forward_ports=($(printf '%s\n' "${forward_ports[@]}" | sort -u))
fi

# Start background job to set up port forwarding as soon as the Coast DinD container appears.
# Uses docker exec directly (not coast exec) because coast exec requires provisioning
# to complete, which is blocked waiting for compose health -- creating a deadlock.
if [ ${#forward_ports[@]} -gt 0 ]; then
  # Resolve the project name from the Coastfile to predict the container name.
  coast_project=$(grep -E '^name\s*=' Coastfile | head -1 | sed 's/.*=\s*"\{0,1\}\([^"]*\)"\{0,1\}/\1/' | tr -d ' ')
  container_name="${coast_project}-coasts-${instance_name}"

  (
    for attempt in $(seq 1 90); do
      sleep 2
      if docker exec "$container_name" true 2>/dev/null; then
        docker exec "$container_name" sh -c \
          "command -v socat >/dev/null 2>&1 || apk add --no-cache socat >/dev/null 2>&1" \
          2>/dev/null || true
        for port in "${forward_ports[@]}"; do
          docker exec "$container_name" sh -c \
            "nohup socat TCP-LISTEN:${port},fork,reuseaddr TCP:host.docker.internal:${port} >/dev/null 2>&1 &" \
            2>/dev/null || true
        done
        break
      fi
    done
  ) &
  FORWARD_PID=$!
fi

# For `coast run`, run the real binary and capture its exit code
"$REAL_COAST" "$@"
rc=$?

# Clean up background forwarder
[ -n "${FORWARD_PID:-}" ] && kill "$FORWARD_PID" 2>/dev/null; wait "$FORWARD_PID" 2>/dev/null

if [ $rc -ne 0 ]; then
  exit $rc
fi

instance_name="${2:-}"
if [ -z "$instance_name" ]; then
  exit $rc
fi

# Parse egress ports from the Coastfile in the current directory
if [ ! -f Coastfile ]; then
  exit $rc
fi

exit $rc

exit $rc
