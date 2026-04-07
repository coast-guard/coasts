# Remote Coasts Specification

> Living spec for remote coast execution. Phase 1 focuses on running ephemeral
> services (DinD, bare services) on a remote machine while keeping the developer
> experience identical to local coasts.

## Overview

Today every coast instance runs locally via a DinD container managed by
`coast-daemon`. Remote coasts split execution in two:

- **Local "shell" coast** — a real Docker container created by `coast-daemon`
  with host bind mounts (`/host-project`, `/workspace`) but no inner Docker
  daemon, no compose services. Its entrypoint is `sleep infinity`. This
  preserves the filesystem bridge so host agents can edit files that flow
  through to `/workspace`.
- **Remote coast** — managed by `coast-service` on the remote machine. Receives
  synced `/workspace` content, runs a DinD container with compose/bare services,
  allocates dynamic ports for each service.

The user's workflow is unchanged: `coast build`, `coast run`, `coast assign`,
`coast exec`, `coast ps`, `coast logs`, `coast stop`, `coast rm`, and
`coast checkout` all work identically. The daemon detects that the instance is
remote and transparently routes operations through an SSH tunnel.

## Architecture

```
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
│                             │  │ Shared Services (phase 1: local)│  │    │
│                             │  └─────────────────────────────────┘  │    │
│                             │                                       │    │
│                             │  state.db (shadow instance,           │    │
│                             │           remote_host, port allocs)   │    │
│                             │  remotes table (registered machines)  │    │
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
│  │  Build artifacts (~/.coast-service/images/)                       │   │
│  │  Image cache (~/.coast-service/image-cache/)                      │   │
│  │  Keystore (encrypted secrets)                                     │   │
│  │  remote-state.db (instances, worktrees)                           │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

## Port Forwarding Chain

All ports are dynamic in the middle layer. Canonical ports only exist at
the endpoints (inside the DinD container where services listen, and on
localhost via `coast checkout`):

```
localhost:3000 (canonical, via coast checkout / socat)
       ↓
localhost:{local_dynamic} (allocated by daemon port_manager)
       ↓ SSH -L tunnel
remote:{remote_dynamic} (allocated by coast-service port_manager)
       ↓ Docker port publish
DinD container :3000 (canonical, where the app listens)
```

This allows multiple instances of the same project on one remote machine
without port conflicts.

## Remote Management CLI

Register and manage remote machines with `coast remote`:

```bash
coast remote add my-vm ubuntu@192.168.1.100          # register
coast remote add my-vm ubuntu@10.0.0.1:2222 --key ~/.ssh/coast_key
coast remote ls                                       # list all
coast remote rm my-vm                                 # remove
coast remote test my-vm                               # SSH connectivity check
coast remote setup my-vm                              # deploy coast-service binary
```

Remotes are stored in the daemon's `state.db` in a `remotes` table:

```sql
CREATE TABLE remotes (
    name TEXT PRIMARY KEY,
    host TEXT NOT NULL,
    user TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    ssh_key TEXT,
    sync_strategy TEXT NOT NULL DEFAULT 'rsync',
    created_at TEXT NOT NULL
);
```

## Coastfile: `Coastfile.remote`

Remote coasts use a new Coastfile type following the existing inheritance
system.

### Naming

| File                          | Type            |
|-------------------------------|-----------------|
| `Coastfile.remote`            | `remote`        |
| `Coastfile.remote.toml`      | `remote`        |
| `Coastfile.remote.light`     | `remote.light`  |
| `Coastfile.remote.light.toml`| `remote.light`  |

### `[remote]` section

Declares that this Coastfile is a remote type. The `[remote]` section contains
only preferences (like sync strategy). Connection details (host, user, port,
SSH key) are **not** stored in the Coastfile — they come from registered remotes
(`coast remote add`) and are resolved at runtime.

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"    # "rsync" (default) or "mutagen"
```

### Validation constraints

