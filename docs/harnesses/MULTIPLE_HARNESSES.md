# Multiple Harnesses

If one repository is used from more than one harness, one way to consolidate
the Coasts setup is to keep the shared `/coasts` workflow in one place and keep
the harness-specific always-on rules in the files for each harness.

## Recommended layout

```text
AGENTS.md
CLAUDE.md
.cursor/rules/coast.md           # optional Cursor-native always-on rules
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md       # optional, thin, harness-specific
.claude/commands/coasts.md   # optional, thin, harness-specific
```

Use this layout like this:

- `AGENTS.md` — short, always-on rules for working with Coasts in Codex and T3
  Code
- `.cursor/rules/coast.md` — optional Cursor-native always-on rules
- `CLAUDE.md` — short, always-on rules for working with Coasts in Claude Code
  and Conductor
- `.agents/skills/coasts/SKILL.md` — canonical reusable `/coasts` workflow
- `.agents/skills/coasts/agents/openai.yaml` — optional Codex/OpenAI metadata
- `.claude/skills/coasts` — Claude-facing mirror or symlink when Claude Code
  also needs the same skill
- `.cursor/commands/coasts.md` — optional Cursor command file; one simple
  option is to have it reuse the same skill
- `.claude/commands/coasts.md` — optional explicit command file; one simple
  option is to have it reuse the same skill

## Step-by-step

1. Put the Coast Runtime rules in the always-on instruction files.
   - `AGENTS.md`, `CLAUDE.md`, or `.cursor/rules/coast.md` should answer the
     "every task" rules: run `coast lookup` first, use `coast exec`, read logs
     with `coast logs`, ask before `coast assign` or `coast run` when there is
     no match.
2. Create one canonical skill for Coasts.
   - Put the reusable `/coasts` workflow in `.agents/skills/coasts/SKILL.md`.
   - Use the Coast CLI directly inside that skill: `coast lookup`,
     `coast ls`, `coast run`, `coast assign`, `coast unassign`,
     `coast checkout`, and `coast ui`.
3. Expose that skill only where a harness needs a different path.
   - Codex, T3 Code, and Cursor can all use `.agents/skills/` directly.
   - Claude Code needs `.claude/skills/`, so mirror or symlink the canonical
     skill into that location.
4. Add a command file only if you want an explicit `/coasts` entrypoint.
   - If you create `.claude/commands/coasts.md` or
     `.cursor/commands/coasts.md`, one simple option is to have the command
     reuse the same skill.
   - If you give the command its own separate instructions, you are taking on a
     second copy of the workflow to maintain.
5. Keep Conductor-specific setup in Conductor, not in the skill.
   - Use Conductor Repository Settings scripts for bootstrap or run behavior
     that belongs to Conductor itself.
   - Keep Coasts policy and use of the `coast` CLI in `CLAUDE.md` and the
     shared skill.

## Concrete `/coasts` example

A good shared `coasts` skill should do three jobs:

1. `Use Existing Coast`
   - run `coast lookup`
   - if a match exists, use `coast exec`, `coast ps`, and `coast logs`
2. `Manage Assignment`
   - run `coast ls`
   - offer `coast run`, `coast assign`, `coast unassign`, or
     `coast checkout`
   - ask before reusing or disrupting an existing slot
3. `Open UI`
   - run `coast ui`

That is the right place for the `/coasts` workflow. The always-on files should
only hold the short rules that must apply even when the skill is never invoked.

## Symlink pattern

If you want Claude Code to reuse the same skill as Codex, T3 Code, or Cursor,
one option is a symlink:

```bash
mkdir -p .claude/skills
ln -s ../../.agents/skills/coasts .claude/skills/coasts
```

A checked-in mirror is also fine if your team prefers not to use symlinks. The
main goal is just to avoid unnecessary drift between copies.

## Harness-specific cautions

- Claude Code: project skills and optional project commands are both valid, but
  keep the logic in the skill.
- Cursor: use `AGENTS.md` or `.cursor/rules/coast.md` for the short Coast
  Runtime rules, use a skill for the reusable workflow, and keep
  `.cursor/commands` optional.
- Conductor: treat it as `CLAUDE.md` plus Conductor scripts and settings first.
  If you add a command and it does not appear, fully close and reopen the app
  before checking again.
- T3 Code: this is the thinnest harness surface here. Use the Codex-style
  `AGENTS.md` plus `.agents/skills` pattern, and do not invent a separate
  T3-specific command layout for docs about Coasts.
- Codex: keep `AGENTS.md` short and put the reusable workflow in
  `.agents/skills`.
