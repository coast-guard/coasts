# Harnesses

每种 harness 都会在不同的位置创建 git worktree。在 Coasts 中，
[`worktree_dir`](../coastfiles/WORKTREE_DIR.md) 数组会告诉它去哪里查找——
包括像 `~/.codex/worktrees` 这样需要额外 bind mount 的外部路径。

每种 harness 对项目级说明、skills 和 commands 也都有各自的约定。下方矩阵展示了每种 harness 支持哪些内容，以便你知道应将 Coasts 的指引放在哪里。每个页面都会介绍该 harness 的 Coastfile 配置、推荐的文件布局，以及任何特定注意事项。

如果同一个仓库会被多个 harness 使用，请参阅 [Multiple Harnesses](MULTIPLE_HARNESSES.md)。

| Harness | Worktree location | Project instructions | Skills | Commands | Page |
|---------|-------------------|----------------------|--------|----------|------|
| OpenAI Codex | `~/.codex/worktrees` | `AGENTS.md` | `.agents/skills/` | Skills 会显示为 `/` commands | [Codex](CODEX.md) |
| Claude Code | `.claude/worktrees` | `CLAUDE.md` | `.claude/skills/` | `.claude/commands/` | [Claude Code](CLAUDE_CODE.md) |
| Cursor | `~/.cursor/worktrees/<project>` | `AGENTS.md` 或 `.cursor/rules/` | `.cursor/skills/` 或 `.agents/skills/` | `.cursor/commands/` | [Cursor](CURSOR.md) |
| Conductor | `~/conductor/workspaces/<project>` | `CLAUDE.md` | -- | -- | [Conductor](CONDUCTOR.md) |
| T3 Code | `~/.t3/worktrees/<project>` | `AGENTS.md` | `.agents/skills/` | -- | [T3 Code](T3_CODE.md) |

## Skills vs Commands

Skills 和 commands 都可以让你定义可复用的 `/coasts` 工作流。你可以根据 harness 的支持情况使用其中一种，或者两者都用。

如果你的 harness 支持 commands，并且你想要一个明确的 `/coasts`
入口，一个简单的做法是添加一个复用该 skill 的 command。
Commands 需要通过名称显式调用，因此你可以准确知道
`/coasts` 工作流会在什么时候运行。Skills 也可以由代理根据上下文自动加载，
这很有用，但也意味着你对这些说明何时被引入的控制会更少。

你也可以两者一起使用。如果这样做，应让 command 复用 skill，而不是
单独维护一份工作流副本。

如果该 harness 只支持 skills（T3 Code），就使用 skill。如果它两者都不支持
（Conductor），就把 `/coasts` 工作流直接放进项目说明文件中。
