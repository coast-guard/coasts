# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) creates worktrees at `$CODEX_HOME/worktrees` (typically `~/.codex/worktrees`). Each worktree lives under an opaque hash directory like `~/.codex/worktrees/a0db/project-name`, starts on a detached HEAD, and is cleaned up automatically based on Codex's retention policy.

From the [Codex docs](https://developers.openai.com/codex/app/worktrees/):

> Can I control where worktrees are created?
> Not today. Codex creates worktrees under `$CODEX_HOME/worktrees` so it can manage them consistently.

Because these worktrees live outside the project root, Coast needs explicit configuration to discover and mount them.

## Setup

Add `~/.codex/worktrees` to `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coast expands `~` at runtime and treats any path starting with `~/` or `/` as external. See [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for details.

After changing `worktree_dir`, existing instances must be **recreated** for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately (Coast reads the new Coastfile), but assigning to a Codex worktree requires the bind mount inside the container.

## What Coast does

- **Bind mount** -- At container creation, Coast mounts `~/.codex/worktrees` into the container at `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` is repo-scoped, so only Codex worktrees belonging to the current project appear, even though the directory contains worktrees for many projects.
- **Naming** -- Detached HEAD worktrees show as their relative path within the external dir (`a0db/my-app`, `eca7/my-app`). Branch-based worktrees show the branch name.
- **Assign** -- `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** -- Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** -- The git watcher scans external directories recursively, filtering by `.git` gitdir pointers. If Codex deletes a worktree, Coast auto-unassigns the instance.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees"]
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

- `.worktrees/` -- Coast-managed worktrees
- `.claude/worktrees/` -- Claude Code (local, no special handling)
- `~/.codex/worktrees/` -- Codex (external, bind-mounted)

## Limitations

- Coast discovers and mounts Codex worktrees but does not create or delete them.
- Codex may clean up worktrees at any time. Coast's orphan detection handles this gracefully.
- New worktrees created by `coast assign` always go in the local `default_worktree_dir`, never in an external directory.
