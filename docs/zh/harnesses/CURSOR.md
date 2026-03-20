# Cursor

[Cursor](https://cursor.com/docs/agent/overview) 可以直接在你当前的 checkout 中工作，它的 Parallel Agents 功能也可以在 `~/.cursor/worktrees/<project-name>/` 下创建 git worktree。

对于 Coasts 的文档来说，这意味着有两种设置情况:

- 如果你只是在当前 checkout 中使用 Cursor，则不需要特定于 Cursor 的 `worktree_dir` 条目
- 如果你使用 Cursor Parallel Agents，请将 Cursor 的 worktree 目录添加到 `worktree_dir`，以便 Coasts 可以发现并分配这些 worktree

## 设置

### 仅当前 checkout

如果 Cursor 只是在编辑你已经打开的 checkout，Coasts 不需要任何特定于 Cursor 的特殊 worktree 路径。Coasts 会像对待任何其他本地仓库根目录一样对待该 checkout。

### Cursor Parallel Agents

如果你使用 Parallel Agents，请将 `~/.cursor/worktrees/<project-name>` 添加到 `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.cursor/worktrees/my-app"]
```

Cursor 会将每个 agent 的 worktree 存储在该项目专属目录下。Coasts 会在运行时展开 `~` 并将该路径视为外部路径，因此必须重新创建现有实例，才能使 bind mount 生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

在修改 Coastfile 后，worktree 列表会立即更新，但要分配到 Cursor Parallel Agent worktree，则需要容器内的外部 bind mount。

## Coasts 指南应放置的位置

### `AGENTS.md` 或 `.cursor/rules/coast.md`

将简短、始终启用的 Coast Runtime 规则放在这里:

- 如果你希望项目说明具有最高可移植性，请使用 `AGENTS.md`
- 如果你希望使用 Cursor 原生项目规则和 settings UI 支持，请使用 `.cursor/rules/coast.md`
- 除非你有明确的理由，否则不要在两者中重复相同的 Coast Runtime 区块

### `.cursor/skills/coasts/SKILL.md` 或共享的 `.agents/skills/coasts/SKILL.md`

将可复用的 `/coasts` 工作流放在这里:

- 对于仅使用 Cursor 的仓库，`.cursor/skills/coasts/SKILL.md` 是一个自然的放置位置
- 对于多 harness 仓库，请将规范技能保存在 `.agents/skills/coasts/SKILL.md` 中；Cursor 可以直接加载它
- 该 skill 应该拥有真实的 `/coasts` 工作流:`coast lookup`、`coast ls`、`coast run`、`coast assign`、`coast unassign`、`coast checkout` 和 `coast ui`

### `.cursor/commands/coasts.md`

Cursor 也支持项目命令。对于 Coasts 的文档，请将命令视为可选项:

- 仅当你想要一个显式的 `/coasts` 入口点时才添加命令
- 一个简单的选择是让该命令复用同一个 skill
- 如果你给该命令提供它自己独立的说明，那么你就需要维护第二份工作流副本

### `.cursor/worktrees.json`

将 `.cursor/worktrees.json` 用于 Cursor 自身的 worktree 引导，而不是用于 Coasts 策略:

- 安装依赖
- 复制或创建 `.env` 文件的符号链接
- 运行数据库迁移或其他一次性引导步骤

不要将 Coast Runtime 规则或 Coast CLI 工作流移入 `.cursor/worktrees.json`。

## 示例布局

### 仅 Cursor

```text
AGENTS.md
.cursor/skills/coasts/SKILL.md
.cursor/commands/coasts.md        # 可选
.cursor/rules/coast.md            # AGENTS.md 的可选替代方案
.cursor/worktrees.json            # 可选，用于 Parallel Agents 引导
```

### Cursor 加其他 harness

```text
AGENTS.md
CLAUDE.md
.agents/skills/coasts/SKILL.md
.agents/skills/coasts/agents/openai.yaml
.claude/skills/coasts -> ../../.agents/skills/coasts
.cursor/commands/coasts.md        # 可选
```

## Coasts 的作用

- **运行** — `coast run <name>` 从最新构建创建一个新的 Coast 实例。使用 `coast run <name> -w <worktree>` 可以一步完成创建并分配一个 Cursor worktree。参见 [Run](../concepts_and_terminology/RUN.md)。
- **当前 checkout** — 当 Cursor 直接在你打开的仓库中工作时，不需要任何特殊的 Cursor 处理。
- **Bind mount** — 对于 Parallel Agents，Coasts 会将 `~/.cursor/worktrees/<project-name>` 挂载到容器中的 `/host-external-wt/{index}`。
- **发现** — `git worktree list --porcelain` 仍然是仓库作用域的，因此 Coasts 只会显示属于当前项目的 Cursor worktree。
- **命名** — Cursor Parallel Agent worktree 会以它们的分支名称显示在 Coasts 的 CLI 和 UI 中。
- **分配** — 当选择 Cursor worktree 时，`coast assign` 会从外部 bind mount 路径重新挂载 `/workspace`。
- **Gitignored 同步** — 继续在主机文件系统上通过绝对路径工作。
- **孤儿检测** — 如果 Cursor 清理了旧 worktree，Coasts 可以检测到缺失的 gitdir，并在需要时取消分配它们。

## 示例

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

- `.claude/worktrees/` — Claude Code worktree
- `~/.codex/worktrees/` — Codex worktree
- `~/.cursor/worktrees/my-app/` — Cursor Parallel Agent worktree

## 限制

- 如果你没有使用 Cursor Parallel Agents，请不要仅仅因为你恰好在 Cursor 中编辑，就添加 `~/.cursor/worktrees/<project-name>`。
- 将 Coast Runtime 规则保存在一个始终启用的位置:`AGENTS.md` 或 `.cursor/rules/coast.md`。同时重复两者会导致内容漂移。
- 将可复用的 `/coasts` 工作流保存在 skill 中。`.cursor/worktrees.json` 用于 Cursor 引导，而不是 Coasts 策略。
- 如果一个仓库会在 Cursor、Codex、Claude Code 或 T3 Code 之间共享，优先使用 [Multiple Harnesses](MULTIPLE_HARNESSES.md) 中的共享布局。
