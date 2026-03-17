# Conductor

[Conductor](https://conductor.build/) runs parallel Claude Code agents, each in its own isolated workspace. Workspaces are git worktrees stored at `~/conductor/workspaces/<project-name>/`. Each workspace is checked out on a named branch.

Because these worktrees live outside the project root, Coast needs explicit configuration to discover and mount them.

## Setup

Add `~/conductor/workspaces/<project-name>` to `worktree_dir`. Unlike Codex (which stores all projects under one flat directory), Conductor nests worktrees under a per-project subdirectory, so the path must include the project name. In the example below, `my-app` must match the actual folder name under `~/conductor/workspaces/` for your repo.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor allows you to configure the workspaces path per-repository, so the default `~/conductor/workspaces` may not match your setup. Check your Conductor repository settings to find the actual path and adjust accordingly — the principle is the same regardless of where the directory lives.

Coast expands `~` at runtime and treats any path starting with `~/` or `/` as external. See [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for details.

After changing `worktree_dir`, existing instances must be **recreated** for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately (Coast reads the new Coastfile), but assigning to a Conductor worktree requires the bind mount inside the container.

## What Coast does

- **Bind mount** — At container creation, Coast mounts `~/conductor/workspaces/<project-name>` into the container at `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` is repo-scoped, so only worktrees belonging to the current project appear.
- **Naming** — Conductor worktrees use named branches, so they appear by branch name in the Coast UI and CLI (e.g., `scroll-to-bottom-btn`). A branch can only be checked out in one Conductor workspace at a time.
- **Assign** — `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** — Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** — The git watcher scans external directories recursively, filtering by `.git` gitdir pointers. If Conductor archives or deletes a workspace, Coast auto-unassigns the instance.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/conductor/workspaces/my-app"]
primary_port = "web"

[ports]
web = 3000
api = 8080

[assign]
default = "none"
[assign.services]
web = "hot"
api = "hot"
```

- `.worktrees/` — Coast-managed worktrees
- `.claude/worktrees/` — Claude Code (local, no special handling)
- `~/.codex/worktrees/` — Codex (external, bind-mounted)
- `~/conductor/workspaces/my-app/` — Conductor (external, bind-mounted; replace `my-app` with your repo folder name)

## Conductor Env Vars

- Avoid relying on Conductor-specific environment variables (e.g., `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) for runtime configuration inside Coasts. Coast manages ports, workspace paths, and service discovery independently — use Coastfile `[ports]` and `coast exec` instead.