# Setup

This page covers everything needed to get a remote coast running: preparing the remote host, deploying coast-service, registering the remote, and running your first remote coast.

## Host Requirements

| Requirement | Why |
|---|---|
| Docker | Runs coast-service and DinD containers |
| `GatewayPorts clientspecified` in sshd_config | Allows SSH reverse tunnels for [shared services](../concepts_and_terminology/SHARED_SERVICES.md) to bind on all interfaces, not just localhost |
| Passwordless sudo for SSH user | The daemon uses `sudo rsync` and `sudo chown` for workspace file management (remote workspace directories may be owned by root from coast-service operations) |
| Bind mount for `/data` (not a Docker volume) | The daemon rsyncs files to the host filesystem via SSH. Named Docker volumes are isolated from the host filesystem and invisible to rsync |
| 50 GB+ disk | Docker images exist on host Docker, in tarballs, and loaded into DinD containers. See [disk management](CLI.md#disk-management) for details |
| SSH access | The daemon connects to the remote via SSH for tunnels, rsync, and coast-service API access |

## Prepare the Remote Host

On a fresh Linux machine (EC2, GCP, bare metal):

```bash
# Install Docker and git
sudo yum install -y docker git          # Amazon Linux
# sudo apt-get install -y docker.io git # Ubuntu/Debian

# Enable Docker and add your user to the docker group
sudo systemctl enable docker
sudo systemctl start docker
sudo usermod -aG docker $(whoami)

# Enable GatewayPorts for shared service tunnels
sudo sh -c 'echo "GatewayPorts clientspecified" >> /etc/ssh/sshd_config'
sudo systemctl restart sshd

# Create the data directory with correct ownership
sudo mkdir -p /data && sudo chown $(whoami):$(whoami) /data
```

Log out and back in for the docker group change to take effect.

## Deploy coast-service

Clone the repository and build the production image:

```bash
git clone https://github.com/coast-guard/coasts.git
cd coasts && git checkout <branch>
docker build -t coast-service -f Dockerfile.coast-service .
```

Run it with a bind mount (not a Docker volume):

```bash
docker run -d \
  --name coast-service \
  --privileged \
  -p 31420:31420 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /data:/data \
  coast-service
```

Verify it is running:

```bash
curl http://localhost:31420/health
# ok
```

### Why `--privileged`

coast-service manages Docker-in-Docker containers. The `--privileged` flag grants the container the capabilities needed to run nested Docker daemons.

### Why bind mount, not Docker volume

The daemon rsyncs workspace files from your laptop to the remote host via SSH. Those files land on the host filesystem at `/data/workspaces/{project}/{instance}/`. If `/data` were a named Docker volume, the files would be isolated inside Docker's storage and invisible to coast-service running inside the container.

Use `-v /data:/data` (bind mount), not `-v coast-data:/data` (named volume).

## Register the Remote

On your local machine:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
```

With a custom SSH port:

```bash
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
```

Test connectivity:

```bash
coast remote test my-vm
```

This verifies SSH access, checks that coast-service is reachable on port 31420 over the SSH tunnel, and reports the remote's architecture and coast-service version.

## Build and Run

```bash
# Build on the remote (uses the remote's native architecture)
coast build --type remote

# Run a remote coast instance
coast run dev-1 --type remote
```

After this, all standard commands work:

```bash
coast ps dev-1                              # service status
coast exec dev-1 -- bash                    # shell into remote DinD
coast logs dev-1                            # stream service logs
coast assign dev-1 --worktree feature/x     # switch worktree
coast checkout dev-1                        # canonical ports → dev-1
coast ports dev-1                           # show port mappings
```

## Multiple Remotes

You can register more than one remote machine:

```bash
coast remote add dev-server ubuntu@10.0.0.1 --key ~/.ssh/key1
coast remote add gpu-box   ubuntu@10.0.0.2 --key ~/.ssh/key2
coast remote ls
```

When running or building, specify which remote to target:

```bash
coast build --type remote --remote gpu-box
coast run dev-1 --type remote --remote gpu-box
```

If only one remote is registered, it is selected automatically.

## Local Dev Setup

For developing coast-service itself, use the dev container which includes DinD, sshd, and cargo-watch hot reload:

```bash
make coast-service-dev
```

Then register the dev container as a remote:

```bash
coast remote add dev-vm root@localhost:2222 --key $(pwd)/.dev/ssh/coast_dev_key
coast remote test dev-vm
```

Use absolute paths for the `--key` flag.
