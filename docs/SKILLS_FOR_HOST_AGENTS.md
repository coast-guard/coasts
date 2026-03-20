# Skills for Host Agents

If you use AI coding agents on the host while your app runs inside Coasts, your
agent usually needs two Coast-specific pieces of setup:

1. an always-on Coast Runtime section in the harness's project instruction file
   or rule file
2. a reusable Coast workflow skill such as `/coasts` when the harness supports
   project skills

Without the first piece, the agent edits files but forgets to use `coast exec`.
Without the second, every Coast assignment, log, and UI flow has to be
re-explained in chat.

This guide keeps the setup concrete and Coast-specific: which file to create,
what text goes in it, and how that changes by harness.

## Why agents need this

Coasts share the [filesystem](concepts_and_terminology/FILESYSTEM.md) between
your host machine and the Coast container. Your agent edits files on the host
and the running services inside the Coast see the changes immediately. But the
agent still needs to:

1. discover which Coast instance matches the current checkout
2. run tests, builds, and runtime commands inside that Coast
3. read logs and service status from the Coast
4. handle worktree assignment safely when no Coast is already attached

## What goes where

- `AGENTS.md`, `CLAUDE.md`, or `.cursor/rules/coast.md` — short Coast rules
  that should apply on every task, even if no skill is invoked
- skill (`.agents/skills/...`, `.claude/skills/...`, or `.cursor/skills/...`)
  — the reusable Coast workflow itself, such as `/coasts`
- command file (`.claude/commands/...` or `.cursor/commands/...`) — optional
  explicit entrypoint for harnesses that support it; one simple option is to
  have the command reuse the skill

If one repo uses more than one harness, keep the canonical Coast skill in one
place and expose it where needed. See
[Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md).

## 1. Always-on Coast Runtime rules