1. **Shared volumes are scoped to the remote machine.** `strategy = "shared"`
   creates a Docker volume on the remote host, shared among all coasts running
   on that remote. The volume is not distributed across different remote
   machines.
2. **`[remote]` is required** when the coastfile type starts with `"remote"`.
3. **Non-remote coastfiles cannot have `[remote]`**.
4. **Inline `host` is rejected** — must use `name` to reference a registered
   remote.

## coast-service

Standalone binary for the remote machine. HTTP API on port 31420.

### Routes

| Route                | Method | Purpose |
|----------------------|--------|---------|
| `/health`            | GET    | Health check |
| `/build`             | POST   | Build: parse coastfile, rewrite compose, pull images, write artifact |
| `/run`               | POST   | Provision DinD container, start services |
| `/exec`              | POST   | Execute command inside DinD |
| `/logs`              | POST   | Stream compose logs from DinD |
| `/ps`                | POST   | Query compose service status |
| `/stop`              | POST   | Stop DinD container |
| `/start`             | POST   | Start stopped DinD container |
| `/rm`                | POST   | Remove DinD container + DB record |
| `/assign`            | POST   | Remount /workspace, restart services |
| `/secret`            | POST   | Set/list secrets in remote keystore |
| `/restart-services`  | POST   | Compose down + up inside DinD |

### State

- **Instances DB** at `$COAST_SERVICE_HOME/state.db` — instances table with
  name, project, status, container_id, worktree, build_id, coastfile_type
- **Build artifacts** at `$COAST_SERVICE_HOME/images/{project}/{build_id}/`
- **Image cache** at `$COAST_SERVICE_HOME/image-cache/`
- **Keystore** at `$COAST_SERVICE_HOME/keystore.db` + `.key` — encrypted
  secret storage via `coast-secrets`
- **Workspaces** at `$COAST_SERVICE_HOME/workspaces/{project}/{instance}/`

### Port allocation

Each DinD container gets dynamic host ports allocated via
`port_manager::allocate_dynamic_ports()`. Canonical ports exist only inside
the container. The dynamic ports are returned in `RunResponse` so the
daemon can set up SSH tunnels targeting them.

## Build Strategy

Builds happen **on the remote machine** via coast-service. This ensures the
build uses the remote's native architecture (e.g. x86_64) regardless of the
local machine's architecture (e.g. ARM Mac). No architecture validation or
cross-compilation is needed.

1. On `coast run --type remote`, the daemon:
   a. Rsyncs the project source (Coastfile, compose.yml, Dockerfiles, inject/)
      to the remote workspace via SSH
   b. Calls `POST /build` on coast-service over the SSH tunnel
   c. coast-service runs the full build natively (`docker build`, image
      caching, secret extraction) under `$COAST_SERVICE_HOME/images/`
   d. coast-service returns `BuildResponse` with the artifact path and
      build metadata
2. coast-service manages its own artifact storage, symlinks, and build
   eviction on the remote (keep 5 latest per type)
3. After the remote build completes, the daemon rsyncs the full artifact
   directory (coastfile.toml, compose.yml, manifest.json, secrets/,
   inject/, image tarballs) back to `~/.coast/images/{project}/{build_id}/`
   on the local machine and creates a `latest-remote` symlink
4. Local storage enables reuse: if another remote has the same
   architecture, the pre-built artifact can be transferred directly
   without rebuilding
5. `coast build --type remote` can also be used standalone to trigger a
   remote build without provisioning an instance
6. `coast build --type remote --remote <name>` specifies which registered
   remote to build on when multiple remotes exist
7. On subsequent `coast run --type remote`, if a local build exists:
   - If the build arch matches the target remote: build is reused
   - If the build arch does NOT match: hard error with guidance to
     rebuild for the target architecture

Build on the remote, store locally, deploy to any same-arch remote.

## Run Flow (remote)

