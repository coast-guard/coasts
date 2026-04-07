# CLI and Configuration

This page covers the `coast remote` command group, the `Coastfile.remote` configuration format, and disk management for remote machines.

## Remote Management Commands

### `coast remote add`

Register a remote machine with the daemon:

```bash
coast remote add <name> <user>@<host> [--key <path>]
coast remote add <name> <user>@<host>:<port> [--key <path>]
```

Examples:

```bash
coast remote add my-vm ubuntu@10.0.0.1 --key ~/.ssh/my_key
coast remote add dev-box ec2-user@10.50.56.218:22 --key ~/.ssh/coast_key
```

Connection details are stored in the daemon's `state.db`. They are never stored in Coastfiles.

### `coast remote ls`

List all registered remotes:

```bash
coast remote ls
```

### `coast remote rm`

Remove a registered remote:

```bash
coast remote rm <name>
```

If instances are still running on the remote, remove them first with `coast rm`.

### `coast remote test`

Verify SSH connectivity and coast-service availability:

```bash
coast remote test <name>
```

This checks SSH access, confirms coast-service is reachable on port 31420 over the SSH tunnel, and reports the remote's architecture and coast-service version.

### `coast remote prune`

Clean up orphaned resources on a remote machine:

```bash
coast remote prune <name>              # remove orphaned resources
coast remote prune <name> --dry-run    # preview what would be removed
```

Prune identifies orphaned resources by cross-referencing Docker volumes and workspace directories against the coast-service instance database. Resources belonging to active instances are never removed.

## Coastfile Configuration

Remote coasts use a separate Coastfile that extends your base configuration. The file name determines the type:

| File | Type |
|------|------|
| `Coastfile.remote` | `remote` |
| `Coastfile.remote.toml` | `remote` |
| `Coastfile.remote.light` | `remote.light` |
| `Coastfile.remote.light.toml` | `remote.light` |

### Minimal example

```toml
[coast]
name = "my-app"
extends = "Coastfile"

[remote]
workspace_sync = "mutagen"
```

### The `[remote]` section

The `[remote]` section declares sync preferences. Connection details (host, user, SSH key) come from `coast remote add` and are resolved at runtime.

| Field | Default | Description |
|-------|---------|-------------|
| `workspace_sync` | `"rsync"` | Sync strategy: `"rsync"` for one-time bulk transfer only, `"mutagen"` for rsync + continuous real-time sync |

### Validation constraints

1. The `[remote]` section is required when the Coastfile type starts with `remote`.
2. Non-remote Coastfiles cannot have a `[remote]` section.
3. Inline host configuration is not supported. Connection details must come from a registered remote.
4. Shared volumes with `strategy = "shared"` create a Docker volume on the remote host, shared among all coasts on that remote. The volume is not distributed across different remote machines.

### Inheritance

Remote Coastfiles use the same [inheritance system](../coastfiles/INHERITANCE.md) as other typed Coastfiles. The `extends = "Coastfile"` directive merges the base configuration with the remote overrides. You can override ports, services, volumes, and assign strategies just like any other typed variant.

## Disk Management

### Per-instance resource usage

Each remote coast instance consumes approximately:

| Resource | Size | Location |
|----------|------|----------|
| DinD Docker volume | 3-5 GB | Remote Docker storage |
| Workspace directory | 50-300 MB | `/data/workspaces/{project}/{instance}` |
| Image tarballs | 2-3 GB | `/data/image-cache/*.tar` (shared across instances) |
| Build artifacts | 200-500 MB | `/data/images/{project}/{build_id}/` |

Recommended minimum disk: **50 GB** for typical projects with 2-3 concurrent instances.

### Resource naming conventions

| Resource | Naming pattern |
|----------|---------------|
| DinD volume | `coast-dind--{project}--{instance}` |
| Workspace | `/data/workspaces/{project}/{instance}` |
| Image cache | `/data/image-cache/*.tar` |
| Build artifacts | `/data/images/{project}/{build_id}/` |

### Cleanup on `coast rm`

When `coast rm` removes a remote instance, it cleans up:

1. The remote DinD container (via coast-service)
2. The DinD Docker volume (`coast-dind--{project}--{name}`)
3. The workspace directory (`/data/workspaces/{project}/{name}`)
4. Local shadow instance record, port allocations, and shell container

### When to prune

If `df -h` on the remote shows high disk usage after removing instances, orphaned resources may be left behind from failed or interrupted operations. Run `coast remote prune` to reclaim space:

```bash
# See what would be removed
coast remote prune my-vm --dry-run

# Actually remove
coast remote prune my-vm
```

Prune never removes resources belonging to active instances.
