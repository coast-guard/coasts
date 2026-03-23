# T3 Code

## Quick setup

Requires the [Coast CLI](../GETTING_STARTED.md). Copy this prompt into your
agent's chat to set up Coasts automatically:

```prompt-copy
t3_code_setup_prompt.txt
```

You can also get the skill content from the CLI: `coast skills-prompt`.

After setup, **restart T3 Code** for the skill and rules changes to take effect.

**Note:** T3 Code may not load project-level skills from `.agents/skills/` or
`.claude/skills/` yet. The setup prompt also places the skill in
`~/.codex/skills/coasts/` so it is available globally to the Codex provider.
The Coast Runtime rules in `AGENTS.md` and `CLAUDE.md` still apply on every
task regardless.

---

[T3 Code](https://github.com/pingdotgg/t3code) creates git worktrees at
`~/.t3/worktrees/<project-name>/`, checked out on named branches.

T3 Code wraps Codex, so it uses `AGENTS.md` for always-on rules and
`.agents/skills/coasts/SKILL.md` for the reusable `/coasts` workflow.

Because these worktrees live outside the project root, Coasts needs explicit
configuration to discover and mount them.

## Setup

Add `~/.t3/worktrees/<project-name>` to `worktree_dir`. T3 Code nests worktrees under a per-project subdirectory, so the path must include the project name. In the example below, `my-app` must match the actual folder name under `~/.t3/worktrees/` for your repo.

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.t3/worktrees/my-app"]
```

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
assigning to a T3 Code worktree requires the bind mount inside the container.

## Where Coasts guidance goes

Use this layout for T3 Code:

- put the short Coast Runtime rules in `AGENTS.md`
- put the reusable `/coasts` workflow in `.agents/skills/coasts/SKILL.md`
- do not add a separate T3-specific project command or slash-command layer for
  Coasts
- if this repo uses multiple harnesses, see
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) and
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

## What Coasts does

- **Run** — `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a T3 Code worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** — At container creation, Coasts mounts
  `~/.t3/worktrees/<project-name>` into the container at
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` is repo-scoped, so only worktrees belonging to the current project appear.
- **Naming** — T3 Code worktrees use named branches, so they appear by branch
  name in the Coasts UI and CLI.
- **Assign** — `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** — Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** — The git watcher scans external directories
  recursively, filtering by `.git` gitdir pointers. If T3 Code removes a
  workspace, Coasts auto-unassigns the instance.

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

- `.claude/worktrees/` — Claude Code (local, no special handling)
- `~/.codex/worktrees/` — Codex (external, bind-mounted)
- `~/.t3/worktrees/my-app/` — T3 Code (external, bind-mounted; replace `my-app` with your repo folder name)

## Troubleshooting

- **Worktree not found** — If Coasts expects a worktree to exist but cannot
  find it, verify that the Coastfile's `worktree_dir` includes
  `~/.t3/worktrees/<project-name>` and that `<project-name>` matches the
  actual folder name under `~/.t3/worktrees/`. See
  [Worktree Directories](../coastfiles/WORKTREE_DIR.md) for syntax and path
  types.

## Limitations

- Avoid relying on T3 Code-specific environment variables for runtime
  configuration inside Coasts. Coasts manages ports, workspace paths, and
  service discovery independently — use Coastfile `[ports]` and `coast exec`
  instead.