1. Parse `Coastfile.remote`, validate constraints
2. Validate and insert shadow instance (status = provisioning)
3. Resolve remote (from `--remote` flag, or auto-select if only one registered)
4. Create shell coast container locally (bind mounts, `sleep infinity`)
5. Transfer build artifact + image cache to remote (with arch validation)
6. Connect to coast-service via SSH tunnel
7. Rsync `/workspace` content to remote
8. Forward `RunRequest` to coast-service
9. coast-service: allocate dynamic ports, create DinD, load images, compose up
10. Receive `RunResponse` with remote dynamic ports
11. Set up SSH `-L` tunnels: `local_dynamic -> remote_dynamic`
12. Store shadow instance (remote_host, port allocations)

## Assign Flow (remote)

1. Resolve worktree path locally
2. Rsync new worktree content to remote `/workspace`
3. Forward `AssignRequest` to coast-service
4. coast-service: remount `/workspace`, compose down + up
5. Update local shadow instance

## Secret Forwarding

When `coast secret set` targets a remote instance:

1. Store in local keystore (existing behavior)
2. Forward `SecretRequest::Set` to coast-service
3. coast-service stores in remote keystore
4. For file secrets: inject into running container via exec
5. For env secrets: stored for next restart

## File Sync

Two-layer sync strategy: rsync for initial bulk transfer, mutagen for
continuous real-time sync. rsync runs from the **host daemon process**.
Mutagen runs **inside the local shell container**.

### Where sync runs

```
Local Machine                          Remote Machine
┌─────────────────────────────┐        ┌──────────────────────────────┐
│  coastd daemon              │        │                              │
│    │                        │        │                              │
│    │ rsync (direct SSH)     │  SSH   │  /data/workspaces/{p}/{i}/   │
│    │────────────────────────│───────▶│    (rsync writes here)       │
│    │                        │        │    │                         │
│    │ docker exec            │        │    │ bind mount              │
│    ▼                        │        │    ▼                         │
│  Shell Container            │  SSH   │  Remote DinD Container       │
│    /workspace (bind mount)  │───────▶│    /workspace                │
│    mutagen (continuous sync)│        │    (compose services running)│
│    SSH key (copied in)      │        │                              │
└─────────────────────────────┘        └──────────────────────────────┘
```

Mutagen is a **coast runtime dependency**, not a host machine dependency.
It is installed in:
- The **coast_image** (built by `coast build` from `[coast.setup]`), used
  by the local shell container
- The **coast-service Docker image** (`Dockerfile.coast-service`), used on
  the remote side

The daemon never runs mutagen directly. It orchestrates via `docker exec`
into the shell container.

### Initial sync (rsync)

On `coast run` and `coast assign`, the daemon runs rsync directly from
the host to transfer workspace files to the remote:

```bash
rsync -rlDzP --delete-after \
  --rsync-path="sudo rsync" \
  --exclude '.git' --exclude 'node_modules' \
  --exclude 'target' --exclude '__pycache__' \
  --exclude '.react-router' --exclude '.next' \
  -e "ssh -p {port} -i {key}" \
  {local_workspace}/ {user}@{host}:{remote_workspace_path}/
```

After rsync completes, the daemon runs `sudo chown -R` on the remote to
give the SSH user ownership of the files (rsync runs as root via
`--rsync-path="sudo rsync"` because the remote workspace may contain
root-owned files from coast-service operations).

rsync exit code 23 (partial transfer) is treated as a non-fatal warning.
This handles races where running dev servers inside the remote DinD
regenerate files (e.g. `.react-router/types/`) while rsync is writing.
Source files transfer successfully; only generated artifacts may fail.

This is fast for the initial transfer and for worktree switches (delta
transfer means only changed files are sent).

### Continuous sync (mutagen)

After the initial rsync, the daemon execs `mutagen sync create` inside
the local shell container:

