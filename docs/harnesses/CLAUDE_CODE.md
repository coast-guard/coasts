# Claude Code

## Quick setup

Requires the [Coast CLI](../GETTING_STARTED.md). Copy this prompt into your
agent's chat to set up Coasts automatically:

```prompt-copy
claude_code_setup_prompt.txt
```

You can also get the skill content from the CLI: `coast skills-prompt`.

After setup, **start a new Claude Code session** — skills and `CLAUDE.md` changes
are loaded at session start.

---

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) creates
worktrees inside the project at `.claude/worktrees/`. Because that directory
lives inside the repo, Coasts can discover and assign Claude Code worktrees
without any external bind mount.

Claude Code is also the harness here with the clearest split between three
layers for Coasts:

- `CLAUDE.md` for short, always-on rules for working with Coasts
- `.claude/skills/coasts/SKILL.md` for the reusable `/coasts` workflow
- `.claude/commands/coasts.md` only when you want a command file as an extra
  entrypoint

## Setup

Add `.claude/worktrees` to `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

Because `.claude/worktrees` is project-relative, no external bind mount is
needed.

## Where Coasts guidance goes

### `CLAUDE.md`

Put the rules for Coasts that should apply on every task here. Keep this short and
operational:

- run `coast lookup` before the first runtime command in a session
- use `coast exec` for tests, builds, and service commands
- use `coast ps` and `coast logs` for runtime feedback
- ask before creating or reassigning a Coast when no match exists

### `.claude/skills/coasts/SKILL.md`

Put the reusable `/coasts` workflow here. This is the right home for a flow
that:

1. runs `coast lookup` and reuses the matching Coast
2. falls back to `coast ls` when there is no match
3. offers `coast run`, `coast assign`, `coast unassign`, `coast checkout`, and
   `coast ui`
4. uses the Coast CLI directly as the contract instead of wrapping it

If this repo also uses Codex, T3 Code, or Cursor, see
[Multiple Harnesses](MULTIPLE_HARNESSES.md) and keep the canonical skill in
`.agents/skills/coasts/`, then expose it to Claude Code.

### `.claude/commands/coasts.md`

Claude Code also supports project command files. For docs about Coasts, treat
this as optional:

- use it only when you specifically want a command file
- one simple option is to have the command reuse the same skill
- if you give the command its own separate instructions, you are taking on a
  second copy of the workflow to maintain

## Example layout

### Claude Code only

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

If this repo also uses Codex, T3 Code, or Cursor, use the shared pattern in
[Multiple Harnesses](MULTIPLE_HARNESSES.md) instead of duplicating it here,
because duplicated provider-specific guidance gets harder to keep in sync every
time you add another harness.

## What Coasts does

- **Run** — `coast run <name>` creates a new Coast instance from the latest build. Use `coast run <name> -w <worktree>` to create and assign a Claude Code worktree in one step. See [Run](../concepts_and_terminology/RUN.md).
- **Discovery** — Coasts reads `.claude/worktrees` like any other local worktree
  directory.
- **Naming** — Claude Code worktrees follow the same local worktree naming
  behavior as other in-repo worktrees in the Coasts UI and CLI.
- **Assign** — `coast assign` can switch `/workspace` to a Claude Code worktree
  without any external bind-mount indirection.
- **Gitignored sync** — Works normally because the worktrees live inside the
  repository tree.
- **Orphan detection** — If Claude Code removes a worktree, Coasts can detect
  the missing gitdir and unassign it when needed.

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

- `.claude/worktrees/` — Claude Code worktrees
- `~/.codex/worktrees/` — Codex worktrees if you also use Codex in this repo

## Troubleshooting

- **Worktree not found** — If Coasts expects a worktree to exist but cannot
  find it, verify that the Coastfile's `worktree_dir` includes
  `.claude/worktrees`. See [Worktree Directories](../coastfiles/WORKTREE_DIR.md)
  for syntax and path types.

## Limitations

- If you duplicate the same `/coasts` workflow across `CLAUDE.md`,
  `.claude/skills`, and `.claude/commands`, those copies will drift. Keep
  `CLAUDE.md` short and keep the reusable workflow in one skill.
- If you want one repo to work cleanly in multiple harnesses, prefer the shared
  pattern in [Multiple Harnesses](MULTIPLE_HARNESSES.md).
