# Remotes

A remote coast runs your services on a remote machine instead of your laptop. The CLI and UI experience is identical to local coasts -- `coast run`, `coast assign`, `coast exec`, `coast ps`, and `coast checkout` all work the same way. The daemon detects that the instance is remote and routes operations through an SSH tunnel to `coast-service` on the remote host.

## Local vs Remote

| | Local Coast | Remote Coast |
|---|---|---|
| DinD container | Runs on your machine | Runs on the remote machine |
| Compose services | Inside local DinD | Inside remote DinD |
| File editing | Direct bind mount | Shell coast (local) + rsync/mutagen sync |
| Port access | `socat` forwarder | SSH `-L` tunnel + `socat` forwarder |
| Shared services | Bridge network | SSH `-R` reverse tunnel |
| Build architecture | Your machine's arch | Remote machine's arch |

## How It Works

Every remote coast creates two containers:

1. A **shell coast** on your local machine. This is a lightweight Docker container (`sleep infinity`) with the same bind mounts as a normal coast (`/host-project`, `/workspace`). It exists so host agents can edit files that sync to the remote.

2. A **remote coast** on the remote machine, managed by `coast-service`. This runs the actual DinD container with your compose services, using dynamic ports.

The daemon bridges them with SSH tunnels:

- **Forward tunnels** (`ssh -L`): map each local dynamic port to the corresponding remote dynamic port, so `localhost:{dynamic}` reaches the remote service.
- **Reverse tunnels** (`ssh -R`): expose local [shared services](SHARED_SERVICES.md) (Postgres, Redis) to the remote DinD container.

## Registering Remotes

Remotes are registered with the daemon and stored in `state.db`:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/coast_key
coast remote test my-vm
coast remote ls
coast remote rm my-vm
```

Connection details (host, user, port, SSH key) live in the daemon's database, not in your Coastfile. The Coastfile only declares sync preferences via the `[remote]` section.

## Remote Builds

Builds happen on the remote machine so images use the remote's native architecture. An ARM Mac can build x86_64 images on an x86_64 remote without cross-compilation.

After building, the artifact is transferred back to your local machine for reuse. If another remote has the same architecture, the pre-built artifact can be deployed directly without rebuilding. See [Builds](BUILDS.md) for more on how build artifacts are structured.

## File Sync

Remote coasts use rsync for initial bulk transfer and mutagen for continuous real-time sync. Both tools run inside coast containers (the shell coast and the coast-service image), not on your host machine. See the [Remote Coasts](../remote_coasts/README.md) guide for details on sync configuration.

## Disk Management

Remote machines accumulate Docker volumes, workspace directories, and image tarballs. When `coast rm` removes a remote instance, all associated resources are cleaned up. For orphaned resources from failed operations, use `coast remote prune`.

## Setup

For full setup instructions including host requirements, coast-service deployment, and Coastfile configuration, see the [Remote Coasts](../remote_coasts/README.md) guide.
