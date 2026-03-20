# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) creates worktrees at `$CODEX_HOME/worktrees` (typically `~/.codex/worktrees`). Each worktree lives under an opaque hash directory like `~/.codex/worktrees/a0db/project-name`, starts on a detached HEAD, and is cleaned up automatically based on Codex's retention policy.

From the [Codex docs](https://developers.openai.com/codex/app/worktrees/):

> Can I control where worktrees are created?
> Not today. Codex creates worktrees under `$CODEX_HOME/worktrees` so it can manage them consistently.

Because these worktrees live outside the project root, Coasts needs explicit
configuration to discover and mount them.

## Setup

Add `~/.codex/worktrees` to `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
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
assigning to a Codex worktree requires the bind mount inside the container.

## Where Coasts guidance goes

Use Codex's project instruction file and shared skill layout for working with
Coasts:

- put the short Coast Runtime rules in `AGENTS.md`
- put the reusable `/coasts` workflow in `.agents/skills/coasts/SKILL.md`
- Codex surfaces that skill as the `/coasts` command
- if you use Codex-specific metadata, keep it beside the skill in
  `.agents/skills/coasts/agents/openai.yaml`
- do not create a separate project command file just for docs about Coasts; the
  skill is the reusable surface
- if this repo also uses Cursor or Claude Code, keep the canonical skill in
  `.agents/skills/` and expose it from there. See
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) and
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md).

For example, a minimal `.agents/skills/coasts/agents/openai.yaml` could look
like this:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

That keeps the skill visible in Codex with a nicer label and makes `/coasts` an
explicit command. Only add `dependencies.tools` if the skill also needs MCP
servers or other OpenAI-managed tool wiring.

## What Coasts does

- **Run** -- `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a Codex worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Bind mount** -- At container creation, Coasts mounts
  `~/.codex/worktrees` into the container at `/host-external-wt/{index}`.
- **Discovery** -- `git worktree list --porcelain` is repo-scoped, so only Codex worktrees belonging to the current project appear, even though the directory contains worktrees for many projects.
- **Naming** -- Detached HEAD worktrees show as their relative path within the external dir (`a0db/my-app`, `eca7/my-app`). Branch-based worktrees show the branch name.
- **Assign** -- `coast assign` remounts `/workspace` from the external bind mount path.
- **Gitignored sync** -- Runs on the host filesystem with absolute paths, works without the bind mount.
- **Orphan detection** -- The git watcher scans external directories
  recursively, filtering by `.git` gitdir pointers. If Codex deletes a
  worktree, Coasts auto-unassigns the instance.

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

- `.claude/worktrees/` -- Claude Code (local, no special handling)
- `~/.codex/worktrees/` -- Codex (external, bind-mounted)

## Limitations

- Codex may clean up worktrees at any time. The orphan detection in Coasts
  handles this gracefully.