```bash
docker exec {shell_container} mutagen sync create \
    --name coast-{project}-{instance} \
    --sync-mode one-way-safe \
    --ignore-vcs \
    --ignore node_modules --ignore target \
    --ignore __pycache__ --ignore .next \
    /workspace/ {user}@{host}:{remote_workspace}/
```

Mutagen watches for file changes via OS-level events (inotify inside the
container), batches changes, and transfers deltas over a persistent SSH
connection. Mutagen's agent binary is pre-installed on the coast-service
image.

**Lifecycle:**
- `coast run`: create shell container, initial rsync, start mutagen session inside shell
- `coast assign`: terminate old session, rsync new worktree, start new session
- `coast stop` / `coast rm`: terminate mutagen session inside shell

**Fallback:** If the mutagen session fails to start inside the shell
container, the daemon logs a warning. The initial rsync still provides
the workspace content, but file changes won't sync in real-time.

## Daemon Request Routing

Every daemon handler that operates on an instance checks `remote_host`.
If set, the request is forwarded to coast-service via the SSH tunnel:

| Handler            | Remote behavior |
|--------------------|-----------------|
| `run`              | Shell coast + artifact transfer + forward |
| `build`            | Builds locally (no forwarding) |
| `assign`           | Rsync worktree + forward |
| `exec`             | Forward |
| `ps`               | Forward |
| `logs`             | Forward |
| `stop`             | Forward + kill tunnels |
| `start`            | Forward + re-establish tunnels |
| `rm`               | Forward + kill tunnels + delete shadow |
| `unassign`         | Clear local worktree assignment |
| `rebuild`          | Error (use build + assign instead) |
| `restart-services` | Forward |
| `secret`           | Local store + forward |
| `checkout`         | Local only (socat on host, no forwarding needed) |

## Protocol

Existing request/response types (`RunRequest`, `AssignRequest`,
`ExecRequest`, `LogsRequest`, `StopRequest`, `StartRequest`, `RmRequest`,
`PsRequest`, `SecretRequest`, `RestartServicesRequest`, `BuildRequest`) are
all `Serialize + Deserialize` and forwarded as-is.

Additional types:

- `RemoteRequest` / `RemoteResponse` — CLI remote management (add/ls/rm/test/setup)
- `SyncWorkspaceRequest` / `SyncWorkspaceResponse` — workspace sync metadata
- `TunnelSetupRequest` / `TunnelSetupResponse` — port tunnel lifecycle

## Phase Roadmap

### Phase 1: Ephemeral services remote (implemented)

- `Coastfile.remote` parsing and validation (runtime remote resolution)
- `coast remote add/ls/rm/test/setup` CLI
- `coast-service` crate with full handler set (build, run, exec, logs, ps,
  stop, start, rm, assign, secret, restart-services)
- Local builds with artifact transfer (arch validation, rsync)
- Shell coast creation (bind mounts, sleep infinity)
- Dynamic port allocation on both sides
- SSH port forwarding (tunnel lifecycle, cleanup)
- Shadow instances with remote_host
- Secret extraction on build, forwarding on set
- Dockerfile for coast-service deployment

### Phase 2: Remote shared services

- Topology config: `[shared_services.X].location = "remote"`
- coast-service manages shared services
- Network routing between local and remote shared services

### Phase 3: VM plane

- `coast vm create/destroy/list`
- Switch a coast between local and remote dynamically
- Multi-VM support (different coasts on different VMs)

## Crate Map

| Crate           | Role                                          | Changes |
|-----------------|-----------------------------------------------|---------|
| `coast-core`    | Types, coastfile parsing, protocol            | RemoteConfig, RemoteEntry, RemoteRequest/Response, arch helper, SyncWorkspace/TunnelSetup types |
| `coast-docker`  | DinD runtime, container management            | No changes (reused by coast-service) |
| `coast-secrets` | Encrypted keystore                            | No changes (reused by coast-service) |
| `coast-daemon`  | Local daemon, request routing                 | Remote module (tunnel, sync, forward), shadow instances, shell coast, build artifact transfer, remote branches in all handlers |
| `coast-cli`     | User-facing commands                          | `coast remote add/ls/rm/test/setup` command group |
| `coast-service` | **New.** Remote control plane                 | Full HTTP API, DinD provisioning, build handler, dynamic port manager, secret management, compose lifecycle |

