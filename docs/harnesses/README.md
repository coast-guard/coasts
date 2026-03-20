# Harnesses

Each harness creates git worktrees in a different location. In Coasts, the
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) array tells it where to look --
including external paths like `~/.codex/worktrees` that require additional
bind mounts.

Each harness also has its own conventions for project-level instructions, skills, and commands. The matrix below shows what each harness supports so you know where to put guidance for Coasts. Each page covers the Coastfile configuration, the recommended file layout, and any caveats specific to that harness.

If one repo is used from multiple harnesses, see [Multiple Harnesses](MULTIPLE_HARNESSES.md).

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills surface as `/` commands | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` or `.cursor/rules/` | `.cursor/skills/` or `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

Skills and commands both let you define a reusable `/coasts` workflow. You can use either or both, depending on what the harness supports.

If your harness supports commands and you want an explicit `/coasts`
entrypoint, one simple option is to add a command that reuses the skill.
Commands are explicitly invoked by name, so you know exactly when the
`/coasts` workflow runs. Skills can also be loaded automatically by the agent
based on context, which is useful but means you have less control over when the
instructions are pulled in.

You can use both. If you do, let the command reuse the skill instead of
maintaining a separate copy of the workflow.

If the harness only supports skills (T3 Code), use a skill. If it supports
neither (Conductor), put the `/coasts` workflow directly in the project
instructions file.
