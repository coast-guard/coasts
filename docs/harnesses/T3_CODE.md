# T3 Code

[T3 Code](https://github.com/pingdotgg/t3code) is an open-source coding agent harness from Ping. Each workspace is a git worktree stored at `~/.t3/worktrees/<project-name>/`, checked out on a named branch.

Because these worktrees live outside the project root, Coast needs explicit configuration to discover and mount them.

## Setup

Add `~/.t3/worktrees/<project-name>` to `worktree_dir`. T3 Code nests worktrees under a per-project subdirectory, so the path must include the project name. In the example below, `my-app` must match the actual folder name under `~/.t3/worktrees/` for your repo.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

Coast expands `~` at runtime and treats any path starting with `~/` or `/` as external. See [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for details.

After changing `worktree_dir`, existing instances must be **recreated** for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately (Coast reads the new Coastfile), but assigning to a T3 Code worktree requires the bind mount inside the container.

## What Coast does

- **Bind mount** — At container creation, Coast mounts `~/.t3/worktrees/<project-name>` into the container at `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` is repo-scoped, so only worktrees belonging to the current project appear.
- **Naming** — T3 Code worktrees use named branches, so they appear by branch name in the Coast UI and CLI.
- **Assign** — `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** — Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** — The git watcher scans external directories recursively, filtering by `.git` gitdir pointers. If T3 Code removes a workspace, Coast auto-unassigns the instance.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.t3/worktrees/my-app"]
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
- `~/.t3/worktrees/my-app/` — T3 Code (external, bind-mounted; replace `my-app` with your repo folder name)

## Limitations

- Coast discovers and mounts T3 Code worktrees but does not create or delete them.
- New worktrees created by `coast assign` always go in the local `default_worktree_dir`, never in an external directory.
- Avoid relying on T3 Code-specific environment variables for runtime configuration inside Coasts. Coast manages ports, workspace paths, and service discovery independently — use Coastfile `[ports]` and `coast exec` instead.