## Integration Test Plan

All integration tests run in the DinDinD environment via
`make run-dind-integration TEST=<name>`. Tests are at
`integrated-examples/remote/` and projects at
`integrated-examples/projects/remote/`.

### Implemented

| Test | What it verifies |
|------|-----------------|
| `test_remote_basic` | Full happy-path lifecycle: remote add, test, ls, build, run, ls, exec, ps, stop, start, rm, remote rm |
| `test_remote_sync` | Mutagen continuous sync: edit file after run, verify on remote; second edit; stop terminates session |
| `test_remote_assign` | Worktree switching: run on main, assign to feature branch, verify remote workspace reflects new branch content, edit in worktree syncs via mutagen |

### Planned: Error paths and edge cases

#### Remote registration errors

- **`test_remote_add_duplicate`** — `coast remote add` with a name that already exists should fail with a clear error, not corrupt state
- **`test_remote_test_unreachable`** — `coast remote test` against a host that doesn't exist (bad IP) should fail gracefully with a timeout, not hang
- **`test_remote_rm_while_instance_running`** — `coast remote rm` when an instance is still running on that remote should either refuse or force-stop the instance first

#### Build errors

- **`test_remote_build_no_coastfile`** — `coast build --type remote` in a directory with no Coastfile.remote should fail with a clear message
- **`test_remote_build_no_remote_registered`** — `coast build --type remote` when no remotes are registered should fail with a clear message
- **`test_remote_arch_mismatch`** — build locally (produces current arch), then attempt `coast run --type remote` against a remote with a different arch — should hard error with "Build artifact is for X but remote is Y"

#### Run errors

- **`test_remote_run_no_build`** — `coast run --type remote` before `coast build` — should fail with "no build found"
- **`test_remote_run_service_unreachable`** — `coast run` when coast-service is not running on the remote — should fail with a connection error, not hang indefinitely
- **`test_remote_run_service_down_mid_provision`** — kill coast-service after `coast run` starts but before provisioning completes — should fail gracefully and clean up the shadow instance
- **`test_remote_run_duplicate_instance`** — `coast run dev-1 --type remote` twice — second should fail with "instance already exists"
- **`test_remote_run_ssh_key_wrong`** — register a remote with a key that doesn't match the remote's authorized_keys — should fail at SSH tunnel establishment with a clear auth error

#### Multi-instance on same remote

- **`test_remote_multi_instance`** — run `dev-1` and `dev-2` of the same project on one remote — both should succeed with different dynamic ports, no port conflicts
- **`test_remote_multi_project`** — run instances of two different projects on the same remote — should work independently
- **`test_remote_multi_instance_stop_one`** — run two instances, stop one, verify the other is still running and accessible

#### Exec/PS/Logs errors

- **`test_remote_exec_stopped_instance`** — `coast exec` on a stopped remote instance should fail with "instance is stopped"
- **`test_remote_exec_bad_command`** — `coast exec dev-1 -- nonexistent_command` should return non-zero exit code
- **`test_remote_ps_stopped_instance`** — `coast ps` on a stopped instance should show empty or appropriate message
- **`test_remote_logs_no_compose`** — `coast logs` on a bare-service project (no compose) — should handle gracefully

#### Assign errors

- **`test_remote_assign_nonexistent_worktree`** — assign to a branch that doesn't exist — should fail clearly
- **`test_remote_assign_stopped_instance`** — assign to a stopped instance — should fail with "cannot assign"
- **`test_remote_assign_back_to_main`** — assign to feature branch, then unassign (return to main) — remote should reflect main content

#### Secret errors

