# Codex

[Codex](https://developers.openai.com/codex/app/worktrees/) 会在 `$CODEX_HOME/worktrees`（通常是 `~/.codex/worktrees`）创建 worktree。每个 worktree 都位于一个不透明的哈希目录下，例如 `~/.codex/worktrees/a0db/project-name`，以 detached HEAD 状态开始，并会根据 Codex 的保留策略自动清理。

摘自 [Codex docs](https://developers.openai.com/codex/app/worktrees/):

> 我可以控制 worktree 的创建位置吗？
> 目前不可以。Codex 会在 `$CODEX_HOME/worktrees` 下创建 worktree，以便能够一致地管理它们。

由于这些 worktree 位于项目根目录之外，Coasts 需要显式
配置才能发现并挂载它们。

## Setup

将 `~/.codex/worktrees` 添加到 `worktree_dir`:

```toml
[coast]
name = "my-app"
worktree_dir = [".worktrees", "~/.codex/worktrees"]
```

Coasts 会在运行时展开 `~`，并将任何以 `~/` 或 `/` 开头的路径视为
外部路径。详情请参见 [Worktree Directories](../coastfiles/WORKTREE_DIR.md)。

更改 `worktree_dir` 后，必须**重新创建**现有实例，才能使 bind mount 生效:

```bash
coast rm my-instance
coast build
coast run my-instance
```

worktree 列表会立即更新（Coasts 会读取新的 Coastfile），但
要分配到 Codex worktree，则需要容器内存在 bind mount。

## Where Coasts guidance goes

在使用 Coasts 时，请使用 Codex 的项目说明文件和共享 skill 布局:

- 将简短的 Coast Runtime 规则放在 `AGENTS.md` 中
- 将可复用的 `/coasts` 工作流放在 `.agents/skills/coasts/SKILL.md` 中
- Codex 会将该 skill 暴露为 `/coasts` 命令
- 如果你使用 Codex 专用元数据，请将其与 skill 一起放在
  `.agents/skills/coasts/agents/openai.yaml`
- 不要仅仅为了编写关于 Coasts 的文档而创建单独的项目命令文件；该
  skill 就是可复用的入口
- 如果此仓库也使用 Cursor 或 Claude Code，请将规范的 skill 保存在
  `.agents/skills/` 中，并从那里进行暴露。参见
  [Multiple Harnesses](MULTIPLE_HARNESSES.md) 和
  [Skills for Host Agents](../SKILLS_FOR_HOST_AGENTS.md)。

例如，一个最小化的 `.agents/skills/coasts/agents/openai.yaml` 可以
像这样:

```yaml
interface:
  display_name: "Coasts"
  short_description: "Inspect, assign, and open Coasts for this repo"
  default_prompt: "Use this skill when the user wants help finding, assigning, or opening a Coast."

policy:
  allow_implicit_invocation: false
```

这样可以让该 skill 在 Codex 中以更友好的标签显示，并使 `/coasts` 成为
显式命令。只有当该 skill 还需要 MCP 服务器或其他由 OpenAI 管理的工具
接线时，才添加 `dependencies.tools`。

## What Coasts does

- **Run** -- `coast run <name>` 会基于最新构建创建一个新的 Coast 实例。使用 `coast run <name> -w <worktree>` 可以一步完成 Codex worktree 的创建和分配。参见 [Run](../concepts_and_terminology/RUN.md)。
- **Bind mount** -- 在创建容器时，Coasts 会将
  `~/.codex/worktrees` 挂载到容器中的 `/host-external-wt/{index}`。
- **Discovery** -- `git worktree list --porcelain` 的作用域是仓库级别，因此即使该目录包含许多项目的 worktree，也只会显示属于当前项目的 Codex worktree。
- **Naming** -- detached HEAD worktree 会显示为其在外部目录中的相对路径（`a0db/my-app`、`eca7/my-app`）。基于分支的 worktree 会显示分支名称。
- **Assign** -- `coast assign` 会从外部 bind mount 路径重新挂载 `/workspace`。
- **Gitignored sync** -- 在主机文件系统上使用绝对路径运行，无需 bind mount 也可工作。
- **Orphan detection** -- git watcher 会递归扫描外部目录，
  并通过 `.git` gitdir 指针进行过滤。如果 Codex 删除了某个
  worktree，Coasts 会自动取消该实例的分配。

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

- `.claude/worktrees/` -- Claude Code（本地，无需特殊处理）
- `~/.codex/worktrees/` -- Codex（外部，使用 bind mount 挂载）

## Limitations

- Codex 可能随时清理 worktree。Coasts 中的 orphan detection
  可以妥善处理这种情况。
