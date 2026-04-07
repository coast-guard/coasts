# coast-service

Remote control plane for Coast. Runs on a remote machine and manages DinD
containers on behalf of the local `coast-daemon`. The daemon communicates with
`coast-service` over an SSH tunnel; the user's workflow is unchanged.

## Architecture

`coast-service` is an HTTP server (Axum) that mirrors the daemon's local
operations: build, run, assign, exec, ps, logs, stop, start, rm, secrets, and
service restarts. It manages its own SQLite state, Docker containers, and
dynamic port allocation.

```
coast-daemon  --SSH tunnel-->  coast-service (:31420)
                                   |
                                   v
                               Docker (DinD)
                                   |
                                   v
                            Coast containers
```

## API

All endpoints expect/return JSON. The service listens on port 31420 by default.

| Method | Path                | Description              |
|--------|---------------------|--------------------------|
| GET    | `/health`           | Returns `ok`             |
| POST   | `/build`            | Build a coast            |
| POST   | `/run`              | Create and start         |
| POST   | `/assign`           | Switch worktree          |
| POST   | `/exec`             | Run command in container |
| POST   | `/ps`               | Container status         |
| POST   | `/logs`             | Stream container logs    |
| POST   | `/stop`             | Stop a coast             |
| POST   | `/start`            | Start a stopped coast    |
| POST   | `/rm`               | Remove a coast           |
| POST   | `/secret`           | Manage secrets           |
| POST   | `/restart-services` | Restart compose services |

## Configuration

| Env var              | Default   | Description                    |
|----------------------|-----------|--------------------------------|
| `COAST_SERVICE_HOME` | `/data`   | State directory (SQLite, etc.) |
| `COAST_SERVICE_PORT` | `31420`   | HTTP listen port               |

## Local Dev

Start the dev container with DinD, sshd, and cargo-watch hot reload:

```bash
make coast-service-dev
```

This will:
1. Generate SSH keys in `.dev/ssh/` (first run only)
2. Build the Docker image
3. Start a privileged container with:
   - Docker-in-Docker on an isolated bridge network
   - sshd on port 2222
   - coast-service on port 31420
   - Source bind-mounted with cargo-watch for hot reload

Then register it as a remote in another terminal:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

Note: use **absolute paths** for the `--key` flag.

## Production

Build the production image:

```bash
docker build -t coast-service -f Dockerfile.coast-service .
```

The production image is a multi-stage build that produces a minimal Debian
runtime with just the `coast-service` binary, Docker client, rsync, and SSH
client.

### Deployment

Full setup for a fresh remote host (e.g. EC2 instance):

```bash
# 1. Install Docker and git
sudo yum install -y docker git          # Amazon Linux
# sudo apt-get install -y docker.io git # Ubuntu

# 2. Enable Docker and add SSH user to docker group
sudo systemctl enable docker
sudo systemctl start docker
sudo usermod -aG docker $(whoami)

# 3. Enable GatewayPorts for shared service tunnels
sudo sh -c 'echo "GatewayPorts clientspecified" >> /etc/ssh/sshd_config'
sudo systemctl restart sshd

# 4. Create data directory with correct ownership
sudo mkdir -p /data && sudo chown $(whoami):$(whoami) /data

# 5. Clone, build, and run coast-service
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .

docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

Then register it from your local machine:

```bash
coast remote add my-vm <user>@<host> --key /path/to/key
coast remote test my-vm
```

### Requirements

**Bind mount, not Docker volume.** Use `-v /data:/data` (bind mount), not
`-v coast-service-data:/data` (named volume). The daemon rsyncs workspace
files to the host via SSH; a named volume is isolated from the host
filesystem and coast-service inside the container won't see the files.

**Passwordless sudo.** The daemon uses `sudo rsync` and `sudo chown` to
manage workspace files (which may be owned by root from coast-service
operations inside the container). The SSH user must have passwordless sudo.
This is the default for `ec2-user` on Amazon Linux and `ubuntu` on Ubuntu
EC2 instances.

**GatewayPorts.** The remote host's sshd must have `GatewayPorts
clientspecified` enabled. Without this, reverse SSH tunnels for shared
services (postgres, redis) bind to `127.0.0.1` only, making them
unreachable from DinD containers.

**Disk space.** Projects with multiple Docker images need sufficient disk.
Recommend 30GB+ for typical projects (images exist on host Docker, in
tarballs, and loaded into DinD containers).

## Testing

Unit and integration tests run as part of the workspace:

```bash
cargo test -p coast-service
```

Full end-to-end remote tests run in the DinDinD environment:

```bash
make run-dind-integration TEST=test_remote_basic
make run-dind-integration TEST=all
```