Add the following block to the harness's always-on project instruction file or
rule file (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/coast.md`, or equivalent):

```text-copy
# Coast Runtime

This project uses Coasts — containerized runtimes for running services, tests,
and other runtime commands. The filesystem is shared between the host and the
container, so file edits on either side are visible to both immediately.

## Discovery

Before the first runtime command in a session, run:

  coast lookup

This prints the instance name, ports, and example commands. Use the instance
name from the output for all subsequent commands.

## What runs where

The filesystem is shared, so only use `coast exec` for things that need the
container runtime (databases, services, integration tests). Everything else
runs directly on the host.

Use `coast exec` for:
- Tests that need running services (integration tests, API tests)
- Service restarts or compose operations
- Anything that talks to databases, caches, or other container services

Run directly on the host:
- Linting, typechecking, formatting
- Git operations
- Playwright and browser tests
- Installing host-side dependencies (npm install, pip install)
- File search, code generation, static analysis

Example:

  coast exec <instance> -- sh -c "cd <dir> && npm test"    # needs DB
  npm run lint                                              # host is fine
  npx playwright test                                       # host is fine

## Runtime feedback

  coast ps <instance>
  coast logs <instance> --service <service>
  coast logs <instance> --service <service> --tail 50

## Creating and assigning Coasts

If `coast lookup` returns no match, run `coast ls` to see what exists.

If an unassigned Coast is already running for this project, prefer assigning
your worktree to it rather than creating a new one:

  coast assign <existing> -w <worktree>

If no Coast is running, ask the user before creating one — Coasts can be
memory intensive:

  coast run <name> -w <worktree>

A project must be built before instances can be created. If `coast run` fails
because no build exists, run `coast build` first.

## Coastfile setup

If the project does not have a Coastfile yet, or if you need to modify the
Coastfile, read the Coastfile docs first:

  coast docs --path coastfiles/README.md

## When confused

Before guessing about Coast behavior, explore the docs:

  coast docs                                     # list all doc pages
  coast docs --path concepts_and_terminology/RUN.md
  coast docs --path concepts_and_terminology/ASSIGN.md
  coast docs --path concepts_and_terminology/BUILDS.md
  coast search-docs "your question here"         # semantic search

## Rules

- Always run `coast lookup` before your first runtime command in a session.
- Use `coast exec` only for things that need the container runtime.
- Run linting, typechecking, formatting, and git on the host directly.
- Use `coast docs` or `coast search-docs` before guessing about Coast behavior.
- Do not run services directly on the host when the project expects Coast.
```

This block belongs in the always-on file because the rules should apply on
every task, not only when the agent explicitly enters a `/coasts` workflow.

## 2. Reusable `/coasts` skill

When the harness supports project skills, save the skill content as a
`SKILL.md` in your skills directory. The full skill text is in
[skills_prompt.txt](skills_prompt.txt) (if in CLI mode, use
`coast skills-prompt`) — everything after the Coast Runtime block is the skill
content, starting from the `---` frontmatter.

If you are using Codex or OpenAI-specific surfaces, you can optionally add
`agents/openai.yaml` beside the skill for display metadata or invocation
policy. That metadata should live beside the skill, not replace it.

## Harness quick start

| Harness | Always-on file | Reusable Coast workflow | Notes |
|---------|----------------|-------------------------|-------|
| OpenAI Codex | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | No separate project command file to recommend for Coast docs. See [Codex](harnesses/CODEX.md). |
| Claude Code | `CLAUDE.md` | `.claude/skills/coasts/SKILL.md` | `.claude/commands/coasts.md` is optional, but keep the logic in the skill. See [Claude Code](harnesses/CLAUDE_CODE.md). |
| Cursor | `AGENTS.md` or `.cursor/rules/coast.md` | `.cursor/skills/coasts/SKILL.md` or shared `.agents/skills/coasts/SKILL.md` | `.cursor/commands/coasts.md` is optional. `.cursor/worktrees.json` is for Cursor worktree bootstrap, not Coast policy. See [Cursor](harnesses/CURSOR.md). |
| Conductor | `CLAUDE.md` | Start with `CLAUDE.md`; use Conductor scripts and settings for Conductor-specific behavior | Do not assume full Claude Code project command behavior. If a new command does not appear, fully close and reopen Conductor. See [Conductor](harnesses/CONDUCTOR.md). |
| T3 Code | `AGENTS.md` | `.agents/skills/coasts/SKILL.md` | This is the most limited harness surface here. Use the Codex-style layout and do not invent a T3-native command layer for Coast docs. See [T3 Code](harnesses/T3_CODE.md). |

## Let the agent set itself up

The fastest way is to let the agent write the right files itself. Copy the
prompt below into your agent's chat — it includes the Coast Runtime block, the
`coasts` skill block, and harness-specific instructions for where each piece
belongs.

```prompt-copy
skills_prompt.txt
```

You can also get the same output from the CLI by running `coast skills-prompt`.

## Manual setup

- **Codex:** put the Coast Runtime section in `AGENTS.md`, then put the
  reusable `coasts` skill in `.agents/skills/coasts/SKILL.md`.
- **Claude Code:** put the Coast Runtime section in `CLAUDE.md`, then put the
  reusable `coasts` skill in `.claude/skills/coasts/SKILL.md`. Only add
  `.claude/commands/coasts.md` if you specifically want a command file.
- **Cursor:** put the Coast Runtime section in `AGENTS.md` if you want the most
  portable instructions, or in `.cursor/rules/coast.md` if you want a
  Cursor-native project rule. Put the reusable `coasts` workflow in
  `.cursor/skills/coasts/SKILL.md` for a Cursor-only repo, or in
  `.agents/skills/coasts/SKILL.md` if the repo is shared with other harnesses.
  Only add `.cursor/commands/coasts.md` if you specifically want an explicit
  command file.
- **Conductor:** put the Coast Runtime section in `CLAUDE.md`. Use Conductor
  Repository Settings scripts for Conductor-specific bootstrap or run behavior.
  If you add a command and it does not appear, fully close and reopen the app.
- **T3 Code:** use the same layout as Codex: `AGENTS.md` plus
  `.agents/skills/coasts/SKILL.md`. Treat T3 Code as a thin Codex-style
  harness here, not as a separate Coast command surface.
- **Multiple harnesses:** keep the canonical skill in
  `.agents/skills/coasts/SKILL.md`. Cursor can load that directly; expose it to
  Claude Code through `.claude/skills/coasts/` if needed.

## Further reading

- Read the [Harnesses guide](harnesses/README.md) for the per-harness matrix
- Read [Multiple Harnesses](harnesses/MULTIPLE_HARNESSES.md) for the shared
  layout pattern
- Read the [Coastfiles documentation](coastfiles/README.md) to learn the full
  configuration schema
- Learn the [Coast CLI](concepts_and_terminology/CLI.md) commands for managing
  instances
- Explore [Coastguard](concepts_and_terminology/COASTGUARD.md), the web UI for
  observing and controlling your Coasts
