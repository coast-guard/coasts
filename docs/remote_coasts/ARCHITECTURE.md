# Architecture

A remote coast splits execution between your local machine and a remote server. The developer experience is unchanged because the daemon transparently routes every operation through an SSH tunnel.

## The Two-Container Split

Every remote coast creates two containers:

### Shell Coast (local)

A lightweight Docker container on your machine. It has the same bind mounts as a normal coast (`/host-project`, `/workspace`) but no inner Docker daemon and no compose services. Its entrypoint is `sleep infinity`.

The shell coast exists for one reason: it preserves the [filesystem bridge](../concepts_and_terminology/FILESYSTEM.md) so host-side agents and editors can edit files under `/workspace`. Those edits are synced to the remote via [rsync and mutagen](FILE_SYNC.md).

### Remote Coast (remote)

Managed by `coast-service` on the remote machine. This is where the actual work happens: a full DinD container running your compose services, with dynamic ports allocated for each service.

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ LOCAL MACHINE                                                            │
│                                                                          │
│  ┌────────────┐    unix     ┌───────────────────────────────────────┐    │
│  │ coast CLI  │───socket───▶│ coast-daemon                         │    │
│  └────────────┘             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shell Coast (sleep infinity)    │  │    │
│                             │  │ - /host-project (bind mount)    │  │    │
│                             │  │ - /workspace (mount --bind)     │  │    │
│                             │  │ - NO inner docker               │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Port Manager                    │  │    │
│                             │  │ - allocates local dynamic ports │  │    │
│                             │  │ - SSH -L tunnels to remote      │  │    │
│                             │  │   dynamic ports                 │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  ┌─────────────────────────────────┐  │    │
│                             │  │ Shared Services (local)         │  │    │
│                             │  │ - postgres, redis, etc.         │  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             └───────────────────┬───────────────────┘    │
│                                                 │                        │
│                                    SSH tunnel   │  rsync / SSH           │
│                                                 │                        │
└─────────────────────────────────────────────────┼────────────────────────┘
                                                  │
┌─────────────────────────────────────────────────┼────────────────────────┐
│ REMOTE MACHINE                                  │                        │
│                                                 ▼                        │
│  ┌───────────────────────────────────────────────────────────────────┐   │
│  │ coast-service (HTTP API on :31420)                                │   │
│  │                                                                   │   │
│  │  ┌───────────────────────────────────────────────────────────┐    │   │
│  │  │ DinD Container (per instance)                             │    │   │
│  │  │  /workspace (synced from local)                           │    │   │
│  │  │  compose services / bare services                         │    │   │
│  │  │  published on dynamic ports (e.g. :52340 -> :3000)        │    │   │
│  │  └───────────────────────────────────────────────────────────┘    │   │
│  │                                                                   │   │
│  │  Port Manager (dynamic port allocation per instance)              │   │
│  │  Build artifacts (/data/images/)                                  │   │
│  │  Image cache (/data/image-cache/)                                 │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## SSH Tunnel Layer

The daemon bridges local and remote using two kinds of SSH tunnels:

### Forward Tunnels (local to remote)

For each service port, the daemon creates an `ssh -L` tunnel that maps a local dynamic port to the corresponding remote dynamic port. This is what makes `localhost:{dynamic_port}` reach the remote service.

```text
ssh -N -L {local_dynamic}:localhost:{remote_dynamic} user@remote
```

When you run `coast ports`, the dynamic column shows these local tunnel endpoints.

### Reverse Tunnels (remote to local)

[Shared services](../concepts_and_terminology/SHARED_SERVICES.md) (Postgres, Redis, etc.) run on your local machine. The daemon creates `ssh -R` tunnels so the remote DinD container can reach them:

```text
ssh -N -R 0.0.0.0:{remote_port}:localhost:{local_port} user@remote
```

Inside the remote DinD container, services connect to shared services via `host.docker.internal:{port}`, which resolves to the Docker bridge gateway where the reverse tunnel is listening.

The remote host's sshd must have `GatewayPorts clientspecified` enabled for reverse tunnels to bind on `0.0.0.0` instead of `127.0.0.1`.

### Tunnel Recovery

SSH tunnels can break when your laptop sleeps or the network changes. The daemon runs a background health loop that:

1. Probes each dynamic port every 5 seconds via TCP connect.
2. If all ports for an instance are dead, kills the stale tunnel processes for that instance and re-establishes them.
3. If only some ports are dead (partial failure), re-establishes just the missing tunnels without disrupting healthy ones.
4. Clears stale remote port bindings via `fuser -k` before creating new reverse tunnels.

Healing is per-instance -- recovering one instance's tunnels never disrupts another's.

## Port Forwarding Chain

All ports are dynamic in the middle layer. Canonical ports only exist at the endpoints: inside the DinD container where services listen, and on your localhost via [`coast checkout`](../concepts_and_terminology/CHECKOUT.md).

```text
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

This three-hop chain allows multiple instances of the same project on one remote machine without port conflicts. Each instance gets its own set of dynamic ports on both sides.

## Request Routing

Every daemon handler checks `remote_host` on the instance. If set, the request is forwarded to coast-service via the SSH tunnel:

| Command | Remote behavior |
|---------|-----------------|
| `coast run` | Create shell coast locally + transfer artifacts + forward to coast-service |
| `coast build` | Build on the remote machine (no forwarding of local build) |
| `coast assign` | Rsync new worktree content + forward assign request |
| `coast exec` | Forward to coast-service |
| `coast ps` | Forward to coast-service |
| `coast logs` | Forward to coast-service |
| `coast stop` | Forward + kill local SSH tunnels |
| `coast start` | Forward + re-establish SSH tunnels |
| `coast rm` | Forward + kill tunnels + delete local shadow instance |
| `coast checkout` | Local only (socat on host, no forwarding needed) |
| `coast secret set` | Store locally + forward to remote keystore |

## coast-service

`coast-service` is the control plane running on the remote machine. It is an HTTP server (Axum) listening on port 31420 that mirrors the daemon's local operations: build, run, assign, exec, ps, logs, stop, start, rm, secrets, and service restarts.

It manages its own SQLite state database, Docker containers (DinD), dynamic port allocation, build artifacts, image cache, and encrypted keystore. The daemon communicates with it exclusively over the SSH tunnel -- coast-service is never exposed to the public internet.

See [Setup](SETUP.md) for deployment instructions.
