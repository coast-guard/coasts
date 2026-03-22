# Cursor

## Quick setup

Requires the [Coast CLI](../GETTING_STARTED.md). Copy this prompt into your
agent's chat to set up Coasts automatically:

```prompt-copy
cursor_setup_prompt.txt
```

You can also get the skill content from the CLI: `coast skills-prompt`.

After setup, **restart Cursor** for the skill and rules changes to take effect.

---

[Cursor](https://cursor.com/docs/agent/overview) can work directly in your
current checkout, and its Parallel Agents feature can also create git
worktrees under `~/.cursor/worktrees/<project-name>/`.

For docs about Coasts, that means there are two setup cases:

- if you are just using Cursor in the current checkout, no Cursor-specific
  `worktree_dir` entry is required
- if you use Cursor Parallel Agents, add the Cursor worktree directory to
  `worktree_dir` so Coasts can discover and assign those worktrees

## Setup

### Current checkout only

If Cursor is just editing the checkout you already opened, Coasts does not need
any special Cursor-specific worktree path. Coasts will treat that checkout like
any other local repository root.

### Cursor Parallel Agents

If you use Parallel Agents, add `~/.cursor/worktrees/<project-name>` to
`worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor stores each agent worktree beneath that per-project directory. Coasts
expands `~` at runtime and treats the path as external, so existing instances
must be recreated for the bind mount to take effect:

```bash
coast rm my-instance
coast build
coast run my-instance
```

The worktree listing updates immediately after the Coastfile change, but
assigning to a Cursor Parallel Agent worktree requires the external bind mount
inside the container.

## Where Coasts guidance goes

### `AGENTS.md` or `.cursor/rules/coast.md`

Put the short, always-on Coast Runtime rules here:

- use `AGENTS.md` if you want the most portable project instructions
- use `.cursor/rules/coast.md` if you want Cursor-native project rules and
  settings UI support
- do not duplicate the same Coast Runtime block in both unless you have a clear
  reason

### `.cursor/skills/coasts/SKILL.md` or shared `.agents/skills/coasts/SKILL.md`

Put the reusable `/coasts` workflow here:

- for a Cursor-only repo, `.cursor/skills/coasts/SKILL.md` is a natural home
- for a multi-harness repo, keep the canonical skill in
  `.agents/skills/coasts/SKILL.md`; Cursor can load that directly
- the skill should own the real `/coasts` workflow: `coast lookup`,
  `coast ls`, `coast run`, `coast assign`, `coast unassign`,
  `coast checkout`, and `coast ui`

### `.cursor/commands/coasts.md`

Cursor also supports project commands. For docs about Coasts, treat commands as
optional:

- add a command only when you want an explicit `/coasts` entrypoint
- one simple option is to have the command reuse the same skill
- if you give the command its own separate instructions, you are taking on a
  second copy of the workflow to maintain

### `.cursor/worktrees.json`

Use `.cursor/worktrees.json` for Cursor's own worktree bootstrap, not for Coasts
policy:

- install dependencies
- copy or symlink `.env` files
- run database migrations or other one-time bootstrap steps

Do not move the Coast Runtime rules or Coast CLI workflow into
`.cursor/worktrees.json`.

## Example layout

### Cursor only

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # optional
.cursor/rules/coast.md            # optional alternative to AGENTS.md
.cursor/worktrees.json            # optional, for Parallel Agents bootstrap
```

### Cursor plus other harnesses

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # optional
```

## What Coasts does

- **Run** — `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a Cursor worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Current checkout** — No special Cursor handling is required when Cursor is
  working directly in the repo you opened.
- **Bind mount** — For Parallel Agents, Coasts mounts
  `~/.cursor/worktrees/<project-name>` into the container at
  `/host-external-wt/{index}`.
- **Discovery** — `git worktree list --porcelain` remains repo-scoped, so Coasts
  only shows Cursor worktrees that belong to the current project.
- **Naming** — Cursor Parallel Agent worktrees appear by their branch names in
  Coasts' CLI and UI.
- **Assign** — `coast assign` remounts `/workspace` from the external bind
  mount path when a Cursor worktree is selected.
- **Gitignored sync** — Continues to work on the host filesystem with absolute
  paths.
- **Orphan detection** — If Cursor cleans up old worktrees, Coasts can detect
  the missing gitdir and unassign them when needed.

## Example

```toml
[coast]
name = "my-app"
compose = "./docker-compose.yml"
worktree_dir = [".worktrees", ".claude/worktrees", "~/.codex/worktrees", "~/.cursor/worktrees/my-app"]
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

- `.claude/worktrees/` — Claude Code worktrees
- `~/.codex/worktrees/` — Codex worktrees
- `~/.cursor/worktrees/my-app/` — Cursor Parallel Agent worktrees

## Limitations

- If you are not using Cursor Parallel Agents, do not add
  `~/.cursor/worktrees/<project-name>` just because you happen to be editing in
  Cursor.
- Keep the Coast Runtime rules in one always-on place: `AGENTS.md` or
  `.cursor/rules/coast.md`. Duplicating both invites drift.
- Keep the reusable `/coasts` workflow in a skill. `.cursor/worktrees.json` is
  for Cursor bootstrap, not Coasts policy.
- If one repo is shared across Cursor, Codex, Claude Code, or T3 Code, prefer
  the shared layout in [Multiple Harnesses](MULTIPLE_HARNESSES.md).