- **`test_remote_secret_set_no_instance`** — `coast secret set nonexistent NAME VALUE` — should fail
- **`test_remote_secret_persists_across_restart`** — set a secret, stop, start, verify the secret is still accessible

#### Stop/Start/Rm errors

- **`test_remote_stop_already_stopped`** — `coast stop` on an already stopped instance — should fail with "already stopped"
- **`test_remote_start_already_running`** — `coast start` on a running instance — should fail with appropriate message
- **`test_remote_rm_stopped`** — `coast rm` on a stopped instance — should succeed (cleanup remote container + shadow)
- **`test_remote_rm_cleans_tunnels`** — after `coast rm`, verify no orphaned SSH tunnel processes remain
- **`test_remote_rm_cleans_mutagen`** — after `coast rm`, verify no orphaned mutagen sessions remain

#### Network/tunnel resilience

- **`test_remote_tunnel_reconnect`** — kill the SSH tunnel process manually, then try `coast exec` — should either reconnect or fail with a clear error (not hang)
- **`test_remote_coast_service_restart`** — restart coast-service while an instance is running — the instance should survive (Docker container persists), and subsequent commands should work after reconnecting

#### Sync edge cases

- **`test_remote_sync_large_file`** — create a large file (>10MB), verify it syncs within reasonable time
- **`test_remote_sync_binary_file`** — create a binary file, verify it syncs correctly (no corruption)
- **`test_remote_sync_delete_file`** — delete a file on the host, verify it's deleted on the remote (rsync --delete + mutagen one-way-safe)
- **`test_remote_sync_gitignored_files`** — files in .gitignore should still sync (mutagen ignores VCS, not gitignored files)
- **`test_remote_sync_rapid_edits`** — save a file 10 times in rapid succession, verify only the final version appears on the remote (mutagen batching)

#### Cleanup and state consistency

- **`test_remote_nuke_cleans_remote`** — `coast nuke` should clean up remote instances and tunnels (if applicable)
- **`test_remote_daemon_restart`** — restart the daemon while a remote instance is running — should the instance survive? What state is the shadow in after restart?
- **`test_remote_stale_shadow`** — create a shadow instance, then manually kill the remote container — `coast ps` / `coast exec` should fail gracefully, `coast rm` should clean up the shadow

---

## Disk Management

### Resource naming conventions

| Resource | Naming pattern | Location |
|----------|---------------|----------|
| DinD volume | `coast-dind--{project}--{instance}` | Remote Docker |
| Workspace | `/data/workspaces/{project}/{instance}` | Remote filesystem |
| Image cache | `/data/image-cache/*.tar` | Remote filesystem |
| Build artifacts | `/data/images/{project}/{build_id}/` | Remote filesystem |

### Disk usage per instance

Each remote coast instance consumes approximately:
- **3-5 GB** for the DinD Docker volume (inner Docker daemon storage, images, layers)
- **50-300 MB** for the workspace directory (project source files)
- **2-3 GB** for cached image tarballs (shared across instances of the same project)

**Recommended minimum disk:** 50 GB for typical projects with 2-3 concurrent instances.

### Cleanup on `coast rm`

When `coast rm` removes a remote instance, it cleans up:
1. The remote DinD container (via coast-service)
2. The DinD Docker volume (`coast-dind--{project}--{name}`)
3. The workspace directory (`/data/workspaces/{project}/{name}`)
4. Local shadow instance record, port allocations, and shell container

### `coast remote prune`

Cleans up orphaned resources left behind by failed or interrupted operations:

```bash
# Show what would be removed
coast remote prune <remote-name> --dry-run

# Actually remove orphaned resources
coast remote prune <remote-name>
```

Prune identifies orphaned resources by cross-referencing Docker volumes and workspace directories against the coast-service instance database. Volumes/workspaces belonging to active instances are never removed.

**When to use:** If `df -h` on the remote shows high disk usage after removing instances, run `coast remote prune` to reclaim space from orphaned volumes.
