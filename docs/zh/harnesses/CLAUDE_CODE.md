# Claude Code

[Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) 会在项目内的 `.claude/worktrees/` 中创建
worktree。由于该目录位于仓库内部，Coasts 可以发现并分配 Claude Code worktree，
而无需任何外部绑定挂载。

Claude Code 也是这里的 harness，并且对 Coasts 的三层结构划分最清晰:

- `CLAUDE.md` 用于与 Coasts 配合工作的简短、始终启用的规则
- `.claude/skills/coasts/SKILL.md` 用于可复用的 `/coasts` 工作流
- `.claude/commands/coasts.md` 仅在你希望将命令文件作为额外入口点时使用

## Setup

将 `.claude/worktrees` 添加到 `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", ".claude/worktrees"]
```

由于 `.claude/worktrees` 是相对于项目的路径，因此不需要外部绑定挂载。

## Where Coasts guidance goes

### `CLAUDE.md`

将应在每个任务中生效的 Coasts 规则放在这里。保持简短且可操作:

- 在会话中第一次运行时命令之前先运行 `coast lookup`
- 对测试、构建和服务命令使用 `coast exec`
- 使用 `coast ps` 和 `coast logs` 获取运行时反馈
- 当没有匹配项时，在创建或重新分配 Coast 之前先询问

### `.claude/skills/coasts/SKILL.md`

将可复用的 `/coasts` 工作流放在这里。这是以下流程的正确归属位置:

1. 运行 `coast lookup` 并复用匹配的 Coast
2. 在没有匹配项时回退到 `coast ls`
3. 提供 `coast run`、`coast assign`、`coast unassign`、`coast checkout` 和
   `coast ui`
4. 直接使用 Coast CLI 作为约定，而不是对其进行包装

如果此仓库也使用 Codex、T3 Code 或 Cursor，请参见
[Multiple Harnesses](MULTIPLE_HARNESSES.md)，并将规范技能保存在
`.agents/skills/coasts/` 中，然后将其暴露给 Claude Code。

### `.claude/commands/coasts.md`

Claude Code 还支持项目命令文件。对于 Coasts 文档，请将其视为可选项:

- 仅当你明确希望使用命令文件时才使用它
- 一个简单的做法是让命令复用同一个技能
- 如果你为该命令提供自己独立的说明，就等于要维护第二份工作流副本

## Example layout

### Claude Code only

```text
CLAUDE.md
.claude/worktrees/
.claude/skills/coasts/SKILL.md
```

如果此仓库也使用 Codex、T3 Code 或 Cursor，请改用
[Multiple Harnesses](MULTIPLE_HARNESSES.md) 中的共享模式，而不是在这里重复，
因为每增加一个 harness，重复的特定提供方指导就会更难保持同步。

## What Coasts does

- **Run** — `coast run <name>` 从最新构建创建一个新的 Coast 实例。使用 `coast run <name> -w <worktree>` 可一步创建并分配 Claude Code worktree。参见 [Run](../concepts_and_terminology/RUN.md)。
- **Discovery** — Coasts 会像读取任何其他本地 worktree 目录一样读取 `.claude/worktrees`。
- **Naming** — Claude Code worktree 在 Coasts UI 和 CLI 中遵循与其他仓库内 worktree 相同的本地 worktree 命名行为。
- **Assign** — `coast assign` 可以将 `/workspace` 切换到 Claude Code worktree，而无需任何外部绑定挂载间接层。
- **Gitignored sync** — 由于 worktree 位于仓库树内，因此可正常工作。
- **Orphan detection** — 如果 Claude Code 删除了某个 worktree，Coasts 可以检测到缺失的 gitdir，并在需要时取消其分配。

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

- `.claude/worktrees/` — Claude Code worktree
- `~/.codex/worktrees/` — 如果你也在此仓库中使用 Codex，则为 Codex worktree

## Limitations

- 如果你在 `CLAUDE.md`、`.claude/skills` 和 `.claude/commands` 中重复相同的 `/coasts` 工作流，
  这些副本会逐渐偏离。请保持 `CLAUDE.md` 简短，并将可复用工作流保留在一个技能中。
- 如果你希望一个仓库能在多个 harness 中良好工作，请优先采用
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) 中的共享模式。
