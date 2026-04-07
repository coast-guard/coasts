# Remote Coasts

> **Beta.** Remote coasts are fully functional but the CLI flags, Coastfile schema, and coast-service API may change in future releases. If you discover a bug or defect, please open a pull request or file an issue.

Remote coasts run your services on a remote machine while keeping the developer experience identical to local coasts. `coast run`, `coast assign`, `coast exec`, `coast ps`, `coast logs`, and all other commands work the same way. The daemon detects that the instance is remote and transparently routes operations through an SSH tunnel.

## Why Remote

Local coasts run everything on your laptop. Each coast instance runs a full Docker-in-Docker container with your entire compose stack: web server, API, workers, databases, caches, mail server. That works until your laptop runs out of RAM or disk space.

A full-stack project with several services can consume significant RAM per coast. Run a few coasts in parallel and you hit your laptop's ceiling.

```text
  coast-1         coast-2         coast-3         coast-4
  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
  │ worker   │   │ worker   │   │ worker   │   │ worker   │
  │ api      │   │ api      │   │ api      │   │ api      │
  │ admin    │   │ admin    │   │ admin    │   │ admin    │
  │ web      │   │ web      │   │ web      │   │ web      │
  │ mailhog  │   │ mailhog  │   │ mailhog  │   │ mailhog  │
  │          │   │          │   │          │   │          │
  │ 12 GB    │   │ 12 GB    │   │ 12 GB    │   │ 12 GB    │
  └──────────┘   └──────────┘   └──────────┘   └──────────┘

  Total: 48 GB RAM on your laptop
```

Remote coasts let you horizontally scale by moving some of your coasts to remote machines. The DinD containers, compose services, and image builds run remotely while your editor and agents stay local. Shared services like Postgres and Redis also stay local, keeping your database in sync across local and remote instances via SSH reverse tunnels.

```text
  Your Machine                         Remote Server
  ┌─────────────────────┐             ┌─────────────────────────┐
  │  editor + agents    │             │  coast-1 (all services) │
  │                     │  SSH        │  coast-2 (all services) │
  │  shared services    │──tunnels──▶ │  coast-3 (all services) │
  │  (postgres, redis)  │             │  coast-4 (all services) │
  └─────────────────────┘             └─────────────────────────┘

  Laptop: lightweight                  Server: 64 GB RAM, 16 CPU
```

Horizontally scale your localhost runtime.

## Quick Start

```bash
# 1. Register a remote machine
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote test my-vm

# 2. Build on the remote (uses remote's native architecture)
coast build --type remote

# 3. Run a remote coast
coast run dev-1 --type remote

# 4. Everything works as usual
coast ps dev-1
coast exec dev-1 -- bash
coast assign dev-1 --worktree feature/x
coast checkout dev-1
```

For full setup instructions including host preparation and coast-service deployment, see [Setup](SETUP.md).

## Reference

| Page | What it covers |
|------|----------------|
| [Architecture](ARCHITECTURE.md) | The two-container split (shell coast + remote coast), SSH tunnel layer, port forwarding chain, and how the daemon routes requests |
| [Setup](SETUP.md) | Host requirements, coast-service deployment, registering remotes, and end-to-end quick start |
| [File Sync](FILE_SYNC.md) | rsync for bulk transfer, mutagen for continuous sync, lifecycle across run/assign/stop, exclusions, and race condition handling |
| [Builds](BUILDS.md) | Building on the remote for native architecture, artifact transfer, the `latest-remote` symlink, architecture reuse, and auto-pruning |
| [CLI and Configuration](CLI.md) | `coast remote` commands, `Coastfile.remote` configuration, disk management, and `coast remote prune` |

## See Also

- [Remotes](../concepts_and_terminology/REMOTES.md) -- concept overview in the terminology glossary
- [Shared Services](../concepts_and_terminology/SHARED_SERVICES.md) -- how local shared services are reverse-tunneled to remote coasts
- [Ports](../concepts_and_terminology/PORTS.md) -- how the SSH tunnel layer fits into the canonical/dynamic port model
