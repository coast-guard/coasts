# Conductor

[Conductor](https://conductor.build/) runs parallel Claude Code agents, each in its own isolated workspace. Workspaces are git worktrees stored at `~/conductor/workspaces/<project-name>/`. Each workspace is checked out on a named branch.

Because these worktrees live outside the project root, Coasts needs explicit
configuration to discover and mount them.

## Setup

Add `~/conductor/workspaces/<project-name>` to `worktree_dir`. Unlike Codex (which stores all projects under one flat directory), Conductor nests worktrees under a per-project subdirectory, so the path must include the project name. In the example below, `my-app` must match the actual folder name under `~/conductor/workspaces/` for your repo.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/conductor/workspaces/my-app"]
```

Conductor allows you to configure the workspaces path per-repository, so the default `~/conductor/workspaces` may not match your setup. Check your Conductor repository settings to find the actual path and adjust accordingly — the principle is the same regardless of where the directory lives.

Coasts expands `~` at runtime and treats any path starting with `~/` or `/` as
external. See [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for
details.

After changing `worktree_dir`, existing instances must be **recreated** for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately (Coasts reads the new Coastfile), but
assigning to a Conductor worktree requires the bind mount inside the container.

## Where Coasts guidance goes

Treat Conductor as its own harness for working with Coasts:

- put the short Coast Runtime rules in `CLAUDE.md`
- use Conductor Repository Settings scripts for setup or run behavior that is
  actually Conductor-specific
- do not assume full Claude Code project command or project skill behavior here
- if you add a command and it does not appear, fully close and reopen
  Conductor before testing again
- if this repo also uses other harnesses, see
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) and
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md) for ways to keep the
  shared `/coasts` workflow in one place

## What Coasts does

- **Run** — `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a Conductor worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — At container creation, Coasts mounts
  `~/conductor/workspaces/<project-name>` into the container at
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` is repo-scoped, so only worktrees belonging to the current project appear.
- **Naming** — Conductor worktrees use named branches, so they appear by branch
  name in the Coasts UI and CLI (e.g., `scroll-to-bottom-btn`). A branch can
  only be checked out in one Conductor workspace at a time.
- **Assign** — `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** — Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** — The git watcher scans external directories
  recursively, filtering by `.git` gitdir pointers. If Conductor archives or
  deletes a workspace, Coasts auto-unassigns the instance.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = ["~/conductor/workspaces/my-app"]
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

- `~/conductor/workspaces/my-app/` — Conductor (external, bind-mounted; replace `my-app` with your repo folder name)

## Conductor Env Vars

- Avoid relying on Conductor-specific environment variables (e.g.,
  `CONDUCTOR_PORT`, `CONDUCTOR_WORKSPACE_PATH`) for runtime configuration
  inside Coasts. Coasts manages ports, workspace paths, and service discovery
  independently — use Coastfile `[ports]` and `coast exec` instead.