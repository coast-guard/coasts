# Filesystem

Your host machine and every Coast instance share the same project files. The host project root is bind-mounted into the DinD container at `/workspace`, so edits on the host appear inside the Coast instantly and vice versa. This is what makes it possible for an agent running on your host machine to edit code while services inside the Coast pick up the changes in real time.

## The Shared Mount

```text
Host machine
│
├── ~/dev/my-app/                     (project root)
│   ├── src/
│   ├── Coastfile
│   ├── docker-compose.yml
│   └── .worktrees/                   (worktrees, gitignored)
│       ├── feature-auth/
│       └── feature-billing/
│
└── Docker daemon (host)
    │
    └── Coast: dev-1 (docker:dind)
        │
        ├── /host-project              ← Docker bind mount of project root (RW, fixed)
        │
        ├── /workspace                 ← mount --bind /host-project (switchable)
        │   ├── src/                     same files, same bytes, instant sync
        │   ├── Coastfile
        │   └── docker-compose.yml
        │
        └── Inner Docker daemon
            └── web service
                └── /app               ← compose bind mount from /workspace/src
```

The host project root is mounted read-write at `/host-project` inside the [DinD container](RUNTIMES_AND_SERVICES.md) when the container is created. After the container starts, an in-container `mount --bind /host-project /workspace` creates the working `/workspace` path with shared mount propagation (`mount --make-rshared`), so inner compose services that bind-mount subdirectories of `/workspace` see the correct content.

This two-stage approach exists for a reason: the Docker bind mount at `/host-project` is fixed at container creation and cannot be changed without recreating the container. But the Linux bind mount at `/workspace` inside the container can be unmounted and re-bound to a different subdirectory — a worktree — without touching the container lifecycle. This is what makes `coast assign` fast.

`/workspace` is read-write. File changes flow both directions instantly. Save a file on the host and a dev server inside the Coast picks it up. Create a file inside the Coast and it appears on the host.

## Host Agents and Coast

```text
┌─── Host machine ──────────────────────────────────────────┐
│                                                           │
│   AI Agent (Cursor, Claude Code, etc.)                    │
│     │                                                     │
│     ├── reads/writes files at <project root>/src/         │
│     │       ↕ (instant, same filesystem)                  │
│     ├── coast logs dev-1 --service web --tail 50          │
│     ├── coast ps dev-1                                    │
│     └── coast exec dev-1 -- npm test                      │
│                                                           │
├───────────────────────────────────────────────────────────┤
│                                                           │
│   Coast: dev-1                                            │
│     └── /workspace/src/  ← same bytes as host project/src │
│         └── web service picks up changes on save          │
│                                                           │
└───────────────────────────────────────────────────────────┘
```

Because the filesystem is shared, an AI coding agent running on the host can edit files freely and the running services inside the Coast see the changes immediately. The agent does not need to run inside the Coast container — it operates from the host as normal.

When the agent needs runtime information — logs, service status, test output — it calls Coast CLI commands from the host:

- `coast logs dev-1 --service web --tail 50` for service output (see [Logs](LOGS.md))
- `coast ps dev-1` for service status (see [Runtimes and Services](RUNTIMES_AND_SERVICES.md))
- `coast exec dev-1 -- npm test` to run commands inside the Coast (see [Exec & Docker](EXEC_AND_DOCKER.md))

This is the fundamental architectural advantage: **code editing happens on the host, runtime happens in the Coast, and the shared filesystem bridges them.** The host agent never needs to be "inside" the Coast to do its work.

## Worktree Switching

When `coast assign` switches a Coast to a different worktree, it remounts `/workspace` to point at that git worktree instead of the project root:

```text
coast assign dev-1 --worktree feature-auth

Before:  /workspace  ←──mount──  /host-project                          (project root)
After:   /workspace  ←──mount──  /host-project/.worktrees/feature-auth   (worktree)
```

The worktree is created on the host at `{project_root}/.worktrees/{worktree_name}`. The `.worktrees` directory name is configurable via `worktree_dir` in your Coastfile and should be in your `.gitignore`.

Inside the container, `/workspace` is lazy-unmounted and re-bound to the worktree subdirectory at `/host-project/.worktrees/{branch_name}`. This remount is fast — it does not recreate the DinD container or restart the inner Docker daemon. Inner compose services are recreated so their bind mounts resolve through the new `/workspace`.

Gitignored files like `node_modules` are synced from the project root into the worktree via rsync with hardlinks, so the initial setup is near-instant even for large dependency trees.

On macOS, file I/O between the host and the Docker VM has inherent overhead. Coast runs `git ls-files` during assign and unassign to diff the worktree, and in large codebases this can add noticeable latency. If parts of your project do not need to be diffed between assigns (docs, test fixtures, scripts), you can exclude them with `exclude_paths` in your Coastfile to reduce this overhead. See [Assign and Unassign](ASSIGN.md) for details.

`coast unassign` reverts `/workspace` back to `/host-project` (the project root). `coast start` after a stop re-applies the correct mount based on whether the instance has an assigned worktree.

## All Mounts

Every Coast container has these mounts:

| Path | Type | Access | Purpose |
|---|---|---|---|
| `/workspace` | bind mount (in-container) | RW | Project root or worktree. Switchable on assign. |
| `/host-project` | Docker bind mount | RW | Raw project root. Fixed at container creation. |
| `/image-cache` | Docker bind mount | RO | Pre-pulled OCI tarballs from `~/.coast/image-cache/`. |
| `/coast-artifact` | Docker bind mount | RO | Build artifact with rewritten compose files. |
| `/coast-override` | Docker bind mount | RO | Generated compose overrides for [shared services](SHARED_SERVICES.md). |
| `/var/lib/docker` | Named volume | RW | Inner Docker daemon state. Persists across container removal. |

The read-only mounts are infrastructure — they carry the build artifact, cached images, and compose overrides that Coast generates. You interact with them indirectly through `coast build` and the Coastfile. The read-write mounts are where your code lives and where the inner daemon stores its state.
